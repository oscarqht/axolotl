//! Native floating control lifecycle and IPC dispatch.
use crate::app::{BrowserApp, Command, TabAction, TabTarget};
use gpui::*;
use std::{collections::HashSet, time::Instant};
use workspace_core::Rect;
impl BrowserApp {
    pub(crate) fn poll_bars(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut events = Vec::new();
        for (pane, bar) in &self.bars {
            while let Some(event) = bar.next_event() {
                events.push((*pane, event));
            }
        }
        for (pane, event) in events {
            if self.model.active_space().pane(pane).is_none() {
                continue;
            }
            let kind = event["type"].as_str().unwrap_or("");
            let tab = event["tab"]
                .as_u64()
                .unwrap_or(self.model.pane(pane).unwrap().active_tab);
            let target = TabTarget::Page(pane, tab);
            let position = self.bars.get(&pane).map(|bar| {
                point(
                    px(bar.bounds.x + event["x"].as_f64().unwrap_or(0.) as f32),
                    px(bar.bounds.y + event["y"].as_f64().unwrap_or(0.) as f32),
                )
            });
            match kind {
                "ready" => {
                    if let Some(bar) = self.bars.get(&pane) {
                        bar.invalidate();
                    }
                    cx.notify();
                }
                "expand" => {
                    if let Some(bar) = self.bars.get_mut(&pane) {
                        bar.expanded = event["value"].as_bool().unwrap_or(false);
                        bar.transitioning = true;
                    }
                    cx.notify();
                }
                "settled" => {
                    if let Some(bar) = self.bars.get_mut(&pane)
                        && Some(bar.expanded) == event["value"].as_bool()
                    {
                        bar.transitioning = false;
                    }
                    cx.notify();
                }
                "metrics" => {
                    if let Some(bar) = self.bars.get_mut(&pane) {
                        bar.midpoints = event["midpoints"]
                            .as_array()
                            .map(|v| {
                                v.iter()
                                    .filter_map(|x| x.as_f64().map(|x| x as f32))
                                    .collect()
                            })
                            .unwrap_or_default();
                    }
                }
                "select" => self.select_target(target, window, cx),
                "edit" => self.begin_edit(target, window, cx),
                "close" => self.tab_action(target, TabAction::Close, window, cx),
                "new" => {
                    self.focus_pane(pane, window, cx);
                    if event["edit"].as_bool() == Some(false) {
                        // The plus button creates a tab without expanding the
                        // compact controls. Address shortcuts still open editing.
                        let tab = self.model.add_tab("about:blank");
                        self.select_target(TabTarget::Page(pane, tab), window, cx);
                    } else {
                        self.command(Command::NewTab, window, cx);
                    }
                }
                "reload" => self.report(self.host.reload(tab)),
                "navigate" => {
                    if self.editing == Some(target) {
                        self.navigate(event["value"].as_str().unwrap_or(""), window, cx);
                    }
                }
                "blur" => {
                    if self.editing == Some(target) {
                        self.cancel_edit(window, cx);
                    }
                }
                "cancel" | "drag-cancel" => {
                    self.cancel_tab_drag(cx);
                    self.cancel_edit(window, cx);
                }
                "wheel" => {
                    if !self.tab_drag_active() {
                        let x = event["x"].as_f64().unwrap_or(0.);
                        let y = event["y"].as_f64().unwrap_or(0.);
                        if let Some(forward) =
                            self.swipes
                                .entry(pane)
                                .or_default()
                                .feed(x, y, Instant::now())
                        {
                            self.cancel_edit(window, cx);
                            self.model.cycle_tab(pane, forward);
                            self.sync_address(window, cx);
                            cx.notify();
                        }
                    }
                }
                "swipe" => {
                    self.cancel_edit(window, cx);
                    self.model
                        .cycle_tab(pane, event["forward"].as_bool().unwrap_or(true));
                    self.sync_address(window, cx);
                    cx.notify();
                }
                "drag-start" => {
                    if let Some(position) = position {
                        self.arm_tab_drag(pane, tab, position);
                        self.tab_drag.as_mut().unwrap().whole =
                            event["whole"].as_bool().unwrap_or(false);
                    }
                }
                "drag-move" => {
                    if let Some(position) = position {
                        self.update_tab_drag(
                            &MouseMoveEvent {
                                position,
                                pressed_button: Some(MouseButton::Left),
                                ..Default::default()
                            },
                            window,
                            cx,
                        );
                    }
                }
                "drag-end" => self.finish_tab_drag(position, window, cx),
                _ => {}
            }
        }
    }

    pub fn prepare_views(&mut self, window: &mut Window, cx: &gpui::App) {
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
            let result = self.host.ensure(id, &url, window, cx);
            self.report(result);
            self.report(self.host.show(id, true));
        }
        // Native children stack in creation order. Rebuild controls above newly
        // created page surfaces, after a captured drag has released its pointer.
        let native_views: HashSet<_> = ids
            .iter()
            .copied()
            .filter(|id| self.host.has_view(*id))
            .collect();
        let expanded: HashSet<_> = self
            .bars
            .iter()
            .filter(|(_, bar)| bar.expanded)
            .map(|(id, _)| *id)
            .collect();
        if native_views != self.chrome_views && !self.tab_drag_active() {
            self.bars.clear();
            self.chrome_views = native_views;
        }
        let active_panes: HashSet<_> = geometry.panes.iter().map(|p| p.id).collect();
        self.bars.retain(|id, _| active_panes.contains(id));
        for pane in &geometry.panes {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.bars.entry(pane.id) {
                match crate::floating_bar::FloatingBar::new(window, cx) {
                    Ok(mut bar) => {
                        bar.expanded = expanded.contains(&pane.id);
                        entry.insert(bar);
                    }
                    Err(error) => {
                        self.report(Err(error));
                        continue;
                    }
                }
            }
            let p = self.model.pane(pane.id).unwrap();
            let editing = match self.editing {
                Some(TabTarget::Page(id, tab)) if id == p.id => Some(tab),
                _ => None,
            };
            let state = serde_json::json!({"active":p.active_tab,"editing":editing,"expanded":self.bars[&pane.id].expanded,"tabs":p.tabs.iter().map(|t| serde_json::json!({"id":t.id,"title":t.title,"url":t.url})).collect::<Vec<_>>()});
            let result = self
                .bars
                .get_mut(&pane.id)
                .unwrap()
                .place(pane.bounds, state);
            self.report(result);
        }
        // Construct every sibling first: a new native child returns focus to
        // GPUI and would otherwise blur an editor in an earlier sibling.
        if let Some(TabTarget::Page(pane, _)) = self.editing
            && let Some(bar) = self.bars.get(&pane)
        {
            let result = bar.focus_editor();
            self.report(result);
        }
    }
}
