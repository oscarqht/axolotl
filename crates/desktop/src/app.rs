use crate::browser::{BrowserEvent, BrowserHost, WryHost};
use gpui::{prelude::*, *};
use gpui_component::input::{InputEvent, InputState};
use std::{collections::HashSet, rc::Rc, time::Duration};
use workspace_core::{Axis, Divider, PaneId, Rect, TabId, Workspace, normalize_url};

#[derive(Clone, Copy)]
pub enum Command {
    Address,
    NewTab,
    CloseTab,
    Reload,
    Back,
    Forward,
    Split(Axis),
    Grid,
    NewSpace,
    Menu,
    ClosePane,
    NextPane,
}

pub struct BrowserApp {
    pub model: Workspace,
    pub host: Rc<dyn BrowserHost>,
    pub address: Entity<InputState>,
    pub focus: FocusHandle,
    pub menu: bool,
    pub dragging: Option<Divider>,
    pub status: String,
    pub maximized: Option<PaneId>,
    pub poll_count: u8,
}

impl BrowserApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let address = cx.new(|cx| InputState::new(window, cx).default_value("about:blank"));
        cx.subscribe_in(&address, window, |this, input, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let value = input.read(cx).value().to_string();
                this.navigate(&value, window, cx);
            }
        })
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
        Self {
            model: Workspace::new("about:blank"),
            host: Rc::new(WryHost::new()),
            address,
            focus: cx.focus_handle(),
            menu: false,
            dragging: None,
            status: String::new(),
            maximized: None,
            poll_count: 0,
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
    pub fn sync_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.model.focused_pane().active_tab().url.clone();
        self.address
            .update(cx, |input, cx| input.set_value(url, window, cx));
    }
    pub fn focus_pane(&mut self, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        self.model.focus_pane(pane);
        self.sync_address(window, cx);
        cx.notify();
    }
    pub fn command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::Address => {
                self.report(self.host.focus_parent(self.active_tab()));
                self.address.update(cx, |input, cx| input.focus(window, cx));
                window.dispatch_action(Box::new(gpui_component::input::SelectAll), cx);
            }
            Command::NewTab => {
                self.report(self.host.focus_parent(self.active_tab()));
                self.model.add_tab("about:blank");
                self.sync_address(window, cx);
                self.command(Command::Address, window, cx);
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
                self.maximized = None;
                self.model.split_focused(axis, "about:blank");
                self.sync_address(window, cx);
            }
            Command::Grid => {
                self.maximized = None;
                self.model.grid("about:blank");
                self.sync_address(window, cx);
            }
            Command::NewSpace => {
                let name = format!("Space {}", self.model.spaces.len() + 1);
                self.model.add_space(&name, "about:blank");
                self.maximized = None;
                self.sync_address(window, cx);
            }
            Command::ClosePane => {
                self.model.close_pane(self.model.focused_pane().id);
                self.maximized = None;
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
            Command::Menu => {
                self.menu = !self.menu;
                if self.menu {
                    self.report(self.host.focus_parent(self.active_tab()));
                    self.report(self.host.hide_all());
                    self.focus.focus(window);
                }
            }
        }
        cx.notify();
    }
    fn navigate(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        match normalize_url(value) {
            Ok(url) => {
                self.status.clear();
                let tab = self.active_tab();
                if let Some(tab) = self.model.tab_mut(tab) {
                    tab.url = url.clone();
                }
                self.report(self.host.navigate(tab, &url));
                self.sync_address(window, cx);
            }
            Err(error) => self.status = error.to_string(),
        }
        cx.notify();
    }
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                        .find(|p| p.active_tab == tab && self.maximized.is_none_or(|id| id == p.id))
                        .map(|p| p.id);
                    let Some(pane) = pane else {
                        continue;
                    };
                    if self.menu || self.dragging.is_some() {
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
        let viewport = window.viewport_size();
        let content = Rect::new(
            6.,
            86.,
            (f32::from(viewport.width) - 12.).max(0.),
            (f32::from(viewport.height) - 110.).max(0.),
        );
        let geometry = self.model.active_space().layout.geometry(content, 6., 100.);
        let visible: Vec<_> = self
            .model
            .active_space()
            .panes
            .iter()
            .filter(|p| {
                self.maximized == Some(p.id)
                    || geometry
                        .panes
                        .iter()
                        .any(|r| r.id == p.id && r.bounds.height > 36. && r.bounds.width > 2.)
            })
            .filter(|p| self.maximized.is_none_or(|id| id == p.id))
            .map(|p| (p.active_tab, p.active_tab().url.clone()))
            .collect();
        // Hide only inactive surfaces; hiding the focused view every frame steals page input.
        for id in &ids {
            if !visible.iter().any(|(tab, _)| tab == id) || self.menu || self.dragging.is_some() {
                self.report(self.host.show(*id, false));
            }
        }
        for (id, url) in visible {
            let result = self.host.ensure(id, &url, window);
            self.report(result);
            self.report(self.host.show(id, !self.menu && self.dragging.is_none()));
        }
    }
}
