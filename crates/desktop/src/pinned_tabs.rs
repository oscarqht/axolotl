//! Window-wide pinned pages. The native shield is behind the raised, retained page;
//! it catches outside clicks and provides a resize grip without framing the page in an iframe.
use crate::{
    app::BrowserApp,
    native_webview::{BuildNativeChild, NativeWebView},
};
use gpui::*;
use serde_json::{Value, json};
use std::sync::mpsc::{self, Receiver};
use workspace_core::{PaneId, Rect, TabId};

#[derive(Default)]
pub struct PinnedUi {
    pub open: Option<TabId>,
    pub menu: Option<Option<TabId>>,
    surface: Option<NativeWebView>,
    events: Option<Receiver<Value>>,
    state: String,
    focus_page: bool,
    pub drag: Option<(TabId, Point<Pixels>, bool)>,
    pub suppress_click: bool,
}

pub fn header_layout(width: f32, count: usize) -> (usize, f32) {
    // Keep a full space chip, caption controls and an empty drag region usable.
    let capacity = (((width - 430.).max(0.) / 34.) as usize).max(1);
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
        if self.pins.open.is_none() && self.pins.menu.is_none() && self.pins.surface.is_none() {
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
        self.pins.menu = None;
        self.pins.surface = None;
        self.pins.events = None;
        self.pins.state.clear();
        self.focus.focus(window);
        cx.notify();
    }

    pub fn toggle_pin(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let closing = self.pins.open == Some(id);
        self.dismiss_pin(window, cx);
        self.close_space_menu(cx);
        self.close_settings(window, cx);
        if !closing {
            self.pins.open = Some(id);
            self.pins.focus_page = true;
            self.notifications.remove(&id);
        }
        cx.notify();
    }

    pub fn pin_menu(&mut self, tab: Option<TabId>, window: &mut Window, cx: &mut Context<Self>) {
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
            176.
        } else {
            42.
        };
        let anchor = (w - right - strip + index as f32 * 34. + 30.).min(w - 4.);
        let width = pin.width.min((w - 8.).max(1.));
        let height = pin.height.min((h - 8.).max(1.));
        Some(Rect::new(
            (anchor - width).max(4.).min((w - width).max(0.)),
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
                let (sender, events) = mpsc::channel();
                let view = crate::browser::with_inline_html(
                    WebViewBuilder::new()
                        .with_transparent(true)
                        .with_focused(false)
                        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                        .with_ipc_handler(move |request| {
                            if let Ok(event) = serde_json::from_str(request.body()) {
                                let _ = sender.send(event);
                            }
                        }),
                    include_str!("../assets/pinned-popup.html"),
                )
                .build_native_child(window, cx)?;
                self.pins.surface = Some(view);
                self.pins.events = Some(events);
            }
            let viewport = window.viewport_size();
            let view = self.pins.surface.as_ref().unwrap();
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
                "rect": rect.map(|r| json!({"x":r.x,"y":r.y-crate::render::TOP,"width":r.width,"height":r.height})),
                "menu": self.pins.menu.is_some(),
                "unpin": self.pins.menu.flatten(),
                "pins": self.model.pinned_tabs.iter().map(|p| json!({"id":p.tab.id,"title":p.tab.display_title(),"url":p.tab.url})).collect::<Vec<_>>()
            }).to_string();
            // Also push on ready: initialization can finish after the first evaluation.
            if state != self.pins.state {
                view.evaluate_script(&format!("window.updatePins?.({state})"))?;
                self.pins.state = state;
            }
            if let (Some(id), Some(r)) = (self.pins.open, rect) {
                self.host.place(
                    id,
                    Bounds {
                        origin: point(px(r.x + 6.), px(r.y + 6.)),
                        size: size(px((r.width - 12.).max(1.)), px((r.height - 22.).max(1.))),
                    },
                )?;
                self.host.raise(id)?;
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
            match event["type"].as_str().unwrap_or("") {
                "ready" => {
                    if let Some(view) = &self.pins.surface {
                        let _ = view
                            .evaluate_script(&format!("window.updatePins({})", self.pins.state));
                        if self.pins.menu.is_some() {
                            let _ = view.focus();
                        }
                    }
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
                "unpin" => {
                    if let Some(id) = self.pins.menu.flatten() {
                        self.dismiss_pin(window, cx);
                        self.model.unpin_tab(id);
                        self.sync_address(window, cx);
                        let _ = self.host.focus(id);
                        cx.notify();
                    }
                }
                "resize" => {
                    if let Some(id) = self.pins.open {
                        self.model.resize_pin(
                            id,
                            event["width"].as_f64().unwrap_or(480.) as f32,
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
    pub(crate) fn evaluate(&self, script: &str) -> anyhow::Result<()> {
        self.surface
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
        assert_eq!(header_layout(1000., 40), (15, 540.));
        assert_eq!(header_layout(360., 12), (0, 30.));
    }
}
