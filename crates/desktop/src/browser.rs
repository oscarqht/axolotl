//! The desktop browser boundary. Wry owns native pages; the workspace owns tabs.
//!
//! Native handles stay on the UI thread; callbacks enqueue events to avoid reentrant updates.

use crate::native_webview::{BuildNativeChild, NativeWebView as WebView};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::Receiver,
    },
};

use anyhow::{Context as _, Result, bail};
use gpui::{Bounds, Pixels, Window};
use workspace_core::TabId;
use wry::{NewWindowResponse, PageLoadEvent, WebViewBuilder};

const START_HTML: &str = include_str!("../assets/start.html");

pub(crate) fn html_data_url(html: &str) -> String {
    use base64::Engine as _;
    format!(
        "data:text/html;charset=utf-8;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(html)
    )
}

pub(crate) fn inline_navigation(url: &str, expected: &str) -> bool {
    url == "about:blank" || (cfg!(target_os = "windows") && url == expected)
}

// WebView2 can report NavigateToString as a data URL. Permit only the exact
// HTML supplied by the application, rather than opening navigation to data URLs.
pub fn with_inline_html<'a>(
    builder: WebViewBuilder<'a>,
    html: impl Into<String>,
) -> WebViewBuilder<'a> {
    let html = html.into();
    let expected = html_data_url(&html);
    builder
        .with_navigation_handler(move |url| inline_navigation(&url, &expected))
        .with_html(html)
}

#[derive(Debug, Clone)]
pub enum BrowserEvent {
    ProtectionChanged,
    Focused(TabId),
    Favicon(TabId, String),
    Notification(TabId),
    Navigated(TabId, String),
    Title(TabId, String, Option<String>),
    Loaded(TabId),
    LoadStarted(TabId),
    VisitCompleted(TabId, String, std::time::Instant),
    // Other platforms still reopen native requests as independent tabs.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    NewTab(TabId, String),
    #[cfg(target_os = "macos")]
    NativePopup(TabId, TabId, String),
    #[cfg(target_os = "macos")]
    Closed(TabId),
    OpenLink(TabId, String),
    PopupLink(TabId, String),
    Shortcut(TabId, String),
    Gesture(TabId, f64, f64),
    /// Whether the page can navigate back/forward, from a context-menu probe.
    History(TabId, bool, bool),
}

#[cfg(target_os = "macos")]
impl BrowserEvent {
    pub fn remap(&mut self, ids: &HashMap<TabId, TabId>) {
        let tab = match self {
            Self::ProtectionChanged => return,
            Self::Focused(t)
            | Self::Favicon(t, _)
            | Self::Notification(t)
            | Self::Navigated(t, _)
            | Self::Title(t, _, _)
            | Self::Loaded(t)
            | Self::LoadStarted(t)
            | Self::VisitCompleted(t, _, _)
            | Self::NewTab(t, _)
            | Self::OpenLink(t, _)
            | Self::PopupLink(t, _)
            | Self::Shortcut(t, _)
            | Self::Gesture(t, _, _)
            | Self::History(t, _, _)
            | Self::Closed(t)
            | Self::NativePopup(t, _, _) => t,
        };
        if let Some(id) = ids.get(tab) {
            *tab = *id;
        }
    }
}

/// The operations a replacement engine must provide. Browser state is never
/// serialized into the layout core, so a future mobile adapter can share it.
pub trait BrowserHost {
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_background_policy(&self, tab: TabId) -> Result<isize>;
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_zoom(&self, tab: TabId) -> Result<f64>;

    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_key(
        &self,
        tab: TabId,
        code: u16,
        text: &str,
        flags: objc2_app_kit::NSEventModifierFlags,
        repeat: bool,
    ) -> Result<()>;
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_click(&self, tab: TabId, modified: bool) -> Result<()>;
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_scroll(&self, tab: TabId, x: f64, y: f64, dx: i32, dy: i32) -> Result<()>;
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_click_at(&self, tab: TabId, x: f64, y: f64, modified: bool) -> Result<()>;
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_option_click(&self, tab: TabId) -> Result<()>;
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_option_click_at(&self, tab: TabId, x: f64, y: f64) -> Result<()>;
    #[cfg(target_os = "macos")]
    fn has_native_focus(&self, tab: TabId) -> Result<bool>;
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn save_test_snapshot(&self, tab: TabId, path: &str) -> Result<()>;
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn set_test_appearance(&self, tab: TabId, dark: Option<bool>) -> Result<()>;
    #[cfg(feature = "smoke")]
    fn evaluate(
        &self,
        tab: TabId,
        script: &str,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<()>;
    #[cfg(feature = "smoke")]
    fn is_visible(&self, tab: TabId) -> bool;
    #[cfg(feature = "smoke")]
    fn is_loaded(&self, tab: TabId) -> bool;
    fn focused_page(&self) -> Option<TabId>;
    fn page_tool(&self, tab: TabId, operation: crate::page_tools::Operation) -> Result<()>;
    fn downloads(&self) -> Rc<RefCell<crate::downloads::DownloadManager>>;
    fn protection(&self) -> Rc<RefCell<crate::protection::Protection>>;
    fn ensure(&self, tab: TabId, url: &str, window: &Window, cx: &gpui::App) -> Result<()>;
    fn has_view(&self, tab: TabId) -> bool;
    fn popup_transition(
        &self,
        tab: TabId,
        opening: bool,
        initial: bool,
        duration: std::time::Duration,
    ) -> Result<()>;
    fn reset_popup_transition(&self, tab: TabId) -> Result<()>;
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn popup_animation_state(&self, tab: TabId) -> Result<serde_json::Value>;
    #[cfg(feature = "smoke")]
    fn panel_clip_state(&self, tab: TabId) -> Result<serde_json::Value>;

    fn rekey(&self, from: TabId, to: TabId) -> Result<()>;
    fn capture_sleep(&self, tab: TabId, image: crate::drag_preview::Snapshot) -> Result<()>;
    fn unload(&self, tab: TabId);
    fn probe_sleep(&self, tab: TabId, callback: Box<dyn Fn(String) + Send>) -> Result<()>;
    fn place(&self, tab: TabId, bounds: Bounds<Pixels>) -> Result<()>;
    fn set_panel_corners(&self, tab: TabId, radius: f32, bottom_only: bool) -> Result<()>;
    fn show(&self, tab: TabId, visible: bool) -> Result<()>;
    #[cfg(target_os = "macos")]
    fn auto_pip_visibility(&self, visible: HashSet<TabId>);
    fn raise(&self, tab: TabId) -> Result<()>;
    fn focus(&self, tab: TabId) -> Result<()>;
    fn focus_parent(&self, tab: TabId) -> Result<()>;
    fn navigate(&self, tab: TabId, url: &str) -> Result<()>;
    fn back(&self, tab: TabId) -> Result<()>;
    fn forward(&self, tab: TabId) -> Result<()>;
    fn reload(&self, tab: TabId) -> Result<()>;
    /// Ask the page whether history exists in each direction; answered by `BrowserEvent::History`.
    fn probe_history(&self, tab: TabId) -> Result<()>;
    fn current_url(&self, tab: TabId) -> Result<String>;
    #[cfg(target_os = "macos")]
    fn show_copy_url_toast(&self, tab: TabId) -> Result<()>;
    fn retain(&self, tabs: &HashSet<TabId>);
    fn drag_preview(
        &self,
        tab: TabId,
        title: &str,
        window: &Window,
        cx: &gpui::App,
    ) -> Result<crate::drag_preview::DragPreview>;
    /// Request presentation after shell interactions involving native child views.
    fn request_redraw(&self);
    fn hide_all(&self) -> Result<()>;
    fn drain_events(&self) -> Vec<BrowserEvent>;
}

/// `navigation.canGoBack` is exact where available. Otherwise assume Back is
/// possible only when the session has more than one entry, and Forward never.
const HISTORY_PROBE: &str = "(()=>{const n=window.navigation;return n&&typeof n.canGoBack==='boolean'?[n.canGoBack,n.canGoForward]:[history.length>1,false]})()";

/// Parse the probe's JSON result; anything unexpected disables both directions.
pub(crate) fn parse_history(value: &str) -> (bool, bool) {
    match serde_json::from_str::<Vec<bool>>(value.trim()) {
        Ok(v) if v.len() == 2 => (v[0], v[1]),
        _ => (false, false),
    }
}

pub struct WryHost(Rc<WryState>);

impl std::ops::Deref for WryHost {
    type Target = WryState;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct WryState {
    downloads: Rc<RefCell<crate::downloads::DownloadManager>>,
    #[cfg(target_os = "macos")]
    pending_popups: RefCell<HashSet<TabId>>,
    #[cfg(target_os = "macos")]
    next_popup: std::cell::Cell<TabId>,
    #[cfg(target_os = "macos")]
    popup_delegates:
        RefCell<HashMap<TabId, objc2::rc::Retained<crate::macos_popup::PopupDelegate>>>,
    protection: Rc<RefCell<crate::protection::Protection>>,
    protection_revisions: RefCell<HashMap<TabId, (u64, crate::protection::Policy)>>,
    protection_notified: RefCell<String>,
    site_zoom: RefCell<crate::page_tools::SiteZoom>,
    unsaved_zoom: RefCell<HashMap<TabId, f64>>,
    views: RefCell<HashMap<TabId, Rc<WebView>>>,
    retired_views: RefCell<Vec<Rc<WebView>>>,
    identities: RefCell<HashMap<TabId, Arc<AtomicU64>>>,
    events: Receiver<BrowserEvent>,
    sender: crate::ui_events::SyncSender<BrowserEvent>,
    visible: Rc<RefCell<HashSet<TabId>>>,
    loaded: Rc<RefCell<HashSet<TabId>>>,
    #[cfg(target_os = "macos")]
    pip_owner: Rc<std::cell::Cell<Option<TabId>>>,
    #[cfg(target_os = "macos")]
    pip_visible: RefCell<HashSet<TabId>>,
    #[cfg(target_os = "macos")]
    pip_pending: RefCell<Vec<TabId>>,
}

impl Default for WryHost {
    fn default() -> Self {
        // Untrusted pages cannot cause unbounded allocation by flooding IPC.
        let (sender, events) = crate::ui_events::sync_channel(512);
        Self(Rc::new(WryState {
            downloads: Rc::new(RefCell::new(crate::downloads::DownloadManager::default())),
            #[cfg(target_os = "macos")]
            pending_popups: RefCell::new(HashSet::new()),
            #[cfg(target_os = "macos")]
            next_popup: std::cell::Cell::new(u64::MAX - 1),
            #[cfg(target_os = "macos")]
            popup_delegates: RefCell::new(HashMap::new()),
            protection: Rc::new(RefCell::new(crate::protection::Protection::restore())),
            protection_revisions: RefCell::new(HashMap::new()),
            protection_notified: RefCell::new(String::new()),
            site_zoom: RefCell::new(crate::page_tools::SiteZoom::restore()),
            unsaved_zoom: RefCell::new(HashMap::new()),
            views: RefCell::new(HashMap::new()),
            retired_views: RefCell::new(Vec::new()),
            identities: RefCell::new(HashMap::new()),
            events,
            sender,
            visible: Rc::new(RefCell::new(HashSet::new())),
            loaded: Rc::new(RefCell::new(HashSet::new())),
            #[cfg(target_os = "macos")]
            pip_owner: Rc::new(std::cell::Cell::new(None)),
            #[cfg(target_os = "macos")]
            pip_visible: RefCell::new(HashSet::new()),
            #[cfg(target_os = "macos")]
            pip_pending: RefCell::new(Vec::new()),
        }))
    }
}

impl WryHost {
    #[cfg(target_os = "macos")]
    fn request_next_pip(&self) {
        while self.pip_owner.get().is_none() {
            let Some(tab) = self.pip_pending.borrow_mut().pop() else {
                break;
            };
            if self.pip_visible.borrow().contains(&tab) || !self.loaded.borrow().contains(&tab) {
                continue;
            }
            if let Ok(view) = self.get(tab) {
                self.pip_owner.set(Some(tab));
                if view
                    .evaluate_script("window.__axolotlNativePip?.()")
                    .is_err()
                {
                    self.pip_owner.set(None);
                }
            }
        }
    }

    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn native_test_click(
        &self,
        tab: TabId,
        x: f64,
        y: f64,
        flags: objc2_app_kit::NSEventModifierFlags,
    ) -> Result<()> {
        use objc2_app_kit::{NSEvent, NSEventType};
        use objc2_foundation::NSPoint;
        use wry::WebViewExtMacOS;
        let view = self.get(tab)?;
        let webview = view.webview();
        let window = view.ns_window();
        let point = NSPoint::new(
            x,
            if webview.isFlipped() {
                y
            } else {
                webview.frame().size.height - y
            },
        );
        let point = webview.convertPoint_toView(point, None);
        for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
            let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind, point, flags, objc2_foundation::NSProcessInfo::processInfo().systemUptime(), window.windowNumber(), None, 0, 1, 1.,
            ).context("create native click event")?;
            // NSWindow hit testing delivers a genuine WebKit input event, not a
            // JavaScript dispatchEvent (which the bridge intentionally rejects).
            window.sendEvent(&event);
        }
        Ok(())
    }
    pub fn new() -> Self {
        Self::default()
    }

    fn downloads_active(&self) -> bool {
        self.downloads.borrow().busy()
    }

    fn get(&self, tab: TabId) -> Result<Rc<WebView>> {
        self.views
            .borrow()
            .get(&tab)
            .cloned()
            .with_context(|| format!("Browser view for tab {tab} is unavailable"))
    }
}

impl WryHost {
    fn create_view(
        &self,
        tab: TabId,
        url: &str,
        navigate: bool,
        build: impl FnOnce(WebViewBuilder<'static>) -> Result<WebView>,
    ) -> Result<()> {
        #[cfg(not(target_os = "macos"))]
        let _ = navigate;
        let identity = Arc::new(AtomicU64::new(tab));
        let ipc_identity = identity.clone();
        let title_identity = identity.clone();
        let load_identity = identity.clone();
        let popup_identity = identity.clone();
        let ipc = self.sender.clone();
        let title = self.sender.clone();
        // Title KVO can precede load-start. Snapshot its document URL so a new
        // page title cannot overwrite the previous visit while events are queued.
        let title_view = Rc::new(RefCell::new(None::<std::rc::Weak<WebView>>));
        let title_source = title_view.clone();
        let load = self.sender.clone();
        let loaded = self.loaded.clone();
        let completed = Rc::new(std::cell::Cell::new(false));
        #[cfg(not(target_os = "macos"))]
        let popup = self.sender.clone();
        #[cfg(target_os = "macos")]
        let popup_host = Rc::downgrade(&self.0);
        let visible = self.visible.clone();
        #[cfg(target_os = "macos")]
        let pip_owner = self.pip_owner.clone();
        #[cfg(target_os = "macos")]
        let pip_host = Rc::downgrade(&self.0);
        #[cfg(target_os = "macos")]
        let pip_load_owner = self.pip_owner.clone();
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let input_bridge = INPUT_BRIDGE
            .replace("__NONCE__", &nonce)
            .replace(
                "__LINK_MODIFIER__",
                if cfg!(target_os = "macos") {
                    "event.metaKey && !event.ctrlKey"
                } else {
                    "event.ctrlKey && !event.metaKey"
                },
            )
            .replace("__GESTURES__", include_str!("../assets/gestures.js"));
        let notifications = include_str!("../assets/notifications.js").replace("__NONCE__", &nonce);
        let native_pip = if cfg!(target_os = "macos") {
            include_str!("../assets/native-pip.js").replace("__NONCE__", &nonce)
        } else {
            String::new()
        };
        let start_url = html_data_url(&crate::icons::html(START_HTML));
        let builder = WebViewBuilder::new()
            // Keep retained webapps running independently of view visibility.
            // Keep Alive only governs our unload timer; WebKit's separate
            // inactive scheduling can otherwise suspend even those pages.
            // Wry applies this on macOS 14+, ignoring unsupported platforms.
            .with_background_throttling(wry::BackgroundThrottlingPolicy::Disabled)
            .with_visible(false)
            .with_focused(false)
            .with_bounds(wry::Rect {
                position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: wry::dpi::LogicalSize::new(1.0, 1.0).into(),
            })
            .with_devtools(cfg!(debug_assertions))
            .with_accept_first_mouse(true)
            // Apply on every document, including subframes on WKWebView and
            // WebView2. Keep this separate from the privileged input bridge.
            .with_initialization_script_for_main_only(
                include_str!("../assets/hide-scrollbars.js"),
                false,
            )
            .with_initialization_script_for_main_only(input_bridge, false)
            .with_initialization_script_for_main_only(
                include_str!("../assets/sleep-activity.js"),
                false,
            )
            .with_initialization_script_for_main_only(notifications, false)
            .with_initialization_script(&native_pip)
            .with_navigation_handler(move |url| {
                allowed_url(&url) || (cfg!(target_os = "windows") && url == start_url)
            })
            .with_ipc_handler(move |request| {
                let tab = ipc_identity.load(Ordering::Relaxed);
                #[cfg(target_os = "macos")]
                {
                    if request.body() == &format!("{nonce}:pip:entered") {
                        pip_owner.set(Some(tab));
                        if let Some(host) = pip_host.upgrade() {
                            host.pip_pending.borrow_mut().clear();
                        }
                        return;
                    }
                    if request.body() == &format!("{nonce}:pip:unavailable") {
                        if pip_owner.get() == Some(tab) {
                            pip_owner.set(None);
                            if let Some(host) = pip_host.upgrade() {
                                WryHost(host).request_next_pip();
                            }
                        }
                        return;
                    }
                    if request.body() == &format!("{nonce}:pip:left") {
                        if pip_owner.get() == Some(tab) {
                            pip_owner.set(None);
                        }
                        return;
                    }
                }
                // Notifications must also arrive from hidden tabs and spaces.
                if request.body() == &format!("{nonce}:notification") {
                    let _ = ipc.try_send(BrowserEvent::Notification(tab));
                    return;
                }
                if let Some(encoded) = request.body().strip_prefix(&format!("{nonce}:favicon:")) {
                    if encoded.len() <= 32768 {
                        let _ = ipc.try_send(BrowserEvent::Favicon(tab, encoded.to_owned()));
                    }
                    return;
                }
                if !visible.borrow().contains(&tab) {
                    return;
                }
                if let Some(value) = request.body().strip_prefix(&format!("{nonce}:wheel:"))
                    && let Some((x, y)) = value.split_once(':')
                    && let (Ok(x), Ok(y)) = (x.parse::<f64>(), y.parse::<f64>())
                    && x.is_finite()
                    && y.is_finite()
                {
                    let _ = ipc.try_send(BrowserEvent::Gesture(tab, x, y));
                    return;
                }
                if let Some(url) = authenticated_popup_link(request.body(), &nonce) {
                    let _ = ipc.try_send(BrowserEvent::PopupLink(tab, url.to_owned()));
                    return;
                }
                if let Some(url) = authenticated_link(request.body(), &nonce) {
                    let _ = ipc.try_send(BrowserEvent::OpenLink(tab, url.to_owned()));
                    return;
                }
                let Some(command) = authenticated_command(request.body(), &nonce) else {
                    return;
                };
                let event = match command {
                    "focus" => BrowserEvent::Focused(tab),
                    "settings" | "address" | "new-tab" | "new-space" | "escape" | "close-tab"
                    | "reload" | "swipe-next" | "swipe-prev" | "history-back"
                    | "history-forward" | "zoom-in" | "zoom-out" | "zoom-reset" | "find-page" => {
                        BrowserEvent::Shortcut(tab, command.to_owned())
                    }
                    _ => return,
                };
                let _ = ipc.try_send(event);
            })
            .with_document_title_changed_handler(move |value| {
                let tab = title_identity.load(Ordering::Relaxed);
                let url = title_source
                    .borrow()
                    .as_ref()
                    .and_then(|v| v.upgrade())
                    .and_then(|v| v.url().ok());
                let _ = title.try_send(BrowserEvent::Title(tab, value, url));
            })
            .with_on_page_load_handler(move |event, url| {
                let tab = load_identity.load(Ordering::Relaxed);
                if matches!(event, PageLoadEvent::Finished) {
                    loaded.borrow_mut().insert(tab);
                } else {
                    loaded.borrow_mut().remove(&tab);
                    completed.set(false);
                    #[cfg(target_os = "macos")]
                    if pip_load_owner.get() == Some(tab) {
                        pip_load_owner.set(None);
                    }
                    #[cfg(not(target_os = "windows"))]
                    let _ = load.try_send(BrowserEvent::LoadStarted(tab));
                }
                let _ = load.try_send(BrowserEvent::Navigated(tab, url.clone()));
                if matches!(event, PageLoadEvent::Finished) {
                    let _ = load.try_send(BrowserEvent::Loaded(tab));
                    // WKWebView calls Finished only from didFinishNavigation (main frame).
                    // WebView2 requires IsSuccess, handled by the native callback below.
                    #[cfg(not(target_os = "windows"))]
                    if !completed.replace(true) {
                        let _ = load.try_send(BrowserEvent::VisitCompleted(
                            tab,
                            url,
                            std::time::Instant::now(),
                        ));
                    }
                }
            })
            .with_new_window_req_handler(move |url, features| {
                let tab = popup_identity.load(Ordering::Relaxed);
                #[cfg(target_os = "macos")]
                {
                    let Some(state) = popup_host.upgrade() else {
                        return NewWindowResponse::Deny;
                    };
                    WryHost(state).create_popup(tab, url, features)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = features;
                    if allowed_url(&url) {
                        let _ = popup.try_send(BrowserEvent::NewTab(tab, url));
                    }
                    NewWindowResponse::Deny
                }
            })
            // Native download hooks own the destination, progress and cancellation.
            .with_download_started_handler(|_, _| true);
        // Set the native UA before the first request, so HTTP and JavaScript
        // detection agree on initial loads, redirects, frames and new tabs.
        #[cfg(target_os = "macos")]
        let builder = if let Some(agent) = crate::browser_identity::user_agent() {
            builder.with_user_agent(agent)
        } else {
            builder
        };
        // Build without a target URL: filtering must be installed before the
        // first network navigation, including redirects and restored tabs.
        let view = build(builder).context("Creating native browser child view")?;
        #[cfg(target_os = "windows")]
        view.set_panel_corner_radius(crate::render::PANEL_CORNER_RADIUS)?;
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS;
            // Native pages are siblings above GPUI, so the pane's overflow clip
            // cannot clip them. A layer mask follows the view through relayout,
            // tab moves and popup promotion without touching document styles.
            let native = view.webview();
            native.setWantsLayer(true);
            let layer = native.layer().context("Creating browser panel layer")?;
            layer.setCornerRadius(f64::from(crate::render::PANEL_CORNER_RADIUS));
            layer.setMasksToBounds(true);
        }
        #[cfg(target_os = "macos")]
        crate::downloads::install(&view, self.downloads.clone())?;
        #[cfg(target_os = "windows")]
        view.setup_downloads(self.downloads.clone())?;
        let view = Rc::new(view);
        *title_view.borrow_mut() = Some(Rc::downgrade(&view));
        #[cfg(target_os = "windows")]
        {
            let sender = self.sender.clone();
            let visit_identity = identity.clone();
            view.observe_visits(move |url| {
                let tab = visit_identity.load(Ordering::Relaxed);
                let event = match url {
                    Some(url) => BrowserEvent::VisitCompleted(tab, url, std::time::Instant::now()),
                    None => BrowserEvent::LoadStarted(tab),
                };
                let _ = sender.try_send(event);
            })?;
        }
        let p = self.protection.borrow();
        let script = p.rules.script(&p.policy);
        #[cfg(target_os = "macos")]
        {
            p.native.apply(&view, &script)?;
            // WebKit performs popup navigation after its delegate returns. A
            // second load here would discard the native WindowProxy linkage.
            if navigate {
                if url == "about:blank" {
                    view.load_html(&crate::icons::html(START_HTML))?;
                } else {
                    view.load_url(url)?;
                }
            }
        }
        #[cfg(target_os = "windows")]
        view.setup_protection(self.protection.clone(), script, url.to_owned(), true, false)?;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = script;
            bail!("Built-in protection requires macOS or Windows");
        }
        self.protection_revisions
            .borrow_mut()
            .insert(tab, (p.revision, p.policy.clone()));
        // `with_focused(false)` is ignored on macOS; explicitly return focus.
        view.focus_parent()
            .context("Returning focus from new browser view")?;
        #[cfg(target_os = "macos")]
        {
            let sender = self.sender.clone();
            let close_identity = identity.clone();
            let delegate = crate::macos_popup::install(&view, move || {
                let _ =
                    sender.try_send(BrowserEvent::Closed(close_identity.load(Ordering::Relaxed)));
            })?;
            self.popup_delegates.borrow_mut().insert(tab, delegate);
        }
        #[cfg(target_os = "windows")]
        {
            let sender = self.sender.clone();
            let identity = identity.clone();
            view.install_page_shortcuts(move |command| {
                let _ = sender.try_send(BrowserEvent::Shortcut(
                    identity.load(Ordering::Relaxed),
                    command.into(),
                ));
            })?;
        }
        self.views.borrow_mut().insert(tab, view);
        self.identities.borrow_mut().insert(tab, identity);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn create_popup(
        &self,
        source: TabId,
        url: String,
        features: wry::NewWindowFeatures,
    ) -> NewWindowResponse {
        use wry::WebViewExtMacOS;
        // WebKit reports window.open('') as an empty request URL. It still
        // creates an about:blank context which script may navigate afterward.
        let url = if url.is_empty() {
            "about:blank".to_owned()
        } else {
            url
        };
        if !allowed_url(&url)
            || self.pending_popups.borrow().len() >= 32
            || !self.protection.borrow().native.ready()
        {
            return NewWindowResponse::Deny;
        }
        let id = self.next_popup.get();
        self.next_popup.set(id - 1);
        let result = self.create_view(id, &url, false, |builder| {
            crate::macos_popup::build_child(
                builder,
                &features.opener.webview,
                features.opener.target_configuration,
            )
        });
        if let Err(error) = result {
            eprintln!("Creating linked popup: {error:#}");
            return NewWindowResponse::Deny;
        }
        let view = self.get(id).expect("created popup");
        self.pending_popups.borrow_mut().insert(id);
        if self
            .sender
            .try_send(BrowserEvent::NativePopup(source, id, url))
            .is_err()
        {
            self.unload(id);
            return NewWindowResponse::Deny;
        }
        NewWindowResponse::Create {
            webview: view.webview().into_super(),
        }
    }
}

impl BrowserHost for WryHost {
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_background_policy(&self, tab: TabId) -> Result<isize> {
        use wry::WebViewExtMacOS;
        let view = self.get(tab)?;
        let preferences = unsafe { view.webview().configuration().preferences() };
        Ok(unsafe { objc2::msg_send![&*preferences, inactiveSchedulingPolicy] })
    }
    fn downloads(&self) -> Rc<RefCell<crate::downloads::DownloadManager>> {
        self.downloads.clone()
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn save_test_snapshot(&self, tab: TabId, path: &str) -> Result<()> {
        use block2::RcBlock;
        use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
        use objc2_foundation::{NSDictionary, NSError};
        use wry::WebViewExtMacOS;
        let path = path.to_owned();
        let callback = RcBlock::new(move |snapshot: *mut NSImage, _error: *mut NSError| unsafe {
            if let Some(snapshot) = snapshot.as_ref()
                && let Some(tiff) = snapshot.TIFFRepresentation()
                && let Some(bitmap) = NSBitmapImageRep::imageRepWithData(&tiff)
                && let Some(png) = bitmap.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            {
                let _ = std::fs::write(&path, png.to_vec());
            }
        });
        unsafe {
            self.get(tab)?
                .webview()
                .takeSnapshotWithConfiguration_completionHandler(None, &callback);
        }
        Ok(())
    }

    fn protection(&self) -> Rc<RefCell<crate::protection::Protection>> {
        self.protection.clone()
    }

    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_key(
        &self,
        tab: TabId,
        code: u16,
        text: &str,
        flags: objc2_app_kit::NSEventModifierFlags,
        repeat: bool,
    ) -> Result<()> {
        use objc2_app_kit::{NSApplication, NSEvent, NSEventType};
        use objc2_foundation::{NSPoint, NSString};
        use wry::WebViewExtMacOS;
        let view = self.get(tab)?;
        let window = view.ns_window();
        window.makeKeyWindow();
        let text = NSString::from_str(text);
        let app = NSApplication::sharedApplication(objc2::MainThreadMarker::new().unwrap());
        for kind in [NSEventType::KeyDown, NSEventType::KeyUp] {
            let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                kind, NSPoint::new(0., 0.), flags, objc2_foundation::NSProcessInfo::processInfo().systemUptime(), window.windowNumber(), None, &text, &text, repeat && kind == NSEventType::KeyDown, code,
            ).context("create native key event")?;
            // Queue through NSApplication, including local monitors and AppKit
            // key-equivalent dispatch, rather than calling WebKit directly.
            app.postEvent_atStart(&event, false);
        }
        Ok(())
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_scroll(&self, tab: TabId, x: f64, y: f64, dx: i32, dy: i32) -> Result<()> {
        crate::smoke_horizontal_scroll::scroll_view(self.get(tab)?.as_ref(), x, y, dx, dy)
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_click(&self, tab: TabId, modified: bool) -> Result<()> {
        self.test_click_at(tab, 60., 50., modified)
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_click_at(&self, tab: TabId, x: f64, y: f64, modified: bool) -> Result<()> {
        use objc2_app_kit::NSEventModifierFlags;
        self.native_test_click(
            tab,
            x,
            y,
            if modified {
                NSEventModifierFlags::Command
            } else {
                NSEventModifierFlags::empty()
            },
        )
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_option_click(&self, tab: TabId) -> Result<()> {
        self.native_test_click(tab, 60., 50., objc2_app_kit::NSEventModifierFlags::Option)
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_option_click_at(&self, tab: TabId, x: f64, y: f64) -> Result<()> {
        self.native_test_click(tab, x, y, objc2_app_kit::NSEventModifierFlags::Option)
    }
    #[cfg(target_os = "macos")]
    fn has_native_focus(&self, tab: TabId) -> Result<bool> {
        use objc2_app_kit::NSView;
        use wry::WebViewExtMacOS;
        let view = self.get(tab)?;
        Ok(view
            .ns_window()
            .firstResponder()
            .and_then(|responder| {
                responder
                    .downcast_ref::<NSView>()
                    .map(|responder| responder.isDescendantOf(&view.webview()))
            })
            .unwrap_or(false))
    }
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn set_test_appearance(&self, tab: TabId, dark: Option<bool>) -> Result<()> {
        use objc2_app_kit::{
            NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        };
        use wry::WebViewExtMacOS;
        // Window-local override exercises AppKit's real appearance notification
        // without changing the user's system preference. None restores OS mode.
        let appearance = dark.and_then(|dark| unsafe {
            NSAppearance::appearanceNamed(if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            })
        });
        self.get(tab)?
            .ns_window()
            .setAppearance(appearance.as_deref());
        Ok(())
    }
    #[cfg(feature = "smoke")]
    fn is_loaded(&self, tab: TabId) -> bool {
        self.loaded.borrow().contains(&tab)
    }
    #[cfg(feature = "smoke")]
    fn evaluate(
        &self,
        tab: TabId,
        script: &str,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<()> {
        self.get(tab)?
            .evaluate_script_with_callback(script, callback)
            .context("Native smoke evaluation")
    }
    #[cfg(feature = "smoke")]
    fn is_visible(&self, tab: TabId) -> bool {
        self.visible.borrow().contains(&tab)
    }

    fn ensure(&self, tab: TabId, url: &str, window: &Window, cx: &gpui::App) -> Result<()> {
        if self.views.borrow().contains_key(&tab) {
            return Ok(());
        }
        if !allowed_url(url) {
            bail!("Only HTTP, HTTPS, and about:blank pages are supported");
        }
        #[cfg(feature = "smoke")]
        if crate::smoke::hold_launch_view() {
            return Ok(());
        }
        if !self.protection.borrow().native.ready() {
            if let Some(error) = self.protection.borrow().native.error() {
                bail!("Native protection: {error}");
            }
            return Ok(());
        }
        self.create_view(tab, url, true, |builder| {
            builder.build_native_child(window, cx)
        })
    }

    fn capture_sleep(&self, tab: TabId, image: crate::drag_preview::Snapshot) -> Result<()> {
        crate::drag_preview::capture(self.get(tab)?.as_ref(), image)
    }
    fn unload(&self, tab: TabId) {
        #[cfg(target_os = "macos")]
        {
            if self.pip_owner.get() == Some(tab) {
                self.pip_owner.set(None);
            }
            self.pip_visible.borrow_mut().remove(&tab);
        }
        self.unsaved_zoom.borrow_mut().remove(&tab);
        #[cfg(target_os = "macos")]
        {
            self.pending_popups.borrow_mut().remove(&tab);
            self.popup_delegates.borrow_mut().remove(&tab);
        }
        if let Some(view) = self.views.borrow_mut().remove(&tab) {
            let _ = view.set_visible(false);
            if self.downloads_active() {
                #[cfg(target_os = "macos")]
                let _ = view.evaluate_script("for (const v of document.querySelectorAll('video')) if (v.webkitPresentationMode === 'picture-in-picture') v.webkitSetPresentationMode('inline')");
                self.retired_views.borrow_mut().push(view);
            }
        }
        self.visible.borrow_mut().remove(&tab);
        self.loaded.borrow_mut().remove(&tab);
    }
    fn probe_sleep(&self, tab: TabId, callback: Box<dyn Fn(String) + Send>) -> Result<()> {
        self.get(tab)?.evaluate_script_with_callback(
            "window.__axolotlSleepBusy ? window.__axolotlSleepBusy() : true",
            callback,
        )?;
        Ok(())
    }
    fn focused_page(&self) -> Option<TabId> {
        let ids: Vec<_> = self.visible.borrow().iter().copied().collect();
        ids.into_iter().find(|id| {
            #[cfg(target_os = "macos")]
            {
                self.has_native_focus(*id).unwrap_or(false)
            }
            #[cfg(target_os = "windows")]
            {
                self.get(*id).is_ok_and(|view| view.has_focus())
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
                let _ = id;
                false
            }
        })
    }

    fn page_tool(&self, tab: TabId, operation: crate::page_tools::Operation) -> Result<()> {
        use crate::page_tools::{Operation, hostname};
        let view = self.get(tab)?;
        if operation == Operation::Find {
            view.focus()?;
            view.evaluate_script(&crate::icons::html(include_str!("../assets/page-find.js")))?;
            return Ok(());
        }
        let url = view.url()?;
        let value = if hostname(&url).is_some() {
            self.site_zoom.borrow_mut().change(&url, operation)?
        } else {
            // Internal documents have no website key: keep their zoom per view.
            let mut levels = self.unsaved_zoom.borrow_mut();
            let current = levels.get(&tab).copied().unwrap_or(1.);
            let value = crate::page_tools::next_zoom(current, operation);
            levels.insert(tab, value);
            value
        };
        view.zoom(value)?;
        if let Some(host) = hostname(&url) {
            for (id, sibling) in self.views.borrow().iter() {
                if *id != tab
                    && sibling.url().ok().and_then(|u| hostname(&u)).as_ref() == Some(&host)
                {
                    sibling.zoom(value)?;
                }
            }
        }
        Ok(())
    }

    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn test_zoom(&self, tab: TabId) -> Result<f64> {
        use wry::WebViewExtMacOS;
        // The test runs on the UI thread; supported macOS targets provide pageZoom.
        Ok(unsafe { self.get(tab)?.webview().pageZoom() })
    }

    fn has_view(&self, tab: TabId) -> bool {
        self.views.borrow().contains_key(&tab)
    }

    fn popup_transition(
        &self,
        tab: TabId,
        opening: bool,
        initial: bool,
        duration: std::time::Duration,
    ) -> Result<()> {
        crate::popup_transition::animate(self.get(tab)?.as_ref(), opening, initial, duration)
    }

    fn reset_popup_transition(&self, tab: TabId) -> Result<()> {
        if let Ok(view) = self.get(tab) {
            crate::popup_transition::reset(&view)?;
        }
        Ok(())
    }

    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn popup_animation_state(&self, tab: TabId) -> Result<serde_json::Value> {
        Ok(crate::popup_transition::state(self.get(tab)?.as_ref()))
    }

    fn rekey(&self, from: TabId, to: TabId) -> Result<()> {
        let mut views = self.views.borrow_mut();
        if views.contains_key(&to) {
            bail!("Destination browser view already exists");
        }
        let view = views
            .remove(&from)
            .context("Source browser view is unavailable")?;
        let identity = self
            .identities
            .borrow_mut()
            .remove(&from)
            .expect("view identity");
        #[cfg(target_os = "macos")]
        {
            self.pending_popups.borrow_mut().remove(&from);
            let mut delegates = self.popup_delegates.borrow_mut();
            if let Some(delegate) = delegates.remove(&from) {
                delegates.insert(to, delegate);
            }
        }
        #[cfg(target_os = "macos")]
        {
            if self.pip_owner.get() == Some(from) {
                self.pip_owner.set(Some(to));
            }
            let mut visible = self.pip_visible.borrow_mut();
            if visible.remove(&from) {
                visible.insert(to);
            }
        }
        identity.store(to, Ordering::Relaxed);
        self.identities.borrow_mut().insert(to, identity);
        views.insert(to, view);
        let level = self.unsaved_zoom.borrow_mut().remove(&from);
        if let Some(level) = level {
            self.unsaved_zoom.borrow_mut().insert(to, level);
        }
        let revision = self.protection_revisions.borrow_mut().remove(&from);
        if let Some(revision) = revision {
            self.protection_revisions.borrow_mut().insert(to, revision);
        }
        for ids in [&self.visible, &self.loaded] {
            let mut ids = ids.borrow_mut();
            if ids.remove(&from) {
                ids.insert(to);
            }
        }
        Ok(())
    }

    fn set_panel_corners(&self, tab: TabId, radius: f32, bottom_only: bool) -> Result<()> {
        let view = self.get(tab)?;
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS;
            if let Some(layer) = view.webview().layer() {
                use objc2_quartz_core::CACornerMask;
                let corners = if bottom_only {
                    if view.webview().isFlipped() {
                        CACornerMask::LayerMinXMaxYCorner | CACornerMask::LayerMaxXMaxYCorner
                    } else {
                        CACornerMask::LayerMinXMinYCorner | CACornerMask::LayerMaxXMinYCorner
                    }
                } else {
                    CACornerMask::all()
                };
                if layer.cornerRadius() != f64::from(radius) {
                    layer.setCornerRadius(f64::from(radius));
                }
                if layer.maskedCorners() != corners {
                    layer.setMaskedCorners(corners);
                }
            }
        }
        #[cfg(target_os = "windows")]
        view.set_panel_corner_shape(radius, bottom_only)?;
        Ok(())
    }

    fn place(&self, tab: TabId, bounds: Bounds<Pixels>) -> Result<()> {
        self.get(tab)?
            .set_bounds(native_bounds(bounds))
            .context("Positioning browser view")
    }

    #[cfg(all(feature = "smoke", target_os = "macos"))]
    fn panel_clip_state(&self, tab: TabId) -> Result<serde_json::Value> {
        use wry::WebViewExtMacOS;
        let view = self.get(tab)?;
        let native = view.webview();
        let layer = native.layer().context("Missing panel layer")?;
        let frame = native.frame();
        Ok(serde_json::json!({
            "radius": layer.cornerRadius(),
            "flipped": native.isFlipped(),
            "x": frame.origin.x,
            "y": frame.origin.y,
            "clips": layer.masksToBounds(),
            "corners": layer.maskedCorners().0,
            "width": frame.size.width,
            "height": frame.size.height,
        }))
    }

    #[cfg(all(feature = "smoke", target_os = "windows"))]
    fn panel_clip_state(&self, tab: TabId) -> Result<serde_json::Value> {
        self.get(tab)?.test_panel_clip_state()
    }

    #[cfg(target_os = "macos")]
    fn auto_pip_visibility(&self, visible: HashSet<TabId>) {
        let previous = {
            let mut previous = self.pip_visible.borrow_mut();
            if *previous == visible {
                return;
            }
            std::mem::replace(&mut *previous, visible.clone())
        };
        // Showing a tab never exits PiP. A dismissal cannot retry until another hide.
        if self.pip_owner.get().is_none() {
            *self.pip_pending.borrow_mut() = previous.difference(&visible).copied().collect();
            self.request_next_pip();
        }
    }

    fn show(&self, tab: TabId, visible: bool) -> Result<()> {
        let Some(view) = self.views.borrow().get(&tab).cloned() else {
            // Inactive tabs are represented in the model before lazy creation.
            if !visible {
                return Ok(());
            }
            bail!("Browser view for tab {tab} is unavailable");
        };
        let mut shown = self.visible.borrow_mut();
        if shown.contains(&tab) == visible {
            return Ok(());
        }
        view.set_visible(visible)
            .context("Changing browser visibility")?;
        if visible {
            shown.insert(tab);
        } else {
            shown.remove(&tab);
        }
        Ok(())
    }

    fn raise(&self, tab: TabId) -> Result<()> {
        let view = self.get(tab)?;
        crate::native_webview::raise(&view)
    }

    fn focus(&self, tab: TabId) -> Result<()> {
        self.get(tab)?.focus().context("Focusing browser view")
    }

    fn focus_parent(&self, tab: TabId) -> Result<()> {
        // An address action should still work before the first page was created.
        if let Some(view) = self.views.borrow().get(&tab) {
            view.focus_parent()
                .context("Returning focus to browser controls")?;
        }
        Ok(())
    }

    fn navigate(&self, tab: TabId, url: &str) -> Result<()> {
        if !allowed_url(url) {
            bail!("Only HTTP, HTTPS, and about:blank pages are supported");
        }
        self.loaded.borrow_mut().remove(&tab);
        if url == "about:blank" {
            return self
                .get(tab)?
                .load_html(&crate::icons::html(START_HTML))
                .context("Opening start page");
        }
        self.get(tab)?
            .load_url(url)
            .context("Navigating browser view")
    }

    fn back(&self, tab: TabId) -> Result<()> {
        self.get(tab)?
            .evaluate_script("history.back()")
            .context("Navigating back")
    }

    fn forward(&self, tab: TabId) -> Result<()> {
        self.get(tab)?
            .evaluate_script("history.forward()")
            .context("Navigating forward")
    }

    fn reload(&self, tab: TabId) -> Result<()> {
        self.get(tab)?.reload().context("Reloading browser view")
    }

    fn probe_history(&self, tab: TabId) -> Result<()> {
        let sender = self.sender.clone();
        let identity = self
            .identities
            .borrow()
            .get(&tab)
            .cloned()
            .context("Browser view identity unavailable")?;
        self.get(tab)?
            .evaluate_script_with_callback(HISTORY_PROBE, move |value| {
                let (back, forward) = parse_history(&value);
                let _ = sender.try_send(BrowserEvent::History(
                    identity.load(Ordering::Relaxed),
                    back,
                    forward,
                ));
            })
            .context("Reading navigation history")
    }

    fn current_url(&self, tab: TabId) -> Result<String> {
        let view = self.get(tab)?;
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS;
            // Wry 0.55 unwraps URL(), which is nil before the first document
            // commits (including failed initial navigations of a lazy tab).
            unsafe { view.webview().URL().and_then(|url| url.absoluteString()) }
                .map(|url| url.to_string())
                .context("Browser has no committed URL yet")
        }
        #[cfg(not(target_os = "macos"))]
        view.url().context("Reading browser URL")
    }

    #[cfg(target_os = "macos")]
    fn show_copy_url_toast(&self, tab: TabId) -> Result<()> {
        self.get(tab)?
            .evaluate_script(include_str!("../assets/copy-url-toast.js"))
            .context("Showing URL copied toast")
    }

    fn retain(&self, tabs: &HashSet<TabId>) {
        #[cfg(target_os = "macos")]
        {
            if self.pip_owner.get().is_some_and(|id| !tabs.contains(&id)) {
                self.pip_owner.set(None);
            }
            self.pip_visible.borrow_mut().retain(|id| tabs.contains(id));
        }
        #[cfg(target_os = "macos")]
        let tabs = &tabs
            .union(&self.pending_popups.borrow())
            .copied()
            .collect::<HashSet<_>>();
        #[cfg(target_os = "macos")]
        self.popup_delegates
            .borrow_mut()
            .retain(|id, _| tabs.contains(id));
        self.protection_revisions
            .borrow_mut()
            .retain(|id, _| tabs.contains(id));
        self.identities
            .borrow_mut()
            .retain(|id, _| tabs.contains(id));
        self.unsaved_zoom
            .borrow_mut()
            .retain(|id, _| tabs.contains(id));
        self.visible.borrow_mut().retain(|id| tabs.contains(id));
        self.loaded.borrow_mut().retain(|id| tabs.contains(id));
        self.views.borrow_mut().retain(|id, view| {
            if tabs.contains(id) {
                return true;
            }
            // Geometry callbacks may briefly retain an Rc until the next frame;
            // hide immediately so a closed tab can never cover the replacement.
            if let Err(error) = view.set_visible(false) {
                eprintln!("Could not hide closed browser view: {error}");
            }
            if self.downloads_active() {
                #[cfg(target_os = "macos")]
                let _ = view.evaluate_script("for (const v of document.querySelectorAll('video')) if (v.webkitPresentationMode === 'picture-in-picture') v.webkitSetPresentationMode('inline')");
                self.retired_views.borrow_mut().push(view.clone());
            }
            false
        });
    }

    fn drag_preview(
        &self,
        tab: TabId,
        title: &str,
        window: &Window,
        cx: &gpui::App,
    ) -> Result<crate::drag_preview::DragPreview> {
        let source = self.views.borrow().get(&tab).cloned();
        let loaded = self.loaded.clone();
        crate::drag_preview::DragPreview::new(
            source,
            move || loaded.borrow().contains(&tab),
            title,
            window,
            cx,
        )
    }

    fn request_redraw(&self) {
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS;
            if let Some(view) = self.views.borrow().values().next()
                && let Some(parent) = unsafe { view.webview().superview() }
            {
                // AppKit can suspend GPUI's display link when native children
                // cover its layer. Invalidate that layer through AppKit so its
                // normal displayLayer callback draws and presents the scene.
                parent.setNeedsDisplay(true);
                if let Some(layer) = parent.layer() {
                    layer.setNeedsDisplay();
                }
            }
        }
    }

    fn hide_all(&self) -> Result<()> {
        let mut failure = None;
        let visible: Vec<_> = self.visible.borrow().iter().copied().collect();
        for tab in visible {
            if let Err(error) = self.show(tab, false) {
                failure = Some(error);
            }
        }
        if let Some(error) = failure {
            return Err(error).context("Hiding browser views");
        }
        Ok(())
    }

    fn drain_events(&self) -> Vec<BrowserEvent> {
        // Downloads outlive closing/sleeping their originating tab or preview.
        // Hidden retired engines are released as soon as the transfers end.
        if !self.downloads_active() {
            self.retired_views.borrow_mut().clear();
        }
        if let Err(error) = self.protection.borrow_mut().poll() {
            eprintln!("Protection: {error}");
        }
        let p = self.protection.borrow();
        if p.native.ready() {
            let needs: Vec<_> = self
                .protection_revisions
                .borrow()
                .iter()
                .filter(|(_, r)| r.0 != p.revision)
                .map(|(id, r)| (*id, r.1.clone()))
                .collect();
            if !needs.is_empty() {
                let script = p.rules.script(&p.policy);
                for (tab, previous_policy) in needs {
                    // Native rules/scripts are shared, but a site switch must
                    // reload only documents whose effective protection changed.
                    let url = self.current_url(tab).unwrap_or_default();
                    let reload = previous_policy.allows(&url) != p.policy.allows(&url);
                    #[cfg(feature = "smoke")]
                    eprintln!(
                        "Protection: applying revision {} policy {} to tab {tab}; reload={reload}",
                        p.revision, p.policy_revision
                    );
                    if let Ok(view) = self.get(tab) {
                        #[cfg(target_os = "macos")]
                        let result = p.native.apply(&view, &script).and_then(|_| {
                            if reload {
                                view.reload().map_err(Into::into)
                            } else {
                                Ok(())
                            }
                        });
                        #[cfg(target_os = "windows")]
                        let result = view.setup_protection(
                            self.protection.clone(),
                            script.clone(),
                            String::new(),
                            false,
                            reload,
                        );
                        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                        let result: Result<()> = Ok(());
                        match result {
                            Ok(()) => {
                                self.protection_revisions
                                    .borrow_mut()
                                    .insert(tab, (p.revision, p.policy.clone()));
                            }
                            Err(error) => eprintln!("Applying protection: {error}"),
                        }
                    }
                }
            }
        }

        let mut events: Vec<_> = self.events.try_iter().collect();
        // Apply for every engine, including transient previews and native popups,
        // before the UI's specialized event handlers consume their navigation.
        for event in &events {
            if let BrowserEvent::Navigated(tab, url) = event
                && let Ok(view) = self.get(*tab)
            {
                self.unsaved_zoom.borrow_mut().remove(tab);
                let value = self.site_zoom.borrow().get(url);
                if let Err(error) = view.zoom(value) {
                    eprintln!("Applying website zoom: {error}");
                }
            }
        }

        let state = format!("{}:{}:{}", p.revision, p.native.ready(), p.value());
        if *self.protection_notified.borrow() != state {
            *self.protection_notified.borrow_mut() = state;
            events.push(BrowserEvent::ProtectionChanged);
        }
        events
    }
}

impl Drop for WryHost {
    fn drop(&mut self) {
        if Rc::strong_count(&self.0) == 1 {
            let _ = self.hide_all();
        }
    }
}

pub fn native_bounds(bounds: Bounds<Pixels>) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(
            f64::from(bounds.origin.x),
            f64::from(bounds.origin.y),
        )
        .into(),
        size: wry::dpi::LogicalSize::new(
            f64::from(bounds.size.width).max(1.0),
            f64::from(bounds.size.height).max(1.0),
        )
        .into(),
    }
}

pub(crate) fn allowed_url(url: &str) -> bool {
    let normalized = url.to_ascii_lowercase();
    normalized.starts_with("https://")
        || normalized.starts_with("http://")
        || normalized == "about:blank"
}

/// Additional document types for embedded frames only. Never use this for
/// address entry, new windows, privileged inline UI, or a top-level target.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn embedded_document_url(url: &str) -> bool {
    let normalized = url.to_ascii_lowercase();
    normalized == "about:srcdoc"
        || normalized.starts_with("about:srcdoc#")
        || normalized.starts_with("blob:")
        || normalized.starts_with("data:")
}

pub(crate) fn authenticated_command<'a>(message: &'a str, nonce: &str) -> Option<&'a str> {
    let (token, command) = message.split_once(':')?;
    if token != nonce {
        return None;
    }
    match command {
        "focus" | "settings" | "address" | "new-tab" | "new-space" | "escape" | "close-tab"
        | "reload" | "swipe-next" | "swipe-prev" | "history-back" | "history-forward"
        | "zoom-in" | "zoom-out" | "zoom-reset" | "find-page" => Some(command),
        _ => None,
    }
}

pub(crate) fn authenticated_link<'a>(message: &'a str, nonce: &str) -> Option<&'a str> {
    let (token, command) = message.split_once(':')?;
    if token != nonce {
        return None;
    }
    command
        .strip_prefix("open-link:")
        .filter(|url| allowed_url(url))
}

pub(crate) fn authenticated_popup_link<'a>(message: &'a str, nonce: &str) -> Option<&'a str> {
    let (token, command) = message.split_once(':')?;
    if token != nonce {
        return None;
    }
    command
        .strip_prefix("popup-link:")
        .filter(|url| allowed_url(url))
}

// The nonce stays inside an initialization-script closure and the original IPC
// function is captured before page scripts can replace it. Only trusted native
// input events produce authenticated browser commands. No privileged RPC exists.
pub(crate) const INPUT_BRIDGE: &str = include_str!("../assets/input-bridge.js");
