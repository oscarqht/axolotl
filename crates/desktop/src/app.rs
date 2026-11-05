use crate::browser::{BrowserEvent, BrowserHost, WryHost};
use gpui::{prelude::*, *};
use gpui_component::input::{InputEvent, InputState};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    time::{Duration, Instant},
};
use workspace_core::{Divider, PaneId, SpaceId, TabId, Workspace, normalize_url_with_engine};

#[cfg(feature = "smoke")]
use workspace_core::Axis;

/// The chrome level is part of the target: the same page can appear in both strips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabTarget {
    Space(SpaceId),
    Page(PaneId, TabId),
}

#[derive(Clone, Copy)]
pub enum TabAction {
    #[cfg(feature = "smoke")]
    NewSibling,
    #[cfg(feature = "smoke")]
    Split(Axis),
    Close,
}

#[derive(Clone, Copy)]
pub enum Command {
    Address,
    Settings,
    NewTab,
    CloseTab,
    Reload,
    Back,
    Forward,
    #[cfg(feature = "smoke")]
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
    pub whole: bool,
}

#[derive(Default)]
pub struct SwipeState {
    last: Option<Instant>,
    distance: f64,
    fired: bool,
}
impl SwipeState {
    pub(crate) fn feed(&mut self, x: f64, y: f64, now: Instant) -> Option<bool> {
        if self
            .last
            .is_none_or(|last| now.duration_since(last) > Duration::from_millis(300))
        {
            self.distance = 0.;
            self.fired = false;
        }
        self.last = Some(now);
        if x.abs() <= y.abs() || self.fired {
            return None;
        }
        self.distance += x;
        if self.distance.abs() < 65. {
            return None;
        }
        self.fired = true;
        Some(self.distance > 0.)
    }
}

pub struct BrowserApp {
    pub swipes: HashMap<PaneId, SwipeState>,
    pub bars: HashMap<PaneId, crate::floating_bar::FloatingBar>,
    pub(crate) chrome_views: HashSet<TabId>,
    pub model: Workspace,
    pub preferences: crate::preferences::Preferences,
    pub settings: Option<crate::settings::SettingsDialog>,
    pub settings_focus: FocusHandle,
    saver: crate::persistence::SessionSaver,
    pub host: Rc<dyn BrowserHost>,
    pub address: Entity<InputState>,
    pub focus: FocusHandle,
    pub editing: Option<TabTarget>,
    pub select_address: bool,
    pub moving_window: bool,
    pub space_scroll: ScrollHandle,
    pub dragging: Option<Divider>,
    pub space_drag: Option<crate::space_drag::SpaceDrag>,
    pub suppress_space_click: bool,
    pub tab_drag: Option<TabDrag>,
    pub suppress_tab_click: bool,
    pub drag_preview: Option<crate::drag_preview::DragPreview>,
    pub split_preview: Option<crate::split_preview::SplitPreview>,
    /// Native context menu/editor for the right-clicked space.
    pub space_menu: Option<crate::space_menu::SpaceMenu>,
    pub status: String,
    pub poll_count: u8,
    /// Last history probe per tab: (can go back, can go forward).
    pub history: HashMap<TabId, (bool, bool)>,
    pub notifications: HashMap<TabId, u32>,
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
                if let Err(error) = view.update_in(cx, |this, window, cx| this.poll(window, cx)) {
                    // Native Windows calls can pump tasks during another update.
                    // A temporary borrow conflict must not stop polling forever.
                    if error.downcast_ref::<std::cell::BorrowMutError>().is_some() {
                        continue;
                    }
                    break;
                }
            }
        })
        .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.space_menu = None;
                this.cancel_tab_drag(cx);
                this.cancel_space_drag(cx);
            } else {
                this.notifications.remove(&this.active_tab());
                cx.notify();
            }
        })
        .detach();
        gpui_component::Theme::sync_system_appearance(Some(window), cx);
        cx.observe_window_appearance(window, |this, window, cx| {
            gpui_component::Theme::sync_system_appearance(Some(window), cx);
            this.host.request_redraw();
            cx.notify();
        })
        .detach();
        let preferences = crate::preferences::Preferences::restore();
        let model = crate::persistence::restore(&preferences.homepage_url);
        let saver = crate::persistence::SessionSaver::new(&model);
        // Cmd+Q, window close and the app menu all end in cx.quit().
        cx.on_app_quit(|this, _| {
            this.saver.flush(&this.model);
            async {}
        })
        .detach();
        Self {
            bars: HashMap::new(),
            chrome_views: HashSet::new(),
            swipes: HashMap::new(),
            model,
            preferences,
            settings: None,
            settings_focus: cx.focus_handle(),
            saver,
            host: Rc::new(WryHost::new()),
            address,
            focus: cx.focus_handle(),
            editing: None,
            select_address: false,
            moving_window: false,
            space_scroll: ScrollHandle::new(),
            dragging: None,
            space_drag: None,
            suppress_space_click: false,
            tab_drag: None,
            suppress_tab_click: false,
            drag_preview: None,
            split_preview: None,
            space_menu: None,
            status: String::new(),
            poll_count: 0,
            history: HashMap::new(),
            notifications: HashMap::new(),
        }
    }

    pub fn report(&mut self, result: anyhow::Result<()>) {
        if let Err(error) = result {
            self.status = error.to_string();
            eprintln!("Browser: {error:#}");
        }
    }
    pub(crate) fn space_notifications(&self, space: SpaceId) -> u64 {
        self.model
            .spaces
            .iter()
            .find(|s| s.id == space)
            .map_or(0, |s| {
                s.panes
                    .iter()
                    .flat_map(|p| &p.tabs)
                    .map(|t| u64::from(self.notifications.get(&t.id).copied().unwrap_or_default()))
                    .sum()
            })
    }

    pub fn active_tab(&self) -> TabId {
        self.model.focused_pane().active_tab
    }
    pub(crate) fn reveal_focused(&self) {
        if let Some(index) = self
            .model
            .spaces
            .iter()
            .position(|s| s.id == self.model.active_space)
        {
            self.space_scroll.scroll_to_item(index);
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
        self.notifications.remove(&self.active_tab());
        self.sync_address(window, cx);
        self.reveal_focused();
        cx.notify();
    }
    pub fn close_space_menu(&mut self, cx: &mut Context<Self>) {
        if self.space_menu.take().is_some() {
            cx.notify();
        }
    }
    pub fn begin_edit(&mut self, target: TabTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.space_menu = None;
        self.select_target(target, window, cx);
        self.editing = None;
        self.sync_address(window, cx);
        self.editing = Some(target);
        if matches!(target, TabTarget::Space(_)) {
            self.address.update(cx, |input, cx| input.focus(window, cx));
        }
        // The input is mounted only while editing. Select after the next render
        // has registered its action handlers, rather than dispatching to the old tab title.
        self.select_address = matches!(target, TabTarget::Space(_));
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
            #[cfg(feature = "smoke")]
            TabAction::NewSibling => match target {
                TabTarget::Space(_) => self.command(Command::NewSpace, window, cx),
                TabTarget::Page(_, _) => self.command(Command::NewTab, window, cx),
            },
            #[cfg(feature = "smoke")]
            TabAction::Split(axis) => self.command(Command::Split(axis), window, cx),
            TabAction::Close => {
                match target {
                    TabTarget::Space(id) => {
                        self.model.close_space(id);
                    }
                    TabTarget::Page(pane, tab) => {
                        self.close_tab(pane, tab);
                    }
                }
                self.sync_address(window, cx);
            }
        }
        cx.notify();
    }
    fn close_tab(&mut self, pane: PaneId, tab: TabId) {
        let replaces_last =
            self.model.active_space().panes.len() == 1 && self.model.focused_pane().tabs.len() == 1;
        if self.model.close_tab(pane, tab) && replaces_last {
            let replacement = self.active_tab();
            self.model.tab_mut(replacement).unwrap().url = self.preferences.homepage_url.clone();
        }
    }
    pub fn focus_pane(&mut self, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if pane != self.model.focused_pane().id {
            self.cancel_edit(window, cx);
        }
        self.model.focus_pane(pane);
        self.notifications.remove(&self.active_tab());
        self.sync_address(window, cx);
        cx.notify();
    }
    pub fn command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            return;
        }
        if !matches!(command, Command::Address) {
            if self.editing.is_some() {
                self.focus.focus(window);
            }
            self.cancel_edit(window, cx);
        }
        match command {
            Command::Settings => self.open_settings(true, window, cx),
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
                let tab = self.model.add_tab(&self.preferences.homepage_url);
                self.begin_edit(
                    TabTarget::Page(self.model.focused_pane().id, tab),
                    window,
                    cx,
                );
            }
            Command::CloseTab => {
                self.report(self.host.focus_parent(self.active_tab()));
                self.focus.focus(window);
                let pane = self.model.focused_pane().id;
                self.close_tab(pane, self.active_tab());
                self.sync_address(window, cx);
            }
            Command::Reload => self.report(self.host.reload(self.active_tab())),
            Command::Back => self.report(self.host.back(self.active_tab())),
            Command::Forward => self.report(self.host.forward(self.active_tab())),
            #[cfg(feature = "smoke")]
            Command::Split(axis) => {
                self.model
                    .split_focused(axis, &self.preferences.homepage_url);
                self.sync_address(window, cx);
            }
            Command::NewSpace => {
                let name = format!("Space {}", self.model.spaces.len() + 1);
                self.model.add_space(&name, &self.preferences.homepage_url);
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
    pub(crate) fn navigate(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        match normalize_url_with_engine(value, self.preferences.search_engine) {
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
        self.saver.tick(&self.model, Instant::now());
        self.poll_settings(window, cx);
        self.scroll_space_drag(cx);
        self.poll_space_menu(window, cx);
        self.poll_bars(window, cx);
        if let Some(preview) = &self.drag_preview {
            let result = preview.update(
                self.tab_drag.expect("preview belongs to a drag").position,
                window.viewport_size(),
            );
            self.report(result);
        }
        self.update_split_preview(window);
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
                BrowserEvent::Notification(tab) => {
                    if self.model.tab(tab).is_some()
                        && !(window.is_window_active() && tab == self.active_tab())
                    {
                        let count = self.notifications.entry(tab).or_default();
                        *count = count.saturating_add(1);
                    }
                }
                BrowserEvent::Focused(tab) => {
                    self.notifications.remove(&tab);
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
                BrowserEvent::History(tab, back, forward) => {
                    self.history.insert(tab, (back, forward));
                }
                BrowserEvent::Loaded(tab) => {
                    if tab == self.active_tab() {
                        self.status.clear();
                    }
                }
                BrowserEvent::OpenLink(source, url) => {
                    if self.model.add_tab_after(source, &url).is_some() {
                        self.cancel_edit(window, cx);
                        self.sync_address(window, cx);
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
                BrowserEvent::Gesture(tab, x, y) => {
                    if self.settings.is_some() {
                        continue;
                    }
                    if self.tab_drag_active() {
                        continue;
                    }
                    let pane = self
                        .model
                        .active_space()
                        .panes
                        .iter()
                        .find(|p| p.active_tab == tab)
                        .map(|p| p.id);
                    if let Some(pane) = pane {
                        let direction =
                            self.swipes
                                .entry(pane)
                                .or_default()
                                .feed(x, y, Instant::now());
                        if let Some(forward) = direction {
                            self.cancel_edit(window, cx);
                            self.model.cycle_tab(pane, forward);
                            self.sync_address(window, cx);
                            self.reveal_focused();
                        }
                    }
                }
                BrowserEvent::Shortcut(tab, key) => {
                    if self.settings.is_some() {
                        continue;
                    }
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
                    if self.dragging.is_some() || self.tab_drag_active() {
                        continue;
                    }
                    self.model.focus_pane(pane);
                    if key == "swipe-next" || key == "swipe-prev" {
                        self.cancel_edit(window, cx);
                        self.model.cycle_tab(pane, key == "swipe-next");
                        self.sync_address(window, cx);
                        self.reveal_focused();
                        continue;
                    }
                    let command = match key.as_str() {
                        "settings" => Some(Command::Settings),
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
}
