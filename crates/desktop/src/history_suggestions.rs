//! Main-strip suggestions use a native child, above native page surfaces.
use crate::{
    app::BrowserApp,
    native_webview::{BuildNativeChild, NativeWebView},
};
use gpui::*;
use serde_json::{Value, json};
use std::{
    rc::Rc,
    sync::mpsc::{self, Receiver},
    time::Instant,
};
use wry::{NewWindowResponse, WebViewBuilder};

pub struct HistoryPopup {
    pub view: Rc<NativeWebView>,
    events: Receiver<Value>,
}
impl Drop for HistoryPopup {
    fn drop(&mut self) {
        let _ = self.view.set_visible(false);
    }
}
pub fn items_json(items: &[crate::browsing_history::HistoryItem]) -> Value {
    json!(
        items
            .iter()
            .map(|i| json!({"title":i.title,"url":i.url}))
            .collect::<Vec<_>>()
    )
}
impl BrowserApp {
    pub fn dismiss_history(&mut self) {
        self.history_popup = None;
        self.history_matches.clear();
        self.history_selected = None;
    }
    pub fn search_history(&mut self, query: &str, cx: &mut Context<Self>) {
        self.history_selected = None;
        self.history_matches = match self
            .browsing_history
            .as_ref()
            .map(|db| db.search(query))
            .transpose()
        {
            Ok(items) => items.unwrap_or_default(),
            Err(error) => {
                self.report(Err(error));
                vec![]
            }
        };
        if self.history_matches.is_empty() {
            self.history_popup = None;
        }
        cx.notify();
    }
    pub fn invalidate_history_suggestions(&mut self, cx: &mut Context<Self>) {
        self.dismiss_history();
        for bar in self.bars.values() {
            let _ = bar.clear_history();
        }
        cx.notify();
    }
    pub fn prune_history(&mut self, cx: &mut Context<Self>) {
        self.last_history_prune = Instant::now();
        if let Some(db) = &self.browsing_history {
            let result = db.prune(
                self.preferences.history_retention_days,
                crate::browsing_history::now(),
            );
            self.report(result);
        }
        self.invalidate_history_suggestions(cx);
    }
    pub fn history_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.history_matches.is_empty() {
            return false;
        }
        let count = self.history_matches.len();
        match key {
            "down" => {
                self.history_selected = Some(self.history_selected.map_or(0, |i| (i + 1) % count))
            }
            "up" => {
                self.history_selected = Some(
                    self.history_selected
                        .map_or(count - 1, |i| (i + count - 1) % count),
                )
            }
            "escape" => self.dismiss_history(),
            "enter" if self.history_selected.is_some() => {
                let url = self.history_matches[self.history_selected.unwrap()]
                    .url
                    .clone();
                self.navigate(&url, window, cx);
            }
            _ => return false,
        }
        cx.notify();
        true
    }
    pub fn prepare_history_popup(&mut self, window: &Window, cx: &App) {
        if self.history_matches.is_empty() {
            self.history_popup = None;
            return;
        }
        let result = (|| -> anyhow::Result<()> {
            if self.history_popup.is_none() {
                let (sender, events) = mpsc::channel();
                let colors = crate::chrome_theme::ChromeColors::current(cx);
                let css = format!(
                    "window.historyTheme='--ink:#{:06x};--surface:#{:06x};--border:#{:06x};--hover:#{:06x}'",
                    colors.ink, colors.surface, colors.border, colors.hover
                );
                let view = crate::browser::with_inline_html(
                    WebViewBuilder::new()
                        .with_focused(false)
                        .with_initialization_script(css)
                        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
                        .with_ipc_handler(move |request| {
                            if let Ok(value) = serde_json::from_str(request.body()) {
                                let _ = sender.send(value);
                            }
                        }),
                    include_str!("../assets/history-suggestions.html"),
                )
                .build_native_child(window, cx)?;
                view.focus_parent()?;
                self.history_popup = Some(HistoryPopup {
                    view: Rc::new(view),
                    events,
                });
            }
            self.history_popup
                .as_ref()
                .unwrap()
                .view
                .evaluate_script(&format!(
                    "window.updateHistory?.({}, {})",
                    items_json(&self.history_matches),
                    json!(self.history_selected)
                ))?;
            Ok(())
        })();
        self.report(result);
    }
    pub fn poll_history_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let events: Vec<_> = self
            .history_popup
            .as_ref()
            .map(|p| p.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            if event["ready"] == true {
                cx.notify();
            }
            if let Some(item) = event["url"]
                .as_str()
                .and_then(|url| self.history_matches.iter().find(|i| i.url == url))
            {
                let url = item.url.clone();
                self.navigate(&url, window, cx);
            }
        }
    }
}
