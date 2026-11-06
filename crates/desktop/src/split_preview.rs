//! Native translucent surface above the live page; GPUI cannot cover child webviews.
use crate::native_webview::{BuildNativeChild, NativeWebView as WebView};
use anyhow::Result;
use gpui::{Bounds, Window, point, px, size};
use workspace_core::Rect;
use wry::WebViewBuilder;

pub struct SplitPreview(WebView);

impl SplitPreview {
    pub fn new(window: &Window, cx: &gpui::App) -> Result<Self> {
        let html = r#"<!doctype html><style>
*{box-sizing:border-box}html,body{margin:0;width:100%;height:100%;overflow:hidden;background:transparent}
body{border:3px solid #66c9ac;background:rgba(68,179,146,.28);border-radius:8px;display:grid;place-items:center;color:#173c32;font:600 16px -apple-system,Segoe UI,sans-serif}
span{padding:8px 14px;background:rgba(233,255,247,.9);border-radius:6px}
</style><span>Release to split</span>"#;
        let view = crate::browser::with_inline_html(
            WebViewBuilder::new()
                .with_visible(false)
                .with_focused(false)
                .with_transparent(true),
            html,
        )
        .build_native_child(window, cx)?;
        #[cfg(not(target_os = "windows"))]
        view.focus_parent()?;
        Ok(Self(view))
    }

    pub fn update_label(&self, bounds: Option<Rect>, label: &str) -> Result<()> {
        if let Some(r) = bounds {
            self.0.evaluate_script(&format!("document.querySelector('span').textContent={};document.body.style.borderColor={};document.body.style.background={}",serde_json::json!(label),serde_json::json!(if label.contains("window"){"#c194ef"}else if label.contains("float"){"#609cde"}else{"#66c9ac"}),serde_json::json!(if label.contains("window"){"rgba(168,109,222,.23)"}else if label.contains("float"){"rgba(74,139,212,.23)"}else{"rgba(68,179,146,.28)"})))?;
            self.0.set_bounds(crate::browser::native_bounds(Bounds {
                origin: point(px(r.x), px(r.y)),
                size: size(px(r.width), px(r.height)),
            }))?;
        }
        self.0.set_visible(bounds.is_some())?;
        Ok(())
    }
}

impl Drop for SplitPreview {
    fn drop(&mut self) {
        let _ = self.0.set_visible(false);
    }
}
