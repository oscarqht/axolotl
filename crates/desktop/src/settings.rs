//! A modal native child must be created after all page and panel surfaces.
use crate::{
    app::BrowserApp,
    native_webview::{BuildNativeChild, NativeWebView},
};
use gpui::*;
use gpui_component::ActiveTheme;
use serde_json::Value;
use std::sync::mpsc::{self, Receiver};
use workspace_core::SearchEngine;
use wry::{NewWindowResponse, WebViewBuilder};

pub struct SettingsDialog {
    pub(crate) view: Option<NativeWebView>,
    events: Option<Receiver<Value>>,
    previous_focus: Option<FocusHandle>,
    return_to_page: bool,
    theme: Value,
}

fn theme(cx: &App) -> Value {
    let colors = crate::chrome_theme::ChromeColors::current(cx);
    serde_json::json!({
        "ink": format!("#{:06x}", colors.ink),
        "surface": format!("#{:06x}", colors.surface),
        "muted": format!("#{:06x}", colors.muted),
        "hover": format!("#{:06x}", colors.hover),
        "border": format!("#{:06x}", colors.border),
        "accent": format!("#{:06x}", colors.accent),
        "scheme": if cx.theme().is_dark() { "dark" } else { "light" },
    })
}

impl BrowserApp {
    pub(crate) fn open_settings(
        &mut self,
        return_to_page: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.is_some() {
            return;
        }
        let previous_focus = window.focused(cx);
        self.cancel_edit(window, cx);
        self.cancel_tab_drag(cx);
        self.dragging = None;
        self.moving_window = false;
        self.space_menu = None;
        self.settings = Some(SettingsDialog {
            view: None,
            events: None,
            previous_focus,
            return_to_page,
            theme: Value::Null,
        });
        cx.notify();
    }

    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.settings.take() else {
            return;
        };
        let return_to_page = dialog.return_to_page;
        let focus = dialog.previous_focus.clone();
        drop(dialog);
        if return_to_page {
            self.report(self.host.focus(self.active_tab()));
        } else {
            focus.unwrap_or_else(|| self.focus.clone()).focus(window);
        }
        cx.notify();
    }

    pub(crate) fn prepare_settings(&mut self, window: &Window, cx: &App) {
        let Some(dialog) = &mut self.settings else {
            return;
        };
        let current_theme = theme(cx);
        let result = (|| -> anyhow::Result<()> {
            if dialog.view.is_none() {
                let (sender, events) = mpsc::channel();
                let state = serde_json::json!({"engine": self.preferences.search_engine.id(), "theme": current_theme});
                let view = crate::browser::with_inline_html(
                    WebViewBuilder::new()
                        .with_transparent(true)
                        .with_focused(false)
                        .with_initialization_script(format!("window.settingsState={state}"))
                        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
                        .with_ipc_handler(move |request| {
                            if let Ok(event) = serde_json::from_str(request.body()) {
                                let _ = sender.send(event);
                            }
                        }),
                    include_str!("../assets/settings.html"),
                )
                .build_native_child(window, cx)?;
                dialog.view = Some(view);
                dialog.events = Some(events);
                dialog.theme = current_theme.clone();
            }
            let view = dialog.view.as_ref().unwrap();
            view.set_bounds(crate::browser::native_bounds(Bounds {
                origin: point(px(0.), px(0.)),
                size: window.viewport_size(),
            }))?;
            if dialog.theme != current_theme {
                view.evaluate_script(&format!("window.applyTheme({current_theme})"))?;
                dialog.theme = current_theme;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.settings = None;
            self.report(Err(error));
        }
    }

    pub(crate) fn poll_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let events: Vec<_> = self
            .settings
            .as_ref()
            .and_then(|d| d.events.as_ref())
            .map(|events| events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            if self.settings.is_none() {
                break;
            }
            match event["type"].as_str().unwrap_or("") {
                "ready" => {
                    if let Some(view) = self.settings.as_ref().and_then(|d| d.view.as_ref()) {
                        self.report(view.focus().map_err(Into::into));
                    }
                }
                "engine" => {
                    if let Some(engine) = event["value"].as_str().and_then(SearchEngine::from_id) {
                        let result = self.preferences.set_search_engine(engine);
                        let message = result.as_ref().err().map(|_| "Couldn't save your preference. This choice applies until the app closes.").unwrap_or("");
                        if let Some(view) = self.settings.as_ref().and_then(|d| d.view.as_ref()) {
                            let text = serde_json::to_string(message).expect("static error text");
                            let _ = view.evaluate_script(&format!("document.querySelector('#error').textContent={text};document.querySelector('#error').hidden={text}.length===0"));
                        }
                        self.report(result.map_err(Into::into));
                    }
                }
                "dismiss" => self.close_settings(window, cx),
                _ => (),
            }
        }
    }
}

impl Drop for SettingsDialog {
    fn drop(&mut self) {
        if let Some(view) = &self.view {
            let _ = view.set_visible(false);
        }
    }
}
