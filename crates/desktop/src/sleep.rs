//! Sleep owns no native page handles. Captures complete (or time out) before unload.
use crate::{app::BrowserApp, drag_preview::Snapshot};
use base64::Engine as _;
use gpui::*;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use workspace_core::{TabId, write_atomic};

#[derive(Default, Serialize, Deserialize)]
struct Entry {
    #[serde(default)]
    sleeping: bool,
    #[serde(default)]
    keep_alive: bool,
}
struct Capture {
    automatic: bool,
    image: Snapshot,
    started: Instant,
}
struct Probe {
    receiver: mpsc::Receiver<String>,
    started: Instant,
}
#[derive(Default)]
pub struct SleepState {
    entries: HashMap<TabId, Entry>,
    images: HashMap<TabId, Arc<Image>>,
    encoded: HashMap<TabId, String>,
    hover: Option<(TabId, Point<Pixels>, crate::drag_preview::DragPreview)>,
    hidden: HashMap<TabId, Instant>,
    visible: Option<HashSet<TabId>>,
    pending_focus: Option<TabId>,
    captures: HashMap<TabId, Capture>,
    probes: HashMap<TabId, Probe>,
    last_probe: HashMap<TabId, Instant>,
    directory: Option<PathBuf>,
}
impl SleepState {
    pub fn restore() -> Self {
        Self::load(crate::persistence::data_dir().map(|p| p.join("sleep")))
    }
    fn load(directory: Option<PathBuf>) -> Self {
        let mut state = Self {
            directory,
            ..Self::default()
        };
        state.entries = state
            .directory
            .as_ref()
            .and_then(|p| std::fs::read(p.join("state.json")).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        for (&id, entry) in &state.entries {
            if entry.sleeping
                && let Some(dir) = &state.directory
                && let Ok(bytes) = std::fs::read(dir.join(format!("{id}.png")))
            {
                state.encoded.insert(
                    id,
                    format!(
                        "data:image/png;base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(&bytes)
                    ),
                );
                state
                    .images
                    .insert(id, Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
            }
        }
        state
    }
    fn save(&self) {
        if let Some(dir) = &self.directory
            && let Err(error) = write_atomic(
                &dir.join("state.json"),
                &serde_json::to_vec(&self.entries).unwrap(),
            )
        {
            eprintln!("Sleep state: {error}");
        }
    }
    pub fn is_sleeping(&self, id: TabId) -> bool {
        self.entries.get(&id).is_some_and(|e| e.sleeping)
    }
    pub fn keep_alive(&self, id: TabId) -> bool {
        self.entries.get(&id).is_some_and(|e| e.keep_alive)
    }
    pub fn image(&self, id: TabId) -> Option<Arc<Image>> {
        self.images.get(&id).cloned()
    }
    pub fn toggle_keep_alive(&mut self, id: TabId) {
        let e = self.entries.entry(id).or_default();
        e.keep_alive = !e.keep_alive;
        self.probes.remove(&id);
        self.save();
    }
    fn clear_image(&mut self, id: TabId) {
        self.images.remove(&id);
        self.encoded.remove(&id);
        if self.hover.as_ref().is_some_and(|(tab, _, _)| *tab == id) {
            self.hover = None;
        }
        if let Some(dir) = &self.directory {
            let _ = std::fs::remove_file(dir.join(format!("{id}.png")));
        }
    }
    pub fn retain(&mut self, ids: &HashSet<TabId>) {
        let removed: Vec<_> = self
            .entries
            .keys()
            .copied()
            .filter(|id| !ids.contains(id))
            .collect();
        if !removed.is_empty() {
            for id in removed {
                self.entries.remove(&id);
                self.clear_image(id);
            }
            self.save();
        }
        self.captures.retain(|id, _| ids.contains(id));
        self.probes.retain(|id, _| ids.contains(id));
        self.hidden.retain(|id, _| ids.contains(id));
        self.last_probe.retain(|id, _| ids.contains(id));
    }
    fn due(&self, id: TabId, now: Instant, minutes: u64) -> bool {
        !self.is_sleeping(id)
            && !self.keep_alive(id)
            && !self.captures.contains_key(&id)
            && self
                .hidden
                .get(&id)
                .is_some_and(|since| now.duration_since(*since).as_secs() / 60 >= minutes)
    }
}
impl BrowserApp {
    pub(crate) fn hover_sleep(
        &mut self,
        id: Option<TabId>,
        position: Point<Pixels>,
        window: &Window,
        cx: &App,
    ) {
        let Some(id) = id.filter(|id| self.sleep.is_sleeping(*id)) else {
            self.sleep.hover = None;
            return;
        };
        if self.settings.is_some() || self.space_menu.is_some() || self.tab_drag_active() {
            return;
        }
        if self
            .sleep
            .hover
            .as_ref()
            .is_some_and(|(tab, _, _)| *tab == id)
        {
            return;
        }
        if let Some(tab) = self.model.tab(id) {
            match crate::drag_preview::DragPreview::cached(
                tab.display_title(),
                self.sleep.encoded.get(&id).cloned().unwrap_or_default(),
                window,
                cx,
            ) {
                Ok(preview) => self.sleep.hover = Some((id, position, preview)),
                Err(error) => self.report(Err(error)),
            }
        }
    }

    pub(crate) fn hover_sleep_space(
        &mut self,
        space: u64,
        position: Point<Pixels>,
        window: &Window,
        cx: &App,
    ) {
        if self.settings.is_some() || self.space_menu.is_some() || self.tab_drag_active() {
            return;
        }
        let Some(space) = self.model.spaces.iter().find(|s| s.id == space) else {
            return;
        };
        let mut tabs: Vec<_> = space
            .panes
            .iter()
            .map(|p| p.active_tab())
            .filter(|t| self.sleep.is_sleeping(t.id))
            .collect();
        if tabs.is_empty() {
            tabs = space
                .panes
                .iter()
                .flat_map(|p| &p.tabs)
                .filter(|t| self.sleep.is_sleeping(t.id))
                .take(4)
                .collect();
        }
        let Some(first) = tabs.first() else {
            self.sleep.hover = None;
            return;
        };
        let id = first.id;
        let tiles = tabs
            .iter()
            .map(|t| {
                (
                    t.display_title().to_owned(),
                    self.sleep.encoded.get(&t.id).cloned().unwrap_or_default(),
                )
            })
            .collect();
        match crate::drag_preview::DragPreview::cached_many(
            space.display_title(),
            tiles,
            window,
            cx,
        ) {
            Ok(preview) => self.sleep.hover = Some((id, position, preview)),
            Err(error) => self.report(Err(error)),
        }
    }

    pub(crate) fn focus_woken_page(&mut self) {
        if self.settings.is_some() || self.space_menu.is_some() || self.editing.is_some() {
            return;
        }
        if let Some(id) = self.sleep.pending_focus.take()
            && self
                .sleep
                .visible
                .as_ref()
                .is_some_and(|ids| ids.contains(&id))
            && self.host.has_view(id)
        {
            self.report(self.host.focus(id));
        }
    }
    pub(crate) fn toggle_keep_alive(&mut self, id: TabId) {
        if self.model.tab(id).is_some() {
            self.saver.checkpoint(&self.model);
            self.sleep.toggle_keep_alive(id);
        }
    }
    pub(crate) fn wake_newly_visible(&mut self, visible: HashSet<TabId>) {
        if let Some(previous) = &self.sleep.visible {
            let wake: Vec<_> = visible.difference(previous).copied().collect();
            for id in wake {
                self.wake_tab(id);
            }
        }
        self.sleep.visible = Some(visible);
    }
    pub(crate) fn wake_tab(&mut self, id: TabId) {
        let changed = self.sleep.is_sleeping(id) || self.sleep.captures.contains_key(&id);
        self.sleep.captures.remove(&id);
        self.sleep.probes.remove(&id);
        if let Some(entry) = self.sleep.entries.get_mut(&id) {
            entry.sleeping = false;
        }
        self.sleep.clear_image(id);
        self.sleep.hidden.remove(&id);
        if changed {
            if id == self.active_tab() || self.pins.open == Some(id) {
                self.sleep.pending_focus = Some(id);
            }
            self.sleep.save();
        }
    }
    pub(crate) fn sleep_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        // A live popup and its opener are one browsing interaction. Recreating
        // either engine would sever their WindowProxy and callback relationship.
        #[cfg(target_os = "macos")]
        if self.native_popup_sources.contains_key(&id)
            || self
                .native_popup_sources
                .values()
                .any(|source| *source == id)
        {
            return;
        }
        if self.model.tab(id).is_none()
            || self.sleep.is_sleeping(id)
            || self.sleep.captures.contains_key(&id)
        {
            return;
        }
        // Save the latest committed URL before destroying the engine.
        if let Ok(url) = self.host.current_url(id)
            && crate::browser::allowed_url(&url)
            && let Some(tab) = self.model.tab_mut(id)
        {
            tab.url = url;
        }
        if self
            .model
            .active_space()
            .panes
            .iter()
            .any(|p| p.active_tab == id)
        {
            let _ = self.host.focus_parent(id);
            self.focus.focus(window);
        }
        self.sleep.entries.entry(id).or_default();
        let image = Rc::new(RefCell::new(None));
        if self.host.capture_sleep(id, image.clone()).is_err() {
            *image.borrow_mut() = Some(String::new());
        }
        self.sleep.captures.insert(
            id,
            Capture {
                automatic: false,
                image,
                started: Instant::now(),
            },
        );
        cx.notify();
    }
    fn auto_sleep_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        self.sleep_tab(id, window, cx);
        if let Some(capture) = self.sleep.captures.get_mut(&id) {
            capture.automatic = true;
        }
    }
    pub(crate) fn poll_sleep(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self.settings.is_some() || self.space_menu.is_some() || self.tab_drag_active() {
            self.sleep.hover = None;
        }
        if let Some((_, position, preview)) = &self.sleep.hover {
            let result = preview.update(*position, window.viewport_size());
            self.report(result);
        }
        let visible: HashSet<_> = self
            .model
            .active_space()
            .panes
            .iter()
            .map(|p| p.active_tab)
            .chain(self.pins.open)
            .collect();
        let ids: Vec<_> = self
            .model
            .spaces
            .iter()
            .flat_map(|s| &s.panes)
            .flat_map(|p| &p.tabs)
            .map(|t| t.id)
            .chain(self.model.pinned_tabs.iter().map(|p| p.tab.id))
            .collect();
        for &id in &ids {
            if visible.contains(&id) {
                self.sleep.hidden.remove(&id);
                self.sleep.probes.remove(&id);
            } else {
                self.sleep.hidden.entry(id).or_insert(now);
            }
        }
        self.sleep.captures.retain(|id, capture| {
            !capture.automatic
                || (self.preferences.auto_sleep
                    && !self.sleep.entries.get(id).is_some_and(|e| e.keep_alive)
                    && self.sleep.hidden.get(id).is_some_and(|since| {
                        now.duration_since(*since).as_secs() / 60 >= self.preferences.sleep_minutes
                    }))
        });
        let completed: Vec<_> = self
            .sleep
            .captures
            .iter()
            .filter(|(_, c)| {
                c.image.borrow().is_some()
                    || now.duration_since(c.started) >= Duration::from_secs(3)
            })
            .map(|(&id, _)| id)
            .collect();
        for id in completed {
            let capture = self.sleep.captures.remove(&id).unwrap();
            let encoded = capture.image.borrow_mut().take().unwrap_or_default();
            if let Some(data) = encoded.strip_prefix("data:image/png;base64,")
                && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data)
            {
                if let Some(dir) = &self.sleep.directory
                    && let Err(error) = write_atomic(&dir.join(format!("{id}.png")), &bytes)
                {
                    eprintln!("Sleep screenshot: {error}");
                }
                self.sleep.encoded.insert(id, encoded.clone());
                self.sleep
                    .images
                    .insert(id, Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
            }
            self.sleep.entries.entry(id).or_default().sleeping = true;
            self.saver.checkpoint(&self.model);
            self.sleep.save();
            // Invalidate AppKit's covered GPUI layer while the native parent is
            // still reachable, including when this is the last loaded page.
            self.host.request_redraw();
            self.host.unload(id);
            window.refresh();
            cx.notify();
        }
        let mut ready = Vec::new();
        self.sleep.probes.retain(|&id, p| {
            if let Ok(value) = p.receiver.try_recv() {
                ready.push((id, value == "false"));
                false
            } else {
                now.duration_since(p.started) < Duration::from_secs(3)
            }
        });
        if !self.preferences.auto_sleep {
            self.sleep.probes.clear();
            return;
        }
        for (id, safe) in ready {
            if safe
                && self.sleep.due(id, now, self.preferences.sleep_minutes)
                && !visible.contains(&id)
            {
                self.auto_sleep_tab(id, window, cx);
            }
        }
        for id in ids {
            if !self.sleep.due(id, now, self.preferences.sleep_minutes)
                || self.sleep.probes.contains_key(&id)
                || self
                    .sleep
                    .last_probe
                    .get(&id)
                    .is_some_and(|last| now.duration_since(*last) < Duration::from_secs(15))
            {
                continue;
            }
            if !self.host.has_view(id) {
                self.auto_sleep_tab(id, window, cx);
                continue;
            }
            let (sender, receiver) = mpsc::channel();
            if self
                .host
                .probe_sleep(
                    id,
                    Box::new(move |v| {
                        let _ = sender.send(v);
                    }),
                )
                .is_ok()
            {
                self.sleep.probes.insert(
                    id,
                    Probe {
                        receiver,
                        started: now,
                    },
                );
            }
            self.sleep.last_probe.insert(id, now);
        }
    }
}

#[cfg(feature = "smoke")]
impl SleepState {
    pub(crate) fn age(&mut self, id: TabId) {
        self.hidden
            .insert(id, Instant::now() - Duration::from_secs(1801));
    }
    pub(crate) fn retry(&mut self, id: TabId) {
        self.last_probe.remove(&id);
    }
    pub(crate) fn check_hover(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.hover.is_some(), "Sleeping hover preview missing");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, SleepState};
    use std::{
        collections::HashSet,
        time::{Duration, Instant},
    };
    use workspace_core::write_atomic;
    #[test]
    fn hidden_deadline_keep_alive_and_capture_exemptions() {
        let mut s = SleepState::default();
        let now = Instant::now();
        s.hidden.insert(1, now);
        assert!(!s.due(1, now + Duration::from_secs(1799), 30));
        assert!(s.due(1, now + Duration::from_secs(1800), 30));
        s.toggle_keep_alive(1);
        assert!(!s.due(1, now + Duration::from_secs(1800), 30));
        s.toggle_keep_alive(1);
        s.hidden.remove(&1);
        assert!(!s.due(1, now + Duration::from_secs(3600), 30));
        s.hidden.insert(1, now);
        s.entries.entry(1).or_default().sleeping = true;
        assert!(!s.due(1, now + Duration::from_secs(3600), 30));
    }
    #[test]
    fn restart_and_close_cleanup() {
        let dir = std::env::temp_dir().join(format!("axolotl-sleep-{}", uuid::Uuid::new_v4()));
        let mut s = SleepState::load(Some(dir.clone()));
        s.entries.insert(
            4,
            Entry {
                sleeping: true,
                keep_alive: true,
            },
        );
        s.save();
        write_atomic(&dir.join("4.png"), b"cached-image").unwrap();
        let mut restored = SleepState::load(Some(dir.clone()));
        assert!(restored.is_sleeping(4));
        assert!(restored.keep_alive(4));
        assert!(restored.image(4).is_some());
        restored.retain(&HashSet::new());
        assert!(!dir.join("4.png").exists());
        assert!(!SleepState::load(Some(dir.clone())).is_sleeping(4));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
