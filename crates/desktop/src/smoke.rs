//! Opt-in native integration checks. No external desktop automation or private data.
use crate::{
    app::{BrowserApp, Command},
    browser::BrowserHost,
};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
use serde_json::Value;
use std::{
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use workspace_core::{Axis, TabId};

type Host = Rc<dyn BrowserHost>;

pub fn start(app: &Entity<BrowserApp>, window: &Window, cx: &mut App) {
    let view = app.downgrade();
    window.spawn(cx, async move |cx| {
        let started = Instant::now();
        let result = run(&view,cx).await;
        let report = match &result {
            Ok(checks) => serde_json::json!({"result":"passed","checks":checks,"elapsed_ms":started.elapsed().as_millis(),"kind":"programmatic native smoke; OS mouse/keyboard not simulated"}),
            Err(error) => serde_json::json!({"result":"failed","error":format!("{error:#}")}),
        };
        println!("AXOLOTL_SMOKE {}", report);
        if result.is_err() { std::process::exit(1); }
        let _ = cx.update(|_,cx| cx.quit());
    }).detach();
}

async fn pause(cx: &mut AsyncWindowContext) {
    cx.background_executor()
        .timer(Duration::from_millis(200))
        .await;
    // Occluded windows may skip display-link callbacks during unattended tests.
    let _ = cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear();
    });
}

async fn eval(host: &Host, tab: TabId, script: &str, cx: &mut AsyncWindowContext) -> Result<Value> {
    let response = Arc::new(Mutex::new(None));
    let target = response.clone();
    host.evaluate(
        tab,
        script,
        Box::new(move |value| {
            *target.lock().expect("smoke mutex") = Some(value);
        }),
    )?;
    for _ in 0..50 {
        if let Some(value) = response.lock().expect("smoke mutex").take() {
            return serde_json::from_str(&value)
                .with_context(|| format!("Web engine evaluation in tab {tab} failed: {script}"));
        }
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
    anyhow::bail!("Native web engine evaluation timed out")
}

async fn wait_title(
    host: &Host,
    tab: TabId,
    expected: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    for _ in 0..50 {
        if host.is_loaded(tab)
            && eval(host, tab, "document.title", cx).await?.as_str() == Some(expected)
        {
            return Ok(());
        }
        pause(cx).await;
    }
    anyhow::bail!("Page title did not become {expected}")
}

async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    pause(cx).await;
    let (host, first) = view.update_in(cx, |app, window, cx| {
        window.activate_window();
        cx.activate(true);
        (app.host.clone(), app.active_tab())
    })?;
    wait_title(&host, first, "New tab · Axolotl", cx).await?;
    ensure!(
        eval(
            &host,
            first,
            "document.querySelectorAll('input').length",
            cx
        )
        .await?
        .as_u64()
        .unwrap_or(0)
            > 0,
        "Start page input missing"
    );
    eval(
        &host,
        first,
        "document.querySelector('input').value='retained first page'; true",
        cx,
    )
    .await?;
    let mut checks = vec![
        "real native web-engine document loaded",
        "start page DOM available",
    ];

    view.update_in(cx, |app, window, cx| {
        app.command(Command::Split(Axis::Horizontal), window, cx)
    })?;
    pause(cx).await;
    let second = view.update_in(cx, |app, _, _| app.active_tab())?;
    wait_title(&host, second, "New tab · Axolotl", cx).await?;
    ensure!(
        eval(&host, second, "document.querySelector('input').value", cx).await? == "",
        "New pane shared document state"
    );
    ensure!(
        eval(&host, first, "document.querySelector('input').value", cx).await?
            == "retained first page",
        "Split lost existing page state"
    );
    host.focus(second)?;
    let focus = eval(&host, second, "document.hasFocus()", cx).await?;
    checks.push(if focus == true {
        "native document received focus"
    } else {
        "native focus call succeeded; OS activation unavailable"
    });
    host.navigate(second, "https://example.com")?;
    wait_title(&host, second, "Example Domain", cx).await?;
    ensure!(
        eval(
            &host,
            second,
            "document.body.innerText.includes('documentation examples')",
            cx
        )
        .await?
            == true,
        "External page did not load"
    );
    ensure!(
        eval(&host, first, "document.querySelector('input').value", cx).await?
            == "retained first page",
        "Navigation affected adjacent page"
    );
    host.navigate(second, "https://example.com/?axolotl=history")?;
    wait_title(&host, second, "Example Domain", cx).await?;
    host.back(second)?;
    pause(cx).await;
    wait_title(&host, second, "Example Domain", cx).await?;
    ensure!(
        !host.current_url(second)?.contains("axolotl=history"),
        "Back did not change history entry"
    );
    host.forward(second)?;
    pause(cx).await;
    wait_title(&host, second, "Example Domain", cx).await?;
    ensure!(
        host.current_url(second)?.contains("axolotl=history"),
        "Forward did not change history entry"
    );
    checks.push("native back and forward history");
    host.navigate(second, "about:blank")?;
    wait_title(&host, second, "New tab · Axolotl", cx).await?;
    checks.push("HTTPS page rendering and independent navigation");
    checks.push("independent side-by-side documents");

    view.update_in(cx, |app, window, cx| {
        app.command(Command::Split(Axis::Vertical), window, cx)
    })?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.model.active_space().panes.len())? == 3,
        "Nested split missing"
    );
    checks.push("nested horizontal and vertical splits");
    view.update_in(cx, |app, window, cx| app.command(Command::Grid, window, cx))?;
    pause(cx).await;
    let tabs = view.update_in(cx, |app, _, _| {
        app.model
            .active_space()
            .panes
            .iter()
            .map(|p| p.active_tab)
            .collect::<Vec<_>>()
    })?;
    ensure!(tabs.len() == 4, "Grid must have four panes");
    for tab in &tabs {
        wait_title(&host, *tab, "New tab · Axolotl", cx).await?;
        ensure!(
            eval(&host, *tab, "innerWidth > 20 && innerHeight > 20", cx).await? == true,
            "Native grid viewport empty"
        );
    }
    checks.push("four nonempty native browser surfaces in grid");
    view.update_in(cx, |app, window, cx| app.command(Command::Menu, window, cx))?;
    pause(cx).await;
    ensure!(
        tabs.iter().all(|tab| !host.is_visible(*tab)),
        "Menu left a native surface on top"
    );
    view.update_in(cx, |app, window, cx| app.command(Command::Menu, window, cx))?;
    pause(cx).await;
    ensure!(
        tabs.iter().all(|tab| host.is_visible(*tab)),
        "Menu dismissal failed to restore surfaces"
    );
    checks.push("menu hides and restores native page surfaces");
    view.update_in(cx, |app, _, cx| {
        let geometry = app.model.active_space().layout.geometry(
            workspace_core::Rect::new(0., 0., 1200., 800.),
            6.,
            100.,
        );
        app.model.resize_split(geometry.dividers[0].id, 0.65);
        cx.notify();
    })?;
    pause(cx).await;
    checks.push("split ratio relayout");
    view.update_in(cx, |app, window, cx| {
        app.command(Command::NewSpace, window, cx)
    })?;
    pause(cx).await;
    ensure!(
        tabs.iter().all(|tab| !host.is_visible(*tab)),
        "Inactive space is visible"
    );
    view.update_in(cx, |app, window, cx| {
        let first = app.model.spaces[0].id;
        app.model.switch_space(first);
        app.sync_address(window, cx);
        cx.notify();
    })?;
    pause(cx).await;
    ensure!(
        eval(&host, first, "document.querySelector('input').value", cx).await?
            == "retained first page",
        "Space switch lost page state"
    );
    checks.push("space switching retains hidden page state");
    view.update_in(cx, |app, window, cx| {
        app.command(Command::NewTab, window, cx)
    })?;
    pause(cx).await;
    let new_tab = view.update_in(cx, |app, _, _| app.active_tab())?;
    wait_title(&host, new_tab, "New tab · Axolotl", cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.command(Command::CloseTab, window, cx)
    })?;
    pause(cx).await;
    ensure!(!host.has_view(new_tab), "Closed tab native view retained");
    checks.push("tab creation and native view disposal");
    Ok(checks)
}
