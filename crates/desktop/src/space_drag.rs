//! Space order is transactional: only a release inside the strip mutates it.
use crate::app::BrowserApp;
use gpui::*;
use workspace_core::SpaceId;

#[derive(Clone, Copy)]
pub struct SpaceDrag {
    pub id: SpaceId,
    origin: Point<Pixels>,
    position: Point<Pixels>,
    pub active: bool,
}

impl BrowserApp {
    pub fn arm_space_drag(&mut self, id: SpaceId, origin: Point<Pixels>) {
        crate::ui_events::wake();
        self.moving_window = false;
        self.suppress_space_click = false;
        self.space_drag = Some(SpaceDrag {
            id,
            origin,
            position: origin,
            active: false,
        });
    }

    pub fn cancel_space_drag(&mut self, cx: &mut Context<Self>) {
        if self.space_drag.take().is_some() {
            cx.notify();
        }
    }

    pub fn update_space_drag(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.space_drag.as_mut() else {
            return;
        };
        if !event.dragging() {
            self.cancel_space_drag(cx);
            return;
        }
        drag.position = event.position;
        let delta = event.position - drag.origin;
        if !drag.active && f32::from(delta.x).powi(2) + f32::from(delta.y).powi(2) >= 25. {
            drag.active = true;
            self.suppress_space_click = true;
            self.space_tooltip = None;
            self.close_space_menu(cx);
            // Escape must reach GPUI even when a native page previously had focus.
            self.report(self.host.focus_parent(self.active_tab()));
            self.focus.focus(window);
        }
        cx.notify();
    }

    fn space_drag_group(&self) -> Option<(&ScrollHandle, usize, usize)> {
        let drag = self.space_drag?;
        let pinned = self.model.spaces.iter().find(|s| s.id == drag.id)?.pinned;
        let boundary = self.model.spaces.iter().take_while(|s| s.pinned).count();
        Some(if pinned {
            (&self.pinned_space_scroll, 0, boundary)
        } else {
            (
                &self.space_scroll,
                boundary,
                self.model.spaces.len() - boundary,
            )
        })
    }

    fn space_drop_slot(&self, position: Point<Pixels>) -> Option<usize> {
        let (scroll, start, count) = self.space_drag_group()?;
        if !scroll.bounds().contains(&position) {
            return None;
        }
        Some(
            start
                + (0..count)
                    .filter(|&i| {
                        scroll
                            .bounds_for_item(i)
                            .is_some_and(|b| position.x > b.center().x + scroll.offset().x)
                    })
                    .count(),
        )
    }

    pub fn space_drop_indicator(&self) -> Option<(Pixels, Pixels, Pixels)> {
        let drag = self.space_drag.filter(|drag| drag.active)?;
        let slot = self.space_drop_slot(drag.position)?;
        let (scroll, start, count) = self.space_drag_group()?;
        let slot = slot - start;
        let strip = scroll.bounds();
        let x = if slot == count {
            scroll.bounds_for_item(slot.checked_sub(1)?)?.right()
        } else {
            scroll.bounds_for_item(slot)?.left() - px(2.)
        } + scroll.offset().x;
        Some((
            x.max(strip.left()).min(strip.right() - px(3.)),
            strip.top(),
            strip.size.height,
        ))
    }

    // Polling continues to scroll while the pointer is held still at an edge.
    pub fn scroll_space_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.space_drag.filter(|drag| drag.active) else {
            return;
        };
        if self
            .model
            .spaces
            .iter()
            .any(|s| s.id == drag.id && s.pinned)
        {
            return;
        }
        let bounds = self.space_scroll.bounds();
        if !bounds.contains(&drag.position) {
            return;
        }
        let left = f32::from(drag.position.x - bounds.left());
        let right = f32::from(bounds.right() - drag.position.x);
        let speed = if left < 32. {
            (32. - left) * 0.6
        } else if right < 32. {
            -(32. - right) * 0.6
        } else {
            0.
        };
        let old = self.space_scroll.offset();
        self.space_scroll.set_offset(point(
            (old.x + px(speed))
                .min(px(0.))
                .max(-self.space_scroll.max_offset().width),
            old.y,
        ));
        if self.space_scroll.offset() != old {
            cx.notify();
        }
    }

    pub fn finish_space_drag(&mut self, position: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        let slot = position.and_then(|position| self.space_drop_slot(position));
        if let Some(drag) = self.space_drag.take() {
            if drag.active
                && let Some(slot) = slot
            {
                self.model.move_space(drag.id, slot);
            }
            cx.notify();
        }
    }
}
