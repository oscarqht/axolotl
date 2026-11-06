//! Window-wide pinned pages. The native shield is behind the raised, retained page;
//! it catches outside clicks. A small corner overlay keeps resizing above the live page.
use crate::{
    app::BrowserApp,
    native_webview::{BuildNativeChild, NativeWebView},
};
use gpui::*;
use serde_json::{Value, json};
use std::sync::mpsc::Receiver;
use workspace_core::{PaneId, Rect, TabId};

#[derive(Default)]
pub struct PinnedUi {
    pub open: Option<TabId>,
    pub menu: Option<Option<TabId>>,
    surface: Option<NativeWebView>,
    grip: Option<NativeWebView>,
    events: Option<Receiver<Value>>,
    state: String,
    generation: u64,
    focus_page: bool,
    resize_origin_x: Option<f32>,
    pub drag: Option<(TabId, Point<Pixels>, bool)>,
    pub suppress_click: bool,
}

pub fn header_layout(width: f32, count: usize) -> (usize, f32) {
    // Keep a full space chip, caption controls and an empty drag region usable.
    let capacity = (((width - 464.).max(0.) / 34.) as usize).max(1);
    let slots = count.min(capacity);
    let visible = if count > capacity { slots - 1 } else { slots };
    (
        visible,
        if slots == 0 {
            0.
        } else {
            slots as f32 * 34. - 4.
        },
    )
}

impl BrowserApp {
    pub fn pin_drop_target(&self, point: Point<Pixels>, window: &Window) -> bool {
        let x = f32::from(point.x);
        f32::from(point.y) >= 0.
            && f32::from(point.y) < crate::render::TOP
            && x >= if cfg!(target_os = "macos") { 84. } else { 8. }
            && x < f32::from(window.viewport_size().width)
                - if cfg!(target_os = "windows") {
                    138.
                } else {
                    0.
                }
    }

    pub fn pin_page(
        &mut self,
        pane: PaneId,
        tab: TabId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_edit(window, cx);
        if self.model.pin_tab(pane, tab) {
            self.sync_address(window, cx);
            self.host.request_redraw();
            cx.notify();
        }
    }

    pub fn dismiss_pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pins.open.is_none() && self.pins.menu.is_none() {
            return;
        }
        if let Some(id) = self.pins.open.take() {
            self.report(self.host.show(id, false));
            let _ = self.host.focus_parent(id);
        }
        // A popup page was raised above the panel controls. Restore their
        // sibling order before the same page can return to an ordinary pane.
        for bar in self.bars.values() {
            let _ = bar.raise();
        }
        if let Some(grip) = &self.pins.grip {
            let _ = grip.set_visible(false);
        }
        self.pins.menu = None;
        if let Some(surface) = &self.pins.surface {
            let _ = surface.set_visible(false);
        }
        self.pins.generation = self.pins.generation.wrapping_add(1);
        self.pins.state.clear();
        self.pins.resize_origin_x = None;
        self.focus.focus(window);
        cx.notify();
    }

    pub fn toggle_pin(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        self.close_downloads(window, cx);
        let closing = self.pins.open == Some(id);
        self.dismiss_pin(window, cx);
        self.close_space_menu(cx);
        self.close_settings(window, cx);
        if !closing {
            self.wake_tab(id);
            self.pins.open = Some(id);
            self.pins.focus_page = true;
            self.clear_tab_notifications(id);
        }
        cx.notify();
    }

    pub fn pin_menu(&mut self, tab: Option<TabId>, window: &mut Window, cx: &mut Context<Self>) {
        self.close_downloads(window, cx);
        self.dismiss_pin(window, cx);
        self.pins.menu = Some(tab);
        cx.notify();
    }

    pub fn pin_rect(&self, window: &Window) -> Option<Rect> {
        let id = self.pins.open?;
        let pin = self.model.pinned_tabs.iter().find(|p| p.tab.id == id)?;
        let size = window.viewport_size();
        let w = f32::from(size.width);
        let h = (f32::from(size.height) - crate::render::TOP).max(1.);
        let (visible, strip) = header_layout(w, self.model.pinned_tabs.len());
        let index = self
            .model
            .pinned_tabs
            .iter()
            .position(|p| p.tab.id == id)?
            .min(visible);
        let right = if cfg!(target_os = "windows") {
            210.
        } else {
            76.
        };
        let anchor = (w - right - strip + index as f32 * 34. + 30.).min(w - 4.);
        let width = pin.width.min((w - 8.).max(1.));
        let height = pin.height.min((h - 8.).max(1.));
        Some(Rect::new(
            self.pins
                .resize_origin_x
                .unwrap_or(anchor - width)
                .max(4.)
                .min((w - width - 4.).max(0.)),
            crate::render::TOP + 4.,
            width,
            height,
        ))
    }

    pub fn prepare_pins(&mut self, window: &Window, cx: &App) {
        if self.pins.open.is_none() && self.pins.menu.is_none() {
            return;
        }
        let result = (|| -> anyhow::Result<()> {
            if self.pins.surface.is_none() {
                let (sender, events) = crate::ui_events::channel();
                let grip_sender = sender.clone();
                let grip = crate::browser::with_inline_html(
                    WebViewBuilder::new()
                        .with_transparent(true)
                        .with_focused(false)
                        .with_visible(false)
                        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                        .with_ipc_handler(move |request| {
                            if let Ok(event) = serde_json::from_str(request.body()) {
                                let _ = grip_sender.send(event);
                            }
                        }),
                    crate::icons::html(include_str!("../assets/pinned-resize.html")),
                )
                .build_native_child(window, cx)?;
                let view = crate::browser::with_inline_html(
                    WebViewBuilder::new()
                        .with_transparent(true)
                        .with_focused(false)
                        .with_initialization_script(format!(
                            "window.spaceEmojiData={}",
                            include_str!("../assets/emoji-data.json")
                        ))
                        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                        .with_ipc_handler(move |request| {
                            if let Ok(event) = serde_json::from_str(request.body()) {
                                let _ = sender.send(event);
                            }
                        }),
                    crate::icons::html(include_str!("../assets/pinned-popup.html")),
                )
                .build_native_child(window, cx)?;
                self.pins.grip = Some(grip);
                self.pins.surface = Some(view);
                self.pins.events = Some(events);
            }
            let viewport = window.viewport_size();
            let view = self.pins.surface.as_ref().unwrap();
            if self.pins.state.is_empty() {
                view.set_visible(true)?;
            }
            view.set_bounds(crate::browser::native_bounds(Bounds {
                origin: point(px(0.), px(crate::render::TOP)),
                size: size(
                    viewport.width,
                    px((f32::from(viewport.height) - crate::render::TOP).max(1.)),
                ),
            }))?;
            crate::native_webview::raise(view)?;
            let rect = self.pin_rect(window);
            let state = json!({
                "generation": self.pins.generation,
                "rect": rect.map(|r| json!({"x":r.x,"y":r.y-crate::render::TOP,"width":r.width,"height":r.height})),
                "menu": self.pins.menu.is_some(),
                "unpin": self.pins.menu.flatten(),
                "keepAlive": self.pins.menu.flatten().is_some_and(|id| self.sleep.keep_alive(id)),
                "pins": self.model.pinned_tabs.iter().map(|p| json!({"id":p.tab.id,"title":p.tab.display_title(),"url":p.tab.url,"icon":p.icon,"sleeping":self.sleep.is_sleeping(p.tab.id)})).collect::<Vec<_>>()
            }).to_string();
            // Also push on ready: initialization can finish after the first evaluation.
            if state != self.pins.state {
                view.evaluate_script(&format!("window.updatePins?.({state})"))?;
                self.pins
                    .grip
                    .as_ref()
                    .unwrap()
                    .evaluate_script(&format!("window.updatePins?.({state})"))?;
                self.pins.state = state;
            }
            let grip = self.pins.grip.as_ref().unwrap();
            grip.set_visible(rect.is_some())?;
            if let (Some(id), Some(r)) = (self.pins.open, rect) {
                // The retained page may have had square corners in a floating panel.
                // Clip the native page itself, not just the shield behind it.
                self.host
                    .set_panel_corners(id, crate::render::PANEL_CORNER_RADIUS, false)?;
                self.host.place(
                    id,
                    Bounds {
                        origin: point(px(r.x), px(r.y)),
                        size: size(px(r.width), px(r.height)),
                    },
                )?;
                self.host.raise(id)?;
                grip.set_bounds(crate::browser::native_bounds(Bounds {
                    origin: point(px(r.x + r.width - 27.), px(r.y + r.height - 19.)),
                    size: size(px(26.), px(18.)),
                }))?;
                crate::native_webview::raise(grip)?;
                if self.pins.focus_page {
                    self.host.focus(id)?;
                    self.pins.focus_page = false;
                }
            }
            Ok(())
        })();
        self.report(result);
    }

    pub fn poll_pins(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let events: Vec<Value> = self
            .pins
            .events
            .as_ref()
            .map(|e| e.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            if self.pins.open.is_none() && self.pins.menu.is_none() {
                continue;
            }
            if event["type"] != "ready"
                && event["generation"].as_u64() != Some(self.pins.generation)
            {
                continue;
            }
            match event["type"].as_str().unwrap_or("") {
                "ready" => {
                    if let Some(view) = &self.pins.surface
                        && !self.pins.state.is_empty()
                    {
                        let _ = view
                            .evaluate_script(&format!("window.updatePins({})", self.pins.state));
                        if let Some(grip) = &self.pins.grip {
                            let _ = grip.evaluate_script(&format!(
                                "window.updatePins({})",
                                self.pins.state
                            ));
                        }
                        if self.pins.menu.is_some() {
                            let _ = view.focus();
                        }
                    }
                }
                "sleep-hover" => {
                    let id = event["id"].as_u64().filter(|_| event["value"] == true);
                    self.hover_sleep(
                        id,
                        point(
                            px(event["x"].as_f64().unwrap_or(0.) as f32),
                            px(event["y"].as_f64().unwrap_or(0.) as f32 + crate::render::TOP),
                        ),
                        window,
                        cx,
                    );
                }
                "dismiss" => self.dismiss_pin(window, cx),
                "open" => {
                    if let Some(id) = event["id"].as_u64() {
                        self.toggle_pin(id, window, cx);
                    }
                }
                "context" => {
                    if let Some(id) = event["id"].as_u64() {
                        self.pins.menu = Some(Some(id));
                        cx.notify();
                    }
                }
                "sleep" => {
                    if let Some(id) = self.pins.menu.flatten() {
                        self.dismiss_pin(window, cx);
                        self.sleep_tab(id, window, cx);
                    }
                }
                "keep-alive" => {
                    if let Some(id) = self.pins.menu.flatten() {
                        self.toggle_keep_alive(id);
                        cx.notify();
                    }
                }
                "recover" => {
                    if let Some(id) = self.pins.menu.flatten() {
                        self.dismiss_pin(window, cx);
                        self.confirm_tab_recovery(id, window, cx);
                    }
                }
                "unpin" => {
                    if let Some(id) = self.pins.menu.flatten() {
                        self.dismiss_pin(window, cx);
                        self.model.unpin_tab(id);
                        self.sync_address(window, cx);
                        let _ = self.host.focus(id);
                        cx.notify();
                    }
                }
                "set-icon" => {
                    if let Some(id) = self.pins.menu.flatten() {
                        let icon = event["icon"].as_str();
                        let valid = icon.is_none()
                            || serde_json::from_str::<Vec<(String, String)>>(include_str!(
                                "../assets/emoji-data.json"
                            ))
                            .is_ok_and(|items| {
                                items.iter().any(|(emoji, _)| Some(emoji.as_str()) == icon)
                            });
                        if valid {
                            self.model.set_pin_icon(id, icon);
                            self.dismiss_pin(window, cx);
                        }
                    }
                }
                "resize-start" => {
                    self.pins.resize_origin_x = self.pin_rect(window).map(|r| r.x);
                }
                "resize" => {
                    if let Some(id) = self.pins.open {
                        self.model.resize_pin(
                            id,
                            (event["width"].as_f64().unwrap_or(480.) as f32).min(
                                self.pins
                                    .resize_origin_x
                                    .map_or(f32::from(window.viewport_size().width) - 8., |x| {
                                        f32::from(window.viewport_size().width) - x - 4.
                                    }),
                            ),
                            event["height"].as_f64().unwrap_or(640.) as f32,
                        );
                        cx.notify();
                    }
                }
                _ => {}
            }
        }
    }
}
use wry::WebViewBuilder;

#[cfg(feature = "smoke")]
impl PinnedUi {
    #[cfg(target_os = "macos")]
    pub(crate) fn snapshot(&self, path: &str) {
        crate::floating_panels::snapshot_surface(
            self.surface.as_ref().unwrap(),
            format!("{path}.shield.png"),
        );
        crate::floating_panels::snapshot_surface(
            self.grip.as_ref().unwrap(),
            format!("{path}.grip.png"),
        );
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn check_grip(&self, rect: Rect) -> anyhow::Result<()> {
        let bounds = self.grip.as_ref().unwrap().bounds()?;
        let position = bounds.position.to_logical::<f64>(1.);
        let size = bounds.size.to_logical::<f64>(1.);
        anyhow::ensure!(
            (position.x - f64::from(rect.x + rect.width - 27.)).abs() < 1.
                && (position.y - f64::from(rect.y + rect.height - 19.)).abs() < 1.
                && size.width == 26.
                && size.height == 18.,
            "resize overlay is not inside the bottom-right corner: {bounds:?}, {rect:?}"
        );
        Ok(())
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn grip_mouse(
        &self,
        kind: objc2_app_kit::NSEventType,
        x: f64,
        y: f64,
    ) -> anyhow::Result<()> {
        use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
        use objc2_foundation::NSPoint;
        use wry::WebViewExtMacOS;
        let grip = self.grip.as_ref().unwrap();
        let window = grip.ns_window();
        let root = window.contentView().unwrap();
        let point = NSPoint::new(
            x,
            if root.isFlipped() {
                y
            } else {
                root.frame().size.height - y
            },
        );
        if kind == NSEventType::LeftMouseDown {
            window.makeKeyWindow();
            grip.focus()?;
            let target = root
                .hitTest(point)
                .ok_or_else(|| anyhow::anyhow!("grip not hit-testable"))?;
            anyhow::ensure!(
                target.isDescendantOf(&grip.webview()),
                "page intercepts internal resize grip"
            );
        }
        let location = root.convertPoint_toView(point, None);
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind, location, NSEventModifierFlags::empty(), objc2_foundation::NSProcessInfo::processInfo().systemUptime(), window.windowNumber(), None, 0, 1, 1.,
        ).ok_or_else(|| anyhow::anyhow!("create native resize event"))?;
        match kind {
            NSEventType::LeftMouseDown => grip.webview().mouseDown(&event),
            NSEventType::LeftMouseDragged => grip.webview().mouseDragged(&event),
            NSEventType::LeftMouseUp => grip.webview().mouseUp(&event),
            _ => window.sendEvent(&event),
        }
        Ok(())
    }
    pub(crate) fn read(
        &self,
        script: &str,
        callback: impl Fn(String) + Send + 'static,
    ) -> anyhow::Result<()> {
        self.surface
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Popup surface missing"))?
            .evaluate_script_with_callback(script, callback)?;
        Ok(())
    }
    pub(crate) fn evaluate(&self, grip: bool, script: &str) -> anyhow::Result<()> {
        (if grip { &self.grip } else { &self.surface })
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Popup surface missing"))?
            .evaluate_script(script)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::header_layout;
    #[test]
    fn header_reserves_controls_and_an_overflow_slot() {
        assert_eq!(header_layout(1000., 0), (0, 0.));
        assert_eq!(header_layout(1000., 2), (2, 64.));
        assert_eq!(header_layout(1000., 40), (14, 506.));
        assert_eq!(header_layout(360., 12), (0, 30.));
    }
}
