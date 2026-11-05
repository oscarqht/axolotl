use crate::browser::{BrowserEvent, BrowserHost, WryHost};
use gpui::{prelude::*, *};
use gpui_component::input::{InputEvent, InputState};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    time::{Duration, Instant},
};
use workspace_core::{Divider, PaneId, SpaceId, TabId, normalize_url_with_engine};

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

/// Native bar IPC is polled; immediate shell events must not consume its drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragInput {
    Shell,
    NativeBar,
}

#[derive(Clone, Copy, Debug)]
pub struct TabDrag {
    pub input: DragInput,
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
    pub sleep: crate::sleep::SleepState,
    pub swipes: HashMap<PaneId, SwipeState>,
    pub bars: HashMap<PaneId, crate::floating_bar::FloatingBar>,
    pub(crate) chrome_views: HashSet<TabId>,
    pub model: crate::persistence::TrackedWorkspace,
    pub pins: crate::pinned_tabs::PinnedUi,
    pub browsing_history: Option<crate::browsing_history::BrowsingHistory>,
    pub visit_ids: HashMap<TabId, (i64, String)>,
    pub document_titles: HashMap<TabId, (String, String)>,
    pub last_history_prune: Instant,
    /// Ignore completions queued before the user confirmed clearing.
    pub history_cleared_at: Option<Instant>,
    pub history_matches: Vec<crate::browsing_history::HistoryItem>,
    pub history_selected: Option<usize>,
    pub(crate) history_search: Option<Task<()>>,
    pub(crate) history_generation: u64,
    pub history_popup: Option<crate::history_suggestions::HistoryPopup>,
    pub preferences: crate::preferences::Preferences,
    pub settings: Option<crate::settings::SettingsDialog>,
    pub(crate) default_browser: crate::default_browser::Integration,
    pub link_popup: Option<crate::link_popup::LinkPopup>,
    pub(crate) pending_page_focus: Option<TabId>,
    pub settings_focus: FocusHandle,
    pub downloads_focus: FocusHandle,
    pub downloads_popup: Option<crate::downloads_ui::DownloadPopup>,
    pub(crate) saver: crate::persistence::SessionSaver,
    window_saver: crate::window_state::WindowSaver,
    last_fullscreen: bool,
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
    pub(crate) persistence_error: Option<String>,
    pub last_url_probe: Instant,
    /// Last history probe per tab: (can go back, can go forward).
    pub history: HashMap<TabId, (bool, bool)>,
    pub notifications: HashMap<TabId, u32>,
    #[cfg(target_os = "macos")]
    pub native_popup_sources: HashMap<TabId, TabId>,
    pub favicons: HashMap<TabId, std::sync::Arc<Image>>,
}

impl BrowserApp {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        saved_window: crate::window_state::SavedWindow,
    ) -> Self {
        let window_saver = crate::window_state::WindowSaver::new(saved_window, window, cx);
        let address = cx.new(|cx| InputState::new(window, cx).default_value("about:blank"));
        cx.subscribe_in(
            &address,
            window,
            |this, input, event, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let value = this
                        .history_selected
                        .and_then(|i| this.history_matches.get(i))
                        .map(|i| i.url.clone())
                        .unwrap_or_else(|| input.read(cx).value().to_string());
                    this.navigate(&value, window, cx);
                }
                InputEvent::Change => {
                    if matches!(this.editing, Some(TabTarget::Space(_))) {
                        this.search_history(input.read(cx).value().as_ref(), cx);
                    }
                }
                InputEvent::Blur
                    if !input.read(cx).focus_handle(cx).is_focused(window)
                        && this.history_matches.is_empty() =>
                {
                    this.cancel_edit(window, cx)
                }
                _ => {}
            },
        )
        .detach();
        let native_events = crate::ui_events::listen();
        cx.spawn_in(window, async move |view, cx| {
            let mut interval = Duration::from_millis(250);
            loop {
                futures_lite::future::race(
                    async {
                        let _ = native_events.recv().await;
                    },
                    cx.background_executor().timer(interval),
                )
                .await;
                while native_events.try_recv().is_ok() {}
                if let Err(error) = view.update_in(cx, |this, window, cx| {
                    this.poll(window, cx);
                    interval = if this.tab_drag.is_some()
                        || this.space_drag.is_some()
                        || this
                            .link_popup
                            .as_ref()
                            .is_some_and(|popup| !popup.presented || popup.closing_until.is_some())
                        || this.sleep.is_capturing()
                    {
                        Duration::from_millis(16)
                    } else {
                        Duration::from_millis(250)
                    };
                }) {
                    // Native Windows calls can pump tasks during another update.
                    // A temporary borrow conflict must not stop polling forever.
                    if error.downcast_ref::<std::cell::BorrowMutError>().is_some() {
                        interval = Duration::from_millis(1);
                        continue;
                    }
                    break;
                }
            }
        })
        .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            // Isolate native control checks from other concurrent GUI smoke apps.
            #[cfg(feature = "smoke")]
            if !window.is_window_active()
                && std::env::args().any(|arg| arg == "--smoke-test")
                && std::env::args().any(|arg| arg == "--smoke-keep-overlays")
            {
                return;
            }
            if !window.is_window_active() {
                this.downloads_popup = None;
                this.dismiss_history();
                this.space_menu = None;
                this.dismiss_pin(window, cx);
                this.cancel_tab_drag(cx);
                this.cancel_space_drag(cx);
            } else {
                this.notifications.remove(&this.active_tab());
                if this.settings.is_some() {
                    this.default_browser.refresh();
                }
                cx.notify();
            }
        })
        .detach();
        cx.observe_window_bounds(window, |this, window, cx| {
            this.window_saver.tick(window, cx);
        })
        .detach();
        gpui_component::Theme::sync_system_appearance(Some(window), cx);
        cx.observe_window_appearance(window, |this, window, cx| {
            gpui_component::Theme::sync_system_appearance(Some(window), cx);
            this.host.request_redraw();
            cx.notify();
        })
        .detach();
        Self::install_space_shortcuts(window, cx);
        #[cfg(target_os = "macos")]
        Self::install_copy_url(cx);
        let preferences = crate::preferences::Preferences::restore();
        let browsing_history = match crate::browsing_history::BrowsingHistory::restore() {
            Ok(db) => {
                if let Err(error) = db.prune(
                    preferences.history_retention_days,
                    crate::browsing_history::now(),
                ) {
                    eprintln!("History retention: {error:#}");
                }
                Some(db)
            }
            Err(error) => {
                eprintln!("Browsing history unavailable: {error:#}");
                None
            }
        };
        let host = Rc::new(WryHost::new());
        let model = crate::persistence::restore(&preferences.homepage_url);
        let saver = crate::persistence::SessionSaver::new(&model);
        // Cmd+Q, window close and the app menu all end in cx.quit().
        cx.on_app_quit(|this, _cx| {
            this.flush_persistence();
            async {}
        })
        .detach();
        Self {
            sleep: crate::sleep::SleepState::restore(),
            last_fullscreen: window.is_fullscreen(),
            bars: HashMap::new(),
            chrome_views: HashSet::new(),
            swipes: HashMap::new(),
            model,
            browsing_history,
            visit_ids: HashMap::new(),
            document_titles: HashMap::new(),
            last_history_prune: Instant::now(),
            history_cleared_at: None,
            history_matches: vec![],
            history_selected: None,
            history_search: None,
            history_generation: 0,
            history_popup: None,
            preferences,
            settings: None,
            default_browser: crate::default_browser::Integration::new(),
            link_popup: None,
            pending_page_focus: None,
            settings_focus: cx.focus_handle(),
            downloads_focus: cx.focus_handle(),
            downloads_popup: None,
            saver,
            window_saver,
            host,
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
            pins: Default::default(),
            status: String::new(),
            persistence_error: None,
            last_url_probe: Instant::now(),
            history: HashMap::new(),
            notifications: HashMap::new(),
            #[cfg(target_os = "macos")]
            native_popup_sources: HashMap::new(),
            favicons: HashMap::new(),
        }
    }

    pub(crate) fn open_external_urls(
        &mut self,
        urls: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let urls = crate::external_urls::http_urls(urls);
        if !urls.is_empty() {
            self.close_settings(window, cx);
            self.dismiss_history();
            self.dismiss_pin(window, cx);
            self.close_link_popup(window, cx);
            self.cancel_tab_drag(cx);
            self.cancel_space_drag(cx);
            self.space_menu = None;
            self.cancel_edit(window, cx);
            for url in urls {
                self.model.add_tab(&url);
            }
            self.sync_address(window, cx);
            self.pending_page_focus = Some(self.active_tab());
        }
        window.activate_window();
        cx.activate(true);
        cx.notify();
    }

    fn flush_persistence(&mut self) {
        self.saver.checkpoint(&self.model, &self.sleep.entries);
        self.window_saver.flush();
        if let Err(error) = crate::persistence::flush() {
            eprintln!("Axolotl: shutdown save failed; recovery data retained: {error:#}");
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
        self.dismiss_history();
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
        self.dismiss_pin(window, cx);
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
        match target {
            TabTarget::Page(_, id) => self.wake_tab(id),
            TabTarget::Space(_) => {
                let ids: Vec<_> = self
                    .model
                    .active_space()
                    .panes
                    .iter()
                    .map(|p| p.active_tab)
                    .collect();
                for id in ids {
                    self.wake_tab(id);
                }
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
        // Complete the native focus handoff before processing queued blur IPC.
        // The next render mounts/selects the input inside this same controller.
        #[cfg(target_os = "windows")]
        if let TabTarget::Page(pane, _) = target
            && let Some(bar) = self.bars.get(&pane)
        {
            self.report(bar.focus_editor());
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
    pub(crate) fn close_tab(&mut self, pane: PaneId, tab: TabId) {
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
        self.wake_tab(self.active_tab());
        self.notifications.remove(&self.active_tab());
        self.sync_address(window, cx);
        cx.notify();
    }
    pub fn command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            return;
        }
        if self.link_popup.is_some() {
            match command {
                Command::CloseTab => {
                    self.dismiss_link_popup(window, cx);
                    return;
                }
                Command::Reload => {
                    self.report(self.host.reload(crate::link_popup::POPUP_TAB));
                    return;
                }
                Command::Back => {
                    self.report(self.host.back(crate::link_popup::POPUP_TAB));
                    return;
                }
                Command::Forward => {
                    self.report(self.host.forward(crate::link_popup::POPUP_TAB));
                    return;
                }
                _ => self.close_link_popup(window, cx),
            }
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
                self.report(self.host.focus_parent(self.active_tab()));
                self.focus.focus(window);
                let name = format!("Space {}", self.model.spaces.len() + 1);
                let space = self.model.add_space(&name, &self.preferences.homepage_url);
                self.begin_edit(TabTarget::Space(space), window, cx);
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
                if self.host.has_view(self.active_tab()) {
                    self.report(self.host.focus(self.active_tab()));
                }
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
        #[cfg(feature = "smoke")]
        let _timing = crate::smoke_performance::time("poll");
        // Runs only after BrowserApp::new has restored the complete workspace.
        let batches: Vec<_> = cx
            .try_global::<crate::external_urls::Incoming>()
            .map(|incoming| incoming.0.incoming.lock().unwrap().drain(..).collect())
            .unwrap_or_default();
        for urls in batches {
            self.open_external_urls(urls, window, cx);
        }
        // AppKit can finish changing fullscreen state after the last resize.
        // Invalidate the header even when no further bounds event arrives.
        let fullscreen = window.is_fullscreen();
        if self.last_fullscreen != fullscreen {
            self.last_fullscreen = fullscreen;
            // Native children can suspend GPUI's display link across transitions.
            self.host.request_redraw();
            cx.notify();
        }
        {
            #[cfg(feature = "smoke")]
            let _timing = crate::smoke_performance::time("persistence");
            self.saver
                .tick(&self.model, &self.sleep.entries, Instant::now());
        }
        self.window_saver.tick(window, cx);
        let error = crate::persistence::error_status();
        if self.persistence_error != error {
            self.persistence_error = error;
            cx.notify();
        }
        if self.last_history_prune.elapsed() >= Duration::from_secs(3600) {
            self.prune_history(cx);
        }
        self.poll_history_popup(window, cx);
        self.poll_downloads(window, cx);
        self.poll_settings(window, cx);
        self.poll_pins(window, cx);
        self.poll_link_popup(window, cx);
        self.scroll_space_drag(cx);
        self.poll_space_menu(window, cx);
        self.poll_bars(window, cx);
        self.poll_sleep(window, cx);
        if let Some(preview) = &self.drag_preview {
            let result = preview.update(
                self.tab_drag.expect("preview belongs to a drag").position,
                window.viewport_size(),
            );
            self.report(result);
        }
        self.update_split_preview(window);
        let mut events = self.host.drain_events();
        if self.last_url_probe.elapsed() >= Duration::from_millis(480) {
            self.last_url_probe = Instant::now();
            if let Some(popup) = &self.link_popup {
                let tab = crate::link_popup::POPUP_TAB;
                if let Ok(url) = self.host.current_url(tab)
                    && popup.url != url
                    && crate::browser::allowed_url(&url)
                {
                    events.push(BrowserEvent::Navigated(tab, url));
                }
            }
            for tab in std::iter::once(self.active_tab())
                .chain(self.model.pinned_tabs.iter().map(|p| p.tab.id))
            {
                if let Ok(url) = self.host.current_url(tab)
                    && self.model.tab(tab).is_some_and(|page| page.url != url)
                    && !url.is_empty()
                {
                    events.push(BrowserEvent::Navigated(tab, url));
                }
            }
        }
        if events.is_empty() {
            return;
        }
        #[cfg(target_os = "macos")]
        let mut popup_ids = HashMap::new();
        for event in events {
            #[cfg(target_os = "macos")]
            let event = {
                let mut event = event;
                event.remap(&popup_ids);
                event
            };
            #[cfg(target_os = "macos")]
            if self.handle_native_popup_event(&event, &mut popup_ids, window, cx) {
                continue;
            }
            if self.handle_popup_event(&event, window, cx) {
                continue;
            }
            let event_tab = match &event {
                BrowserEvent::Focused(id)
                | BrowserEvent::Favicon(id, _)
                | BrowserEvent::Notification(id)
                | BrowserEvent::Navigated(id, _)
                | BrowserEvent::Title(id, _, _)
                | BrowserEvent::Loaded(id)
                | BrowserEvent::LoadStarted(id)
                | BrowserEvent::VisitCompleted(id, _, _)
                | BrowserEvent::NewTab(id, _)
                | BrowserEvent::OpenLink(id, _)
                | BrowserEvent::PopupLink(id, _)
                | BrowserEvent::Shortcut(id, _)
                | BrowserEvent::Gesture(id, _, _)
                | BrowserEvent::History(id, _, _) => Some(*id),
                #[cfg(target_os = "macos")]
                BrowserEvent::NativePopup(..) | BrowserEvent::Closed(_) => None,
                BrowserEvent::ProtectionChanged => None,
            };
            if event_tab.is_some_and(|id| self.sleep.is_sleeping(id)) {
                continue;
            }
            match event {
                BrowserEvent::Favicon(tab, encoded) => {
                    use base64::Engine;
                    if self.model.tab(tab).is_some()
                        && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded)
                    {
                        self.favicons.insert(
                            tab,
                            std::sync::Arc::new(Image::from_bytes(ImageFormat::Png, bytes)),
                        );
                    }
                }
                BrowserEvent::ProtectionChanged => {}
                BrowserEvent::Notification(tab) => {
                    if self.model.tab(tab).is_some()
                        && !(window.is_window_active() && tab == self.active_tab())
                    {
                        let count = self.notifications.entry(tab).or_default();
                        *count = count.saturating_add(1);
                    }
                }
                BrowserEvent::Focused(tab) => {
                    // Ignore focus notifications from a completed WebView2
                    // handoff, including those pumped during sibling creation.
                    #[cfg(target_os = "windows")]
                    if let Some(TabTarget::Page(pane, _)) = self.editing
                        && (crate::native_webview::is_creating()
                            || self.bars.get(&pane).is_some_and(|bar| bar.has_focus()))
                    {
                        continue;
                    }
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
                BrowserEvent::LoadStarted(tab) => {
                    self.visit_ids.remove(&tab);
                }
                BrowserEvent::VisitCompleted(tab, url, completed_at) => {
                    if self
                        .history_cleared_at
                        .is_some_and(|cleared| completed_at <= cleared)
                    {
                        continue;
                    }
                    if self.model.tab(tab).is_some() {
                        let title = self
                            .document_titles
                            .get(&tab)
                            .filter(|(u, _)| u == &url)
                            .map(|(_, title)| title.as_str())
                            .unwrap_or("");
                        let result = self
                            .browsing_history
                            .as_ref()
                            .map(|db| db.record(&url, title, crate::browsing_history::now()))
                            .transpose();
                        match result {
                            Ok(Some(Some(id))) => {
                                self.visit_ids.insert(tab, (id, url));
                            }
                            Ok(_) => (),
                            Err(error) => self.report(Err(error)),
                        }
                    }
                }
                BrowserEvent::Title(tab, title, url) => {
                    if let Some(url) = &url {
                        self.document_titles
                            .insert(tab, (url.clone(), title.clone()));
                    }
                    if !title.is_empty()
                        && let Some((id, visit_url)) = self.visit_ids.get(&tab)
                        && url.as_ref() == Some(visit_url)
                        && let Some(db) = &self.browsing_history
                    {
                        let result = db.title(*id, &title);
                        self.report(result);
                    }
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
                #[cfg(target_os = "macos")]
                BrowserEvent::NativePopup(..) | BrowserEvent::Closed(_) => unreachable!(),
                BrowserEvent::PopupLink(source, url) => {
                    self.open_link_popup(source, url, window, cx)
                }
                BrowserEvent::OpenLink(source, url) => {
                    if self.model.pinned_tabs.iter().any(|p| p.tab.id == source) {
                        self.model.add_tab(&url);
                        self.dismiss_pin(window, cx);
                        self.sync_address(window, cx);
                        continue;
                    }
                    if self.model.add_tab_after(source, &url).is_some() {
                        self.cancel_edit(window, cx);
                        self.sync_address(window, cx);
                    }
                }
                BrowserEvent::NewTab(source, url) => {
                    if self.model.pinned_tabs.iter().any(|p| p.tab.id == source) {
                        self.model.add_tab(&url);
                        self.dismiss_pin(window, cx);
                        self.sync_address(window, cx);
                        continue;
                    }
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
                        self.wake_tab(self.active_tab());
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
                    if key == "escape" {
                        self.dismiss_pin(window, cx);
                        continue;
                    }
                    if self.pins.open == Some(tab) {
                        match key.as_str() {
                            "settings" => self.open_settings(true, window, cx),
                            "reload" => self.report(self.host.reload(tab)),
                            "close-tab" => self.dismiss_pin(window, cx),
                            "new-tab" => {
                                self.model.add_tab("about:blank");
                                self.dismiss_pin(window, cx);
                            }
                            _ => {}
                        }
                        continue;
                    }
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
                    self.wake_tab(self.active_tab());
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
                        "new-space" => Some(Command::NewSpace),
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

// Window close can release this entity before GPUI's weak app-quit observer
// runs. Flush while the model and retained native window are still available.
impl Drop for BrowserApp {
    fn drop(&mut self) {
        self.flush_persistence();
    }
}
