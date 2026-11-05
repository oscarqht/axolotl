//! Process-exit probe: native AppKit key queue/menu, with an isolated profile.
use crate::{app::BrowserApp, smoke::pause};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSView};
use objc2_foundation::{NSPoint, NSString};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub fn probe(app: &Entity<BrowserApp>, window: &Window, cx: &mut App) {
    let Some(mode) =
        std::env::args().find_map(|a| a.strip_prefix("--quit-probe=").map(str::to_owned))
    else {
        return;
    };
    assert!(std::env::var_os("AXOLOTL_DATA_DIR").is_some());
    let view = app.downgrade();
    window.spawn(cx, async move |cx| {
        let result: Result<()> = async {
            if mode == "restore" {
                view.update_in(cx, |app, _, cx| {
                    println!("AXOLOTL_QUIT_SESSION {}", serde_json::from_str::<serde_json::Value>(&app.model.to_json()).expect("session JSON"));
                    cx.quit();
                })?;
                return Ok(());
            }
            for _ in 0..5 { pause(cx).await; }
            let (host, tab) = view.update_in(cx, |app, window, cx| { window.activate_window(); cx.activate(true); (app.host.clone(), app.active_tab()) })?;
            for _ in 0..60 {
                if host.has_view(tab) { break; }
                pause(cx).await;
            }
            ensure!(host.has_view(tab), "native page not created");
            if !mode.ends_with("-existing") {
                crate::smoke::wait_title(&host, tab, "New tab · Axolotl", cx).await?;
            }
            if mode.contains("pip") {
                crate::smoke_auto_pip::videos(&host, tab, cx).await?;
                view.update_in(cx, |app, _, cx| { app.model.add_tab("about:blank"); cx.notify(); })?;
                crate::smoke_auto_pip::state(&view, Some(tab), cx).await?;
            }
            view.update_in(cx, |app, _, _| {
                app.model.set_custom_title(tab, "Quit regression saved tab");
            })?;
            host.focus(tab)?;
            view.update_in(cx, |app, _, _| {
                println!("AXOLOTL_QUIT_SESSION {}", serde_json::from_str::<serde_json::Value>(&app.model.to_json()).expect("session JSON"));
            })?;
            println!("AXOLOTL_QUIT_READY {mode}");
            cx.update(|window, cx| -> Result<()> {
                let native_app = NSApplication::sharedApplication(MainThreadMarker::new().context("main thread")?);
                if mode.starts_with("menu") {
                    // Execute the actual menu item's target/action, outside GPUI's borrow.
                    let menu = native_app.mainMenu().context("main menu")?;
                    let submenu = menu.itemAtIndex(0).context("app menu")?.submenu().context("submenu")?;
                    let index = submenu.indexOfItemWithTitle(&NSString::from_str("Quit Axolotl"));
                    ensure!(index >= 0, "Quit menu item missing");
                    cx.foreground_executor().spawn(async move {
                        unsafe { let _: () = msg_send![&*submenu, performActionForItemAtIndex: index]; }
                    }).detach();
                } else {
                    let RawWindowHandle::AppKit(handle) = window.window_handle()?.as_raw() else { anyhow::bail!("AppKit required") };
                    let root = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
                    let native = root.window().context("native window")?;
                    let key = NSString::from_str("q");
                    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(NSEventType::KeyDown, NSPoint::new(0.,0.), NSEventModifierFlags::Command, 0., native.windowNumber(), None, &key, &key, false, 12).context("quit key event")?;
                    native_app.postEvent_atStart(&event, false);
                }
                Ok(())
            })??;
            Ok(())
        }.await;
        if let Err(error) = result { eprintln!("Quit probe failed: {error:#}"); std::process::exit(1); }
    }).detach();
}
