//! Title-only hover surface above native page pixels, without taking focus.
use crate::{
    app::BrowserApp,
    native_webview::{BuildNativeChild, NativeWebView},
};
use gpui::*;
use std::{
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};
use workspace_core::SpaceId;
use wry::WebViewBuilder;

pub struct SpaceTooltip {
    id: SpaceId,
    position: Point<Pixels>,
    since: Instant,
    view: Option<NativeWebView>,
    events: Option<Receiver<serde_json::Value>>,
    pub(crate) presented: bool,
}

impl BrowserApp {
    pub(crate) fn hover_space_title(&mut self, id: Option<SpaceId>, position: Point<Pixels>) {
        self.space_tooltip = id.map(|id| SpaceTooltip {
            id,
            position,
            since: Instant::now(),
            view: None,
            events: None,
            presented: false,
        });
        crate::ui_events::wake();
    }

    pub(crate) fn leave_space_title(&mut self, id: SpaceId) {
        // Hover listeners can deliver the new tab's enter before the old tab's exit.
        if self
            .space_tooltip
            .as_ref()
            .is_some_and(|tooltip| tooltip.id == id)
        {
            self.hover_space_title(None, point(px(0.), px(0.)));
        }
    }

    pub(crate) fn poll_space_tooltip(&mut self, window: &Window, cx: &App) {
        if self.space_menu.is_some()
            || self.settings.is_some()
            || self.editing.is_some()
            || self.space_drag.is_some_and(|d| d.active)
            || self.tab_drag_active()
        {
            self.space_tooltip = None;
            return;
        }
        let Some(tooltip) = &mut self.space_tooltip else {
            return;
        };
        let Some(space) = self
            .model
            .spaces
            .iter()
            .find(|s| s.id == tooltip.id && s.pinned)
        else {
            self.space_tooltip = None;
            return;
        };
        if tooltip.since.elapsed() < Duration::from_millis(450) {
            return;
        }
        let result = (|| -> anyhow::Result<()> {
            if tooltip.view.is_none() {
                let colors = crate::chrome_theme::ChromeColors::current(cx);
                let title = space.display_title();
                let title = if title.is_empty() { "New tab" } else { title };
                let title = serde_json::to_string(title)?.replace('<', "\\u003c");
                let width = (f32::from(window.viewport_size().width) - 16.).clamp(1., 360.);
                let html = format!(
                    r#"<!doctype html><meta charset="utf-8"><style>
*{{box-sizing:border-box}}html,body{{margin:0;background:transparent}}#title{{display:inline-block;max-width:{width}px;padding:6px 9px;border:1px solid #{border:06x};border-radius:6px;background:#{surface:06x};color:#{ink:06x};font:12px/18px -apple-system,Segoe UI,sans-serif;white-space:pre-wrap;overflow-wrap:anywhere}}
</style><div id="title" role="tooltip"></div><script>const el=document.querySelector('#title');el.textContent={title};const r=el.getBoundingClientRect();window.ipc.postMessage(JSON.stringify({{width:Math.ceil(r.width),height:Math.ceil(r.height)}}));</script>"#,
                    border = colors.border,
                    surface = colors.surface,
                    ink = colors.ink
                );
                let (sender, events) = crate::ui_events::channel();
                let view = crate::browser::with_inline_html(
                    WebViewBuilder::new()
                        .with_visible(false)
                        .with_focused(false)
                        .with_transparent(true)
                        .with_ipc_handler(move |request| {
                            if let Ok(event) = serde_json::from_str(request.body()) {
                                let _ = sender.send(event);
                            }
                        }),
                    html,
                )
                .build_native_child(window, cx)?;
                view.set_bounds(crate::browser::native_bounds(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: size(px(width), px(100.)),
                }))?;
                tooltip.events = Some(events);
                tooltip.view = Some(view);
            }
            for event in tooltip
                .events
                .as_ref()
                .into_iter()
                .flat_map(|e| e.try_iter())
            {
                let viewport = window.viewport_size();
                let width = (event["width"].as_f64().unwrap_or(1.) as f32)
                    .clamp(1., f32::from(viewport.width).max(1.));
                let height = (event["height"].as_f64().unwrap_or(1.) as f32).clamp(
                    1.,
                    (f32::from(viewport.height) - crate::render::TOP - 4.).max(1.),
                );
                let view = tooltip.view.as_ref().unwrap();
                view.set_bounds(crate::browser::native_bounds(Bounds {
                    origin: point(
                        (tooltip.position.x + px(8.))
                            .min(viewport.width - px(width))
                            .max(px(0.)),
                        px(crate::render::TOP + 4.),
                    ),
                    size: size(px(width), px(height)),
                }))?;
                view.set_visible(true)?;
                tooltip.presented = true;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.space_tooltip = None;
            self.report(Err(error));
        }
    }
}

#[cfg(feature = "smoke")]
impl SpaceTooltip {
    pub(crate) fn evaluate(
        &self,
        script: &str,
        callback: impl Fn(String) + Send + 'static,
    ) -> anyhow::Result<()> {
        self.view
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Tooltip not mounted"))?
            .evaluate_script_with_callback(script, callback)?;
        Ok(())
    }
}
