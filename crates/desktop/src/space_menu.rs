//! A last-created native child keeps the space menu above pages and panel controls.
use crate::{
    app::BrowserApp,
    native_webview::{BuildNativeChild, NativeWebView},
};
use anyhow::Context as _;
use gpui::*;
use serde_json::Value;
use std::sync::mpsc::Receiver;
use workspace_core::SpaceId;
use wry::{NewWindowResponse, WebViewBuilder};

pub struct SpaceMenu {
    pub space: SpaceId,
    pub(crate) needs_raise: bool,
    position: Point<Pixels>,
    view: Option<NativeWebView>,
    events: Option<Receiver<Value>>,
}

impl BrowserApp {
    pub(crate) fn open_space_menu(
        &mut self,
        space: SpaceId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.model.spaces.iter().any(|s| s.id == space) {
            return;
        }
        self.close_downloads(window, cx);
        self.dismiss_pin(window, cx);
        self.cancel_edit(window, cx);
        self.cancel_tab_drag(cx);
        self.space_menu = Some(SpaceMenu {
            space,
            needs_raise: false,
            position,
            view: None,
            events: None,
        });
        cx.notify();
    }

    pub(crate) fn prepare_space_menu(&mut self, window: &Window, cx: &App) {
        let Some(menu) = &mut self.space_menu else {
            return;
        };
        let Some(space) = self.model.spaces.iter().find(|s| s.id == menu.space) else {
            self.space_menu = None;
            return;
        };
        let result = (|| -> anyhow::Result<()> {
            if menu.view.is_none() {
                let (sender, events) = crate::ui_events::channel();
                let colors = crate::chrome_theme::ChromeColors::current(cx);
                let state = serde_json::json!({
                    "x": f32::from(menu.position.x), "y": f32::from(menu.position.y),
                    "title": space.custom_title.as_deref().unwrap_or(""),
                    "placeholder": space.display_title(),
                    "icon": space.icon,
                    "ink": format!("#{:06x}", colors.ink),
                    "surface": format!("#{:06x}", colors.surface),
                    "hover": format!("#{:06x}", colors.hover),
                    "border": format!("#{:06x}", colors.border),
                });
                let view = crate::browser::with_inline_html(
                    WebViewBuilder::new()
                        .with_transparent(true)
                        .with_focused(false)
                        .with_initialization_script(format!("window.spaceMenuState={state}"))
                        .with_initialization_script(format!(
                            "window.spaceEmojiData={}",
                            include_str!("../assets/emoji-data.json")
                        ))
                        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
                        .with_ipc_handler(move |request| {
                            if let Ok(event) = serde_json::from_str(request.body()) {
                                let _ = sender.send(event);
                            }
                        }),
                    include_str!("../assets/space-menu.html"),
                )
                .build_native_child(window, cx)?;
                menu.events = Some(events);
                menu.view = Some(view);
            }
            if menu.needs_raise {
                crate::native_webview::raise(menu.view.as_ref().unwrap())?;
                menu.needs_raise = false;
            }
            menu.view
                .as_ref()
                .unwrap()
                .set_bounds(crate::browser::native_bounds(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: window.viewport_size(),
                }))?;
            Ok(())
        })();
        if let Err(error) = result {
            self.space_menu = None;
            self.report(Err(error));
        }
    }

    pub(crate) fn poll_space_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let events: Vec<_> = self
            .space_menu
            .as_ref()
            .and_then(|m| m.events.as_ref())
            .map(|events| events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            let Some(menu) = &self.space_menu else {
                break;
            };
            match event["type"].as_str().unwrap_or("") {
                "new-space" => {
                    self.space_menu = None;
                    self.command(crate::app::Command::NewSpace, window, cx);
                }
                "ready" => {
                    if let Some(view) = &menu.view {
                        self.report(view.focus().context("focusing space menu"));
                    }
                }
                "sleep" => {
                    let space = menu.space;
                    self.space_menu = None;
                    let ids: Vec<_> = self
                        .model
                        .spaces
                        .iter()
                        .find(|s| s.id == space)
                        .into_iter()
                        .flat_map(|s| &s.panes)
                        .flat_map(|p| &p.tabs)
                        .map(|t| t.id)
                        .collect();
                    for id in ids {
                        if !self.sleep.keep_alive(id) {
                            self.sleep_tab(id, window, cx);
                        }
                    }
                    cx.notify();
                }
                "save" | "dismiss" => {
                    if event["type"] == "save"
                        && let Some(value) = event["value"].as_str()
                    {
                        let icon = event["icon"].as_str().filter(|icon| {
                            serde_json::from_str::<Vec<(String, String)>>(include_str!(
                                "../assets/emoji-data.json"
                            ))
                            .is_ok_and(|items| items.iter().any(|(emoji, _)| emoji == icon))
                        });
                        self.model.set_space_details(menu.space, value, icon);
                    }
                    self.space_menu = None;
                    self.focus.focus(window);
                    cx.notify();
                }
                _ => {}
            }
        }
    }
}

#[cfg(feature = "smoke")]
impl SpaceMenu {
    pub(crate) fn evaluate(
        &self,
        script: &str,
        callback: impl Fn(String) + Send + 'static,
    ) -> anyhow::Result<()> {
        self.view
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Space menu not mounted"))?
            .evaluate_script_with_callback(script, callback)?;
        Ok(())
    }
}

impl Drop for SpaceMenu {
    fn drop(&mut self) {
        if let Some(view) = &self.view {
            let _ = view.set_visible(false);
        }
    }
}
