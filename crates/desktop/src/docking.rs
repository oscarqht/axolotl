//! Pointer drag state and destinations shared by tab chips and panel handles.
use crate::app::{BrowserApp, DragInput, TabDrag};
use gpui::*;
use workspace_core::{PaneId, Rect, SplitEdge, TabId};
impl BrowserApp {
    pub fn tab_drag_active(&self) -> bool {
        self.tab_drag.is_some_and(|drag| drag.active)
    }

    pub fn cancel_tab_drag(&mut self, cx: &mut Context<Self>) {
        self.drag_preview = None;
        self.split_preview = None;
        self.host.request_redraw();
        if self.tab_drag.take().is_some() {
            cx.notify();
        }
    }

    pub fn arm_tab_drag(&mut self, source: PaneId, tab: TabId, origin: Point<Pixels>) {
        self.suppress_tab_click = false;
        self.tab_drag = Some(TabDrag {
            input: DragInput::Shell,
            source,
            tab,
            origin,
            position: origin,
            active: false,
            whole: false,
        });
    }

    pub fn update_tab_drag(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .tab_drag
            .is_some_and(|drag| drag.input != DragInput::Shell)
        {
            return;
        }
        self.update_drag(event, window, cx);
    }

    pub(crate) fn native_drag_from(&self, pane: PaneId) -> bool {
        self.tab_drag
            .is_some_and(|drag| drag.input == DragInput::NativeBar && drag.source == pane)
    }

    pub(crate) fn update_native_tab_drag(
        &mut self,
        pane: PaneId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.native_drag_from(pane) {
            self.update_drag(
                &MouseMoveEvent {
                    position,
                    pressed_button: Some(MouseButton::Left),
                    ..Default::default()
                },
                window,
                cx,
            );
        }
    }

    fn update_drag(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.tab_drag else { return };
        if !event.dragging() {
            self.cancel_tab_drag(cx);
            return;
        }
        self.tab_drag.as_mut().unwrap().position = event.position;
        let delta = event.position - drag.origin;
        if !drag.active && (f32::from(delta.x).powi(2) + f32::from(delta.y).powi(2)) >= 25. {
            self.cancel_edit(window, cx);
            self.tab_drag.as_mut().unwrap().active = true;
            self.suppress_tab_click = true;
            self.report(self.host.focus_parent(self.active_tab()));
            self.focus.focus(window);
            if let Some(tab) = self.model.tab(drag.tab) {
                let result = self.host.ensure(drag.tab, &tab.url, window, cx);
                self.report(result);
            }
            let viewport = window.viewport_size();
            let geometry = self.model.active_space().layout.geometry(
                Rect::new(
                    0.,
                    crate::render::TOP,
                    f32::from(viewport.width),
                    (f32::from(viewport.height) - crate::render::TOP).max(0.),
                ),
                6.,
                100.,
            );
            if let Some(pane) = geometry.panes.iter().find(|pane| pane.id == drag.source) {
                let r = pane.bounds;
                self.report(self.host.place(
                    drag.tab,
                    Bounds {
                        origin: point(px(r.x), px(r.y)),
                        size: size(px(r.width), px(r.height.max(1.))),
                    },
                ));
            }
            let title = self
                .model
                .tab(drag.tab)
                .map_or("New tab", |tab| tab.display_title())
                .to_owned();
            // Create the half-pane overlay first so the thumbnail remains above it.
            match crate::split_preview::SplitPreview::new(window, cx) {
                Ok(preview) => self.split_preview = Some(preview),
                Err(error) => self.report(Err(error)),
            }
            match self.host.drag_preview(drag.tab, &title, window, cx) {
                Ok(preview) => self.drag_preview = Some(preview),
                Err(error) => self.report(Err(error)),
            }
        }
        if let Some(preview) = &self.drag_preview {
            let result = preview.update(event.position, window.viewport_size());
            self.report(result);
        }
        self.update_split_preview(window);
        if self.tab_drag_active() {
            self.host.request_redraw();
            cx.notify();
        }
    }

    /// Use measured native chip midpoints, including horizontal scroll offsets.
    pub fn tab_drop_target(
        &self,
        position: Point<Pixels>,
        _window: &Window,
    ) -> Option<(PaneId, usize)> {
        let drag = self.tab_drag.filter(|drag| drag.active)?;
        if drag.whole {
            return None;
        }
        let x = f32::from(position.x);
        let y = f32::from(position.y);
        self.bars.iter().find_map(|(pane, bar)| {
            let r = bar.bounds;
            if x < r.x || x > r.x + r.width || y < r.y || y > r.y + r.height {
                return None;
            }
            let count = self.model.pane(*pane)?.tabs.len();
            let index = if bar.expanded {
                bar.midpoints
                    .iter()
                    .filter(|mid| x - r.x > **mid)
                    .count()
                    .min(count)
            } else {
                count
            };
            Some((*pane, index))
        })
    }

    pub fn split_drop_target(
        &self,
        position: Point<Pixels>,
        window: &Window,
    ) -> Option<(PaneId, SplitEdge, Rect)> {
        let drag = self.tab_drag.filter(|drag| drag.active)?;
        let viewport = window.viewport_size();
        let geometry = self.model.active_space().layout.geometry(
            Rect::new(
                0.,
                crate::render::TOP,
                f32::from(viewport.width),
                (f32::from(viewport.height) - crate::render::TOP).max(0.),
            ),
            6.,
            100.,
        );
        geometry.panes.iter().find_map(|pane| {
            if pane.id == drag.source
                && (drag.whole || self.model.pane(drag.source)?.tabs.len() < 2)
            {
                return None;
            }
            let r = pane.bounds;
            let content = r;
            let edge = SplitEdge::at(content, f32::from(position.x), f32::from(position.y))?;
            Some((pane.id, edge, edge.preview(content)))
        })
    }

    fn center_drop_target(
        &self,
        position: Point<Pixels>,
        window: &Window,
    ) -> Option<(PaneId, Rect)> {
        let drag = self.tab_drag.filter(|d| d.active)?;
        let viewport = window.viewport_size();
        let geometry = self.model.active_space().layout.geometry(
            Rect::new(
                0.,
                crate::render::TOP,
                f32::from(viewport.width),
                f32::from(viewport.height) - crate::render::TOP,
            ),
            6.,
            100.,
        );
        geometry.panes.iter().find_map(|p| {
            let r = p.bounds;
            let x = f32::from(position.x);
            let y = f32::from(position.y);
            (p.id != drag.source && x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
                .then_some((p.id, r))
        })
    }

    pub(crate) fn update_split_preview(&mut self, window: &Window) {
        let position = self.tab_drag.filter(|d| d.active).map(|d| d.position);
        let slot = position.and_then(|p| self.tab_drop_target(p, window));
        for (id, bar) in &self.bars {
            let _ = bar.preview(slot.filter(|(pane, _)| pane == id).map(|(_, index)| index));
        }
        let bounds = position.filter(|_| slot.is_none()).and_then(|p| {
            self.split_drop_target(p, window)
                .map(|(_, _, r)| r)
                .or_else(|| self.center_drop_target(p, window).map(|(_, r)| r))
        });
        if let Some(preview) = &self.split_preview {
            let merge = position.is_some_and(|p| self.split_drop_target(p, window).is_none());
            let result = preview.update(bounds, merge);
            self.report(result);
        }
    }

    pub fn finish_tab_drag(
        &mut self,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Native pointer capture delivers its release through the IPC poll.
        // A shell mouse-up/move can arrive first; it must not consume that drag.
        if self
            .tab_drag
            .is_some_and(|drag| drag.input != DragInput::Shell)
        {
            return;
        }
        self.finish_drag(position, window, cx);
    }

    pub(crate) fn finish_native_tab_drag(
        &mut self,
        pane: PaneId,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.native_drag_from(pane) {
            self.finish_drag(position, window, cx);
        }
    }

    fn finish_drag(
        &mut self,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_preview = None;
        self.split_preview = None;
        self.host.request_redraw();
        let target = position.and_then(|p| self.tab_drop_target(p, window));
        let split = position.and_then(|p| self.split_drop_target(p, window));
        let center = position.and_then(|p| self.center_drop_target(p, window));
        if let Some(drag) = self.tab_drag.take() {
            let moved = if !drag.active {
                false
            } else if !drag.whole && position.is_some_and(|p| self.pin_drop_target(p, window)) {
                self.model.pin_tab(drag.source, drag.tab)
            } else if drag.whole {
                if let Some((pane, edge, _)) = split {
                    self.model.move_pane(drag.source, pane, Some(edge))
                } else if let Some((pane, _)) = center {
                    self.model.move_pane(drag.source, pane, None)
                } else {
                    false
                }
            } else if let Some((pane, index)) = target {
                self.model.move_tab(drag.source, drag.tab, pane, index)
            } else if let Some((pane, edge, _)) = split {
                self.model
                    .split_tab(drag.source, drag.tab, pane, edge)
                    .is_some()
            } else if let Some((pane, _)) = center {
                let index = self.model.pane(pane).unwrap().tabs.len();
                self.model.move_tab(drag.source, drag.tab, pane, index)
            } else {
                false
            };
            if moved {
                self.sync_address(window, cx);
                self.reveal_focused();
                if !self.model.pinned_tabs.iter().any(|p| p.tab.id == drag.tab) {
                    self.report(self.host.focus(drag.tab));
                }
            }
            cx.notify();
        }
        for bar in self.bars.values() {
            let _ = bar.preview(None);
        }
    }
}
