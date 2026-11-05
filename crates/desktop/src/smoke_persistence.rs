//! Exercise restored state and edits followed by exit before the next UI poll.
use crate::app::BrowserApp;
use gpui::*;

fn inspect(app: &BrowserApp) -> anyhow::Result<()> {
    let protection = app.host.protection();
    let protection = protection.borrow();
    let state = serde_json::json!({
        "session": serde_json::from_str::<serde_json::Value>(&app.model.to_json())?,
        "sleep": app.sleep.entries,
        "settings": {
            "auto_sleep": app.preferences.auto_sleep,
            "sleep_minutes": app.preferences.sleep_minutes,
            "search_engine": app.preferences.search_engine.id(),
            "homepage_url": app.preferences.homepage_url,
            "history_retention_days": app.preferences.history_retention_days,
        },
        "protection": {
            "enabled": protection.policy.enabled,
            "exceptions": protection.policy.exceptions,
        },
        "sleep_live_views": app.sleep.entries.keys().filter(|id| app.host.has_view(**id)).collect::<Vec<_>>(),
        "sleep_images": app.sleep.entries.keys().filter(|id| app.sleep.image(**id).is_some()).collect::<Vec<_>>(),
    });
    println!("AXOLOTL_PERSISTENCE_STATE {state}");
    Ok(())
}

pub fn probe(app: &Entity<BrowserApp>, window: &Window, cx: &mut App) {
    let Some(stage) = std::env::args().find_map(|a| {
        a.strip_prefix("--persistence-exit-probe=")
            .map(str::to_owned)
    }) else {
        return;
    };
    assert!(std::env::var_os("AXOLOTL_DATA_DIR").is_some());
    let app = app.clone();
    window
        .spawn(cx, async move |cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(300))
                .await;
            let result = app.update_in(cx, |app, window, cx| -> anyhow::Result<()> {
                match stage.as_str() {
                    "inspect" => inspect(app)?,
                    "seed" => {
                        let space = app.model.active_space;
                        app.model
                            .set_space_details(space, "Restart space", Some("🚀"));
                    }
                    "wake-pins" => {
                        let pins: Vec<_> = app.model.pinned_tabs.iter().map(|p| p.tab.id).collect();
                        for pin in pins {
                            app.wake_tab(pin);
                        }
                    }
                    "restore" => {
                        anyhow::ensure!(app.model.active_space().name == "Exit checkpoint");
                        anyhow::ensure!(
                            app.model
                                .focused_pane()
                                .active_tab()
                                .custom_title
                                .as_deref()
                                == Some("Last edit before exit")
                        );
                        anyhow::ensure!(app.sleep.keep_alive(app.active_tab()));
                        anyhow::ensure!(
                            app.preferences.search_engine == workspace_core::SearchEngine::Bing
                        );
                    }
                    "close" | "quit" => {
                        app.model.add_space("Exit checkpoint", "about:blank");
                        let tab = app.active_tab();
                        app.toggle_keep_alive(tab);
                        app.preferences
                            .set_search_engine(workspace_core::SearchEngine::Bing)?;
                        // Mutate after checkpoint, then close in this same update.
                        app.model.set_custom_title(tab, "Last edit before exit");
                    }
                    _ => anyhow::bail!("unknown persistence probe stage"),
                }
                println!("AXOLOTL_PERSISTENCE_EXIT {stage}");
                if stage == "close" {
                    window.remove_window();
                } else {
                    cx.quit();
                }
                Ok(())
            });
            match result {
                Ok(Ok(())) => (),
                error => {
                    eprintln!("Persistence exit probe failed: {error:?}");
                    std::process::exit(1);
                }
            }
        })
        .detach();
}
