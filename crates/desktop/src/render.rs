use crate::app::{BrowserApp, Command, TabAction, TabTarget};
use gpui::{prelude::*, *};
use gpui_component::{InteractiveElementExt, input::Input};
use workspace_core::{Axis, PaneId, Rect};

pub const TOP: f32 = 38.;
pub const TAB_WIDTH: f32 = 204.;
pub const HEADER_CONTROL_HEIGHT: f32 = 30.;
pub const PANEL_CORNER_RADIUS: f32 = 10.;
pub const PANEL_OUTER_GAP: f32 = 6.;
// Windows centers its tabs in the caption row. Count the row's bottom
// padding toward the visible gutter so it matches the panel dividers.
const PANEL_TOP_GAP: f32 = if cfg!(target_os = "windows") {
    PANEL_OUTER_GAP - (TOP - HEADER_CONTROL_HEIGHT) / 2.
} else {
    PANEL_OUTER_GAP
};

/// Shared by panel painting, native view placement, and docking hit targets.
pub(crate) fn panel_content_rect(width: f32, height: f32) -> Rect {
    Rect::new(
        PANEL_OUTER_GAP,
        TOP + PANEL_TOP_GAP,
        (width - 2. * PANEL_OUTER_GAP).max(0.),
        (height - TOP - PANEL_TOP_GAP - PANEL_OUTER_GAP).max(0.),
    )
}

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
    fn pinned_header(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let colors = crate::chrome_theme::ChromeColors::current(cx);
        let (visible, width) = crate::pinned_tabs::header_layout(
            f32::from(window.viewport_size().width),
            self.model.pinned_tabs.len(),
        );
        div()
            .id("pinned-tabs")
            .w(px(width))
            .flex_shrink_0()
            .flex()
            .gap_1()
            .children(self.model.pinned_tabs.iter().take(visible).map(|pin| {
                let id = pin.tab.id;
                let title = if pin.tab.display_title().is_empty() {
                    pin.tab.url.clone()
                } else {
                    pin.tab.display_title().to_owned()
                };

                div()
                    .id(("pinned-tab", id))
                    .size(px(30.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .bg(rgb(if self.pins.open == Some(id) {
                        colors.active_tab
                    } else {
                        colors.surface
                    }))
                    .border_1()
                    .border_color(rgb(if self.pins.open == Some(id) {
                        colors.accent
                    } else {
                        colors.border
                    }))
                    .hover(|s| s.bg(rgb(colors.hover)))
                    .when(!self.sleep.is_sleeping(id), |button| {
                        button.tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(title.clone()).build(window, cx)
                        })
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                        this.hover_sleep(
                            hovered.then_some(id),
                            window.mouse_position(),
                            window,
                            cx,
                        );
                    }))
                    .when(self.sleep.is_sleeping(id), |button| {
                        button.child(div().opacity(0.5).child("💤"))
                    })
                    .when(
                        !self.sleep.is_sleeping(id)
                            && pin.icon.is_none()
                            && !self.favicons.contains_key(&id),
                        |button| button.child(div().text_color(rgb(colors.muted)).child("◉")),
                    )
                    .children(
                        pin.icon
                            .as_ref()
                            .filter(|_| !self.sleep.is_sleeping(id))
                            .map(|icon| div().text_size(px(18.)).child(icon.clone())),
                    )
                    .children(
                        self.favicons
                            .get(&id)
                            .filter(|_| !self.sleep.is_sleeping(id) && pin.icon.is_none())
                            .map(|image| img(image.clone()).size(px(16.)).relative()),
                    )
                    .when(
                        self.pins
                            .drag
                            .is_some_and(|(tab, _, active)| tab == id && active),
                        |el| el.opacity(0.5),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            // Dismiss after a drag actually starts so a click can toggle the open pin.
                            let _ = window;
                            this.pins.suppress_click = false;
                            this.pins.drag = Some((id, event.position, false));
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.pin_menu(Some(id), window, cx);
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if !this.pins.suppress_click {
                            this.toggle_pin(id, window, cx);
                        }
                    }))
            }))
            .when(visible < self.model.pinned_tabs.len(), |strip| {
                strip.child(
                    div()
                        .id("pinned-overflow")
                        .size(px(30.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors.hover)))
                        .child("•••")
                        .tooltip(|w, cx| {
                            gpui_component::tooltip::Tooltip::new("More pinned tabs").build(w, cx)
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.pin_menu(None, window, cx)),
                        ),
                )
            })
            .into_any_element()
    }

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
        let sleeping = match target {
            TabTarget::Space(id) => self
                .model
                .spaces
                .iter()
                .find(|s| s.id == id)
                .is_some_and(|s| s.panes.iter().all(|p| self.sleep.is_sleeping(p.active_tab))),
            TabTarget::Page(_, id) => self.sleep.is_sleeping(id),
        };
        let notifications = match target {
            TabTarget::Space(space) => self.space_notifications(space),
            TabTarget::Page(_, tab) => {
                u64::from(self.notifications.get(&tab).copied().unwrap_or_default())
            }
        };
        div()
            .id((kind, id))
            .h(px(HEADER_CONTROL_HEIGHT))
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
            .when(
                self.space_drag
                    .is_some_and(|d| d.active && target == TabTarget::Space(d.id)),
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
            .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                if !*hovered {
                    this.hover_sleep(None, window.mouse_position(), window, cx);
                    return;
                }
                match target {
                    TabTarget::Page(_, id) => {
                        this.hover_sleep(Some(id), window.mouse_position(), window, cx)
                    }
                    TabTarget::Space(id) => {
                        this.hover_sleep_space(id, window.mouse_position(), window, cx)
                    }
                }
            }))
            // Keep tab clicks from bubbling into the surrounding pane header.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(if editing {
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .px_1()
                    .child(Input::new(&self.address).appearance(false))
                    .when_some(
                        self.history_popup.as_ref().map(|p| p.view.clone()),
                        |el, popup| {
                            let height = self.history_matches.len() as f32 * 53. + 8.;
                            el.child(
                                canvas(
                                    move |bounds, window, _| {
                                        let width =
                                            480_f32.min(f32::from(window.viewport_size().width));
                                        let x = f32::from(bounds.origin.x).min(
                                            (f32::from(window.viewport_size().width) - width)
                                                .max(0.),
                                        );
                                        let _ = popup.set_bounds(crate::browser::native_bounds(
                                            Bounds {
                                                origin: point(px(x), px(TOP)),
                                                size: size(
                                                    px(width),
                                                    px(height.min(
                                                        (f32::from(window.viewport_size().height)
                                                            - TOP)
                                                            .max(1.),
                                                    )),
                                                ),
                                            },
                                        ));
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            )
                        },
                    )
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
                    .when(sleeping, |title| title.opacity(0.5))
                    .children(
                        match target {
                            TabTarget::Space(id) => self
                                .model
                                .spaces
                                .iter()
                                .find(|s| s.id == id)
                                .and_then(|s| s.icon.as_ref()),
                            TabTarget::Page(_, _) => None,
                        }
                        .map(|icon| div().flex_shrink_0().mr_1().child(icon.clone())),
                    )
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
                            } else if let TabTarget::Space(id) = target {
                                this.arm_space_drag(id, event.position);
                            }
                        }),
                    )
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if matches!(target, TabTarget::Page(_, _)) && this.suppress_tab_click {
                            return;
                        }
                        if matches!(target, TabTarget::Space(_)) && this.suppress_space_click {
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
            .when(notifications > 0, |tab| {
                tab.child(
                    div()
                        .id(("notification-badge", id))
                        .flex_shrink_0()
                        .min_w(px(18.))
                        .h(px(18.))
                        .px(px(4.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(rgb(0xd71932))
                        .text_color(rgb(0xffffff))
                        .text_size(px(10.))
                        .font_weight(FontWeight::BOLD)
                        .cursor_pointer()
                        .child(if notifications > 99 {
                            "99+".to_owned()
                        } else {
                            notifications.to_string()
                        })
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(format!(
                                "{notifications} unread notifications"
                            ))
                            .build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.select_target(target, window, cx);
                        })),
                )
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
            .rounded(px(PANEL_CORNER_RADIUS))
            .overflow_hidden()
            .bg(rgb(colors.surface))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .rounded(px(PANEL_CORNER_RADIUS))
                    .bg(rgb(colors.page))
                    .child(if self.sleep.is_sleeping(tab) {
                        div()
                            .id(("sleeping-pane", tab))
                            .size_full()
                            .relative()
                            .cursor_pointer()
                            .children(self.sleep.image(tab).map(|image| {
                                img(image)
                                    .size_full()
                                    .rounded(px(PANEL_CORNER_RADIUS))
                                    .opacity(0.5)
                            }))
                            .when(self.sleep.image(tab).is_none(), |el| {
                                el.child(div().opacity(0.5).child(format!(
                                    "💤 {}",
                                    if pane.active_tab().display_title().is_empty() {
                                        "New tab"
                                    } else {
                                        pane.active_tab().display_title()
                                    }
                                )))
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.focus_pane(pane_id, window, cx);
                                cx.notify();
                            }))
                            .into_any_element()
                    } else if unavailable {
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(colors.muted))
                            .child("Page unavailable")
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
        let downloading = self.host.downloads().borrow().downloading_count();
        let content = panel_content_rect(f32::from(viewport.width), f32::from(viewport.height));
        let (_, pin_width) = crate::pinned_tabs::header_layout(
            f32::from(viewport.width),
            self.model.pinned_tabs.len(),
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
            .when(
                self.tab_drag_active() || self.space_drag.is_some_and(|d| d.active),
                |root| root.cursor(CursorStyle::ClosedHand),
            )
            .on_action(cx.listener(|this, _: &crate::Settings, window, cx| {
                this.open_settings(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::Address, window, cx| {
                this.command(Command::Address, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::NewTab, window, cx| {
                this.command(Command::NewTab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::NewSpace, window, cx| {
                this.command(Command::NewSpace, window, cx)
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
                    if matches!(this.editing, Some(TabTarget::Space(_)))
                        && !this.history_matches.is_empty()
                    {
                        this.dismiss_history();
                        cx.notify();
                        return;
                    }
                    this.close_downloads(window, cx);
                    this.dismiss_link_popup(window, cx);
                    this.close_settings(window, cx);
                    this.dismiss_pin(window, cx);
                    this.cancel_tab_drag(cx);
                    this.cancel_space_drag(cx);
                    this.close_space_menu(cx);
                    this.cancel_edit(window, cx);
                    cx.notify();
                }),
            )
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(this.editing, Some(TabTarget::Space(_)))
                    && this.history_key(&event.keystroke.key, window, cx)
                {
                    cx.stop_propagation();
                    window.prevent_default();
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close_downloads(window, cx);
                    this.dismiss_pin(window, cx);
                    this.close_space_menu(cx);
                    this.cancel_space_drag(cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    if this.pins.open.is_some() || this.pins.menu.is_some() {
                        this.dismiss_pin(window, cx);
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if let Some((id, origin, active)) = this.pins.drag
                    && (active
                        || (f32::from(event.position.x - origin.x).abs()
                            + f32::from(event.position.y - origin.y).abs())
                            > 5.)
                {
                    if !active {
                        this.dismiss_pin(window, cx);
                    }
                    this.pins.drag = Some((id, origin, true));
                    this.pins.suppress_click = true;
                    cx.notify();
                }
                this.update_tab_drag(event, window, cx);
                this.update_space_drag(event, window, cx);
                if this.moving_window {
                    this.moving_window = false;
                    window.start_window_move();
                }
                if let Some(divider) = &this.dragging {
                    this.host.request_redraw();
                    this.model.resize_split(
                        divider.id,
                        divider.drag_ratio(event.position.x.into(), event.position.y.into()),
                    );
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    if let Some((id, _, active)) = this.pins.drag.take()
                        && active
                        && this.pin_drop_target(event.position, window)
                    {
                        let w = f32::from(window.viewport_size().width);
                        let (_, strip) =
                            crate::pinned_tabs::header_layout(w, this.model.pinned_tabs.len());
                        let right = if cfg!(target_os = "windows") {
                            210.
                        } else {
                            76.
                        };
                        let slot = ((f32::from(event.position.x) - (w - right - strip) + 17.) / 34.)
                            .max(0.) as usize;
                        this.model.move_pin(id, slot);
                        cx.notify();
                    }
                    this.finish_tab_drag(Some(event.position), window, cx);
                    this.finish_space_drag(Some(event.position), cx);
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
                    this.pins.drag = None;
                    this.finish_tab_drag(None, window, cx);
                    this.finish_space_drag(None, cx);
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
                    .when(
                        self.tab_drag.is_some_and(|d| {
                            d.active && !d.whole && self.pin_drop_target(d.position, window)
                        }),
                        |header| header.bg(rgb(colors.hover)),
                    )
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(!cfg!(target_os = "windows"), |header| header.pr_2())
                    .child(
                        div()
                            .w(px(
                                if cfg!(target_os = "macos") && !window.is_fullscreen() {
                                    84.
                                } else {
                                    8.
                                },
                            ))
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .id("spaces")
                            .track_scroll(&self.space_scroll)
                            .when(cfg!(target_os = "windows"), |spaces| {
                                // Reserve caption buttons, gaps, and an empty drag target.
                                spaces.max_w(px(
                                    (f32::from(viewport.width) - 262. - pin_width).max(0.)
                                ))
                            })
                            .when(!cfg!(target_os = "windows"), |spaces| {
                                spaces.max_w(px(
                                    (f32::from(viewport.width) - 246. - pin_width).max(0.)
                                ))
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
                            .h(px(HEADER_CONTROL_HEIGHT))
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
                                cx.listener(|this, _, window, cx| {
                                    #[cfg(target_os = "macos")]
                                    this.report(crate::macos_titlebar::start_window_move(
                                        window, cx,
                                    ));
                                    #[cfg(not(target_os = "macos"))]
                                    {
                                        let _ = (window, cx);
                                        this.moving_window = true;
                                    }
                                }),
                            )
                            .when(!cfg!(target_os = "windows"), |drag| {
                                drag.on_double_click(|_, window, _| window.titlebar_double_click())
                            }),
                    )
                    .child(self.pinned_header(window, cx))
                    .child(
                        div()
                            .id("downloads-button")
                            .relative()
                            .track_focus(&self.downloads_focus)
                            .w(px(30.))
                            .h(px(HEADER_CONTROL_HEIGHT))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .cursor_pointer()
                            .when(self.downloads_popup.is_some(), |s| s.bg(rgb(colors.hover)))
                            .hover(|s| s.bg(rgb(colors.hover)))
                            .tooltip(move |window, cx| {
                                let label = if downloading == 0 {
                                    "Downloads".to_owned()
                                } else {
                                    format!("Downloads ({downloading} downloading)")
                                };
                                gpui_component::tooltip::Tooltip::new(label).build(window, cx)
                            })
                            .child(
                                div()
                                    .w(px(16.))
                                    .h(px(21.))
                                    .border_b_2()
                                    .border_color(rgb(colors.ink))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(22.))
                                    .child("↓"),
                            )
                            .when(downloading > 0, |button| {
                                button.child(
                                    div()
                                        .id("downloads-badge")
                                        .absolute()
                                        .top(px(-2.))
                                        .right(px(-2.))
                                        .min_w(px(14.))
                                        .h(px(14.))
                                        .px(px(3.))
                                        .rounded_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .bg(rgb(0xd71932))
                                        .text_color(rgb(0xffffff))
                                        .text_size(px(9.))
                                        .font_weight(FontWeight::BOLD)
                                        .child(downloading.to_string()),
                                )
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.downloads_focus.focus(window);
                                this.toggle_downloads(window, cx);
                            }))
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.toggle_downloads(window, cx);
                                }
                            })),
                    )
                    .child(
                        div()
                            .id("settings-button")
                            .track_focus(&self.settings_focus)
                            .w(px(30.))
                            .h(px(HEADER_CONTROL_HEIGHT))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .cursor_pointer()
                            .text_size(px(20.))
                            .hover(|s| s.bg(rgb(colors.hover)))
                            .tooltip(|window, cx| {
                                gpui_component::tooltip::Tooltip::new("Settings")
                                    .action(&crate::Settings, None)
                                    .build(window, cx)
                            })
                            .child("⚙")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.settings_focus.focus(window);
                                this.open_settings(false, window, cx)
                            }))
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.open_settings(false, window, cx);
                                }
                            })),
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
            .children(self.space_drop_indicator().map(|(x, y, height)| {
                div()
                    .absolute()
                    .left(x)
                    .top(y)
                    .w(px(3.))
                    .h(height)
                    .bg(rgb(colors.accent))
            }))
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
            .when(
                self.persistence_error.is_some() || !self.status.is_empty(),
                |root| {
                    root.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(22.))
                            .px_2()
                            .bg(rgb(colors.surface))
                            .child(
                                self.persistence_error
                                    .as_ref()
                                    .unwrap_or(&self.status)
                                    .clone(),
                            ),
                    )
                },
            )
    }
}
