//! Page-only operations; neither settings nor browser chrome are zoom targets.
use crate::app::BrowserApp;
use gpui::{Action, Context};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum Operation {
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Find,
}
impl Operation {
    pub fn from_command(command: &str) -> Option<Self> {
        match command {
            "zoom-in" => Some(Self::ZoomIn),
            "zoom-out" => Some(Self::ZoomOut),
            "zoom-reset" => Some(Self::ZoomReset),
            "find-page" => Some(Self::Find),
            _ => None,
        }
    }
}

#[derive(Clone, Action, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = axolotl, no_json)]
pub struct PageTool {
    pub operation: Operation,
}

pub fn shortcut(key: &str, primary: bool, shift: bool, other: bool) -> Option<Operation> {
    if !primary || other {
        return None;
    }
    match key {
        "=" | "+" => Some(Operation::ZoomIn),
        "-" if !shift => Some(Operation::ZoomOut),
        "0" if !shift => Some(Operation::ZoomReset),
        key if !shift && key.eq_ignore_ascii_case("f") => Some(Operation::Find),
        _ => None,
    }
}

pub fn hostname(address: &str) -> Option<String> {
    let url = url::Url::parse(address).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.host_str().map(str::to_owned))?
}

const STEPS: &[f64] = &[
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2., 2.5, 3., 4., 5.,
];
pub fn next_zoom(current: f64, operation: Operation) -> f64 {
    match operation {
        Operation::ZoomIn => STEPS
            .iter()
            .copied()
            .find(|v| *v > current + 0.001)
            .unwrap_or(5.),
        Operation::ZoomOut => STEPS
            .iter()
            .rev()
            .copied()
            .find(|v| *v < current - 0.001)
            .unwrap_or(0.25),
        _ => 1.,
    }
}

#[derive(Default)]
pub struct SiteZoom(HashMap<String, f64>);
impl SiteZoom {
    pub fn restore() -> Self {
        #[cfg(not(test))]
        match crate::persistence::load::<HashMap<String, f64>>("site_zoom") {
            Ok(Some(values)) => return Self::validated(values),
            Ok(None) => {}
            Err(error) => eprintln!("Restoring website zoom: {error:#}"),
        }
        Self::default()
    }
    fn validated(mut values: HashMap<String, f64>) -> Self {
        values.retain(|_, value| value.is_finite() && (0.25..=5.).contains(value));
        Self(values)
    }
    pub fn get(&self, address: &str) -> f64 {
        hostname(address)
            .and_then(|host| self.0.get(&host).copied())
            .unwrap_or(1.)
    }
    pub fn change(&mut self, address: &str, operation: Operation) -> anyhow::Result<f64> {
        let value = next_zoom(self.get(address), operation);
        if let Some(host) = hostname(address) {
            if value == 1. {
                self.0.remove(&host);
            } else {
                self.0.insert(host, value);
            }
            #[cfg(not(test))]
            crate::persistence::save("site_zoom", &self.0)?;
        }
        Ok(value)
    }
}

impl BrowserApp {
    pub(crate) fn install_page_tools(cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        gpui::App::on_action(cx, move |action: &PageTool, cx| {
            let view = view.clone();
            let operation = action.operation;
            cx.defer(move |cx| {
                let _ = view.update(cx, |app, _| {
                    if app.settings.is_some() {
                        return;
                    }
                    let fallback = if app
                        .link_popup
                        .as_ref()
                        .is_some_and(|p| p.closing_until.is_none())
                    {
                        crate::link_popup::POPUP_TAB
                    } else {
                        app.pins.open.unwrap_or_else(|| app.active_tab())
                    };
                    let tab = app.host.focused_page().unwrap_or(fallback);
                    app.report(app.host.page_tool(tab, operation));
                });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hostname_shares_schemes_ports_not_subdomains() {
        let mut zoom = SiteZoom::default();
        zoom.change("https://EXAMPLE.com:443/a", Operation::ZoomIn)
            .unwrap();
        assert_eq!(zoom.get("http://example.com:8080/b"), 1.1);
        assert_eq!(zoom.get("https://sub.example.com"), 1.);
        assert_eq!(zoom.get("https://other.com"), 1.);
        for url in [
            "about:blank",
            "data:text/html,hello",
            "file:///test",
            "invalid",
        ] {
            assert_eq!(hostname(url), None);
        }
        zoom.change("http://example.com", Operation::ZoomReset)
            .unwrap();
        assert!(zoom.0.is_empty());
    }
    #[test]
    fn steps_reset_and_limits() {
        assert_eq!(next_zoom(1., Operation::ZoomIn), 1.1);
        assert_eq!(next_zoom(1.1, Operation::ZoomOut), 1.);
        assert_eq!(next_zoom(5., Operation::ZoomIn), 5.);
        assert_eq!(next_zoom(0.25, Operation::ZoomOut), 0.25);
        assert_eq!(next_zoom(2., Operation::ZoomReset), 1.);
    }
    #[test]
    fn sqlite_restart_preserves_zoom_and_unrelated_data() {
        let dir =
            std::env::temp_dir().join(format!("axolotl-zoom-{id}", id = uuid::Uuid::new_v4()));
        let mut zoom = SiteZoom::default();
        zoom.change("https://example.com", Operation::ZoomIn)
            .unwrap();
        crate::persistence::save_to(&dir, "settings", &serde_json::json!({"untouched":true}))
            .unwrap();
        crate::persistence::save_to(&dir, "site_zoom", &zoom.0).unwrap();
        let restored = SiteZoom::validated(
            crate::persistence::load_from(&dir, "site_zoom")
                .unwrap()
                .unwrap(),
        );
        assert_eq!(restored.get("http://example.com:123"), 1.1);
        assert_eq!(
            crate::persistence::load_from::<serde_json::Value>(&dir, "settings")
                .unwrap()
                .unwrap()["untouched"],
            true
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn invalid_saved_levels_are_ignored() {
        let values = HashMap::from([
            ("small.example".into(), 0.1),
            ("huge.example".into(), 9.),
            ("nan.example".into(), f64::NAN),
            ("valid.example".into(), 1.25),
        ]);
        let zoom = SiteZoom::validated(values);
        assert_eq!(zoom.0.len(), 1);
        assert_eq!(zoom.get("https://valid.example"), 1.25);
        assert_eq!(zoom.get("https://small.example"), 1.);
    }

    #[test]
    fn reserved_keys_reject_extra_modifiers() {
        for (key, shift, expected) in [
            ("=", false, Operation::ZoomIn),
            ("+", true, Operation::ZoomIn),
            ("-", false, Operation::ZoomOut),
            ("0", false, Operation::ZoomReset),
            ("F", false, Operation::Find),
        ] {
            assert_eq!(shortcut(key, true, shift, false), Some(expected));
            assert_eq!(shortcut(key, false, shift, false), None);
            assert_eq!(shortcut(key, true, shift, true), None);
        }
        assert_eq!(shortcut("f", true, true, false), None);
    }
}
