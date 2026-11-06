//! A modal native child must be created after all page and panel surfaces.
use crate::{
    app::BrowserApp,
    native_webview::{BuildNativeChild, NativeWebView},
};
use anyhow::Context as _;
use gpui::*;
use gpui_component::ActiveTheme;
use serde_json::Value;
use std::sync::mpsc::Receiver;
use workspace_core::SearchEngine;
use wry::{NewWindowResponse, WebViewBuilder};

pub struct SettingsDialog {
    default_browser_state: Value,
    pub(crate) view: Option<NativeWebView>,
    events: Option<Receiver<Value>>,
    previous_focus: Option<FocusHandle>,
    return_to_page: bool,
    theme: Value,
    protection_state: Value,
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
        self.close_downloads(window, cx);
        if self.settings.is_some() {
            return;
        }
        self.dismiss_pin(window, cx);
        self.close_link_popup(window, cx);
        let previous_focus = window.focused(cx);
        self.cancel_edit(window, cx);
        self.cancel_tab_drag(cx);
        self.dragging = None;
        self.moving_window = false;
        self.space_menu = None;
        self.default_browser.refresh();
        self.settings = Some(SettingsDialog {
            default_browser_state: Value::Null,
            view: None,
            events: None,
            previous_focus,
            return_to_page,
            theme: Value::Null,
            protection_state: Value::Null,
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
        let protection_state = self.host.protection().borrow().value();
        let result = (|| -> anyhow::Result<()> {
            if dialog.view.is_none() {
                let (sender, events) = crate::ui_events::channel();
                let state = serde_json::json!({"autoSleep": self.preferences.auto_sleep, "sleepMinutes": self.preferences.sleep_minutes, "engine": self.preferences.search_engine.id(), "homepage": self.preferences.homepage_url, "historyDays": self.preferences.history_retention_days, "theme": current_theme, "protection": protection_state, "defaultBrowser": self.default_browser.state});
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
            if dialog.default_browser_state != self.default_browser.state {
                view.evaluate_script(&format!(
                    "window.defaultBrowserState?.({})",
                    self.default_browser.state
                ))?;
                dialog.default_browser_state = self.default_browser.state.clone();
            }
            if dialog.protection_state != protection_state {
                view.evaluate_script(&format!("window.protectionState?.({protection_state})"))?;
                dialog.protection_state = protection_state;
            }
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
        self.default_browser.poll(self.settings.is_some());
        if let Some(dialog) = &self.settings
            && dialog.default_browser_state != self.default_browser.state
        {
            cx.notify();
        }
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
                "default-browser" => {
                    self.default_browser.make_default();
                    cx.notify();
                }
                "refresh-default-browser" => {
                    self.default_browser.refresh();
                    cx.notify();
                }
                "new-space" => {
                    self.close_settings(window, cx);
                    self.command(crate::app::Command::NewSpace, window, cx);
                }
                "ready" => {
                    if let Some(view) = self.settings.as_ref().and_then(|d| d.view.as_ref()) {
                        self.report(view.focus().context("focusing settings"));
                    }
                }
                "protection" => {
                    if let Some(enabled) = event["value"].as_bool() {
                        let result = self.host.protection().borrow_mut().set_enabled(enabled);
                        self.report(result);
                        cx.notify();
                    }
                }
                "update-filters" => {
                    self.host.protection().borrow_mut().update();
                    cx.notify();
                }
                "sleep" => {
                    if let (Some(enabled), Some(minutes)) =
                        (event["enabled"].as_bool(), event["minutes"].as_u64())
                    {
                        let result = self.preferences.set_sleep(enabled, minutes);
                        if let Some(view) = self.settings.as_ref().and_then(|d| d.view.as_ref()) {
                            let message = if result.is_err() {
                                "Couldn't save sleep settings. Changes apply until the app closes."
                            } else {
                                ""
                            };
                            let message = serde_json::to_string(message).unwrap();
                            let _ = view.evaluate_script(&format!("document.querySelector('#sleep-error').textContent={message};document.querySelector('#sleep-error').hidden={message}.length===0"));
                        }
                        self.report(result.map_err(Into::into));
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
                "homepage" => {
                    if let Some(value) = event["value"].as_str() {
                        let result = self.preferences.set_homepage_url(value);
                        let invalid = result.as_ref().err().and_then(|error| {
                            error.downcast_ref::<workspace_core::NavigationError>()
                        });
                        let message = if let Some(error) = invalid {
                            error.to_string()
                        } else if result.is_err() {
                            "Couldn't save your homepage. This choice applies until the app closes."
                                .into()
                        } else {
                            String::new()
                        };
                        let response = serde_json::json!({
                            "value": if invalid.is_some() { None } else { Some(&self.preferences.homepage_url) },
                            "error": message,
                            "invalid": invalid.is_some(),
                        });
                        if let Some(view) = self.settings.as_ref().and_then(|d| d.view.as_ref()) {
                            self.report(
                                view.evaluate_script(&format!("window.homepageResult({response})"))
                                    .context("updating homepage result"),
                            );
                        }
                        self.report(result);
                    }
                }
                "history-retention" => {
                    let days = if event["value"].is_null() {
                        Some(None)
                    } else {
                        event["value"]
                            .as_u64()
                            .and_then(|d| u32::try_from(d).ok())
                            .filter(|d| *d > 0)
                            .map(Some)
                    };
                    if let Some(days) = days {
                        let result = self
                            .preferences
                            .set_history_retention(days)
                            .map_err(Into::into);
                        let error = result
                            .as_ref()
                            .err()
                            .map(|e: &anyhow::Error| e.to_string())
                            .unwrap_or_default();
                        self.report(result);
                        self.prune_history(cx);
                        if let Some(view) = self.settings.as_ref().and_then(|d| d.view.as_ref()) {
                            let _ = view.evaluate_script(&format!(
                                "window.historyResult({})",
                                serde_json::json!({"error":error})
                            ));
                        }
                    }
                }
                "history-clear" => {
                    let result = self
                        .browsing_history
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Browsing history is unavailable."))
                        .and_then(|db| db.clear());
                    let error = result
                        .as_ref()
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_default();
                    if result.is_ok() {
                        self.history_cleared_at = Some(std::time::Instant::now());
                        self.visit_ids.clear();
                        self.invalidate_history_suggestions(cx);
                    }
                    self.report(result);
                    if let Some(view) = self.settings.as_ref().and_then(|d| d.view.as_ref()) {
                        let _ = view.evaluate_script(&format!(
                            "window.historyResult({})",
                            serde_json::json!({"error":error,"cleared":true})
                        ));
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
