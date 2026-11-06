//! Queued AppKit history shortcuts, including native controls and shell focus.
use crate::{
    app::Command,
    smoke::{Host, eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use objc2_app_kit::NSEventModifierFlags as Flags;
use workspace_core::{Axis, TabId};

async fn prepare(host: &Host, tab: TabId, cx: &mut AsyncWindowContext) -> Result<()> {
    eval(host, tab, "history.replaceState(null,'','#one');history.pushState(null,'','#two');history.pushState(null,'','#three');window.historyKeys=0;addEventListener('keydown',e=>{if(e.metaKey&&['[',']'].includes(e.key)){historyKeys++;e.preventDefault();e.stopImmediatePropagation()}},true);true", cx).await?;
    Ok(())
}

async fn press(
    host: &Host,
    tab: TabId,
    forward: bool,
    expected: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    host.test_key(
        tab,
        if forward { 30 } else { 33 },
        if forward { "]" } else { "[" },
        Flags::Command,
        false,
    )?;
    for _ in 0..20 {
        pause(cx).await;
        if eval(host, tab, "location.hash", cx).await? == expected {
            // Allow any accidental second navigation to settle before asserting.
            pause(cx).await;
            ensure!(
                eval(host, tab, "location.hash", cx).await? == expected,
                "History shortcut navigated twice"
            );
            ensure!(
                eval(host, tab, "historyKeys", cx).await? == 0,
                "Page intercepted reserved shortcut"
            );
            return Ok(());
        }
    }
    anyhow::bail!("History shortcut did not reach {expected}")
}

pub async fn run(
    view: &WeakEntity<crate::app::BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, first) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    let url = crate::smoke_page_tools::fixture()?;
    host.navigate(first, &url)?;
    wait_title(&host, first, "Page tools fixture", cx).await?;
    // Full document navigations must work as well as same-document SPA history.
    for query in ["?one", "?two"] {
        host.navigate(first, &format!("{url}{query}"))?;
        wait_title(&host, first, "Page tools fixture", cx).await?;
    }
    host.focus(first)?;
    for (forward, expected) in [(false, "?one"), (true, "?two")] {
        host.test_key(
            first,
            if forward { 30 } else { 33 },
            if forward { "]" } else { "[" },
            Flags::Command,
            false,
        )?;
        let mut reached = false;
        for _ in 0..30 {
            pause(cx).await;
            if host.is_loaded(first) && eval(&host, first, "location.search", cx).await? == expected
            {
                reached = true;
                break;
            }
        }
        ensure!(reached, "Full-document history did not reach {expected}");
    }
    prepare(&host, first, cx).await?;
    host.focus(first)?;
    eval(
        &host,
        first,
        "document.querySelector('input').focus();true",
        cx,
    )
    .await?;
    press(&host, first, false, "#two", cx).await?;
    press(&host, first, true, "#three", cx).await?;
    // Forward at the history boundary is harmless.
    press(&host, first, true, "#three", cx).await?;

    // Chrome focus uses the same reserved shortcuts, not WebKit defaults.
    view.update_in(cx, |app, window, _| {
        app.host.focus_parent(first).unwrap();
        app.focus.focus(window);
    })?;
    press(&host, first, false, "#two", cx).await?;
    press(&host, first, true, "#three", cx).await?;
    // Also exercise the shell action path used by GPUI bindings.
    view.update_in(cx, |_, window, cx| {
        window.dispatch_action(Box::new(crate::Back), cx)
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    ensure!(
        eval(&host, first, "location.hash", cx).await? == "#two",
        "Chrome history action failed or navigated twice"
    );
    press(&host, first, true, "#three", cx).await?;

    view.update_in(cx, |app, window, cx| {
        app.command(Command::Split(Axis::Horizontal), window, cx)
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    let second = view.update_in(cx, |app, _, _| app.active_tab())?;
    host.navigate(second, &url)?;
    wait_title(&host, second, "Page tools fixture", cx).await?;
    prepare(&host, second, cx).await?;
    host.focus(first)?;
    press(&host, first, false, "#two", cx).await?;
    ensure!(
        eval(&host, second, "location.hash", cx).await? == "#three",
        "History targeted sibling split"
    );

    // Pins have no normal pane target once removed from their source stack.
    view.update_in(cx, |app, window, cx| {
        let pane = app
            .model
            .active_space()
            .panes
            .iter()
            .find(|p| p.active_tab == first)
            .unwrap()
            .id;
        app.pin_page(pane, first, window, cx);
        app.toggle_pin(first, window, cx);
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    host.focus(first)?;
    press(&host, first, true, "#three", cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.dismiss_pin(window, cx);
        app.open_link_popup(second, url.clone(), window, cx);
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    let popup = crate::link_popup::POPUP_TAB;
    wait_title(&host, popup, "Page tools fixture", cx).await?;
    prepare(&host, popup, cx).await?;
    host.focus(popup)?;
    press(&host, popup, false, "#two", cx).await?;
    press(&host, popup, true, "#three", cx).await?;
    ensure!(
        eval(&host, second, "location.hash", cx).await? == "#three",
        "Preview history changed underlying page"
    );

    view.update_in(cx, |app, window, cx| {
        app.close_link_popup(window, cx);
        let pane = app
            .model
            .active_space()
            .panes
            .iter()
            .find(|p| p.active_tab == second)
            .unwrap()
            .id;
        app.model.float_pane(
            pane,
            workspace_core::Rect {
                x: 80.,
                y: 100.,
                width: 500.,
                height: 400.,
            },
            None,
        );
        app.model.pin_floating(pane);
        app.reveal_floating(pane);
        cx.notify();
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    host.focus(second)?;
    press(&host, second, false, "#two", cx).await?;
    press(&host, second, true, "#three", cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.command(Command::Settings, window, cx)
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    press(&host, second, false, "#three", cx).await?;
    Ok(vec![
        "Cmd+[ and Cmd+] navigate exactly once with native input, chrome, split, pin, preview and floating focus; page interception blocked; forward boundary and Settings are no-ops",
    ])
}
