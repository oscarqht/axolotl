//! Space-index shortcuts through GPUI actions and AppKit's real event queue.
use crate::{
    app::{BrowserApp, TabTarget},
    smoke::{Host, eval, pause},
};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSView};
use objc2_foundation::{NSPoint, NSString};
use workspace_core::{SpaceId, TabId};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Unlike NSWindow.sendEvent, queued events pass through AppKit's local monitor.
fn post_digit(window: &Window, digit: usize) -> Result<()> {
    let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)?.as_raw() else {
        anyhow::bail!("Expected AppKit window");
    };
    let root = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let native = root.window().context("Missing native shortcut window")?;
    let app =
        NSApplication::sharedApplication(MainThreadMarker::new().context("Expected main thread")?);
    let key = NSString::from_str(&digit.to_string());
    let code = [18, 19, 20, 21, 23, 22, 26, 28, 25][digit - 1];
    for kind in [NSEventType::KeyDown, NSEventType::KeyUp] {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind, NSPoint::new(0.,0.), NSEventModifierFlags::Command, 0., native.windowNumber(), None, &key, &key, false, code,
        ).context("create native digit shortcut")?;
        app.postEvent_atStart(&event, false);
    }
    Ok(())
}

async fn wait_space(
    view: &WeakEntity<BrowserApp>,
    expected: SpaceId,
    cx: &mut AsyncWindowContext,
) -> Result<TabId> {
    for _ in 0..30 {
        pause(cx).await;
        let selected = view.update_in(cx, |app, _, _| {
            (app.model.active_space == expected).then(|| app.active_tab())
        })?;
        if let Some(tab) = selected {
            return Ok(tab);
        }
    }
    anyhow::bail!("Shortcut did not select expected space {expected}")
}

async fn native_select(
    view: &WeakEntity<BrowserApp>,
    digit: usize,
    cx: &mut AsyncWindowContext,
) -> Result<TabId> {
    let expected = view.update_in(cx, |app, window, _| -> Result<_> {
        let expected = app.model.spaces[digit - 1].id;
        post_digit(window, digit)?;
        Ok(expected)
    })??;
    wait_space(view, expected, cx).await
}

async fn assert_page_focus(host: &Host, tab: TabId, cx: &mut AsyncWindowContext) -> Result<()> {
    // Verify responder ownership even when another desktop app is foreground.
    for _ in 0..20 {
        pause(cx).await;
        if host.has_view(tab) && host.is_loaded(tab) && host.has_native_focus(tab)? {
            return Ok(());
        }
    }
    anyhow::bail!("Selected space page {tab} did not receive the native first responder")
}

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, first, first_tab) = view.update_in(cx, |app, window, cx| {
        let first = app.model.active_space;
        let tab = app.active_tab();
        for i in 2..=3 {
            app.model.add_space(format!("Shortcut {i}"), "about:blank");
        }
        app.model.set_space_pinned(first, true);
        app.model.switch_space(first);
        app.focus.focus(window);
        cx.notify();
        (app.host.clone(), first, tab)
    })?;
    pause(cx).await;
    eval(&host, first_tab, "window.spaceShortcutIdentity={};document.body.innerHTML='<input value=\"retained space state\">';true", cx).await?;

    // The unavailable ninth shortcut must preserve an active editor and draft.
    let (draft, editing) = view.update_in(cx, |app, window, cx| {
        app.begin_edit(TabTarget::Space(first), window, cx);
        app.address.update(cx, |input, cx| {
            input.set_value("unfinished shortcut draft", window, cx)
        });
        let draft = app.address.read(cx).value().to_string();
        let editing = app.editing;
        window.dispatch_action(Box::new(crate::SelectSpace { index: 8 }), cx);
        (draft, editing)
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |_, window, _| post_digit(window, 9))??;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, cx| -> Result<()> {
        ensure!(
            app.model.active_space == first && app.editing == editing,
            "Unavailable shortcut changed selection or editing"
        );
        ensure!(
            app.address.read(cx).value().as_str() == draft,
            "Unavailable shortcut changed address draft"
        );
        Ok(())
    })??;

    // An action dispatched from the actual input reaches the enclosing shell.
    let second = view.update_in(cx, |app, window, cx| {
        let second = app.model.spaces[1].id;
        window.dispatch_action(Box::new(crate::SelectSpace { index: 1 }), cx);
        second
    })?;
    wait_space(view, second, cx).await?;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.editing.is_none(),
            "Valid selection kept unfinished editor"
        );
        Ok(())
    })??;

    // Native Cmd+1 from a page must pass the local monitor rather than WebKit.
    host.focus(view.update_in(cx, |app, _, _| app.active_tab())?)?;
    let selected = native_select(view, 1, cx).await?;
    ensure!(selected == first_tab, "Cmd+1 selected wrong first space");
    assert_page_focus(&host, first_tab, cx).await?;
    ensure!(eval(&host, first_tab, "!!window.spaceShortcutIdentity&&document.querySelector('input').value==='retained space state'", cx).await? == true, "Space switching replaced live document");

    // Invalid native Cmd+9 must leave Settings open; a valid Cmd+2 closes it.
    view.update_in(cx, |app, window, cx| app.open_settings(true, window, cx))?;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |_, window, _| post_digit(window, 9))??;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.settings.is_some() && app.model.active_space == first,
            "Unavailable Cmd+9 dismissed Settings or changed space"
        );
        Ok(())
    })??;
    let second_tab = native_select(view, 2, cx).await?;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.settings.is_none(),
            "Valid native shortcut left Settings open"
        );
        Ok(())
    })??;
    assert_page_focus(&host, second_tab, cx).await?;

    // The same global monitor works with a temporary native popup focused.
    view.update_in(cx, |app, window, cx| {
        app.open_link_popup(second_tab, "about:blank".into(), window, cx)
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |_, window, _| post_digit(window, 9))??;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.link_popup.is_some() && app.model.active_space == second,
            "Unavailable Cmd+9 dismissed popup or changed space"
        );
        Ok(())
    })??;
    native_select(view, 1, cx).await?;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.link_popup.is_none() && !app.host.has_view(crate::link_popup::POPUP_TAB),
            "Valid shortcut kept temporary popup"
        );
        Ok(())
    })??;
    assert_page_focus(&host, first_tab, cx).await?;

    // Cmd+9 means exactly the ninth current slot, even with more than nine.
    let ninth = view.update_in(cx, |app, _, cx| {
        for i in 4..=10 {
            app.model.add_space(format!("Shortcut {i}"), "about:blank");
        }
        app.model.switch_space(first);
        let ninth = app.model.spaces[8].id;
        cx.notify();
        ninth
    })?;
    pause(cx).await;
    let ninth_tab = native_select(view, 9, cx).await?;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.model.active_space == ninth
                && app.model.active_space != app.model.spaces.last().unwrap().id,
            "Cmd+9 selected last space instead of exact ninth"
        );
        Ok(())
    })??;
    assert_page_focus(&host, ninth_tab, cx).await?;

    view.update_in(cx, |app, window, cx| {
        app.model.set_space_pinned(ninth, true);
        app.model.move_space(ninth, 0);
        app.focus.focus(window);
        window.dispatch_action(Box::new(crate::SelectSpace { index: 1 }), cx);
    })?;
    wait_space(view, first, cx).await?;
    let reordered_first = native_select(view, 1, cx).await?;
    ensure!(
        reordered_first == ninth_tab,
        "Cmd+1 did not follow reordered first space"
    );
    let reordered_second = native_select(view, 2, cx).await?;
    ensure!(
        reordered_second == first_tab,
        "Cmd+2 did not follow reordered current spaces"
    );
    assert_page_focus(&host, first_tab, cx).await?;
    ensure!(eval(&host, first_tab, "!!window.spaceShortcutIdentity&&document.querySelector('input').value==='retained space state'", cx).await? == true, "Reordering/switching lost page state");
    Ok(vec![
        "GPUI space actions bubble from shell and address editor; valid selection cancels editing while unavailable slots preserve the draft",
        "AppKit-queued native Cmd+1, Cmd+2 and Cmd+9 reach the global monitor with web pages, Settings and temporary popup focused",
        "unavailable native slots preserve modal state; valid selection dismisses Settings/popup and focuses selected native page",
        "Cmd+9 selects exactly ninth among ten spaces; pinned and regular slots use current space order and retain live page state",
    ])
}
