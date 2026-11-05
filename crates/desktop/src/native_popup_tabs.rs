//! Adopt native WebKit popups into workspace tabs outside delegate callbacks.
use crate::{
    app::{BrowserApp, TabTarget},
    browser::BrowserEvent,
};
use gpui::{Context, Window};
use std::collections::HashMap;
use workspace_core::TabId;

impl BrowserApp {
    pub(crate) fn handle_native_popup_event(
        &mut self,
        event: &BrowserEvent,
        popup_ids: &mut HashMap<TabId, TabId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match event {
            BrowserEvent::NativePopup(source, pending, url) => {
                let mut source = *source;
                if source == crate::link_popup::POPUP_TAB {
                    // Keep the actual native opener alive by promoting its
                    // preview view, instead of disposing it as the child opens.
                    if self.link_popup.is_none() {
                        self.host.unload(*pending);
                        return true;
                    }
                    self.promote_link_popup(window, cx);
                    if self.link_popup.is_some() || !self.host.has_view(self.active_tab()) {
                        self.host.unload(*pending);
                        return true;
                    }
                    source = self.active_tab();
                    popup_ids.insert(crate::link_popup::POPUP_TAB, source);
                }
                let pin = self.model.pinned_tabs.iter().any(|p| p.tab.id == source);
                let pane = self
                    .model
                    .active_space()
                    .panes
                    .iter()
                    .find(|p| p.tabs.iter().any(|t| t.id == source))
                    .map(|p| p.id);
                if pin || pane.is_some() {
                    self.cancel_edit(window, cx);
                    self.wake_tab(source);
                    if let Some(pane) = pane {
                        self.model.focus_pane(pane);
                    }
                    let tab = self.model.add_tab(url);
                    match self.host.rekey(*pending, tab) {
                        Ok(()) => {
                            popup_ids.insert(*pending, tab);
                            self.native_popup_sources.insert(tab, source);
                            self.pending_page_focus = Some(tab);
                            self.dismiss_pin(window, cx);
                            self.close_link_popup(window, cx);
                            self.sync_address(window, cx);
                        }
                        Err(error) => {
                            self.model.close_tab(self.model.focused_pane().id, tab);
                            self.report(Err(error));
                            self.host.unload(*pending);
                        }
                    }
                } else {
                    self.host.unload(*pending);
                }
                return true;
            }
            BrowserEvent::Closed(tab) => {
                // WebKit authorizes script closure; never interpret page IPC as close.
                if let Some(source) = self.native_popup_sources.remove(tab) {
                    let saved_space = self.model.active_space;
                    let foreground = (self.active_tab() == *tab || self.pins.open == Some(*tab))
                        && self.settings.is_none()
                        && self.editing.is_none()
                        && self.space_menu.is_none()
                        && self.link_popup.is_none()
                        && self.pins.open.is_none_or(|pin| pin == *tab);
                    let location = self.model.spaces.iter().find_map(|s| {
                        s.panes
                            .iter()
                            .find(|p| p.tabs.iter().any(|t| t.id == *tab))
                            .map(|p| (s.id, p.id))
                    });
                    if let Some((space, pane)) = location {
                        self.model.active_space = space;
                        self.close_tab(pane, *tab);
                        self.model.active_space = saved_space;
                    } else if let Some(index) =
                        self.model.pinned_tabs.iter().position(|p| p.tab.id == *tab)
                    {
                        if self.pins.open == Some(*tab) {
                            self.dismiss_pin(window, cx);
                        }
                        self.model.pinned_tabs.remove(index);
                    }
                    self.host.unload(*tab);
                    if foreground {
                        if let Some(pane) = self
                            .model
                            .active_space()
                            .panes
                            .iter()
                            .find(|p| p.tabs.iter().any(|t| t.id == source))
                            .map(|p| p.id)
                        {
                            self.select_target(TabTarget::Page(pane, source), window, cx);
                            self.pending_page_focus = Some(source);
                        } else if self.model.pinned_tabs.iter().any(|p| p.tab.id == source) {
                            self.toggle_pin(source, window, cx);
                        }
                    }
                    self.sync_address(window, cx);
                }
                return true;
            }
            _ => {}
        }
        false
    }
}
