use crate::browser::{BrowserEvent, BrowserHost, WryHost};
use gpui::{prelude::*, *};
use gpui_component::input::{InputEvent, InputState};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};
use workspace_core::{Axis, Divider, PaneId, Rect, SpaceId, TabId, Workspace, normalize_url};

/// The chrome level is part of the target: the same page can appear in both strips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabTarget {
    Space(SpaceId),
    Page(PaneId, TabId),
}

#[derive(Clone, Copy)]
pub enum TabAction {
    NewSibling,
    Split(Axis),
    Close,
}

#[derive(Clone, Copy, PartialEq)]
pub struct TabMenu {
    pub target: TabTarget,
    pub position: Point<Pixels>,
}

#[derive(Clone, Copy)]
pub enum Command {
    Address,
    NewTab,
    CloseTab,
    Reload,
    Back,
    Forward,
    Split(Axis),
    NewSpace,
    NextPane,
}

#[derive(Clone, Copy, Debug)]
pub struct TabDrag {
    pub source: PaneId,
    pub tab: TabId,
    pub origin: Point<Pixels>,
    pub position: Point<Pixels>,
    pub active: bool,
}

pub struct BrowserApp {
    pub model: Workspace,
    pub host: Rc<dyn BrowserHost>,
    pub address: Entity<InputState>,
    pub focus: FocusHandle,
    pub menu: Option<TabMenu>,
    pub menu_surface: Option<(TabMenu, crate::tab_menu::NativeTabMenu)>,
    pub editing: Option<TabTarget>,
    pub select_address: bool,
    pub moving_window: bool,
    pub space_scroll: ScrollHandle,
    pub pane_scrolls: HashMap<PaneId, ScrollHandle>,
    pub dragging: Option<Divider>,
    pub tab_drag: Option<TabDrag>,
    pub suppress_tab_click: bool,
    pub drag_preview: Option<crate::drag_preview::DragPreview>,
    pub status: String,
    pub poll_count: u8,
}

impl BrowserApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let address = cx.new(|cx| InputState::new(window, cx).default_value("about:blank"));
        cx.subscribe_in(
            &address,
            window,
            |this, input, event, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let value = input.read(cx).value().to_string();
                    this.navigate(&value, window, cx);
                }
                InputEvent::Blur if !input.read(cx).focus_handle(cx).is_focused(window) => {
                    this.cancel_edit(window, cx)
                }
                _ => {}
            },
        )
        .detach();
        cx.spawn_in(window, async |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                if view
                    .update_in(cx, |this, window, cx| this.poll(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.cancel_tab_drag(cx);
            }
        })
        .detach();
        Self {
            model: Workspace::new("about:blank"),
            host: Rc::new(WryHost::new()),
            address,
            focus: cx.focus_handle(),
            menu: None,
            menu_surface: None,
            editing: None,
            select_address: false,
            moving_window: false,
            space_scroll: ScrollHandle::new(),
            pane_scrolls: HashMap::new(),
            dragging: None,
            tab_drag: None,
            suppress_tab_click: false,
            drag_preview: None,
            status: String::new(),
            poll_count: 0,
        }
    }

    pub fn tab_drag_active(&self) -> bool {
        self.tab_drag.is_some_and(|drag| drag.active)
    }

    pub fn cancel_tab_drag(&mut self, cx: &mut Context<Self>) {
        self.drag_preview = None;
        self.host.request_redraw();
        if self.tab_drag.take().is_some() {
            cx.notify();
        }
    }

    pub fn arm_tab_drag(&mut self, source: PaneId, tab: TabId, origin: Point<Pixels>) {
        self.suppress_tab_click = false;
        self.tab_drag = Some(TabDrag {
            source,
            tab,
            origin,
            position: origin,
            active: false,
        });
    }

    pub fn update_tab_drag(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.tab_drag else { return };
        if !event.dragging() {
            self.cancel_tab_drag(cx);
            return;
        }
        self.tab_drag.as_mut().unwrap().position = event.position;
        let delta = event.position - drag.origin;
        if !drag.active && (f32::from(delta.x).powi(2) + f32::from(delta.y).powi(2)) >= 25. {
            self.cancel_edit(window, cx);
            self.menu = None;
            self.tab_drag.as_mut().unwrap().active = true;
            self.suppress_tab_click = true;
            self.report(self.host.focus_parent(self.active_tab()));
            self.focus.focus(window);
            if let Some(tab) = self.model.tab(drag.tab) {
                let result = self.host.ensure(drag.tab, &tab.url, window);
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
                        origin: point(px(r.x), px(r.y + 34.)),
                        size: size(px(r.width), px((r.height - 34.).max(1.))),
                    },
                ));
            }
            let title = self
                .model
                .tab(drag.tab)
                .map_or("New tab", |tab| tab.title.as_str());
            match self.host.drag_preview(drag.tab, title, window) {
                Ok(preview) => self.drag_preview = Some(preview),
                Err(error) => self.report(Err(error)),
            }
        }
        if let Some(preview) = &self.drag_preview {
            let result = preview.update(event.position, window.viewport_size());
            self.report(result);
        }
        if self.tab_drag_active() {
            self.host.request_redraw();
            cx.notify();
        }
    }

    /// Hit testing shares the exact fixed tab width/gap and scroll offset with rendering.
    pub fn tab_drop_target(
        &self,
        position: Point<Pixels>,
        window: &Window,
    ) -> Option<(PaneId, usize)> {
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
            let r = pane.bounds;
            let x = f32::from(position.x);
            let y = f32::from(position.y);
            if pane.id == drag.source
                || x < r.x + 4.
                || x >= r.x + r.width - 4.
                || y < r.y
                || y >= r.y + 34.
            {
                return None;
            }
            let offset = self
                .pane_scrolls
                .get(&pane.id)
                .map_or(0., |s| f32::from(s.offset().x));
            let count = self.model.pane(pane.id)?.tabs.len();
            Some((
                pane.id,
                crate::render::tab_insertion_index(x - r.x - 4. - offset, count),
            ))
        })
    }

    pub fn finish_tab_drag(
        &mut self,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_preview = None;
        self.host.request_redraw();
        let target = position.and_then(|position| self.tab_drop_target(position, window));
        if let Some(drag) = self.tab_drag.take() {
            if let Some((pane, index)) = target
                && self.model.move_tab(drag.source, drag.tab, pane, index)
            {
                self.sync_address(window, cx);
                self.reveal_focused();
            }
            cx.notify();
        }
    }

    pub fn report(&mut self, result: anyhow::Result<()>) {
        if let Err(error) = result {
            self.status = error.to_string();
            eprintln!("Browser: {error:#}");
        }
    }
    pub fn active_tab(&self) -> TabId {
        self.model.focused_pane().active_tab
    }
    fn reveal_focused(&self) {
        if let Some(index) = self
            .model
            .spaces
            .iter()
            .position(|s| s.id == self.model.active_space)
        {
            self.space_scroll.scroll_to_item(index);
        }
        let pane = self.model.focused_pane();
        if let Some(scroll) = self.pane_scrolls.get(&pane.id)
            && let Some(index) = pane.tabs.iter().position(|t| t.id == pane.active_tab)
        {
            scroll.scroll_to_item(index);
        }
    }
    pub fn sync_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let url = self.model.focused_pane().active_tab().url.clone();
        self.address
            .update(cx, |input, cx| input.set_value(url, window, cx));
    }
    pub fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            self.sync_address(window, cx);
            cx.notify();
        }
    }
    pub fn select_target(
        &mut self,
        target: TabTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.report(self.host.focus_parent(self.active_tab()));
        if self.editing != Some(target) {
            self.cancel_edit(window, cx);
        }
        self.menu = None;
        match target {
            TabTarget::Space(id) => {
                self.model.switch_space(id);
            }
            TabTarget::Page(pane, tab) => {
                self.model.switch_tab(pane, tab);
            }
        }
        if self.editing.is_none() {
            self.focus.focus(window);
        }
        self.sync_address(window, cx);
        self.reveal_focused();
        cx.notify();
    }
    pub fn begin_edit(&mut self, target: TabTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.select_target(target, window, cx);
        self.editing = None;
        self.sync_address(window, cx);
        self.editing = Some(target);
        self.address.update(cx, |input, cx| input.focus(window, cx));
        // The input is mounted only while editing. Select after the next render
        // has registered its action handlers, rather than dispatching to the old tab title.
        self.select_address = true;
        cx.notify();
    }
    pub fn open_menu(
        &mut self,
        target: TabTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu.is_some_and(|menu| menu.target == target) {
            self.menu = None;
        } else {
            self.cancel_edit(window, cx);
            self.report(self.host.focus_parent(self.active_tab()));
            self.focus.focus(window);
            self.menu = Some(TabMenu { target, position });
        }
        self.host.request_redraw();
        cx.notify();
    }
    pub fn tab_action(
        &mut self,
        target: TabTarget,
        action: TabAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_target(target, window, cx);
        self.cancel_edit(window, cx);
        match action {
            TabAction::NewSibling => match target {
                TabTarget::Space(_) => self.command(Command::NewSpace, window, cx),
                TabTarget::Page(_, _) => self.command(Command::NewTab, window, cx),
            },
            TabAction::Split(axis) => self.command(Command::Split(axis), window, cx),
            TabAction::Close => {
                match target {
                    TabTarget::Space(id) => {
                        self.model.close_space(id);
                    }
                    TabTarget::Page(pane, tab) => {
                        self.model.close_tab(pane, tab);
                    }
                }
                self.sync_address(window, cx);
            }
        }
        cx.notify();
    }
    pub fn focus_pane(&mut self, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if pane != self.model.focused_pane().id {
            self.cancel_edit(window, cx);
        }
        self.model.focus_pane(pane);
        self.sync_address(window, cx);
        cx.notify();
    }
    pub fn command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        if !matches!(command, Command::Address) {
            if self.editing.is_some() {
                self.focus.focus(window);
            }
            self.cancel_edit(window, cx);
        }
        match command {
            Command::Address => {
                let target = self.editing.unwrap_or(TabTarget::Page(
                    self.model.focused_pane().id,
                    self.active_tab(),
                ));
                self.begin_edit(target, window, cx);
            }
            Command::NewTab => {
                self.report(self.host.focus_parent(self.active_tab()));
                self.focus.focus(window);
                self.model.add_tab("about:blank");
                self.sync_address(window, cx);
            }
            Command::CloseTab => {
                self.report(self.host.focus_parent(self.active_tab()));
                self.focus.focus(window);
                let pane = self.model.focused_pane().id;
                self.model.close_tab(pane, self.active_tab());
                self.sync_address(window, cx);
            }
            Command::Reload => self.report(self.host.reload(self.active_tab())),
            Command::Back => self.report(self.host.back(self.active_tab())),
            Command::Forward => self.report(self.host.forward(self.active_tab())),
            Command::Split(axis) => {
                self.model.split_focused(axis, "about:blank");
                self.sync_address(window, cx);
            }
            Command::NewSpace => {
                let name = format!("Space {}", self.model.spaces.len() + 1);
                self.model.add_space(&name, "about:blank");
                self.sync_address(window, cx);
            }
            Command::NextPane => {
                let space = self.model.active_space();
                let index = space
                    .panes
                    .iter()
                    .position(|p| p.id == space.focused_pane)
                    .unwrap_or(0);
                let next = space.panes[(index + 1) % space.panes.len()].id;
                self.focus_pane(next, window, cx);
                self.report(self.host.focus(self.active_tab()));
            }
        }
        self.reveal_focused();
        cx.notify();
    }
    fn navigate(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        match normalize_url(value) {
            Ok(url) => {
                self.status.clear();
                if self.editing.is_none() {
                    return;
                }
                let tab = self.active_tab();
                if let Some(tab) = self.model.tab_mut(tab) {
                    tab.url = url.clone();
                }
                self.report(self.host.navigate(tab, &url));
                self.cancel_edit(window, cx);
                self.report(self.host.focus(tab));
            }
            Err(error) => self.status = error.to_string(),
        }
        cx.notify();
    }
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(event) = self
            .menu_surface
            .as_ref()
            .and_then(|(_, menu)| menu.next_event())
        {
            let target = self.menu.take().map(|menu| menu.target);
            self.menu_surface = None;
            self.report(self.host.focus_parent(self.active_tab()));
            self.focus.focus(window);
            if let (Some(target), crate::tab_menu::MenuEvent::Action(action)) = (target, event) {
                self.tab_action(target, action, window, cx);
            }
            self.host.request_redraw();
            cx.notify();
        }
        if let Some(preview) = &self.drag_preview {
            let result = preview.update(
                self.tab_drag.expect("preview belongs to a drag").position,
                window.viewport_size(),
            );
            self.report(result);
        }
        let mut events = self.host.drain_events();
        self.poll_count = (self.poll_count + 1) % 12;
        if self.poll_count == 0 {
            let tab = self.active_tab();
            if let Ok(url) = self.host.current_url(tab)
                && self.model.tab(tab).is_some_and(|page| page.url != url)
                && !url.is_empty()
            {
                events.push(BrowserEvent::Navigated(tab, url));
            }
        }
        if events.is_empty() {
            return;
        }
        for event in events {
            match event {
                BrowserEvent::Focused(tab) => {
                    // Native child focus is outside GPUI's focus tree, so it may
                    // not emit InputEvent::Blur (including clicks in the same pane).
                    self.cancel_edit(window, cx);
                    let pane = self
                        .model
                        .active_space()
                        .panes
                        .iter()
                        .find(|p| p.active_tab == tab)
                        .map(|p| p.id);
                    if let Some(pane) = pane
                        && pane != self.model.focused_pane().id
                    {
                        self.focus_pane(pane, window, cx);
                    }
                }
                BrowserEvent::Navigated(tab, url) => {
                    if let Some(page) = self.model.tab_mut(tab) {
                        page.url = url;
                    }
                    if tab == self.active_tab() {
                        self.sync_address(window, cx);
                    }
                }
                BrowserEvent::Title(tab, title) => {
                    if let Some(page) = self.model.tab_mut(tab) {
                        page.title = title;
                    }
                }
                BrowserEvent::Loaded(tab) => {
                    if tab == self.active_tab() {
                        self.status.clear();
                    }
                }
                BrowserEvent::NewTab(source, url) => {
                    let pane = self
                        .model
                        .active_space()
                        .panes
                        .iter()
                        .find(|p| p.tabs.iter().any(|t| t.id == source))
                        .map(|p| p.id);
                    if let Some(pane) = pane {
                        self.cancel_edit(window, cx);
                        self.model.focus_pane(pane);
                        self.model.add_tab(&url);
                        self.sync_address(window, cx);
                    }
                }
                BrowserEvent::Shortcut(tab, key) => {
                    let pane = self
                        .model
                        .active_space()
                        .panes
                        .iter()
                        .find(|p| p.active_tab == tab)
                        .map(|p| p.id);
                    let Some(pane) = pane else {
                        continue;
                    };
                    if self.menu.is_some() || self.dragging.is_some() || self.tab_drag_active() {
                        continue;
                    }
                    self.model.focus_pane(pane);
                    let command = match key.as_str() {
                        "address" => Some(Command::Address),
                        "new-tab" => Some(Command::NewTab),
                        "close-tab" => Some(Command::CloseTab),
                        "reload" => Some(Command::Reload),
                        _ => None,
                    };
                    if let Some(command) = command {
                        self.command(command, window, cx);
                    }
                }
            }
        }
        cx.notify();
    }
    pub fn prepare_views(&mut self, window: &mut Window) {
        let ids: HashSet<_> = self
            .model
            .spaces
            .iter()
            .flat_map(|s| &s.panes)
            .flat_map(|p| &p.tabs)
            .map(|t| t.id)
            .collect();
        self.host.retain(&ids);
        let panes: HashSet<_> = self
            .model
            .spaces
            .iter()
            .flat_map(|s| &s.panes)
            .map(|p| p.id)
            .collect();
        self.pane_scrolls.retain(|id, _| panes.contains(id));
        for id in panes {
            self.pane_scrolls.entry(id).or_default();
        }
        let viewport = window.viewport_size();
        let content = Rect::new(
            0.,
            crate::render::TOP,
            f32::from(viewport.width),
            (f32::from(viewport.height) - crate::render::TOP).max(0.),
        );
        let geometry = self.model.active_space().layout.geometry(content, 6., 100.);
        let visible: Vec<_> = self
            .model
            .active_space()
            .panes
            .iter()
            .filter(|p| {
                geometry
                    .panes
                    .iter()
                    .any(|r| r.id == p.id && r.bounds.height > 36. && r.bounds.width > 2.)
            })
            .map(|p| (p.active_tab, p.active_tab().url.clone()))
            .collect();
        // Hide only inactive surfaces; hiding the focused view every frame steals page input.
        for id in &ids {
            if !visible.iter().any(|(tab, _)| tab == id) {
                self.report(self.host.show(*id, false));
            }
        }
        for (id, url) in visible {
            let result = self.host.ensure(id, &url, window);
            self.report(result);
            self.report(self.host.show(id, true));
        }
        if let Some(menu) = self.menu {
            if !self
                .menu_surface
                .as_ref()
                .is_some_and(|(current, _)| *current == menu)
            {
                self.menu_surface = None;
                match crate::tab_menu::NativeTabMenu::new(
                    menu.position,
                    matches!(menu.target, TabTarget::Space(_)),
                    window,
                ) {
                    Ok(surface) => self.menu_surface = Some((menu, surface)),
                    Err(error) => {
                        self.report(Err(error));
                        self.menu = None;
                    }
                }
            }
            if let Some((_, surface)) = &self.menu_surface {
                let result = surface.place(window.viewport_size());
                self.report(result);
            }
        } else {
            self.menu_surface = None;
        }
    }
}
