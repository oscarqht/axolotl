use crate::app::{BrowserApp, Command, TabAction, TabTarget};
use gpui::{prelude::*, *};
use gpui_component::{InteractiveElementExt, input::Input};
use workspace_core::{Axis, PaneId, Rect};

pub const TOP: f32 = 38.;
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
            .w(px(if editing { 320. } else { 204. }))
            .flex_shrink_0()
            .flex()
            .items_center()
            .rounded_md()
            .bg(rgb(if active { 0x303b49 } else { SURFACE }))
            .border_1()
            .border_color(rgb(if editing { ACCENT } else { 0x303845 }))
            .text_color(rgb(if active { INK } else { MUTED }))
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
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
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

    fn pane(&self, pane_id: PaneId, rect: Rect, cx: &Context<Self>) -> AnyElement {
        let pane = self.model.pane(pane_id).expect("layout pane exists");
        let focused = self.model.focused_pane().id == pane_id;
        let tab = pane.active_tab;
        let page = self.host.has_view(tab).then(|| self.host.clone());
        let unavailable = page.is_none();
        let tabs = pane
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
                    .child(
                        if self.menu.is_some() || self.dragging.is_some() || unavailable {
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
                        },
                    ),
            )
            .into_any_element()
    }

    fn menu_item(
        &self,
        id: &'static str,
        label: &'static str,
        target: TabTarget,
        action: TabAction,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .h(px(32.))
            .px_2()
            .flex()
            .items_center()
            .rounded_md()
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0x3a4859)))
            .child(label)
            .on_click(
                cx.listener(move |this, _, window, cx| this.tab_action(target, action, window, cx)),
            )
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
        let panes = geometry
            .panes
            .iter()
            .map(|p| self.pane(p.id, p.bounds, cx))
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
                    this.cancel_edit(window, cx);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
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
                    this.model
                        .resize_split(divider.id, (position - start - 3.) / (length - 6.).max(1.));
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.moving_window = false;
                    if this.dragging.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.moving_window = false;
                    if this.dragging.take().is_some() {
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
                            this.report(this.host.hide_all());
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
            .when_some(self.menu, |root, menu| {
                let x = f32::from(menu.position.x)
                    .min((f32::from(viewport.width) - 232.).max(4.))
                    .max(4.);
                let y = f32::from(menu.position.y)
                    .min((f32::from(viewport.height) - 146.).max(TOP))
                    .max(TOP);
                root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.menu = None;
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(x))
                                .top(px(y))
                                .w(px(224.))
                                .p_1()
                                .rounded_lg()
                                .bg(rgb(0x25303d))
                                .border_1()
                                .border_color(rgb(0x465366))
                                .shadow_lg()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(self.menu_item(
                                    "menu-new",
                                    if matches!(menu.target, TabTarget::Space(_)) {
                                        "New sibling space"
                                    } else {
                                        "New sibling tab"
                                    },
                                    menu.target,
                                    TabAction::NewSibling,
                                    cx,
                                ))
                                .child(self.menu_item(
                                    "menu-h",
                                    "Split horizontally  ↔",
                                    menu.target,
                                    TabAction::Split(Axis::Horizontal),
                                    cx,
                                ))
                                .child(self.menu_item(
                                    "menu-v",
                                    "Split vertically  ↕",
                                    menu.target,
                                    TabAction::Split(Axis::Vertical),
                                    cx,
                                ))
                                .child(self.menu_item(
                                    "menu-close",
                                    if matches!(menu.target, TabTarget::Space(_)) {
                                        "Close space"
                                    } else {
                                        "Close tab"
                                    },
                                    menu.target,
                                    TabAction::Close,
                                    cx,
                                )),
                        ),
                )
            })
    }
}
