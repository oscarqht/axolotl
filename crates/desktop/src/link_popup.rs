//! Temporary native page and a full-window backdrop, outside the saved workspace.
use crate::{
    app::BrowserApp,
    native_webview::{BuildNativeChild, NativeWebView},
};
use gpui::*;
use std::{
    sync::mpsc::{self, Receiver},
    time::Instant,
};
use workspace_core::{PaneId, SpaceId, TabId};
use wry::{NewWindowResponse, WebViewBuilder};

pub(crate) const POPUP_TAB: TabId = u64::MAX;

pub struct LinkPopup {
    pub source: TabId,
    pub pane: PaneId,
    pub space: SpaceId,
    pub url: String,
    pub title: String,
    pub(crate) backdrop: Option<NativeWebView>,
    events: Option<Receiver<String>>,
    focus_pending: bool,
    pub(crate) ready: bool,
    pub(crate) presented: bool,
    pub(crate) closing_until: Option<Instant>,
    opening_pending: bool,
}

/// Whole-window coordinates, deliberately independent of the originating pane.
pub(crate) fn popup_bounds(viewport: Size<Pixels>) -> Bounds<Pixels> {
    let width = (f32::from(viewport.width) - 64.).clamp(1., 960.);
    let height = (f32::from(viewport.height) - 96.).clamp(45., 760.);
    Bounds {
        origin: point(
            px((f32::from(viewport.width) - width) / 2.),
            px((f32::from(viewport.height) - height) / 2.),
        ),
        size: size(px(width), px(height)),
    }
}

impl BrowserApp {
    /// Transient page events never mutate the persisted workspace.
    pub(crate) fn handle_popup_event(
        &mut self,
        event: &crate::browser::BrowserEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::browser::BrowserEvent as E;
        let tab = match event {
            E::Focused(t)
            | E::Notification(t)
            | E::Loaded(t)
            | E::LoadStarted(t)
            | E::VisitCompleted(t, _, _)
            | E::Navigated(t, _)
            | E::Title(t, _, _)
            | E::NewTab(t, _)
            | E::OpenLink(t, _)
            | E::PopupLink(t, _)
            | E::Shortcut(t, _)
            | E::Gesture(t, _, _)
            | E::History(t, _, _)
            | E::Favicon(t, _) => *t,
            E::ProtectionChanged => return false,
        };
        if tab != POPUP_TAB {
            return false;
        }
        if self.link_popup.is_none() {
            return true;
        }
        match event {
            E::Navigated(_, url) => self.link_popup.as_mut().unwrap().url = url.clone(),
            E::Title(_, title, _) => self.link_popup.as_mut().unwrap().title = title.clone(),
            E::PopupLink(_, url) => self.open_link_popup(POPUP_TAB, url.clone(), window, cx),
            E::NewTab(_, url) => {
                self.report(self.host.navigate(POPUP_TAB, url));
            }
            E::OpenLink(_, url) => {
                let source = self.link_popup.as_ref().unwrap().source;
                self.model.add_tab_after(source, url);
                self.close_link_popup(window, cx);
                self.sync_address(window, cx);
            }
            E::Shortcut(_, key) => {
                use crate::app::Command;
                let command = match key.as_str() {
                    "new-space" => Some(Command::NewSpace),
                    "new-tab" => Some(Command::NewTab),
                    "close-tab" => Some(Command::CloseTab),
                    "reload" => Some(Command::Reload),
                    "settings" => Some(Command::Settings),
                    "address" => Some(Command::Address),
                    _ => None,
                };
                if key == "escape" {
                    self.dismiss_link_popup(window, cx);
                } else if let Some(command) = command {
                    self.command(command, window, cx);
                }
            }
            _ => (),
        }
        true
    }
    pub(crate) fn open_link_popup(
        &mut self,
        source: TabId,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.is_some() || !crate::browser::allowed_url(&url) {
            return;
        }
        let origin = if source == POPUP_TAB {
            self.link_popup
                .as_ref()
                .map(|p| (p.source, p.pane, p.space))
        } else {
            self.model
                .active_space()
                .panes
                .iter()
                .find(|p| p.active_tab == source)
                .map(|p| (source, p.id, self.model.active_space))
        };
        let Some((source, pane, space)) = origin else {
            return;
        };
        self.cancel_edit(window, cx);
        self.cancel_tab_drag(cx);
        self.cancel_space_drag(cx);
        self.space_menu = None;
        self.dragging = None;
        self.moving_window = false;
        if let Some(popup) = &mut self.link_popup {
            if popup.closing_until.take().is_some() {
                popup.opening_pending = true;
            }
            popup.source = source;
            popup.pane = pane;
            popup.space = space;
            popup.url = url.clone();
            popup.title.clear();
            popup.focus_pending = true;
            self.report(self.host.navigate(POPUP_TAB, &url));
        } else {
            self.link_popup = Some(LinkPopup {
                source,
                pane,
                space,
                url,
                title: String::new(),
                backdrop: None,
                events: None,
                focus_pending: true,
                ready: false,
                presented: false,
                closing_until: None,
                opening_pending: true,
            });
        }
        cx.notify();
    }

    /// Natural dismissal keeps both surfaces alive until the fade finishes.
    pub(crate) fn dismiss_link_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let duration = crate::popup_transition::duration(false);
        let Some(popup) = &mut self.link_popup else {
            return;
        };
        if popup.closing_until.is_some() {
            return;
        }
        if duration.is_zero() || !popup.presented {
            self.close_link_popup(window, cx);
            return;
        }
        let result = (|| -> anyhow::Result<()> {
            crate::popup_transition::animate(
                popup.backdrop.as_ref().unwrap(),
                false,
                false,
                duration,
            )?;
            self.host
                .popup_transition(POPUP_TAB, false, false, duration)?;
            Ok(())
        })();
        if let Err(error) = result {
            self.report(Err(error));
            self.close_link_popup(window, cx);
            return;
        }
        self.link_popup.as_mut().unwrap().closing_until = Some(Instant::now() + duration);
        self.report(self.host.focus_parent(POPUP_TAB));
        self.focus.focus(window);
        cx.notify();
    }

    /// Commands that replace the overlay dispose synchronously before new UI.
    pub(crate) fn close_link_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = self.link_popup.take() else {
            return;
        };
        self.report(self.host.focus_parent(POPUP_TAB));
        self.report(self.host.reset_popup_transition(POPUP_TAB));
        self.report(self.host.show(POPUP_TAB, false));
        drop(popup);
        self.host.retain(
            &self
                .model
                .spaces
                .iter()
                .flat_map(|s| &s.panes)
                .flat_map(|p| &p.tabs)
                .map(|t| t.id)
                .chain(self.model.pinned_tabs.iter().map(|p| p.tab.id))
                .collect(),
        );
        self.focus.focus(window);
        self.report(self.host.focus(self.active_tab()));
        self.host.request_redraw();
        cx.notify();
    }

    pub(crate) fn promote_link_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = self.link_popup.as_ref() else {
            return;
        };
        let (pane, space, url, title) = (
            popup.pane,
            popup.space,
            self.host
                .current_url(POPUP_TAB)
                .ok()
                .filter(|url| crate::browser::allowed_url(url))
                .unwrap_or_else(|| popup.url.clone()),
            popup.title.clone(),
        );
        if self.model.pane(pane).is_none() {
            self.close_link_popup(window, cx);
            return;
        }
        self.model.switch_space(space);
        self.model.focus_pane(pane);
        let tab = self.model.add_tab(&url);
        self.report(self.host.reset_popup_transition(POPUP_TAB));
        if let Err(error) = self.host.rekey(POPUP_TAB, tab) {
            self.model.close_tab(pane, tab);
            self.report(Err(error));
            return;
        }
        self.model.tab_mut(tab).unwrap().title = title;
        self.link_popup = None;
        self.pending_page_focus = Some(tab);
        self.sync_address(window, cx);
        self.report(self.host.focus(tab));
        cx.notify();
    }

    pub(crate) fn prepare_link_popup(&mut self, window: &Window, cx: &App) {
        let Some(popup) = &mut self.link_popup else {
            return;
        };
        let result = (|| -> anyhow::Result<()> {
            if popup.backdrop.is_none() {
                let (sender, events) = mpsc::sync_channel(32);
                let view = crate::browser::with_inline_html(
                    WebViewBuilder::new()
                        .with_transparent(true)
                        .with_visible(false)
                        .with_accept_first_mouse(true)
                        .with_focused(false)
                        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
                        .with_ipc_handler(move |r| {
                            if matches!(
                                r.body().as_str(),
                                "ready" | "dismiss" | "promote" | "new-space" | "new-tab"
                            ) {
                                let _ = sender.try_send(r.body().clone());
                            }
                        }),
                    include_str!("../assets/link-popup.html"),
                )
                .build_native_child(window, cx)?;
                popup.backdrop = Some(view);
                popup.events = Some(events);
            }
            let bounds = popup_bounds(window.viewport_size());
            let view = popup.backdrop.as_ref().unwrap();
            view.set_bounds(crate::browser::native_bounds(Bounds {
                origin: point(px(0.), px(0.)),
                size: window.viewport_size(),
            }))?;
            let colors = crate::chrome_theme::ChromeColors::current(cx);
            let state = serde_json::json!({"x":f32::from(bounds.origin.x), "y":f32::from(bounds.origin.y), "width":f32::from(bounds.size.width), "height":f32::from(bounds.size.height), "title":popup.title, "url":popup.url, "surface":format!("#{:06x}", colors.surface), "ink":format!("#{:06x}", colors.ink)});
            view.evaluate_script(&format!("window.updatePopup?.({state})"))?;
            self.host.ensure(POPUP_TAB, &popup.url, window, cx)?;
            if !self.host.has_view(POPUP_TAB) {
                return Ok(());
            }
            self.host.place(
                POPUP_TAB,
                Bounds {
                    origin: point(bounds.origin.x, bounds.origin.y + px(44.)),
                    size: size(bounds.size.width, bounds.size.height - px(44.)),
                },
            )?;
            // Wait for the backdrop document before revealing either surface.
            if !popup.ready {
                return Ok(());
            }
            if popup.opening_pending && popup.closing_until.is_none() {
                let duration = crate::popup_transition::duration(true);
                crate::popup_transition::animate(view, true, !popup.presented, duration)?;
                self.host
                    .popup_transition(POPUP_TAB, true, !popup.presented, duration)?;
                view.set_visible(true)?;
                self.host.show(POPUP_TAB, true)?;
                popup.presented = true;
                popup.opening_pending = false;
            }
            if popup.focus_pending && popup.closing_until.is_none() {
                self.host.focus(POPUP_TAB)?;
                popup.focus_pending = false;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.report(Err(error));
        }
    }

    pub(crate) fn poll_link_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .link_popup
            .as_ref()
            .and_then(|p| p.closing_until)
            .is_some_and(|until| Instant::now() >= until)
        {
            self.close_link_popup(window, cx);
        }
        let events: Vec<_> = self
            .link_popup
            .as_ref()
            .and_then(|p| p.events.as_ref())
            .map(|e| e.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event.as_str() {
                "ready" => {
                    if let Some(popup) = &mut self.link_popup {
                        popup.ready = true;
                    }
                    cx.notify();
                }
                "dismiss" => self.dismiss_link_popup(window, cx),
                "promote" => self.promote_link_popup(window, cx),
                "new-space" | "new-tab" => {
                    self.close_link_popup(window, cx);
                    self.command(
                        if event == "new-space" {
                            crate::app::Command::NewSpace
                        } else {
                            crate::app::Command::NewTab
                        },
                        window,
                        cx,
                    );
                }
                _ => (),
            }
        }
    }
}
impl Drop for LinkPopup {
    fn drop(&mut self) {
        if let Some(view) = &self.backdrop {
            let _ = view.set_visible(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::popup_bounds;
    use gpui::{px, size};
    #[test]
    fn centered_in_whole_window_at_different_sizes() {
        for (w, h) in [(1400., 900.), (900., 600.), (640., 420.)] {
            let b = popup_bounds(size(px(w), px(h)));
            assert_eq!(b.origin.x * 2. + b.size.width, px(w));
            assert_eq!(b.origin.y * 2. + b.size.height, px(h));
            assert!(b.size.width <= px(960.));
            assert!(b.size.height <= px(760.));
        }
    }
}
