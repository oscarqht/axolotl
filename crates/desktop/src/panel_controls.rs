//! Native floating control lifecycle and IPC dispatch.
use crate::app::{BrowserApp, Command, DragInput, TabAction, TabTarget};
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
            if self.settings.is_some() {
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
                "new-space" => self.command(Command::NewSpace, window, cx),
                "settings" => self.open_settings(false, window, cx),
                "pin" => self.pin_page(pane, tab, window, cx),
                "protection" => {
                    if let Some(page) = self.model.tab(tab) {
                        let result = self.host.protection().borrow_mut().toggle_site(&page.url);
                        self.report(result);
                        cx.notify();
                    }
                }
                "select" => self.select_target(target, window, cx),
                "edit" => self.begin_edit(target, window, cx),
                "close" => self.tab_action(target, TabAction::Close, window, cx),
                "new" => {
                    self.focus_pane(pane, window, cx);
                    self.command(Command::NewTab, window, cx);
                }
                "reload" => self.report(self.host.reload(tab)),
                "back" => self.report(self.host.back(tab)),
                "forward" => self.report(self.host.forward(tab)),
                "menu-open" => {
                    // The probe answers asynchronously; until then both directions stay disabled.
                    self.history.insert(tab, (false, false));
                    if let Some(bar) = self.bars.get_mut(&pane) {
                        bar.menu_open = true;
                    }
                    if self.model.tab(tab).is_some() {
                        // Tabs without a native view yet simply keep Back/Forward disabled.
                        let _ = self.host.probe_history(tab);
                    }
                    cx.notify();
                }
                "menu-close" => {
                    if let Some(bar) = self.bars.get_mut(&pane) {
                        bar.menu_open = false;
                    }
                    cx.notify();
                }
                "set-title" => {
                    if let Some(value) = event["value"].as_str() {
                        self.model.set_custom_title(tab, value);
                    }
                    cx.notify();
                }
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
                "cancel" => {
                    self.cancel_tab_drag(cx);
                    self.cancel_edit(window, cx);
                }
                "drag-cancel" => {
                    if self.native_drag_from(pane) {
                        self.cancel_tab_drag(cx);
                    }
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
                        self.tab_drag.as_mut().unwrap().input = DragInput::NativeBar;
                        self.tab_drag.as_mut().unwrap().whole =
                            event["whole"].as_bool().unwrap_or(false);
                    }
                }
                "drag-move" => {
                    if let Some(position) = position {
                        self.update_native_tab_drag(pane, position, window, cx);
                    }
                }
                "drag-end" => self.finish_native_tab_drag(pane, position, window, cx),
                _ => {}
            }
        }
    }

    pub fn prepare_views(&mut self, window: &mut Window, cx: &gpui::App) {
        let mut ids: HashSet<_> = self
            .model
            .spaces
            .iter()
            .flat_map(|s| &s.panes)
            .flat_map(|p| &p.tabs)
            .map(|t| t.id)
            .chain(self.model.pinned_tabs.iter().map(|p| p.tab.id))
            .collect();
        if self.link_popup.is_some() {
            ids.insert(crate::link_popup::POPUP_TAB);
        }
        self.host.retain(&ids);
        self.notifications.retain(|id, _| ids.contains(id));
        self.favicons.retain(|id, _| ids.contains(id));
        for pin in self.model.pinned_tabs.clone() {
            self.report(self.host.ensure(pin.tab.id, &pin.tab.url, window, cx));
        }
        if window.is_window_active() {
            self.notifications.remove(&self.active_tab());
        }
        let viewport = window.viewport_size();
        let content = Rect::new(
            0.,
            crate::render::TOP,
            f32::from(viewport.width),
            (f32::from(viewport.height) - crate::render::TOP).max(0.),
        );
        let geometry = self.model.active_space().layout.geometry(content, 6., 100.);
        let mut visible: Vec<_> = self
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
        if let Some(id) = self.pins.open
            && let Some(tab) = self.model.tab(id)
        {
            visible.push((id, tab.url.clone()));
        }
        // Hide only inactive surfaces; hiding the focused view every frame steals page input.
        for id in &ids {
            if *id != crate::link_popup::POPUP_TAB && !visible.iter().any(|(tab, _)| tab == id) {
                self.report(self.host.show(*id, false));
            }
        }
        for (id, url) in visible {
            let result = self.host.ensure(id, &url, window, cx);
            self.report(result);
            if self.host.has_view(id) {
                self.report(self.host.show(id, true));
            }
        }
        // Native children stack in creation order. Rebuild controls above newly
        // created page surfaces, after a captured drag has released its pointer.
        let native_views: HashSet<_> = ids
            .iter()
            .copied()
            .filter(|id| *id != crate::link_popup::POPUP_TAB && self.host.has_view(*id))
            .collect();
        let expanded: HashSet<_> = self
            .bars
            .iter()
            .filter(|(_, bar)| bar.expanded)
            .map(|(id, _)| *id)
            .collect();
        if native_views != self.chrome_views && !self.tab_drag_active() {
            self.space_menu = None;
            if let Some(dialog) = &mut self.settings {
                dialog.view = None;
            }
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
            let tabs: Vec<_> = p
                .tabs
                .iter()
                .map(|t| {
                    let (back, forward) = self.history.get(&t.id).copied().unwrap_or_default();
                    serde_json::json!({
                        "id": t.id,
                        "protection": self.host.protection().borrow().policy.allows(&t.url),
                        "protectionGlobal": self.host.protection().borrow().policy.enabled,
                        "protectionReady": self.host.protection().borrow().native.ready(),
                        "protectionSite": crate::protection::host(&t.url),
                        "notifications": self.notifications.get(&t.id).copied().unwrap_or_default(),
                        "title": t.display_title(),
                        "customTitle": t.custom_title,
                        "url": t.url,
                        "canBack": back,
                        "canForward": forward,
                        "canReload": self.host.has_view(t.id),
                    })
                })
                .collect();
            let state = serde_json::json!({"active":p.active_tab,"editing":editing,"expanded":self.bars[&pane.id].expanded,"tabs":tabs});
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
        // Create last, above every page and floating panel control.
        self.prepare_pins(window, cx);
        self.prepare_space_menu(window, cx);
        self.prepare_settings(window, cx);
        self.prepare_link_popup(window, cx);
        if let Some(tab) = self.promoted_focus.take() {
            self.report(self.host.focus(tab));
        }
    }
}
