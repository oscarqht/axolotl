//! The desktop browser boundary. Wry owns native pages; the workspace owns tabs.
//!
//! Native handles stay on the UI thread; callbacks enqueue events to avoid reentrant updates.

use crate::native_webview::{BuildNativeChild, NativeWebView as WebView};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::mpsc::{self, Receiver, SyncSender},
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
    Focused(TabId),
    Favicon(TabId, String),
    Notification(TabId),
    Navigated(TabId, String),
    Title(TabId, String),
    Loaded(TabId),
    NewTab(TabId, String),
    OpenLink(TabId, String),
    Shortcut(TabId, String),
    Gesture(TabId, f64, f64),
    /// Whether the page can navigate back/forward, from a context-menu probe.
    History(TabId, bool, bool),
}

/// The operations a replacement engine must provide. Browser state is never
/// serialized into the layout core, so a future mobile adapter can share it.
pub trait BrowserHost {
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
    fn ensure(&self, tab: TabId, url: &str, window: &Window, cx: &gpui::App) -> Result<()>;
    fn has_view(&self, tab: TabId) -> bool;
    fn place(&self, tab: TabId, bounds: Bounds<Pixels>) -> Result<()>;
    fn show(&self, tab: TabId, visible: bool) -> Result<()>;
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

pub struct WryHost {
    views: RefCell<HashMap<TabId, Rc<WebView>>>,
    events: Receiver<BrowserEvent>,
    sender: SyncSender<BrowserEvent>,
    visible: Rc<RefCell<HashSet<TabId>>>,
    loaded: Rc<RefCell<HashSet<TabId>>>,
}

impl Default for WryHost {
    fn default() -> Self {
        // Untrusted pages cannot cause unbounded allocation by flooding IPC.
        let (sender, events) = mpsc::sync_channel(512);
        Self {
            views: RefCell::new(HashMap::new()),
            events,
            sender,
            visible: Rc::new(RefCell::new(HashSet::new())),
            loaded: Rc::new(RefCell::new(HashSet::new())),
        }
    }
}

impl WryHost {
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, tab: TabId) -> Result<Rc<WebView>> {
        self.views
            .borrow()
            .get(&tab)
            .cloned()
            .with_context(|| format!("Browser view for tab {tab} is unavailable"))
    }
}

impl BrowserHost for WryHost {
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
    fn test_click(&self, tab: TabId, modified: bool) -> Result<()> {
        use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
        use objc2_foundation::NSPoint;
        use wry::WebViewExtMacOS;
        let view = self.get(tab)?;
        let webview = view.webview();
        let window = view.ns_window();
        let point = NSPoint::new(
            60.,
            if webview.isFlipped() {
                50.
            } else {
                webview.frame().size.height - 50.
            },
        );
        let point = webview.convertPoint_toView(point, None);
        let flags = if modified {
            NSEventModifierFlags::Command
        } else {
            NSEventModifierFlags::empty()
        };
        for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
            let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind, point, flags, 0., window.windowNumber(), None, 0, 1, 1.,
            ).context("create native click event")?;
            // NSWindow hit testing delivers a genuine WebKit input event, not a
            // JavaScript dispatchEvent (which the bridge intentionally rejects).
            window.sendEvent(&event);
        }
        Ok(())
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
        let ipc = self.sender.clone();
        let title = self.sender.clone();
        let load = self.sender.clone();
        let loaded = self.loaded.clone();
        let popup = self.sender.clone();
        let visible = self.visible.clone();
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
        let start_url = html_data_url(START_HTML);
        let builder = WebViewBuilder::new()
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
            .with_initialization_script_for_main_only(notifications, false)
            .with_navigation_handler(move |url| {
                allowed_url(&url) || (cfg!(target_os = "windows") && url == start_url)
            })
            .with_ipc_handler(move |request| {
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
                if let Some(url) = authenticated_link(request.body(), &nonce) {
                    let _ = ipc.try_send(BrowserEvent::OpenLink(tab, url.to_owned()));
                    return;
                }
                let Some(command) = authenticated_command(request.body(), &nonce) else {
                    return;
                };
                let event = match command {
                    "focus" => BrowserEvent::Focused(tab),
                    "escape" | "settings" | "address" | "new-tab" | "close-tab" | "reload"
                    | "swipe-next" | "swipe-prev" => {
                        BrowserEvent::Shortcut(tab, command.to_owned())
                    }
                    _ => return,
                };
                let _ = ipc.try_send(event);
            })
            .with_document_title_changed_handler(move |value| {
                let _ = title.try_send(BrowserEvent::Title(tab, value));
            })
            .with_on_page_load_handler(move |event, url| {
                if matches!(event, PageLoadEvent::Finished) {
                    loaded.borrow_mut().insert(tab);
                } else {
                    loaded.borrow_mut().remove(&tab);
                }
                let _ = load.try_send(BrowserEvent::Navigated(tab, url));
                if matches!(event, PageLoadEvent::Finished) {
                    let _ = load.try_send(BrowserEvent::Loaded(tab));
                }
            })
            .with_new_window_req_handler(move |url, _| {
                if allowed_url(&url) {
                    let _ = popup.try_send(BrowserEvent::NewTab(tab, url));
                }
                // The shell creates a normal tab, never a second native window.
                NewWindowResponse::Deny
            })
            // Download management belongs to a later browser milestone. Deny
            // here rather than writing files without a visible download flow.
            .with_download_started_handler(|_, _| false);
        // Set the native UA before the first request, so HTTP and JavaScript
        // detection agree on initial loads, redirects, frames and new tabs.
        #[cfg(target_os = "macos")]
        let builder = if let Some(agent) = crate::browser_identity::user_agent() {
            builder.with_user_agent(agent)
        } else {
            builder
        };
        let builder = if url == "about:blank" {
            builder.with_html(include_str!("../assets/start.html"))
        } else {
            builder.with_url(url)
        };
        let view = builder
            .build_native_child(window, cx)
            .context("Creating native browser child view")?;
        // `with_focused(false)` is ignored on macOS; explicitly return focus.
        view.focus_parent()
            .context("Returning focus from new browser view")?;
        self.views.borrow_mut().insert(tab, Rc::new(view));
        Ok(())
    }

    fn has_view(&self, tab: TabId) -> bool {
        self.views.borrow().contains_key(&tab)
    }

    fn place(&self, tab: TabId, bounds: Bounds<Pixels>) -> Result<()> {
        self.get(tab)?
            .set_bounds(native_bounds(bounds))
            .context("Positioning browser view")
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
                .load_html(include_str!("../assets/start.html"))
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
        self.get(tab)?
            .evaluate_script_with_callback(HISTORY_PROBE, move |value| {
                let (back, forward) = parse_history(&value);
                let _ = sender.try_send(BrowserEvent::History(tab, back, forward));
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

    fn retain(&self, tabs: &HashSet<TabId>) {
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
        self.events.try_iter().collect()
    }
}

impl Drop for WryHost {
    fn drop(&mut self) {
        let _ = self.hide_all();
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

pub(crate) fn authenticated_command<'a>(message: &'a str, nonce: &str) -> Option<&'a str> {
    let (token, command) = message.split_once(':')?;
    if token != nonce {
        return None;
    }
    match command {
        "escape" | "focus" | "settings" | "address" | "new-tab" | "close-tab" | "reload"
        | "swipe-next" | "swipe-prev" => Some(command),
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

// The nonce stays inside an initialization-script closure and the original IPC
// function is captured before page scripts can replace it. Only trusted native
// input events produce authenticated browser commands. No privileged RPC exists.
pub(crate) const INPUT_BRIDGE: &str = include_str!("../assets/input-bridge.js");
