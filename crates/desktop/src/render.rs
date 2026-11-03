use crate::app::{BrowserApp, Command};
use gpui::{prelude::*, *};
use gpui_component::input::Input;
use workspace_core::{Axis, PaneId, Rect};

const TOP: f32 = 86.;
const INK: u32 = 0xdde5ed;
const SURFACE: u32 = 0x181e27;
const MUTED: u32 = 0x8491a2;
const ACCENT: u32 = 0x8fddc3;

impl BrowserApp {
    fn button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        command: Command,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .px_2()
            .h(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0x303b49)))
            .child(label.into())
            .on_click(cx.listener(move |this, _, window, cx| this.command(command, window, cx)))
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
                let id = tab.id;
                let active = id == pane.active_tab;
                let title: String = tab.title.chars().take(26).collect();
                div()
                    .id(("tab", id))
                    .px_2()
                    .h(px(28.))
                    .max_w(px(200.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .bg(rgb(if active { 0x303b49 } else { SURFACE }))
                    .cursor_pointer()
                    .text_color(rgb(if active { INK } else { MUTED }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.report(this.host.focus_parent(this.active_tab()));
                        this.focus.focus(window);
                        this.model.focus_pane(pane_id);
                        this.model.switch_tab(pane_id, id);
                        this.sync_address(window, cx);
                        cx.notify();
                    }))
                    .child(if title.is_empty() {
                        "New tab".to_string()
                    } else {
                        title
                    })
                    .child(
                        div()
                            .id(("close-tab", id))
                            .text_color(rgb(MUTED))
                            .child("×")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.model.focus_pane(pane_id);
                                this.model.close_tab(pane_id, id);
                                this.sync_address(window, cx);
                                cx.notify();
                            })),
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
            .border_1()
            .border_color(rgb(if focused { ACCENT } else { 0x303845 }))
            .rounded_md()
            .overflow_hidden()
            .child(
                div()
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.focus_pane(pane_id, window, cx)
                        }),
                    )
                    .child(
                        div()
                            .text_color(rgb(if focused { ACCENT } else { MUTED }))
                            .px_1()
                            .child(format!("{}", pane_id)),
                    )
                    .child(
                        div()
                            .id(("pane-tabs", pane_id))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .gap_1()
                            .overflow_x_scroll()
                            .children(tabs),
                    )
                    .child(self.button(("new-tab", pane_id), "+", Command::NewTab, cx))
                    .child(
                        div()
                            .id(("maximize", pane_id))
                            .px_2()
                            .cursor_pointer()
                            .child("⤢")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.maximized = if this.maximized == Some(pane_id) {
                                    None
                                } else {
                                    Some(pane_id)
                                };
                                cx.notify();
                            })),
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
                    .child(if self.menu || self.dragging.is_some() || unavailable {
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(0x627083))
                            .child(if unavailable {
                                "Page unavailable · see status below"
                            } else {
                                "Page paused visually"
                            })
                            .into_any_element()
                    } else {
                        canvas(
                            move |bounds, _, _| {
                                if let Some(page) = &page {
                                    let result = page.place(tab, bounds);
                                    if let Err(error) = result {
                                        eprintln!("Webview bounds: {error}");
                                    }
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
        let viewport = window.viewport_size();
        let content = Rect {
            x: 6.,
            y: TOP,
            width: (f32::from(viewport.width) - 12.).max(0.),
            height: (f32::from(viewport.height) - TOP - 24.).max(0.),
        };
        let geometry = self.model.active_space().layout.geometry(content, 6., 100.);
        let panes: Vec<_> = if let Some(id) = self.maximized {
            vec![self.pane(id, content, cx)]
        } else {
            geometry
                .panes
                .iter()
                .map(|p| self.pane(p.id, p.bounds, cx))
                .collect()
        };
        let dividers = if self.maximized.is_none() {
            geometry.dividers
        } else {
            vec![]
        };
        let spaces = self
            .model
            .spaces
            .iter()
            .map(|space| {
                let id = space.id;
                div()
                    .id(("space", id))
                    .h(px(30.))
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .bg(rgb(if id == self.model.active_space {
                        0x303b49
                    } else {
                        SURFACE
                    }))
                    .text_color(rgb(if id == self.model.active_space {
                        ACCENT
                    } else {
                        MUTED
                    }))
                    .cursor_pointer()
                    .child(space.name.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.model.switch_space(id);
                        this.maximized = None;
                        this.sync_address(window, cx);
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();
        div().size_full().relative().bg(rgb(0x11161e)).text_color(rgb(INK)).text_size(px(12.)).font_family(if cfg!(target_os = "windows") { "Segoe UI" } else { ".SystemUIFont" })
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &crate::Address, window,cx| this.command(Command::Address,window,cx)))
            .on_action(cx.listener(|this, _: &crate::NewTab, window,cx| this.command(Command::NewTab,window,cx)))
            .on_action(cx.listener(|this, _: &crate::CloseTab, window,cx| this.command(Command::CloseTab,window,cx)))
            .on_action(cx.listener(|this, _: &crate::Reload, window,cx| this.command(Command::Reload,window,cx)))
            .on_action(cx.listener(|this, _: &crate::NextPane, window,cx| this.command(Command::NextPane,window,cx)))
            .on_mouse_move(cx.listener(|this,event: &MouseMoveEvent,_,cx| {
                if let Some(divider) = &this.dragging {
                    let (position,start,length) = match divider.axis {
                        Axis::Horizontal => (f32::from(event.position.x),divider.container.x,divider.container.width),
                        Axis::Vertical => (f32::from(event.position.y),divider.container.y,divider.container.height),
                    };
                    this.model.resize_split(divider.id, (position-start-3.)/(length-6.).max(1.)); cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left,cx.listener(|this,_,_,cx| { if this.dragging.take().is_some() {cx.notify();} }))
            .on_mouse_up_out(MouseButton::Left,cx.listener(|this,_,_,cx| { if this.dragging.take().is_some() {cx.notify();} }))
            .child(div().h(px(42.)).px_2().flex().items_center().gap_1()
                .child(div().px_2().text_color(rgb(ACCENT)).child("axolotl"))
                .child(div().id("spaces").flex_1().min_w_0().flex().gap_1().overflow_x_scroll().children(spaces)).child(self.button("new-space","+",Command::NewSpace,cx))
                .child(self.button("split-h","Split ↔",Command::Split(Axis::Horizontal),cx))
                .child(self.button("split-v","Split ↕",Command::Split(Axis::Vertical),cx))
                .child(self.button("grid","▦ Grid",Command::Grid,cx))
                .child(self.button("menu","···",Command::Menu,cx)))
            .child(div().h(px(38.)).px_2().flex().items_center().gap_1()
                .child(self.button("back","←",Command::Back,cx))
                .child(self.button("forward","→",Command::Forward,cx))
                .child(self.button("reload","↻",Command::Reload,cx))
                .child(div().flex_1().capture_any_mouse_down(cx.listener(|this,_,_,_| { this.report(this.host.focus_parent(this.active_tab())); })).child(Input::new(&self.address)))
                .child(div().w(px(80.)).text_color(rgb(MUTED)).text_center().child(format!("Pane {}",self.model.focused_pane().id))))
            .children(panes)
            .children(dividers.into_iter().map(|divider| {
                let bounds = divider.bounds;
                div().id(("divider",divider.id)).absolute().left(px(bounds.x)).top(px(bounds.y)).w(px(bounds.width)).h(px(bounds.height))
                    .cursor(match divider.axis {Axis::Horizontal=>CursorStyle::ResizeLeftRight,Axis::Vertical=>CursorStyle::ResizeUpDown})
                    .hover(|s|s.bg(rgb(ACCENT)))
                    .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx| {
                        this.dragging = Some(divider); this.report(this.host.hide_all()); cx.notify();
                    }))
            }))
            .child(div().absolute().bottom_0().left_0().right_0().h(px(20.)).px_3().flex().items_center().text_size(px(10.)).text_color(rgb(MUTED))
                .child(if self.status.is_empty() {"⌘/Ctrl L  Address    ·    T  New tab    ·    W  Close tab    ·    R  Reload    ·    Drag dividers to resize".to_string()} else {self.status.clone()}))
            .when(self.menu, |root| root.child(
                div().absolute().inset_0().bg(rgba(0x00000055))
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,window,cx|this.command(Command::Menu,window,cx)))
                    .child(div().absolute().right(px(12.)).top(px(42.)).w(px(270.)).p_3().rounded_lg().bg(rgb(0x25303d)).border_1().border_color(rgb(0x465366)).shadow_lg()
                        .on_mouse_down(MouseButton::Left,|_,_,cx|cx.stop_propagation())
                        .child(div().mb_2().text_color(rgb(ACCENT)).child("Space layout"))
                        .child(self.button("menu-h","Split side by side",Command::Split(Axis::Horizontal),cx))
                        .child(self.button("menu-v","Split top and bottom",Command::Split(Axis::Vertical),cx))
                        .child(self.button("menu-grid","Arrange 2 × 2 grid",Command::Grid,cx))
                        .child(self.button("menu-close","Close focused pane",Command::ClosePane,cx))
                        .child(div().mt_3().text_size(px(11.)).text_color(rgb(MUTED)).child("Pages remain loaded while this menu is open."))
                        .child(self.button("menu-done","Done",Command::Menu,cx)))
            ))
    }
}
