//! A small native surface keeps the floating controls above WKWebView/WebView2.
use crate::native_webview::{BuildNativeChild, NativeWebView};
use anyhow::Result;
use gpui::{Bounds, Window, point, px, size};
use serde_json::Value;
use std::{
    cell::RefCell,
    sync::mpsc::{self, Receiver},
};
use workspace_core::Rect;
use wry::{NewWindowResponse, WebViewBuilder};

/// Room above the bar for six menu items, three dividers, padding and gap.
pub const MENU_HEIGHT: f32 = 224.;
pub const MENU_WIDTH: f32 = 200.;

pub struct FloatingBar {
    view: NativeWebView,
    events: Receiver<Value>,
    state: RefCell<String>,
    pub bounds: Rect,
    pub expanded: bool,
    pub transitioning: bool,
    /// A tab context menu is open; the native surface grows upward to hold it.
    pub menu_open: bool,
    expanded_width: f32,
    pub midpoints: Vec<f32>,
}
impl FloatingBar {
    pub fn new(window: &Window, cx: &gpui::App) -> Result<Self> {
        let (sender, events) = mpsc::channel();
        let view = crate::browser::with_inline_html(
            WebViewBuilder::new()
                .with_transparent(true)
                .with_focused(false)
                .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
                .with_ipc_handler(move |request| {
                    if let Ok(value) = serde_json::from_str(request.body()) {
                        let _ = sender.send(value);
                    }
                }),
            include_str!("../assets/floating-bar.html").replace(
                "__FLOATING_SCRIPT__",
                include_str!("../assets/floating-bar.js"),
            ),
        )
        .build_native_child(window, cx)?;
        view.focus_parent()?;
        Ok(Self {
            view,
            events,
            state: RefCell::new(String::new()),
            bounds: Rect::new(0., 0., 1., 1.),
            expanded: false,
            transitioning: false,
            menu_open: false,
            expanded_width: 144.,
            midpoints: vec![],
        })
    }
    pub(crate) fn raise(&self) -> Result<()> {
        crate::native_webview::raise(&self.view)
    }

    pub fn place(&mut self, pane: Rect, mut state: Value) -> Result<()> {
        let editing = !state["editing"].is_null();
        let available = (pane.width - 24.).max(1.);
        // Chip width 172 + gap 4, fixed controls/padding 78, native inset 8.
        let count = state["tabs"].as_array().map_or(0, Vec::len);
        let expanded_width =
            (86. + 176. * count as f32 + if editing { 148. } else { 0. }).min(available);
        let open = self.expanded || editing;
        if open && (expanded_width - self.expanded_width).abs() > 0.1 {
            self.transitioning = true;
        }
        self.expanded_width = expanded_width;
        let target = if open {
            expanded_width
        } else {
            144_f32.min(pane.width)
        };
        // Retain room for the old pill while collapsing/removing chips or
        // leaving address editing; shrink the child only after CSS settles.
        let mut width = if self.transitioning {
            self.bounds.width.max(target).min(available)
        } else {
            target
        };
        let bar_height = 48_f32.min(pane.height);
        let bottom = pane.y + (pane.height - 60.).max(0.) + bar_height;
        let mut top = bottom - bar_height;
        if self.menu_open {
            width = width.max(MENU_WIDTH).min(pane.width);
            top = (bottom - bar_height - MENU_HEIGHT).max(pane.y);
        }
        self.bounds = Rect::new(pane.x + (pane.width - width) / 2., top, width, bottom - top);
        let r = self.bounds;
        self.view.set_bounds(crate::browser::native_bounds(Bounds {
            origin: point(px(r.x), px(r.y)),
            size: size(px(r.width), px(r.height)),
        }))?;
        state["menu"] = serde_json::json!(self.menu_open);
        state["surfaceWidth"] = serde_json::json!(width);
        state["expandedWidth"] = serde_json::json!(expanded_width);
        let serialized = state.to_string();
        if *self.state.borrow() != serialized {
            self.view
                .evaluate_script(&format!("window.updateBar({serialized})"))?;
            *self.state.borrow_mut() = serialized;
        }
        Ok(())
    }
    /// Deliver native events to the originating WKWebView, including releases
    /// outside its bounds. The fixture supplies OS button state separately.
    #[cfg(all(feature = "smoke", target_os = "macos"))]
    pub fn test_mouse(&self, kind: objc2_app_kit::NSEventType, x: f64, y: f64) -> Result<()> {
        use objc2_app_kit::{NSEvent, NSEventModifierFlags};
        use objc2_foundation::NSPoint;
        use wry::WebViewExtMacOS;
        let window = self.view.ns_window();
        if kind == objc2_app_kit::NSEventType::LeftMouseDown {
            window.makeKeyWindow();
            self.view.focus()?;
        }
        let root = window.contentView().unwrap();
        let location = NSPoint::new(
            x,
            if root.isFlipped() {
                y
            } else {
                root.frame().size.height - y
            },
        );
        let location = root.convertPoint_toView(location, None);
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind, location, NSEventModifierFlags::empty(), objc2_foundation::NSProcessInfo::processInfo().systemUptime(), window.windowNumber(), None, 0, 1, 1.,
        ).ok_or_else(|| anyhow::anyhow!("create native drag event"))?;
        match kind {
            objc2_app_kit::NSEventType::LeftMouseDown => self.view.webview().mouseDown(&event),
            objc2_app_kit::NSEventType::LeftMouseDragged => {
                self.view.webview().mouseDragged(&event)
            }
            objc2_app_kit::NSEventType::LeftMouseUp => self.view.webview().mouseUp(&event),
            _ => window.sendEvent(&event),
        }
        Ok(())
    }
    pub fn invalidate(&self) {
        self.state.borrow_mut().clear();
    }
    pub fn focus_editor(&self) -> Result<()> {
        self.view.focus()?;
        self.view.evaluate_script("if(document.activeElement?.tagName!=='INPUT'){const input=document.querySelector('input');input?.focus();input?.select()}")?;
        Ok(())
    }
    pub fn next_event(&self) -> Option<Value> {
        self.events.try_recv().ok()
    }
    pub fn preview(&self, index: Option<usize>) -> Result<()> {
        self.view
            .evaluate_script(&format!("window.previewSlot({})", serde_json::json!(index)))?;
        Ok(())
    }
    #[cfg(feature = "smoke")]
    pub fn evaluate(&self, script: &str, callback: impl Fn(String) + Send + 'static) -> Result<()> {
        self.view.evaluate_script_with_callback(script, callback)?;
        Ok(())
    }
}
impl Drop for FloatingBar {
    fn drop(&mut self) {
        let _ = self.view.set_visible(false);
    }
}
