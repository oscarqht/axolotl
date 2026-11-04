//! Opt-in native integration checks. No external desktop automation or private data.
use crate::{
    app::{BrowserApp, Command, TabAction, TabTarget},
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
        app.begin_edit(TabTarget::Space(app.model.active_space), window, cx);
    })?;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        app.address.update(cx, |input, cx| {
            input.replace("https://example.com/draft", window, cx)
        });
        ensure!(
            app.address.read(cx).value().as_ref() == "https://example.com/draft",
            "Inline edit did not select the existing URL after mounting"
        );
        app.cancel_edit(window, cx);
        ensure!(
            app.model.focused_pane().active_tab().url == "about:blank",
            "Space edit cancellation navigated"
        );
        Ok(())
    })??;
    checks.push("space inline input mounts with full URL selected; cancel preserves document");

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
    view.update_in(cx, |app, _, cx| {
        app.model.grid("about:blank");
        cx.notify();
    })?;
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
    view.update_in(cx, |app, window, cx| {
        app.open_menu(
            TabTarget::Space(app.model.active_space),
            point(px(160.), px(30.)),
            window,
            cx,
        )
    })?;
    pause(cx).await;
    ensure!(
        tabs.iter().all(|tab| !host.is_visible(*tab)),
        "Menu left a native surface on top"
    );
    view.update_in(cx, |app, window, cx| {
        app.open_menu(
            TabTarget::Space(app.model.active_space),
            point(px(160.), px(30.)),
            window,
            cx,
        )
    })?;
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
    // Exercise the same target-bound handlers used by both compact tab strips.
    view.update_in(cx, |app, window, cx| {
        let pane = app.model.focused_pane().id;
        let target = TabTarget::Page(pane, app.active_tab());
        let original = app.model.focused_pane().active_tab().url.clone();
        app.begin_edit(target, window, cx);
        app.address.update(cx, |input, cx| {
            input.set_value("https://example.com/unfinished", window, cx)
        });
        app.cancel_edit(window, cx);
        ensure!(
            app.model.focused_pane().active_tab().url == original,
            "Cancel navigated"
        );
        app.begin_edit(target, window, cx);
        app.address.update(cx, |input, cx| {
            input.set_value("https://example.com", window, cx)
        });
        app.address.update(cx, |_, cx| {
            cx.emit(gpui_component::input::InputEvent::PressEnter { secondary: false })
        });
        Ok(())
    })??;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        app.command(Command::Address, window, cx);
        app.address.update(cx, |input, cx| {
            input.set_value("https://example.com/escape", window, cx)
        });
    })?;
    pause(cx).await;
    cx.update(|window, cx| window.dispatch_action(Box::new(gpui_component::input::Escape), cx))?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.editing.is_none())?,
        "Escape action did not dismiss inline input"
    );
    checks.push("Escape action routes from focused inline input to shell");
    let edited = view.update_in(cx, |app, _, _| app.active_tab())?;
    wait_title(&host, edited, "Example Domain", cx).await?;
    ensure!(
        view.update_in(cx, |app, _, _| app.editing.is_none())?,
        "Enter left inline editor open"
    );
    checks.push("inline URL cancellation and Enter navigation on native page");

    view.update_in(cx, |app, window, cx| {
        let space = app.model.active_space;
        let pane = app.model.focused_pane().id;
        let target = TabTarget::Page(pane, app.active_tab());
        app.begin_edit(TabTarget::Space(space), window, cx);
        let other = app
            .model
            .active_space()
            .panes
            .iter()
            .find(|p| p.id != pane)
            .unwrap()
            .id;
        app.focus_pane(other, window, cx);
        ensure!(app.editing.is_none(), "Pane switch kept unfinished edit");
        let before = app.model.pane(pane).unwrap().tabs.len();
        app.tab_action(target, TabAction::NewSibling, window, cx);
        ensure!(
            app.model.focused_pane().id == pane
                && app.model.pane(pane).unwrap().tabs.len() == before + 1,
            "Sibling targeted wrong pane"
        );
        app.tab_action(
            TabTarget::Page(pane, app.active_tab()),
            TabAction::Close,
            window,
            cx,
        );
        app.tab_action(TabTarget::Space(space), TabAction::NewSibling, window, cx);
        let sibling = app.model.active_space;
        app.tab_action(
            TabTarget::Space(space),
            TabAction::Split(Axis::Vertical),
            window,
            cx,
        );
        ensure!(
            app.model.active_space == space,
            "Space split targeted current space instead of menu space"
        );
        ensure!(
            app.model
                .spaces
                .iter()
                .find(|s| s.id == sibling)
                .unwrap()
                .panes
                .len()
                == 1,
            "Space split changed sibling"
        );
        app.tab_action(TabTarget::Space(sibling), TabAction::Close, window, cx);
        app.tab_action(TabTarget::Space(space), TabAction::Close, window, cx);
        ensure!(app.model.spaces.len() == 1, "Final space removed");
        let pane = app.model.focused_pane().id;
        while app.model.pane(pane).unwrap().tabs.len() > 1 {
            app.model.close_tab(pane, app.active_tab());
        }
        let last = app.active_tab();
        app.tab_action(TabTarget::Page(pane, last), TabAction::Close, window, cx);
        ensure!(
            app.active_tab() != last && app.model.focused_pane().active_tab().url == "about:blank",
            "Final tab not replaced"
        );
        Ok(())
    })??;
    pause(cx).await;
    checks.push("targeted sibling, split and close actions; final tab and space guards; switching cancels edits");
    // Exercise the drag handlers against retained native documents, including cancellation.
    let (source, moved, destination, destination_tab) = view.update_in(cx, |app, window, cx| {
        app.model = workspace_core::Workspace::default();
        let source = app.model.focused_pane().id;
        let moved = app.active_tab();
        let destination = app.model.split_focused(Axis::Horizontal, "about:blank");
        let destination_tab = app.active_tab();
        app.sync_address(window, cx);
        cx.notify();
        (source, moved, destination, destination_tab)
    })?;
    pause(cx).await;
    wait_title(&host, moved, "New tab · Axolotl", cx).await?;
    wait_title(&host, destination_tab, "New tab · Axolotl", cx).await?;
    host.navigate(moved, "https://example.com/?dock=before")?;
    wait_title(&host, moved, "Example Domain", cx).await?;
    eval(&host, moved, "document.body.insertAdjacentHTML('beforeend', '<input>'); document.querySelector('input').value='docked state'; window.dockIdentity={}; history.pushState({}, '', '?dock=history'); true", cx).await?;
    host.back(moved)?;
    pause(cx).await;
    ensure!(
        host.current_url(moved)?.contains("dock=before"),
        "History fixture back failed before docking: {}",
        host.current_url(moved)?
    );
    host.forward(moved)?;
    pause(cx).await;
    ensure!(
        host.current_url(moved)?.contains("dock=history"),
        "History fixture forward failed before docking"
    );
    let width_before = eval(&host, destination_tab, "innerWidth", cx).await?;
    view.update_in(cx, |app, window, cx| {
        let origin = point(px(60.), px(crate::render::TOP + 16.));
        let destination_point = point(window.viewport_size().width / 2. + px(20.), origin.y);
        app.arm_tab_drag(source, moved, origin);
        app.update_tab_drag(
            &MouseMoveEvent {
                position: origin + point(px(2.), px(0.)),
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        ensure!(!app.tab_drag_active(), "Click crossed drag threshold");
        app.finish_tab_drag(Some(destination_point), window, cx);
        ensure!(app.model.pane(source).is_some(), "Click moved tab");
        app.arm_tab_drag(source, moved, origin);
        app.update_tab_drag(
            &MouseMoveEvent {
                position: destination_point,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        ensure!(app.tab_drag_active(), "Drag threshold did not start drag");
        ensure!(
            app.tab_drop_target(destination_point, window) == Some((destination, 0)),
            "Wrong insertion target"
        );
        app.finish_tab_drag(None, window, cx);
        ensure!(
            app.model.pane(source).is_some(),
            "Outside release moved tab"
        );
        app.arm_tab_drag(source, moved, origin);
        app.update_tab_drag(
            &MouseMoveEvent {
                position: destination_point,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        app.cancel_tab_drag(cx);
        ensure!(app.model.pane(source).is_some(), "Cancel moved tab");
        app.arm_tab_drag(source, moved, origin);
        app.update_tab_drag(
            &MouseMoveEvent {
                position: destination_point,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        app.finish_tab_drag(Some(point(px(30.), px(200.))), window, cx);
        ensure!(
            app.model.pane(source).is_some(),
            "Page-content release moved tab"
        );
        app.arm_tab_drag(source, moved, origin);
        app.update_tab_drag(
            &MouseMoveEvent {
                position: destination_point,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        app.finish_tab_drag(Some(destination_point), window, cx);
        ensure!(app.model.pane(source).is_none(), "Emptied source retained");
        ensure!(
            app.active_tab() == moved && app.model.focused_pane().id == destination,
            "Dock did not activate destination"
        );
        ensure!(
            app.model.focused_pane().tabs[0].id == moved,
            "Dock insertion wrong"
        );
        Ok(())
    })??;
    pause(cx).await;
    ensure!(
        eval(
            &host,
            moved,
            "document.querySelector('input').value === 'docked state' && !!window.dockIdentity",
            cx
        )
        .await?
            == true,
        "Dock recreated native document"
    );
    ensure!(
        host.current_url(moved)?.contains("dock=history"),
        "Dock lost URL"
    );
    host.back(moved)?;
    pause(cx).await;
    ensure!(
        !host.current_url(moved)?.contains("dock=history"),
        "Dock lost back history: {}",
        host.current_url(moved)?
    );
    host.forward(moved)?;
    pause(cx).await;
    ensure!(
        host.current_url(moved)?.contains("dock=history"),
        "Dock lost forward history"
    );
    ensure!(
        host.has_view(destination_tab),
        "Dock discarded destination view"
    );
    ensure!(
        eval(&host, moved, "innerWidth", cx).await?.as_f64() > width_before.as_f64(),
        "Collapsed layout did not expand page"
    );
    checks
        .push("tab drag threshold, outside/content drop and explicit cancellation preserve layout");
    checks.push("docking retains native DOM identity, form state, URL and back/forward history; activates insertion and expands emptied source");
    Ok(checks)
}
