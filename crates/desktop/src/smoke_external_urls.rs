//! Opt-in subprocess probe for real OS URL events and single-instance launches.
use crate::app::BrowserApp;
use gpui::*;
use std::time::Duration;

pub fn probe(app: &Entity<BrowserApp>, window: &Window, cx: &mut App) {
    if !std::env::args().any(|arg| arg == "--external-url-probe") {
        return;
    }
    let Some(dir) = crate::persistence::data_dir() else {
        return;
    };
    #[cfg(target_os = "macos")]
    let registered = {
        use objc2_app_kit::NSWorkspace;
        use objc2_foundation::{NSBundle, NSString, NSURL};
        let identity = NSBundle::mainBundle().bundleIdentifier();
        ["http", "https"].iter().all(|scheme| {
            let url = NSURL::URLWithString(&NSString::from_str(&format!("{scheme}://example.com")))
                .unwrap();
            identity.as_ref().is_some_and(|identity| {
                NSWorkspace::sharedWorkspace()
                    .URLsForApplicationsToOpenURL(&url)
                    .iter()
                    .any(|app| {
                        NSBundle::bundleWithURL(&app)
                            .and_then(|b| b.bundleIdentifier())
                            .as_ref()
                            == Some(identity)
                    })
            })
        })
    };
    #[cfg(not(target_os = "macos"))]
    let registered = serde_json::Value::Null;
    let app = app.downgrade();
    window.spawn(cx, async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            if app.update_in(cx, |app, window, cx| {
                if dir.join("minimize-probe").exists() {
                    let _ = std::fs::remove_file(dir.join("minimize-probe"));
                    window.minimize_window();
                }
                let state = serde_json::json!({
                    "workspace": serde_json::from_str::<serde_json::Value>(&app.model.to_json()).unwrap(),
                    "active": window.is_window_active(),
                    "settings": app.settings.is_some(),
                    "browserRegistered": registered,
                    "defaultBrowser": app.default_browser.state,
                });
                let _ = workspace_core::write_atomic(&dir.join("external-probe.json"), &serde_json::to_vec(&state).unwrap());
                if dir.join("quit-probe").exists() { cx.quit(); }
            }).is_err() { break; }
        }
    }).detach();
}
