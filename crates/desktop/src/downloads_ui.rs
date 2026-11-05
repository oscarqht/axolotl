//! A native overlay keeps download controls above WKWebView / WebView2 pages.
use crate::{
    app::BrowserApp,
    native_webview::{BuildNativeChild, NativeWebView},
};
use gpui::*;
use serde_json::{Value, json};
use std::sync::mpsc::{self, Receiver};

pub struct DownloadPopup {
    pub(crate) view: Option<NativeWebView>,
    events: Option<Receiver<Value>>,
    state: String,
}

impl BrowserApp {
    pub(crate) fn close_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.downloads_popup.take().is_some() {
            self.downloads_focus.focus(window);
            cx.notify();
        }
    }

    pub(crate) fn toggle_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.downloads_popup.is_some() {
            self.close_downloads(window, cx);
            return;
        }
        self.dismiss_pin(window, cx);
        self.close_space_menu(cx);
        self.close_settings(window, cx);
        self.cancel_edit(window, cx);
        self.cancel_tab_drag(cx);
        self.cancel_space_drag(cx);
        self.downloads_popup = Some(DownloadPopup {
            view: None,
            events: None,
            state: String::new(),
        });
        cx.notify();
    }

    pub(crate) fn prepare_downloads(&mut self, window: &Window, cx: &App) {
        let Some(popup) = &mut self.downloads_popup else {
            return;
        };
        let result = (|| -> anyhow::Result<()> {
            if popup.view.is_none() {
                let (sender, events) = mpsc::sync_channel(128);
                let view = crate::browser::with_inline_html(
                    wry::WebViewBuilder::new()
                        .with_transparent(true)
                        .with_focused(false)
                        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                        .with_ipc_handler(move |request| {
                            if let Ok(event) = serde_json::from_str(request.body()) {
                                let _ = sender.try_send(event);
                            }
                        }),
                    include_str!("../assets/downloads.html"),
                )
                .build_native_child(window, cx)?;
                popup.events = Some(events);
                popup.view = Some(view);
            }
            let colors = crate::chrome_theme::ChromeColors::current(cx);
            let items = self.host.downloads().borrow().items();
            let state = json!({
                "items": items,
                "right": if cfg!(target_os = "windows") { 176 } else { 42 },
                "revealLabel": if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in Explorer" },
                "theme": {"ink":format!("#{:06x}",colors.ink),"muted":format!("#{:06x}",colors.muted),"surface":format!("#{:06x}",colors.surface),"hover":format!("#{:06x}",colors.hover),"border":format!("#{:06x}",colors.border),"accent":format!("#{:06x}",colors.accent)}
            }).to_string();
            let view = popup.view.as_ref().unwrap();
            view.set_bounds(crate::browser::native_bounds(Bounds {
                origin: point(px(0.), px(crate::render::TOP)),
                size: size(
                    window.viewport_size().width,
                    px((f32::from(window.viewport_size().height) - crate::render::TOP).max(1.)),
                ),
            }))?;
            // Pages can be created or raised while transfers run or previews animate.
            crate::native_webview::raise(view)?;
            if state != popup.state {
                view.evaluate_script(&format!("window.updateDownloads?.({state})"))?;
                popup.state = state;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.downloads_popup = None;
            self.report(Err(error));
        }
    }

    pub(crate) fn poll_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.host.downloads().borrow_mut().poll() {
            cx.notify();
        }
        let events: Vec<Value> = self
            .downloads_popup
            .as_ref()
            .and_then(|p| p.events.as_ref())
            .map(|events| events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event["type"].as_str().unwrap_or("") {
                "ready" => {
                    let result = self.downloads_popup.as_mut().map(|popup| {
                        popup.state.clear();
                        popup
                            .view
                            .as_ref()
                            .map(|view| view.focus().map_err(Into::into))
                            .unwrap_or(Ok(()))
                    });
                    if let Some(result) = result {
                        self.report(result);
                    }
                    cx.notify();
                }
                "dismiss" => self.close_downloads(window, cx),
                "cancel" | "open" | "reveal" => {
                    let Some(id) = event["id"].as_str() else {
                        continue;
                    };
                    let downloads = self.host.downloads();
                    let result = match event["type"].as_str().unwrap() {
                        "cancel" => downloads.borrow_mut().cancel(id),
                        "open" => downloads.borrow().open(id),
                        _ => downloads.borrow().reveal(id),
                    };
                    if let Some(view) = self.downloads_popup.as_ref().and_then(|p| p.view.as_ref())
                    {
                        let message = result
                            .as_ref()
                            .err()
                            .map(|e| e.to_string())
                            .unwrap_or_default();
                        let _ = view.evaluate_script(&format!(
                            "window.downloadMessage?.({})",
                            json!(message)
                        ));
                    }
                    self.report(result);
                    cx.notify();
                }
                _ => {}
            }
        }
    }
}

#[cfg(feature = "smoke")]
impl DownloadPopup {
    pub(crate) fn evaluate(
        &self,
        script: &str,
        callback: impl Fn(String) + Send + 'static,
    ) -> anyhow::Result<()> {
        self.view
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Downloads not mounted"))?
            .evaluate_script_with_callback(script, callback)?;
        Ok(())
    }
}

impl Drop for DownloadPopup {
    fn drop(&mut self) {
        if let Some(view) = &self.view {
            let _ = view.set_visible(false);
        }
    }
}
