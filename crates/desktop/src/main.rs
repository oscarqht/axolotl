mod app;
mod browser;
mod drag_preview;
mod render;
#[cfg(feature = "smoke")]
mod smoke;
mod tab_menu;

use app::BrowserApp;
use gpui::{prelude::*, *};
use gpui_component::{Root, Theme, ThemeMode};

actions!(
    axolotl,
    [
        Quit, Address, NewTab, CloseTab, Reload, NextPane, Back, Forward
    ]
);

fn main() {
    eprintln!("Axolotl: initializing platform");
    // WebView2 child HWNDs require GPUI's non-DirectComposition window path.
    #[cfg(target_os = "windows")]
    unsafe {
        std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "true");
    }
    Application::new().run(|cx: &mut App| {
        eprintln!("Axolotl: platform launched");
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
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
            KeyBinding::new(&format!("{modifier}-w"), CloseTab, None),
            KeyBinding::new(&format!("{modifier}-r"), Reload, None),
            KeyBinding::new("f6", NextPane, None),
            KeyBinding::new("escape", gpui_component::input::Escape, None),
            KeyBinding::new("alt-left", Back, None),
            KeyBinding::new("alt-right", Forward, None),
        ]);
        cx.set_menus(vec![
            Menu {
                name: "Axolotl".into(),
                items: vec![MenuItem::action("Quit Axolotl", Quit)],
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
        let bounds = Bounds::centered(None, size(px(1280.), px(860.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(640.), px(420.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Axolotl".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.), px(13.))),
                }),
                ..Default::default()
            },
            |window, cx| {
                let app = cx.new(|cx| BrowserApp::new(window, cx));
                app.read(cx).focus.focus(window);
                #[cfg(feature = "smoke")]
                if std::env::args().any(|arg| arg == "--smoke-test") {
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
