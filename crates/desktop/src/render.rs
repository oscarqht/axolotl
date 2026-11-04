use crate::app::{BrowserApp, Command, TabTarget};
use gpui::{prelude::*, *};
use gpui_component::{InteractiveElementExt, input::Input};
use workspace_core::{Axis, PaneId, Rect};

pub const TOP: f32 = 38.;
pub const TAB_WIDTH: f32 = 204.;
pub const TAB_GAP: f32 = 4.;

pub fn tab_insertion_index(x: f32, count: usize) -> usize {
    (((x + TAB_WIDTH / 2. + TAB_GAP) / (TAB_WIDTH + TAB_GAP))
        .floor()
        .max(0.) as usize)
        .min(count)
}
const INK: u32 = 0xdde5ed;
const SURFACE: u32 = 0x181e27;
const MUTED: u32 = 0x8491a2;
const ACCENT: u32 = 0x8fddc3;

impl BrowserApp {
    fn compact_tab(
        &self,
        target: TabTarget,
        title: &str,
        active: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let (kind, id) = match target {
            TabTarget::Space(id) => ("space", id),
            TabTarget::Page(_, id) => ("page", id),
        };
        let editing = self.editing == Some(target);
        div()
            .id((kind, id))
            .h(px(30.))
            .w(px(if editing { 320. } else { TAB_WIDTH }))
            .flex_shrink_0()
            .flex()
            .items_center()
            .rounded_md()
            .bg(rgb(if active { 0x303b49 } else { SURFACE }))
            .border_1()
            .border_color(rgb(if editing { ACCENT } else { 0x303845 }))
            .text_color(rgb(if active { INK } else { MUTED }))
            .when(
                self.tab_drag.is_some_and(|drag| {
                    drag.active && target == TabTarget::Page(drag.source, drag.tab)
                }),
                |tab| tab.opacity(0.55),
            )
            // Keep tab clicks from bubbling into the surrounding pane header.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(if editing {
                div()
                    .flex_1()
                    .min_w_0()
                    .px_1()
                    .child(Input::new(&self.address).appearance(false))
                    .into_any_element()
            } else {
                div()
                    .id((
                        if kind == "space" {
                            "space-title"
                        } else {
                            "page-title"
                        },
                        id,
                    ))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .cursor_pointer()
                    .child(div().truncate().child(if title.is_empty() {
                        "New tab".to_owned()
                    } else {
                        title.to_owned()
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, _| {
                            if let TabTarget::Page(pane, tab) = target {
                                this.arm_tab_drag(pane, tab, event.position);
                            }
                        }),
                    )
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if matches!(target, TabTarget::Page(_, _)) && this.suppress_tab_click {
                            return;
                        }
                        if event.click_count() == 2 {
                            this.begin_edit(target, window, cx);
                        } else {
                            this.select_target(target, window, cx);
                        }
                    }))
                    .into_any_element()
            })
            .child(
                div()
                    .id((
                        if kind == "space" {
                            "space-menu"
                        } else {
                            "page-menu"
                        },
                        id,
                    ))
                    .w(px(28.))
                    .h(px(28.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x465366)))
                    .text_color(rgb(
                        if self.menu.is_some_and(|menu| menu.target == target) {
                            ACCENT
                        } else {
                            MUTED
                        },
                    ))
                    .child("•")
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_menu(target, event.position(), window, cx);
                    })),
            )
            .into_any_element()
    }

    fn pane(
        &self,
        pane_id: PaneId,
        rect: Rect,
        drop: Option<(PaneId, usize)>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let pane = self.model.pane(pane_id).expect("layout pane exists");
        let focused = self.model.focused_pane().id == pane_id;
        let tab = pane.active_tab;
        let page = self.host.has_view(tab).then(|| self.host.clone());
        let unavailable = page.is_none();
        let mut tabs = pane
            .tabs
            .iter()
            .map(|tab| {
                self.compact_tab(
                    TabTarget::Page(pane_id, tab.id),
                    &tab.title,
                    tab.id == pane.active_tab,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        if let Some((target, index)) = drop
            && target == pane_id
        {
            tabs.insert(
                index,
                div()
                    .w(px(TAB_WIDTH))
                    .h(px(30.))
                    .flex_shrink_0()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(ACCENT))
                    .bg(rgb(0x29483f))
                    .px_2()
                    .flex()
                    .items_center()
                    .text_color(rgb(ACCENT))
                    .child("Drop tab here")
                    .into_any_element(),
            );
        }
        div()
            .absolute()
            .left(px(rect.x))
            .top(px(rect.y))
            .w(px(rect.width))
            .h(px(rect.height))
            .bg(rgb(SURFACE))
            .child(
                div()
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .px_1()
                    .border_b_1()
                    .border_color(rgb(if focused { ACCENT } else { 0x303845 }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.focus_pane(pane_id, window, cx)
                        }),
                    )
                    .child(
                        div()
                            .id(("pane-tabs", pane_id))
                            .track_scroll(
                                self.pane_scrolls.get(&pane_id).expect("pane scroll handle"),
                            )
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .gap_1()
                            .overflow_x_scroll()
                            .children(tabs),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(34.))
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .bg(rgb(0xf5f6f8))
                    .child(if unavailable {
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(0x627083))
                            .child(if unavailable { "Page unavailable" } else { "" })
                            .into_any_element()
                    } else {
                        canvas(
                            move |bounds, _, _| {
                                if let Some(page) = &page
                                    && let Err(error) = page.place(tab, bounds)
                                {
                                    eprintln!("Webview bounds: {error}");
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .size_full()
                        .into_any_element()
                    }),
            )
            .into_any_element()
    }
}

impl Render for BrowserApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.prepare_views(window);
        if self.select_address && self.editing.is_some() {
            self.select_address = false;
            window.dispatch_action(Box::new(gpui_component::input::SelectAll), cx);
        }
        let viewport = window.viewport_size();
        let content = Rect::new(
            0.,
            TOP,
            f32::from(viewport.width),
            (f32::from(viewport.height) - TOP).max(0.),
        );
        let geometry = self.model.active_space().layout.geometry(content, 6., 100.);
        let drop_target = self.tab_drop_target(
            self.tab_drag
                .map_or(window.mouse_position(), |drag| drag.position),
            window,
        );
        let panes = geometry
            .panes
            .iter()
            .map(|p| self.pane(p.id, p.bounds, drop_target, cx))
            .collect::<Vec<_>>();
        let spaces = self
            .model
            .spaces
            .iter()
            .map(|space| {
                self.compact_tab(
                    TabTarget::Space(space.id),
                    &space.focused_pane().active_tab().title,
                    space.id == self.model.active_space,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        div()
            .size_full()
            .relative()
            .bg(rgb(0x11161e))
            .text_color(rgb(INK))
            .text_size(px(12.))
            .font_family(if cfg!(target_os = "windows") {
                "Segoe UI"
            } else {
                ".SystemUIFont"
            })
            .track_focus(&self.focus)
            .when(self.tab_drag_active(), |root| {
                root.cursor(CursorStyle::ClosedHand)
            })
            .on_action(cx.listener(|this, _: &crate::Address, window, cx| {
                this.command(Command::Address, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::NewTab, window, cx| {
                this.command(Command::NewTab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::CloseTab, window, cx| {
                this.command(Command::CloseTab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::Reload, window, cx| {
                this.command(Command::Reload, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::Back, window, cx| {
                this.command(Command::Back, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::Forward, window, cx| {
                this.command(Command::Forward, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::NextPane, window, cx| {
                this.command(Command::NextPane, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &gpui_component::input::Escape, window, cx| {
                    this.menu = None;
                    this.cancel_tab_drag(cx);
                    this.cancel_edit(window, cx);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                this.update_tab_drag(event, window, cx);
                if this.moving_window {
                    this.moving_window = false;
                    window.start_window_move();
                }
                if let Some(divider) = &this.dragging {
                    let (position, start, length) = match divider.axis {
                        Axis::Horizontal => (
                            f32::from(event.position.x),
                            divider.container.x,
                            divider.container.width,
                        ),
                        Axis::Vertical => (
                            f32::from(event.position.y),
                            divider.container.y,
                            divider.container.height,
                        ),
                    };
                    this.host.request_redraw();
                    this.model
                        .resize_split(divider.id, (position - start - 3.) / (length - 6.).max(1.));
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.finish_tab_drag(Some(event.position), window, cx);
                    this.moving_window = false;
                    if this.dragging.take().is_some() {
                        this.host.request_redraw();
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.finish_tab_drag(None, window, cx);
                    this.moving_window = false;
                    if this.dragging.take().is_some() {
                        this.host.request_redraw();
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .h(px(TOP))
                    .flex()
                    .items_center()
                    .gap_1()
                    .pr_2()
                    .child(
                        div()
                            .w(px(if cfg!(target_os = "macos") { 84. } else { 8. }))
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .id("spaces")
                            .track_scroll(&self.space_scroll)
                            .max_w(relative(0.85))
                            .min_w_0()
                            .flex()
                            .gap_1()
                            .overflow_x_scroll()
                            .children(spaces),
                    )
                    .child(
                        div()
                            .id("window-drag")
                            .flex_1()
                            .h_full()
                            .min_w(px(32.))
                            .window_control_area(WindowControlArea::Drag)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| this.moving_window = true),
                            )
                            .on_double_click(|_, window, _| window.titlebar_double_click()),
                    ),
            )
            .children(panes)
            .children(geometry.dividers.into_iter().map(|divider| {
                let bounds = divider.bounds;
                div()
                    .id(("divider", divider.id))
                    .absolute()
                    .left(px(bounds.x))
                    .top(px(bounds.y))
                    .w(px(bounds.width))
                    .h(px(bounds.height))
                    .cursor(match divider.axis {
                        Axis::Horizontal => CursorStyle::ResizeLeftRight,
                        Axis::Vertical => CursorStyle::ResizeUpDown,
                    })
                    .hover(|s| s.bg(rgb(ACCENT)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.cancel_edit(window, cx);
                            this.dragging = Some(divider);
                            cx.notify();
                        }),
                    )
            }))
            .when(!self.status.is_empty(), |root| {
                root.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(px(22.))
                        .px_2()
                        .bg(rgb(SURFACE))
                        .child(self.status.clone()),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::tab_insertion_index;

    #[test]
    fn insertion_uses_tab_midpoints_and_clamps_empty_strip_space() {
        assert_eq!(tab_insertion_index(-10., 3), 0);
        assert_eq!(tab_insertion_index(100., 3), 0);
        assert_eq!(tab_insertion_index(110., 3), 1);
        assert_eq!(tab_insertion_index(309., 3), 1);
        assert_eq!(tab_insertion_index(310., 3), 2);
        assert_eq!(tab_insertion_index(320., 3), 2);
        assert_eq!(tab_insertion_index(10000., 3), 3);
    }
}
