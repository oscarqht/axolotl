//! PiP follows workspace visibility, never OS focus/occlusion. A reservation spans
//! asynchronous frame discovery and selection so two tabs cannot open two windows.
use crate::{native_webview::NativeWebView, pip_window::PipWindow};
use anyhow::Result;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};
use workspace_core::TabId;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Message {
    Reset,
    Candidate {
        request: String,
        frame: String,
        area: f64,
        aspect: f64,
        existing: bool,
    },
    Ready {
        request: String,
        frame: String,
    },
    Stopped {
        request: String,
        frame: String,
    },
    External {
        frame: String,
        existing: bool,
    },
}
struct Candidate {
    frame: String,
    area: f64,
    aspect: f64,
}
struct Pending {
    tab: TabId,
    request: String,
    source: Rc<NativeWebView>,
    started: Instant,
    candidate: Option<Candidate>,
    selecting: bool,
    existing: bool,
}
struct Active {
    tab: TabId,
    request: String,
    source: Rc<NativeWebView>,
    window: PipWindow,
}
#[derive(Default)]
struct Visibility {
    workspace: HashSet<TabId>,
    previous: HashSet<TabId>,
    dismissed: HashSet<TabId>,
}
impl Visibility {
    fn workspace_update(
        &mut self,
        panes: &HashSet<TabId>,
        pin: Option<TabId>,
        live: &HashSet<TabId>,
    ) -> (HashSet<TabId>, Vec<TabId>) {
        let navigated = self.workspace != *panes;
        self.workspace.clone_from(panes);
        let visible = panes.iter().copied().chain(pin).collect();
        let hidden = self.update(&visible, live);
        // Pin overlays also dismiss on app deactivation, Settings, and split
        // focus. Only a workspace tab/space change may start automatic PiP.
        (visible, if navigated { hidden } else { Vec::new() })
    }
    fn update(&mut self, visible: &HashSet<TabId>, live: &HashSet<TabId>) -> Vec<TabId> {
        self.dismissed
            .retain(|id| live.contains(id) && !visible.contains(id));
        let mut hidden: Vec<_> = self
            .previous
            .difference(visible)
            .copied()
            .filter(|id| live.contains(id) && !self.dismissed.contains(id))
            .collect();
        hidden.sort_unstable();
        self.previous = visible.clone();
        hidden
    }
}
pub struct AutoPip {
    enabled: bool,
    visibility: Visibility,
    queue: VecDeque<TabId>,
    pending: Option<Pending>,
    active: Option<Active>,
    external: HashSet<(TabId, String)>,
    sender: SyncSender<(TabId, Message)>,
    receiver: Receiver<(TabId, Message)>,
    invalidated: HashSet<TabId>,
}
impl Default for AutoPip {
    fn default() -> Self {
        let (sender, receiver) = mpsc::sync_channel(256);
        Self {
            enabled: true,
            visibility: Visibility::default(),
            queue: VecDeque::new(),
            pending: None,
            active: None,
            external: HashSet::new(),
            sender,
            receiver,
            invalidated: HashSet::new(),
        }
    }
}
fn command(source: &NativeWebView, kind: &str, request: &str, frame: Option<&str>) -> Result<()> {
    let value = serde_json::json!({"kind":kind,"request":request,"frame":frame});
    source.evaluate_script(&format!("window.__axolotlAutoPip?.({value})"))?;
    Ok(())
}

/// A pending discovery reserves opening, but must not discard later visibility
/// transitions. Only an actual presentation prevents another source queuing.
fn admit_hidden(queue: &mut VecDeque<TabId>, hidden: Vec<TabId>, enabled: bool, presenting: bool) {
    if enabled && !presenting {
        queue.extend(hidden);
    }
}

impl AutoPip {
    #[cfg(feature = "smoke")]
    pub fn test_state(&self, cx: &gpui::App) -> serde_json::Value {
        self.active
            .as_ref()
            .map(|a| serde_json::json!({"tab":a.tab,"native":a.window.test_state(cx)}))
            .unwrap_or(serde_json::Value::Null)
    }
    #[cfg(feature = "smoke")]
    pub fn test_adjust_geometry(&self, cx: &gpui::App) -> Result<()> {
        if let Some(active) = &self.active {
            active.window.test_adjust_geometry(cx)?;
        }
        Ok(())
    }
    #[cfg(feature = "smoke")]
    pub fn test_close(&self, cx: &mut gpui::App) -> Result<()> {
        if let Some(active) = &self.active {
            active.window.test_close(cx)?;
        }
        Ok(())
    }
    pub fn shutdown(&mut self, cx: &mut gpui::App) -> Result<()> {
        self.cancel_pending();
        self.close(cx)
    }
    pub fn sender(&self) -> SyncSender<(TabId, Message)> {
        self.sender.clone()
    }
    pub fn enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    pub fn owned_tab(&self) -> Option<TabId> {
        self.active.as_ref().map(|a| a.tab)
    }
    pub fn owns(&self, tab: TabId) -> bool {
        self.active.as_ref().is_some_and(|a| a.tab == tab)
    }
    pub fn invalidate(&mut self, tab: TabId) {
        self.invalidated.insert(tab);
        self.external.retain(|(id, _)| *id != tab);
    }
    pub fn rekey(&mut self, from: TabId, to: TabId) {
        if self.visibility.workspace.remove(&from) {
            self.visibility.workspace.insert(to);
        }
        if self.visibility.previous.remove(&from) {
            self.visibility.previous.insert(to);
        }
        if self.visibility.dismissed.remove(&from) {
            self.visibility.dismissed.insert(to);
        }
        for id in &mut self.queue {
            if *id == from {
                *id = to;
            }
        }
        if let Some(p) = self.pending.as_mut().filter(|p| p.tab == from) {
            p.tab = to;
        }
        if let Some(a) = self.active.as_mut().filter(|a| a.tab == from) {
            a.tab = to;
        }
        self.external = self
            .external
            .drain()
            .map(|(id, frame)| (if id == from { to } else { id }, frame))
            .collect();
    }
    fn cancel_pending(&mut self) {
        if let Some(p) = self.pending.take() {
            let _ = command(&p.source, "restore", &p.request, None);
        }
    }
    fn close(&mut self, cx: &mut gpui::App) -> Result<()> {
        let restore = if let Some(active) = &self.active {
            let restored = command(&active.source, "restore", &active.request, None);
            active.window.close(cx)?;
            restored
        } else {
            Ok(())
        };
        self.active = None;
        restore
    }
    pub fn sync(
        &mut self,
        visible: &HashSet<TabId>,
        pin: Option<TabId>,
        live: &HashSet<TabId>,
        views: &HashMap<TabId, Rc<NativeWebView>>,
        cx: &mut gpui::App,
    ) -> Result<()> {
        let (visible, hidden) = self.visibility.workspace_update(visible, pin, live);
        #[cfg(feature = "smoke")]
        if !hidden.is_empty() {
            eprintln!(
                "PiP hidden {hidden:?} enabled={} external={:?}",
                self.enabled, self.external
            );
        }
        self.external.retain(|(id, _)| views.contains_key(id));
        let mut stopped = false;
        for (tab, message) in self.receiver.try_iter() {
            #[cfg(feature = "smoke")]
            eprintln!("PiP message tab={tab} {message:?}");
            match message {
                Message::Reset => {
                    self.invalidated.insert(tab);
                    self.external.retain(|(id, _)| *id != tab);
                }
                Message::External { frame, existing } => {
                    if existing {
                        self.external.insert((tab, frame));
                    } else {
                        self.external.remove(&(tab, frame));
                    }
                }
                Message::Candidate {
                    request,
                    frame,
                    area,
                    aspect,
                    existing,
                } => {
                    if let Some(p) = self
                        .pending
                        .as_mut()
                        .filter(|p| p.tab == tab && p.request == request && !p.selecting)
                    {
                        p.existing |= existing;
                        if area.is_finite()
                            && area > 0.
                            && aspect.is_finite()
                            && aspect > 0.
                            && p.candidate.as_ref().is_none_or(|c| area > c.area)
                        {
                            p.candidate = Some(Candidate {
                                frame,
                                area,
                                aspect,
                            });
                        }
                    }
                }
                Message::Ready { request, frame } => {
                    if self.pending.as_ref().is_some_and(|p| {
                        p.tab == tab
                            && p.request == request
                            && p.selecting
                            && p.candidate.as_ref().is_some_and(|c| c.frame == frame)
                    }) && self.enabled
                        && !visible.contains(&tab)
                        && live.contains(&tab)
                        && !self.invalidated.contains(&tab)
                        && self.external.is_empty()
                    {
                        let p = self.pending.take().unwrap();
                        match PipWindow::new(p.source.clone(), p.candidate.unwrap().aspect, cx) {
                            Ok(window) => {
                                self.active = Some(Active {
                                    tab,
                                    request,
                                    source: p.source,
                                    window,
                                })
                            }
                            Err(error) => {
                                let _ = command(&p.source, "restore", &p.request, None);
                                eprintln!("Automatic picture in picture: {error:#}");
                            }
                        }
                    }
                }
                Message::Stopped { request, frame } => {
                    let _ = frame; // Ancestor-frame unload also ends the presentation.
                    if self
                        .active
                        .as_ref()
                        .is_some_and(|a| a.tab == tab && a.request == request)
                    {
                        stopped = true;
                    }
                    if self
                        .pending
                        .as_ref()
                        .is_some_and(|p| p.tab == tab && p.request == request)
                    {
                        // Cancel below, outside the receiver borrow.
                        self.invalidated.insert(tab);
                    }
                }
            }
        }
        let closed = self.active.as_ref().is_some_and(|a| a.window.is_closed(cx));
        if closed && let Some(active) = &self.active {
            self.visibility.dismissed.insert(active.tab);
        }
        if stopped
            || closed
            || self.active.as_ref().is_some_and(|a| {
                !self.enabled
                    || visible.contains(&a.tab)
                    || !live.contains(&a.tab)
                    || self.invalidated.contains(&a.tab)
                    || !self.external.is_empty()
            })
        {
            self.close(cx)?;
        }
        if self.pending.as_ref().is_some_and(|p| {
            !self.enabled
                || visible.contains(&p.tab)
                || !live.contains(&p.tab)
                || self.invalidated.contains(&p.tab)
                || p.existing
                || !self.external.is_empty()
                || p.started.elapsed() > Duration::from_secs(2)
        }) {
            self.cancel_pending();
        }
        if let Some(p) = &mut self.pending
            && !p.selecting
            && p.started.elapsed() >= Duration::from_millis(180)
        {
            if let Some(candidate) = &p.candidate {
                p.selecting = true;
                command(&p.source, "select", &p.request, Some(&candidate.frame))?;
            } else {
                self.cancel_pending();
            }
        }
        if !self.enabled || self.active.is_some() || !self.external.is_empty() {
            self.queue.clear();
        }
        // Admission follows automatic return/manual close: returning source A
        // and hiding source B in the same tick must retain B's transition.
        admit_hidden(
            &mut self.queue,
            hidden,
            self.enabled,
            self.active.is_some() || !self.external.is_empty(),
        );
        if self.enabled
            && self.active.is_none()
            && self.pending.is_none()
            && self.external.is_empty()
        {
            while let Some(tab) = self.queue.pop_front() {
                if visible.contains(&tab)
                    || !live.contains(&tab)
                    || self.invalidated.contains(&tab)
                    || self.visibility.dismissed.contains(&tab)
                {
                    continue;
                }
                if let Some(source) = views.get(&tab) {
                    let request = uuid::Uuid::new_v4().to_string();
                    command(source, "probe", &request, None)?;
                    self.pending = Some(Pending {
                        tab,
                        request,
                        source: source.clone(),
                        started: Instant::now(),
                        candidate: None,
                        selecting: false,
                        existing: false,
                    });
                    break;
                }
            }
        }
        self.invalidated.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ids(values: &[TabId]) -> HashSet<TabId> {
        values.iter().copied().collect()
    }
    #[test]
    fn only_model_visibility_transitions_trigger_and_closed_tabs_do_not() {
        let mut state = Visibility::default();
        let live = ids(&[1, 2, 3]);
        assert!(state.update(&ids(&[1, 2]), &live).is_empty());
        // Another split is focused, app deactivates or minimizes: same model visibility.
        assert!(state.update(&ids(&[1, 2]), &live).is_empty());
        assert_eq!(state.update(&ids(&[2, 3]), &live), vec![1]);
        assert!(state.update(&ids(&[3]), &ids(&[1, 3])).is_empty());
    }
    #[test]
    fn dismissal_resets_only_after_revisit() {
        let mut state = Visibility::default();
        let live = ids(&[1, 2]);
        state.update(&ids(&[1]), &live);
        assert_eq!(state.update(&ids(&[2]), &live), vec![1]);
        state.dismissed.insert(1);
        assert!(state.update(&ids(&[2]), &live).is_empty());
        state.update(&ids(&[1]), &live);
        assert!(!state.dismissed.contains(&1));
        assert_eq!(state.update(&ids(&[2]), &live), vec![1]);
    }

    #[test]
    fn transitions_wait_behind_pending_discovery_instead_of_disappearing() {
        let mut queue = VecDeque::new();
        admit_hidden(&mut queue, vec![1], true, false);
        let pending_tab = queue.pop_front();
        assert_eq!(pending_tab, Some(1));
        // A blank tab's discovery is still outstanding when the user leaves
        // the playing source. No presentation is open, so reserve its turn.
        admit_hidden(&mut queue, vec![2], true, false);
        assert_eq!(queue.pop_front(), Some(2));
        assert!(queue.is_empty());
    }

    #[test]
    fn returning_source_and_hiding_another_source_preserves_new_transition() {
        let mut visibility = Visibility::default();
        let live = ids(&[1, 2]);
        visibility.update(&ids(&[2]), &live);
        let hidden = visibility.update(&ids(&[1]), &live);
        assert_eq!(hidden, vec![2]);
        // Source 1 was floating and now returns to its visible tab. Admission
        // uses the state after closing its window, allowing source 2 to follow.
        let mut queue = VecDeque::new();
        admit_hidden(&mut queue, hidden, true, false);
        assert_eq!(queue.pop_front(), Some(2));
    }

    #[test]
    fn existing_presentations_and_disabled_setting_discard_new_transitions() {
        for (enabled, presenting) in [(true, true), (false, false), (false, true)] {
            let mut queue = VecDeque::new();
            admit_hidden(&mut queue, vec![1, 2], enabled, presenting);
            assert!(queue.is_empty());
        }
    }

    #[test]
    fn source_closed_while_discovery_waits_is_no_longer_eligible() {
        let mut visibility = Visibility::default();
        visibility.update(&ids(&[1, 2]), &ids(&[1, 2, 3]));
        // Closing source 1 while switching to source 3 must produce only 2.
        assert_eq!(visibility.update(&ids(&[3]), &ids(&[2, 3])), vec![2]);
    }
    #[test]
    fn pin_overlay_returns_but_only_navigation_can_trigger_entry() {
        let mut state = Visibility::default();
        let live = ids(&[1, 2, 3]);
        state.workspace_update(&ids(&[1]), Some(3), &live);
        // App deactivation/outside click hides the overlay only.
        assert!(state.workspace_update(&ids(&[1]), None, &live).1.is_empty());
        state.workspace_update(&ids(&[1]), Some(3), &live);
        let (visible, hidden) = state.workspace_update(&ids(&[2]), None, &live);
        assert_eq!(hidden, vec![1, 3]);
        assert!(!visible.contains(&3));
        let (visible, _) = state.workspace_update(&ids(&[2]), Some(3), &live);
        assert!(visible.contains(&3)); // Return an existing PiP to its pin.
    }
}
