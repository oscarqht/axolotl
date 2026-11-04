//! The desktop browser boundary. Wry owns native pages; the workspace owns tabs.
//!
//! All methods and native handles stay on GPUI's UI thread. Callbacks only enqueue
//! events, avoiding reentrant GPUI entity updates from WebKit/WebView2 callbacks.

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

fn html_data_url(html: &str) -> String {
    use base64::Engine as _;
    format!(
        "data:text/html;charset=utf-8;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(html)
    )
}

fn inline_navigation(url: &str, expected: &str) -> bool {
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
    Navigated(TabId, String),
    Title(TabId, String),
    Loaded(TabId),
    NewTab(TabId, String),
    Shortcut(TabId, String),
}

/// The operations a replacement engine must provide. Browser state is never
/// serialized into the layout core, so a future mobile adapter can share it.
pub trait BrowserHost {
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
    fn focus(&self, tab: TabId) -> Result<()>;
    fn focus_parent(&self, tab: TabId) -> Result<()>;
    fn navigate(&self, tab: TabId, url: &str) -> Result<()>;
    fn back(&self, tab: TabId) -> Result<()>;
    fn forward(&self, tab: TabId) -> Result<()>;
    fn reload(&self, tab: TabId) -> Result<()>;
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
        let input_bridge = INPUT_BRIDGE.replace("__NONCE__", &nonce);
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
            .with_initialization_script(input_bridge)
            .with_navigation_handler(move |url| {
                allowed_url(&url) || (cfg!(target_os = "windows") && url == start_url)
            })
            .with_ipc_handler(move |request| {
                if !visible.borrow().contains(&tab) {
                    return;
                }
                let Some(command) = authenticated_command(request.body(), &nonce) else {
                    return;
                };
                let event = match command {
                    "focus" => BrowserEvent::Focused(tab),
                    "address" | "new-tab" | "close-tab" | "reload" => {
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

    fn current_url(&self, tab: TabId) -> Result<String> {
        self.get(tab)?.url().context("Reading browser URL")
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

fn allowed_url(url: &str) -> bool {
    let normalized = url.to_ascii_lowercase();
    normalized.starts_with("https://")
        || normalized.starts_with("http://")
        || normalized == "about:blank"
}

fn authenticated_command<'a>(message: &'a str, nonce: &str) -> Option<&'a str> {
    let (token, command) = message.split_once(':')?;
    if token != nonce {
        return None;
    }
    match command {
        "focus" | "address" | "new-tab" | "close-tab" | "reload" => Some(command),
        _ => None,
    }
}

// The nonce stays inside an initialization-script closure and the original IPC
// function is captured before page scripts can replace it. Only trusted native
// input events produce authenticated browser commands. No privileged RPC exists.
const INPUT_BRIDGE: &str = r#"
(() => {
    const post = window.ipc.postMessage.bind(window.ipc);
    const nonce = '__NONCE__';
    const send = value => post(nonce + ':' + value);
    window.addEventListener('pointerdown', event => {
        if (event.isTrusted) send('focus');
    }, true);
    window.addEventListener('focus', event => {
        if (event.isTrusted) send('focus');
    });
    window.addEventListener('keydown', event => {
        if (!event.isTrusted || !(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
        const command = {l:'address', t:'new-tab', w:'close-tab', r:'reload'}[event.key.toLowerCase()];
        if (command) { event.preventDefault(); event.stopImmediatePropagation(); send(command); }
    }, true);
})();
"#;

#[cfg(test)]
mod tests {
    use super::{allowed_url, authenticated_command};

    #[test]
    fn inline_navigation_allows_only_the_supplied_html() {
        let expected = super::html_data_url("<h1>Trusted overlay</h1>");
        assert!(super::inline_navigation("about:blank", &expected));
        assert_eq!(
            super::inline_navigation(&expected, &expected),
            cfg!(target_os = "windows")
        );
        for url in [
            super::html_data_url("<script>window.ipc.postMessage('close')</script>"),
            "https://example.com".into(),
            "file:///C:/Windows/win.ini".into(),
            "javascript:alert(1)".into(),
        ] {
            assert!(!super::inline_navigation(&url, &expected));
        }
        // A permitted internal payload must not make data URLs public navigation.
        assert!(!allowed_url(&expected));
    }

    #[test]
    fn bridge_requires_nonce_and_fixed_command() {
        assert_eq!(
            authenticated_command("secret:address", "secret"),
            Some("address")
        );
        assert_eq!(
            authenticated_command("secret:focus", "secret"),
            Some("focus")
        );
        for message in [
            "address",
            "wrong:address",
            "secret:run-code",
            "secret:reload:extra",
            "secret:",
        ] {
            assert_eq!(authenticated_command(message, "secret"), None);
        }
    }

    #[test]
    fn navigation_never_allows_local_files_or_executable_schemes() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,a",
            "httpsx://test",
            "about:config",
        ] {
            assert!(!allowed_url(url), "allowed {url}");
        }
        for url in [
            "https://example.com",
            "http://localhost:3000",
            "about:blank",
            "HTTPS://example.com",
        ] {
            assert!(allowed_url(url), "denied {url}");
        }
    }
}
