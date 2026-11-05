use crate::app::{BrowserApp, Command, TabAction, TabTarget};
use gpui::{prelude::*, *};
use gpui_component::{InteractiveElementExt, input::Input};
use workspace_core::{Axis, PaneId, Rect};

pub const TOP: f32 = 38.;
pub const TAB_WIDTH: f32 = 204.;

#[cfg(target_os = "windows")]
fn window_controls(window: &Window, cx: &App) -> impl IntoElement {
    let colors = crate::chrome_theme::ChromeColors::current(cx);
    div()
        .id("window-controls")
        .flex()
        .flex_shrink_0()
        .h_full()
        .children(
            [
                ("minimize", WindowControlArea::Min, '\u{e921}'),
                (
                    "maximize-restore",
                    WindowControlArea::Max,
                    if window.is_maximized() {
                        '\u{e923}'
                    } else {
                        '\u{e922}'
                    },
                ),
                ("close", WindowControlArea::Close, '\u{e8bb}'),
            ]
            .into_iter()
            .map(|(id, area, glyph)| {
                div()
                    .id(id)
                    .w(px(46.))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family("Segoe MDL2 Assets")
                    .text_size(px(10.))
                    .window_control_area(area)
                    .hover(move |style| {
                        if area == WindowControlArea::Close {
                            style.bg(rgb(0xc42b1c)).text_color(rgb(0xffffff))
                        } else {
                            style.bg(rgb(colors.hover))
                        }
                    })
                    .child(glyph.to_string())
            }),
        )
}

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
        let colors = crate::chrome_theme::ChromeColors::current(cx);
        let editing = self.editing == Some(target);
        div()
            .id((kind, id))
            .h(px(30.))
            .w(px(if editing { 320. } else { TAB_WIDTH }))
            .flex_shrink_0()
            .flex()
            .items_center()
            .rounded_md()
            .bg(rgb(if active {
                colors.active_tab
            } else {
                colors.surface
            }))
            .border_1()
            .border_color(rgb(if editing {
                colors.accent
            } else {
                colors.border
            }))
            .text_color(rgb(if active { colors.ink } else { colors.muted }))
            .when(
                self.tab_drag.is_some_and(|drag| {
                    drag.active && target == TabTarget::Page(drag.source, drag.tab)
                }),
                |tab| tab.opacity(0.55),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if let TabTarget::Space(id) = target {
                        cx.stop_propagation();
                        this.open_space_menu(id, event.position, window, cx);
                    }
                }),
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
                            "space-close"
                        } else {
                            "page-close"
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
                    .when(
                        !matches!(target, TabTarget::Space(_)) || self.model.spaces.len() > 1,
                        |button| button.cursor_pointer().hover(|s| s.bg(rgb(colors.hover))),
                    )
                    .text_color(rgb(colors.muted))
                    .opacity(
                        if matches!(target, TabTarget::Space(_)) && self.model.spaces.len() == 1 {
                            0.35
                        } else {
                            1.
                        },
                    )
                    .child("×")
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        if matches!(target, TabTarget::Space(_)) && this.model.spaces.len() == 1 {
                            return;
                        }
                        this.tab_action(target, TabAction::Close, window, cx);
                    })),
            )
            .into_any_element()
    }

    fn pane(&self, pane_id: PaneId, rect: Rect, cx: &Context<Self>) -> AnyElement {
        let colors = crate::chrome_theme::ChromeColors::current(cx);
        let pane = self.model.pane(pane_id).expect("layout pane exists");
        let tab = pane.active_tab;
        let page = self.host.has_view(tab).then(|| self.host.clone());
        let unavailable = page.is_none();
        div()
            .absolute()
            .left(px(rect.x))
            .top(px(rect.y - TOP))
            .w(px(rect.width))
            .h(px(rect.height))
            .bg(rgb(colors.surface))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .bg(rgb(colors.page))
                    .child(if unavailable {
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(colors.muted))
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
        let colors = crate::chrome_theme::ChromeColors::current(cx);
        self.prepare_views(window, cx);
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
                    space.display_title(),
                    space.id == self.model.active_space,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        div()
            .size_full()
            .relative()
            .bg(rgb(colors.shell))
            .text_color(rgb(colors.ink))
            .text_size(px(12.))
            .font_family(if cfg!(target_os = "windows") {
                "Segoe UI"
            } else {
                ".SystemUIFont"
            })
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
                    this.cancel_tab_drag(cx);
                    this.close_space_menu(cx);
                    this.cancel_edit(window, cx);
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.close_space_menu(cx);
                }
            }))
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
                    .when(!cfg!(target_os = "windows"), |header| header.pr_2())
                    .child(
                        div()
                            .w(px(if cfg!(target_os = "macos") { 84. } else { 8. }))
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .id("spaces")
                            .track_scroll(&self.space_scroll)
                            .when(cfg!(target_os = "windows"), |spaces| {
                                // Reserve caption buttons, gaps, and an empty drag target.
                                spaces.max_w(px((f32::from(viewport.width) - 194.).max(0.)))
                            })
                            .when(!cfg!(target_os = "windows"), |spaces| {
                                spaces.max_w(relative(0.85))
                            })
                            .min_w_0()
                            .flex()
                            .gap_1()
                            .overflow_x_scroll()
                            .children(spaces),
                    )
                    .child(
                        div()
                            .id("new-space")
                            .w(px(30.))
                            .h(px(30.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(colors.hover)))
                            .text_size(px(22.))
                            .child("+")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.command(Command::NewSpace, window, cx)
                            })),
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
                            .when(!cfg!(target_os = "windows"), |drag| {
                                drag.on_double_click(|_, window, _| window.titlebar_double_click())
                            }),
                    )
                    .when(cfg!(target_os = "windows"), |header| {
                        #[cfg(target_os = "windows")]
                        {
                            header.child(window_controls(window, cx))
                        }
                        #[cfg(not(target_os = "windows"))]
                        {
                            header
                        }
                    }),
            )
            // Keep native caption hit regions outside the focusable content.
            // GPUI's automatic focus handler prevents the default mouse-down,
            // which would swallow Windows caption dragging and button actions.
            .child(
                div()
                    .absolute()
                    .top(px(TOP))
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .track_focus(&self.focus)
                    .children(panes),
            )
            .children(geometry.dividers.into_iter().map(|divider| {
                let bounds = divider.bounds;
                div()
                    .id(("divider", divider.id))
                    .absolute()
                    .left(px(bounds.x))
                    .top(px(bounds.y))
                    .w(px(bounds.width))
                    .h(px(bounds.height))
                    .bg(rgb(colors.divider))
                    .cursor(match divider.axis {
                        Axis::Horizontal => CursorStyle::ResizeLeftRight,
                        Axis::Vertical => CursorStyle::ResizeUpDown,
                    })
                    .hover(|s| s.bg(rgb(colors.accent)))
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
                        .bg(rgb(colors.surface))
                        .child(self.status.clone()),
                )
            })
    }
}
