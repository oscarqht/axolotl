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

pub(crate) type Host = Rc<dyn BrowserHost>;

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

pub(crate) async fn pause(cx: &mut AsyncWindowContext) {
    cx.background_executor()
        .timer(Duration::from_millis(200))
        .await;
    // Occluded windows may skip display-link callbacks during unattended tests.
    let _ = cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear();
    });
}

pub(crate) async fn eval(
    host: &Host,
    tab: TabId,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> Result<Value> {
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

pub(crate) async fn bar_eval(
    view: &WeakEntity<BrowserApp>,
    pane: workspace_core::PaneId,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> Result<Value> {
    let response = Arc::new(Mutex::new(None));
    let target = response.clone();
    view.update_in(cx, |app, _, _| {
        app.bars
            .get(&pane)
            .context("Missing native bar")?
            .evaluate(script, move |v| *target.lock().unwrap() = Some(v))
    })??;
    for _ in 0..50 {
        if let Some(value) = response.lock().unwrap().take() {
            return serde_json::from_str(&value)
                .with_context(|| format!("Bar evaluation returned {value:?} for {script}"));
        }
        pause(cx).await;
    }
    anyhow::bail!("Bar evaluation timed out: {script}")
}

async fn check_scrollbars(host: &Host, tab: TabId, cx: &mut AsyncWindowContext) -> Result<()> {
    let result = eval(host, tab, include_str!("../assets/smoke-scrollbars.js"), cx).await?;
    for check in [
        "rootHidden",
        "nestedHidden",
        "rootScrolls",
        "nestedScrolls",
        "overflowPreserved",
        "noNestedGutter",
    ] {
        ensure!(
            result[check] == true,
            "Scrollbar check {check} failed: {result}"
        );
    }
    Ok(())
}

async fn wait_preview(
    view: &WeakEntity<BrowserApp>,
    title: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    let mut captured = false;
    // Windows may initialize a lazy page plus both overlay WebView2 controllers.
    for _ in 0..150 {
        let response = Arc::new(Mutex::new(None));
        let target = response.clone();
        view.update_in(cx, |app, _, _| {
            app.drag_preview
                .as_ref()
                .context("Missing native drag card")?
                .evaluate(
                    &format!("Boolean(document.querySelector('img')?.naturalWidth > 0 && document.querySelector('header').textContent === {})", serde_json::to_string(title)?),
                    move |value| {
                        *target.lock().unwrap() = Some(value);
                    },
                )
        })??;
        pause(cx).await;
        if response.lock().unwrap().as_deref() == Some("true") {
            captured = true;
            break;
        }
    }
    ensure!(captured, "Native drag thumbnail never decoded");
    Ok(())
}

pub(crate) async fn wait_title(
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
    #[cfg(target_os = "macos")]
    if std::env::args().any(|arg| arg == "--smoke-input") {
        return crate::smoke_input::run(view, cx).await;
    }
    if std::env::args().any(|arg| arg == "--smoke-bar-sizing") {
        return crate::smoke_panels::check_sizing(view, cx).await;
    }

    view.update_in(cx, |_, window, cx| {
        use gpui_component::ActiveTheme;
        ensure!(
            cx.theme().mode == gpui_component::ThemeMode::from(window.appearance()),
            "Startup theme did not follow OS appearance"
        );
        Ok(())
    })??;
    #[cfg(target_os = "macos")]
    {
        for dark in [true, false, true] {
            host.set_test_appearance(first, Some(dark))?;
            pause(cx).await;
            view.update_in(cx, |_, _, cx| {
                use gpui_component::ActiveTheme;
                ensure!(
                    cx.theme().is_dark() == dark,
                    "Native appearance change did not update theme"
                );
                let colors = crate::chrome_theme::ChromeColors::current(cx);
                ensure!(
                    colors.shell == if dark { 0x11161e } else { 0xe9edf1 },
                    "Shell palette did not switch"
                );
                ensure!(
                    colors.divider == if dark { 0x11161e } else { 0xd7dfe7 },
                    "Split border did not switch"
                );
                Ok(())
            })??;
            if std::env::args().any(|arg| arg == "--smoke-hold-theme") {
                eprintln!(
                    "AXOLOTL_THEME_READY: {}",
                    if dark { "dark" } else { "light" }
                );
                cx.background_executor()
                    .timer(Duration::from_secs(12))
                    .await;
            }
        }
        host.set_test_appearance(first, None)?;
        pause(cx).await;
    }

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
    check_scrollbars(&host, first, cx).await?;
    let mut checks = vec![
        "OS theme at startup; native light/dark appearance changes update shell and split borders",
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
    check_scrollbars(&host, second, cx).await?;
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
    check_scrollbars(&host, second, cx).await?;
    // A marker ensures the reload created a new document before checking again.
    eval(&host, second, "window.axolotlBeforeReload = true", cx).await?;
    host.reload(second)?;
    for _ in 0..50 {
        pause(cx).await;
        if eval(&host, second, "!window.axolotlBeforeReload", cx).await? == true {
            break;
        }
    }
    wait_title(&host, second, "Example Domain", cx).await?;
    ensure!(
        eval(&host, second, "!window.axolotlBeforeReload", cx).await? == true,
        "Reload did not replace the document"
    );
    check_scrollbars(&host, second, cx).await?;
    checks.push("native scrollbars hidden in existing/new tabs and after navigation/reload; root and nested elements still scroll on both axes");
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
    ensure!(
        tabs.iter().all(|tab| host.is_visible(*tab)),
        "Floating bars hid pages"
    );
    ensure!(
        view.update_in(cx, |app, _, _| app.bars.len())? == 4,
        "Missing floating bars"
    );
    checks.push("native floating bars coexist with all four live page surfaces");
    let before_resize = eval(&host, tabs[0], "[innerWidth, innerHeight]", cx).await?;
    view.update_in(cx, |app, _, cx| {
        let geometry = app.model.active_space().layout.geometry(
            workspace_core::Rect::new(0., 0., 1200., 800.),
            6.,
            100.,
        );
        app.dragging = Some(geometry.dividers[0]);
        app.model.resize_split(geometry.dividers[0].id, 0.65);
        cx.notify();
    })?;
    pause(cx).await;
    ensure!(
        tabs.iter().all(|tab| host.is_visible(*tab)),
        "Resize hid native pages"
    );
    ensure!(
        eval(&host, tabs[0], "[innerWidth, innerHeight]", cx).await? != before_resize,
        "Native bounds did not update while resizing"
    );
    view.update_in(cx, |app, _, cx| {
        app.dragging = None;
        cx.notify();
    })?;
    checks.push("split ratio relayout keeps native pages visible during resize");
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
    let pane = view.update_in(cx, |app, _, _| app.model.focused_pane().id)?;
    let editor = bar_eval(view,pane,"({focused:document.activeElement?.tagName==='INPUT',value:document.querySelector('input')?.value,active:document.activeElement?.tagName,hasFocus:document.hasFocus(),bar:bar.className,expanded,hostExpanded:state.expanded,viewport:innerWidth,expected:state.surfaceWidth,visibility:getComputedStyle(tabs).visibility})",cx).await?;
    ensure!(
        editor["focused"] == true && editor["value"] == "about:blank",
        "New tab editor not focused: {editor}"
    );
    bar_eval(view,pane,"document.querySelector('input').value='example.com'; document.querySelector('input').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true})); true",cx).await?;
    pause(cx).await;
    wait_title(&host, new_tab, "Example Domain", cx).await?;
    view.update_in(cx, |app, _, _| {
        ensure!(
            app.active_tab() == new_tab,
            "Enter navigated a different tab"
        );
        ensure!(
            app.model.tab(new_tab).unwrap().url == "https://example.com/",
            "New tab URL was not normalized"
        );
        ensure!(app.editing.is_none(), "Enter left URL editing open");
        Ok(())
    })??;
    checks.push("new tab retains URL focus after native creation; typing replaces selection and Enter navigates");
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
        let others: Vec<_> = app
            .model
            .active_space()
            .panes
            .iter()
            .filter(|p| p.id != pane)
            .map(|p| p.id)
            .collect();
        for other in others {
            app.model.close_pane(other);
        }
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
    // Route CloseTab through the GPUI action used by Cmd+W, with actual native views.
    let (original, right, bottom, closed, extra) = view.update_in(cx, |app, window, cx| {
        app.model = workspace_core::Workspace::default();
        let original = app.model.focused_pane().id;
        let right = app.model.split_focused(Axis::Horizontal, "about:blank");
        let bottom = app.model.split_focused(Axis::Vertical, "about:blank");
        let closed = app.active_tab();
        let extra = app.model.add_tab("about:blank");
        app.sync_address(window, cx);
        app.focus.focus(window);
        cx.notify();
        (original, right, bottom, closed, extra)
    })?;
    pause(cx).await;
    wait_title(&host, extra, "New tab · Axolotl", cx).await?;
    view.update_in(cx, |_, window, cx| {
        window.dispatch_action(Box::new(crate::CloseTab), cx)
    })?;
    pause(cx).await;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.model.focused_pane().id == bottom && app.active_tab() == closed,
            "Close action did not select the remaining tab in its pane"
        );
        Ok(())
    })??;
    ensure!(!host.has_view(extra), "Closed extra native view retained");
    wait_title(&host, closed, "New tab · Axolotl", cx).await?;
    let right_tab = view.update_in(cx, |app, _, _| app.model.pane(right).unwrap().active_tab)?;
    let before = eval(&host, right_tab, "innerHeight", cx).await?;
    view.update_in(cx, |_, window, cx| {
        window.dispatch_action(Box::new(crate::CloseTab), cx)
    })?;
    pause(cx).await;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.model.pane(bottom).is_none(),
            "Empty nested pane retained"
        );
        ensure!(
            app.model.active_space().layout.pane_ids() == [original, right],
            "Nested split not collapsed"
        );
        ensure!(
            app.model.focused_pane().id == original,
            "Pane focus not transferred"
        );
        Ok(())
    })??;
    ensure!(!host.has_view(closed), "Removed pane native view retained");
    ensure!(
        eval(&host, right_tab, "innerHeight", cx).await?.as_f64() > before.as_f64(),
        "Surviving native pane did not expand"
    );
    // Close the first leaf as well, leaving the final pane for the blank guard.
    view.update_in(cx, |_, window, cx| {
        window.dispatch_action(Box::new(crate::CloseTab), cx)
    })?;
    pause(cx).await;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.model.active_space().layout == workspace_core::Layout::Pane(right),
            "Root split not collapsed"
        );
        ensure!(
            app.active_tab() == right_tab,
            "Focus did not reach final pane"
        );
        Ok(())
    })??;
    view.update_in(cx, |_, window, cx| {
        window.dispatch_action(Box::new(crate::CloseTab), cx)
    })?;
    pause(cx).await;
    let fresh = view.update_in(cx, |app, _, _| -> Result<TabId> {
        ensure!(
            app.model.focused_pane().id == right && app.active_tab() != right_tab,
            "Final pane was not retained with a fresh tab"
        );
        Ok(app.active_tab())
    })??;
    wait_title(&host, fresh, "New tab · Axolotl", cx).await?;
    ensure!(
        !host.has_view(right_tab),
        "Final closed native view retained"
    );
    checks.push("CloseTab action selects tabs, collapses nested/root splits, transfers focus, expands native panes and replaces final tab");
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
    pause(cx).await;
    view.update_in(cx, |app, _, _| {
        ensure!(
            app.model.tab(moved).unwrap().title == "Example Domain",
            "Drag source title is stale"
        );
        Ok(())
    })??;
    view.update_in(cx, |app, window, cx| {
        let origin = point(px(60.), px(crate::render::TOP + 16.));
        app.arm_tab_drag(source, moved, origin);
        app.update_tab_drag(
            &MouseMoveEvent {
                position: point(window.viewport_size().width / 2. + px(20.), origin.y),
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
    })?;
    pause(cx).await;
    ensure!(
        host.is_visible(moved) && host.is_visible(destination_tab),
        "Drag hid native pages"
    );
    wait_preview(view, "Example Domain", cx).await?;
    if std::env::args().any(|arg| arg == "--smoke-hold-preview") {
        eprintln!("AXOLOTL_PREVIEW_READY: holding native drag state for 20 seconds");
        cx.background_executor()
            .timer(Duration::from_secs(20))
            .await;
    }
    view.update_in(cx, |app, _, cx| {
        app.cancel_tab_drag(cx);
        ensure!(app.drag_preview.is_none(), "Cancel retained drag card");
        Ok(())
    })??;
    checks.push("drag keeps native pages visible and decodes a captured page thumbnail; cancellation disposes card");
    let lazy = view.update_in(cx, |app, window, cx| {
        app.model.focus_pane(source);
        let lazy = app.model.add_tab("about:blank");
        app.model.switch_tab(source, moved);
        app.model.focus_pane(destination);
        ensure!(
            !app.host.has_view(lazy),
            "Lazy fixture already has a native view"
        );
        let origin = point(px(260.), px(crate::render::TOP + 16.));
        app.arm_tab_drag(source, lazy, origin);
        app.update_tab_drag(
            &MouseMoveEvent {
                position: origin + point(px(20.), px(50.)),
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        Ok(lazy)
    })??;
    wait_preview(view, "New tab", cx).await?;
    ensure!(
        host.is_visible(moved) && host.is_visible(destination_tab) && !host.is_visible(lazy),
        "Inactive-tab capture changed page visibility"
    );
    view.update_in(cx, |app, _, cx| {
        app.cancel_tab_drag(cx);
        app.model.close_tab(source, lazy);
        cx.notify();
    })?;
    checks.push(
        "dragging a lazy inactive tab captures its native page without replacing the active page",
    );

    view.update_in(cx, |app, window, cx| {
        let origin = point(px(60.), px(crate::render::TOP + 16.));
        let bar = &app.bars[&destination];
        let destination_point = point(px(bar.bounds.x + 4.), px(bar.bounds.y + 20.));
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
            app.tab_drop_target(destination_point, window) == Some((destination, 1)),
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
        let center = point(
            window.viewport_size().width / 4.,
            px(crate::render::TOP + 34.)
                + (window.viewport_size().height - px(crate::render::TOP + 34.)) / 2.,
        );
        app.finish_tab_drag(Some(center), window, cx);
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
            app.model.focused_pane().tabs[1].id == moved,
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
    // Exercise actual native pages through the same split drag handlers on every side.
    for edge in [
        workspace_core::SplitEdge::Left,
        workspace_core::SplitEdge::Right,
        workspace_core::SplitEdge::Top,
        workspace_core::SplitEdge::Bottom,
    ] {
        let (source, destination, position) = view.update_in(cx, |app, window, cx| {
            let source = app.model.focused_pane().id;
            // First iteration splits within the multi-tab source; later iterations
            // move its sole tab to a nested target and collapse the empty source.
            let destination = if edge == workspace_core::SplitEdge::Left {
                source
            } else {
                destination
            };
            let viewport = window.viewport_size();
            let geometry = app.model.active_space().layout.geometry(
                workspace_core::Rect::new(
                    0.,
                    crate::render::TOP,
                    f32::from(viewport.width),
                    f32::from(viewport.height) - crate::render::TOP,
                ),
                6.,
                100.,
            );
            let r = geometry
                .panes
                .iter()
                .find(|p| p.id == destination)
                .unwrap()
                .bounds;
            let content = r;
            let (x, y) = match edge {
                workspace_core::SplitEdge::Left => (0.1, 0.5),
                workspace_core::SplitEdge::Right => (0.9, 0.5),
                workspace_core::SplitEdge::Top => (0.5, 0.1),
                workspace_core::SplitEdge::Bottom => (0.5, 0.75),
            };
            let position = point(
                px(content.x + content.width * x),
                px(content.y + content.height * y),
            );
            let origin = point(px(r.x + 60.), px(r.y + 16.));
            app.arm_tab_drag(source, moved, origin);
            app.update_tab_drag(
                &MouseMoveEvent {
                    position,
                    pressed_button: Some(MouseButton::Left),
                    ..Default::default()
                },
                window,
                cx,
            );
            ensure!(
                app.split_drop_target(position, window)
                    .map(|(p, e, _)| (p, e))
                    == Some((destination, edge)),
                "Wrong edge target"
            );
            ensure!(
                app.split_preview.is_some(),
                "Missing native split placeholder"
            );
            Ok((source, destination, position))
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
            "Preview changed native document"
        );
        if std::env::args().any(|arg| arg == "--smoke-hold-split") {
            eprintln!("AXOLOTL_SPLIT_READY: {edge:?}");
            for _ in 0..50 {
                pause(cx).await;
            }
        }
        let before = view.update_in(cx, |app, _, _| format!("{:?}", app.model))?;
        cx.update(|window, cx| {
            window.dispatch_action(Box::new(gpui_component::input::Escape), cx)
        })?;
        pause(cx).await;
        view.update_in(cx, |app, window, cx| {
            ensure!(app.tab_drag.is_none(), "Escape did not abort drag");
            // A mouse-up after Escape must be inert.
            app.finish_tab_drag(Some(position), window, cx);
            ensure!(
                format!("{:?}", app.model) == before,
                "Cancel changed layout: before={before}; after={:?}",
                app.model
            );
            ensure!(app.split_preview.is_none(), "Cancel retained placeholder");
            app.arm_tab_drag(source, moved, position - point(px(10.), px(10.)));
            app.update_tab_drag(
                &MouseMoveEvent {
                    position,
                    pressed_button: Some(MouseButton::Left),
                    ..Default::default()
                },
                window,
                cx,
            );
            app.finish_tab_drag(Some(position), window, cx);
            ensure!(
                app.active_tab() == moved && app.model.focused_pane().id != destination,
                "Split did not focus moved tab"
            );
            if source != destination {
                ensure!(
                    app.model.pane(source).is_none(),
                    "Split retained empty source"
                );
            }
            ensure!(app.split_preview.is_none(), "Drop retained placeholder");
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
            "Split recreated document"
        );
        host.back(moved)?;
        pause(cx).await;
        ensure!(
            !host.current_url(moved)?.contains("dock=history"),
            "Split lost back history"
        );
        host.forward(moved)?;
        pause(cx).await;
        ensure!(
            host.current_url(moved)?.contains("dock=history"),
            "Split lost forward history"
        );
    }
    checks.push("all four native edge splits, same-pane and nested cross-pane moves, empty-source collapse, cancellation and retained DOM/form/history");

    checks.extend(crate::smoke_panels::run(view, &host, moved, destination, cx).await?);
    Ok(checks)
}
