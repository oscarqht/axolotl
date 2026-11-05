mod app;
mod app_database;
mod browser;
#[cfg(target_os = "macos")]
mod browser_identity;
#[cfg(test)]
mod browser_tests;
mod browsing_history;
mod chrome_theme;
#[cfg(target_os = "macos")]
mod copy_url;
mod default_browser;
mod docking;
mod downloads;
mod downloads_ui;
mod drag_preview;
mod external_urls;
mod floating_bar;
#[cfg(test)]
mod gesture_tests;
mod history_suggestions;
mod link_popup;
#[cfg(target_os = "macos")]
mod macos_keyboard;
#[cfg(target_os = "macos")]
mod macos_passkeys;
#[cfg(target_os = "macos")]
mod macos_popup;
#[cfg(target_os = "macos")]
mod macos_quit;
#[cfg(target_os = "macos")]
mod macos_titlebar;
#[cfg(target_os = "macos")]
mod native_popup_tabs;
mod native_webview;
mod panel_controls;
mod persistence;
mod persistence_worker;
mod pinned_tabs;
mod popup_transition;
mod preferences;
mod protection;
mod protection_native;
#[cfg(any(test, target_os = "windows"))]
mod protection_script;
mod render;
mod settings;
mod sleep;
#[cfg(feature = "smoke")]
mod smoke;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_copy_url;
#[cfg(feature = "smoke")]
mod smoke_corners;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_docking;
#[cfg(feature = "smoke")]
mod smoke_downloads;
#[cfg(feature = "smoke")]
mod smoke_external_urls;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_google_popup;
#[cfg(feature = "smoke")]
mod smoke_history;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_identity;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_input;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_keyboard;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_link_popup;
#[cfg(feature = "smoke")]
mod smoke_menu;
#[cfg(feature = "smoke")]
mod smoke_notifications;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_oauth;
#[cfg(feature = "smoke")]
mod smoke_panels;
#[cfg(feature = "smoke")]
mod smoke_performance;
#[cfg(feature = "smoke")]
mod smoke_persistence;
#[cfg(feature = "smoke")]
mod smoke_pins;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_popup_transition;
#[cfg(feature = "smoke")]
mod smoke_protection;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_quit;
#[cfg(feature = "smoke")]
mod smoke_responsiveness;
#[cfg(feature = "smoke")]
mod smoke_settings;
#[cfg(feature = "smoke")]
mod smoke_sleep;
#[cfg(feature = "smoke")]
mod smoke_space_drag;
#[cfg(feature = "smoke")]
mod smoke_space_menu;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_space_shortcuts;
#[cfg(all(feature = "smoke", target_os = "macos"))]
mod smoke_x_oauth;
mod space_drag;
mod space_menu;
mod space_shortcuts;
mod split_preview;
mod ui_events;
mod window_state;

use app::BrowserApp;
use gpui::{prelude::*, *};
use gpui_component::Root;

actions!(
    axolotl,
    [
        Quit, Address, NewTab, NewSpace, CloseTab, Reload, NextPane, Back, Forward, Settings,
        CopyUrl
    ]
);

/// Zero-based position in the current space strip.
#[derive(Clone, Action, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = axolotl, no_json)]
pub struct SelectSpace {
    pub index: usize,
}

fn main() {
    let instance = match external_urls::SingleInstance::start() {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(error) => {
            eprintln!("Axolotl: could not deliver launch request: {error:#}");
            std::process::exit(1);
        }
    };
    #[cfg(target_os = "windows")]
    if !(cfg!(feature = "smoke")
        && std::env::args().any(|arg| arg.starts_with("--smoke") || arg.contains("-probe")))
        && let Err(error) = default_browser::register()
    {
        eprintln!(
            "Axolotl: browser registration failed: {error:#}. Retry with Make default in Settings."
        );
    }
    eprintln!("Axolotl: initializing platform");
    // WebView2 child HWNDs require GPUI's non-DirectComposition window path.
    #[cfg(target_os = "windows")]
    unsafe {
        std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "true");
    }
    let incoming = instance.incoming.clone();
    let application = Application::new();
    // Install before AppKit starts dispatching cold-launch Apple events.
    application.on_open_urls(move |urls| external_urls::enqueue(&incoming, urls));
    application.run(move |cx: &mut App| {
        cx.set_global(external_urls::Incoming(instance));
        eprintln!("Axolotl: platform launched");
        gpui_component::init(cx);
        #[cfg(target_os = "macos")]
        macos_quit::install(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.bind_keys([
            KeyBinding::new(&format!("{modifier}-q"), Quit, None),
            KeyBinding::new(&format!("{modifier}-l"), Address, None),
            KeyBinding::new(&format!("{modifier}-t"), NewTab, None),
            KeyBinding::new(&format!("{modifier}-shift-t"), NewSpace, None),
            KeyBinding::new(&format!("{modifier}-w"), CloseTab, None),
            KeyBinding::new(&format!("{modifier}-r"), Reload, None),
            KeyBinding::new("f6", NextPane, None),
            KeyBinding::new(&format!("{modifier}-,"), Settings, None),
            KeyBinding::new("escape", gpui_component::input::Escape, None),
            KeyBinding::new("alt-left", Back, None),
            KeyBinding::new("alt-right", Forward, None),
        ]);
        #[cfg(target_os = "macos")]
        cx.bind_keys([KeyBinding::new("cmd-shift-c", CopyUrl, None)]);
        #[cfg(target_os = "macos")]
        cx.bind_keys((1..=9).map(|number| {
            KeyBinding::new(
                &format!("cmd-{number}"),
                SelectSpace { index: number - 1 },
                None,
            )
        }));
        #[cfg(target_os = "macos")]
        cx.on_action(|_: &macos_passkeys::EnablePasskeys, cx| macos_passkeys::enable(cx));
        cx.set_menus(vec![
            Menu {
                name: "Axolotl".into(),
                items: vec![
                    MenuItem::action("Settings…", Settings),
                    #[cfg(target_os = "macos")]
                    MenuItem::action("Enable Passkeys…", macos_passkeys::EnablePasskeys),
                    MenuItem::action("Quit Axolotl", Quit),
                ],
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::os_action("Cut", gpui_component::input::Cut, OsAction::Cut),
                    MenuItem::os_action("Copy", gpui_component::input::Copy, OsAction::Copy),
                    MenuItem::os_action("Paste", gpui_component::input::Paste, OsAction::Paste),
                    MenuItem::os_action(
                        "Select All",
                        gpui_component::input::SelectAll,
                        OsAction::SelectAll,
                    ),
                ],
            },
        ]);
        let saved_window = window_state::SavedWindow::restore();
        let (bounds, display_id) = saved_window.startup(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(bounds),
                display_id,
                window_min_size: Some(size(px(640.), px(420.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Axolotl".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.), px(13.))),
                }),
                ..Default::default()
            },
            |window, cx| {
                #[cfg(target_os = "macos")]
                macos_titlebar::install(window).expect("configure native titlebar input");
                let app = cx.new(|cx| BrowserApp::new(window, cx, saved_window));
                #[cfg(feature = "smoke")]
                smoke_external_urls::probe(&app, window, cx);
                app.read(cx).focus.focus(window);
                #[cfg(all(feature = "smoke", target_os = "macos"))]
                window_state::probe(&app, window, cx);
                #[cfg(feature = "smoke")]
                smoke_space_menu::session_probe(&app, window, cx);
                #[cfg(feature = "smoke")]
                smoke_persistence::probe(&app, window, cx);
                #[cfg(all(feature = "smoke", target_os = "macos"))]
                smoke_quit::probe(&app, window, cx);
                #[cfg(feature = "smoke")]
                if std::env::args()
                    .any(|arg| arg == "--smoke-test" || arg == "--smoke-sleep-session")
                {
                    smoke::start(&app, window, cx);
                }
                cx.new(|cx| Root::new(app, window, cx))
            },
        )
        .expect("open Axolotl window");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        eprintln!("Axolotl: window created");
        cx.activate(true);
    });
}
