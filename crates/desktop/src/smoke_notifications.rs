//! Real native page API -> host IPC -> unread state -> floating badge checks.
use crate::{
    app::{BrowserApp, Command, TabTarget},
    smoke::{bar_eval, eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use workspace_core::Axis;

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, first, pane) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.active_tab(),
            app.model.focused_pane().id,
        )
    })?;
    let space = view.update_in(cx, |app, _, _| app.model.active_space)?;
    let foreground = view.update_in(cx, |_, window, _| window.is_window_active())?;
    eval(&host, first, "new Notification('viewed'); true", cx).await?;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        if foreground {
            ensure!(
                app.notifications.is_empty(),
                "Viewed notification accumulated"
            );
        } else {
            ensure!(
                app.notifications.get(&first) == Some(&1),
                "Inactive window did not accumulate notification"
            );
        }
        app.select_target(TabTarget::Page(pane, first), window, cx);
        Ok(())
    })??;
    let second = view.update_in(cx, |app, window, cx| {
        app.command(Command::NewTab, window, cx);
        app.cancel_edit(window, cx);
        app.active_tab()
    })?;
    pause(cx).await;
    wait_title(&host, second, "New tab · Axolotl", cx).await?;
    let third = view.update_in(cx, |app, window, cx| {
        app.command(Command::NewTab, window, cx);
        app.cancel_edit(window, cx);
        app.active_tab()
    })?;
    pause(cx).await;
    wait_title(&host, third, "New tab · Axolotl", cx).await?;
    eval(&host, second, "new Notification('another tab'); true", cx).await?;
    // Native evaluate executes inside a retained, hidden WKWebView/WebView2.
    eval(
        &host,
        first,
        "new Notification('one'); new Notification('two'); true",
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.notifications.get(&first).copied())? == Some(2),
        "Hidden notification IPC lost"
    );
    ensure!(
        view.update_in(cx, |app, _, _| app.space_notifications(space))? == 3,
        "Space total did not sum unread tabs"
    );
    let compact = bar_eval(view, pane, "(()=>{const b=document.querySelector('#count .notification-badge');const c=getComputedStyle(b);return {count:b.textContent,red:c.backgroundColor,white:c.color,round:c.borderRadius,visible:b.getBoundingClientRect().width>0,indicator:document.querySelector('#count').textContent}})()", cx).await?;
    ensure!(
        compact["count"] == "3"
            && compact["red"] == "rgb(215, 25, 50)"
            && compact["white"] == "rgb(255, 255, 255)"
            && compact["round"] == "999px"
            && compact["visible"] == true
            && compact["indicator"]
                .as_str()
                .is_some_and(|s| s.starts_with("3/3")),
        "Compact badge styling/total failed: {compact}"
    );
    bar_eval(view, pane, "expand(true); true", cx).await?;
    pause(cx).await;
    pause(cx).await;
    let expanded = bar_eval(view, pane, &format!("(()=>{{const b=document.querySelector('[data-id=\"{first}\"] .notification-badge');return {{count:b?.textContent,visible:b?.getBoundingClientRect().width>0,expanded:document.querySelector('#bar').classList.contains('expanded')}}}})()"), cx).await?;
    ensure!(
        expanded["count"] == "2" && expanded["visible"] == true && expanded["expanded"] == true,
        "Expanded chip badge missing: {expanded}"
    );
    // Select through the actual floating-bar click/IPC path.
    bar_eval(
        view,
        pane,
        &format!("document.querySelector('[data-id=\"{first}\"]').click(); true"),
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.active_tab() == first
            && !app.notifications.contains_key(&first))?,
        "Selection did not clear unread count"
    );
    ensure!(
        bar_eval(
            view,
            pane,
            &format!(
                "document.querySelector('[data-id=\"{first}\"] .notification-badge') === null"
            ),
            cx
        )
        .await?
            == true,
        "Zero badges must disappear"
    );
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(pane, second), window, cx)
    })?;
    pause(cx).await;
    eval(&host, first, "new Notification('move me'); true", cx).await?;
    pause(cx).await;
    let destination = view.update_in(cx, |app, window, cx| {
        let dest = app.model.split_focused(Axis::Horizontal, "about:blank");
        app.model.move_tab(pane, first, dest, 0);
        // Keep viewing the source panel so the moved tab remains unread.
        app.select_target(TabTarget::Page(pane, second), window, cx);
        dest
    })?;
    pause(cx).await;
    pause(cx).await;
    ensure!(
        bar_eval(
            view,
            pane,
            "document.querySelector('#count .notification-badge') === null",
            cx
        )
        .await?
            == true,
        "Source total retained moved count"
    );
    ensure!(
        bar_eval(
            view,
            destination,
            "document.querySelector('#count .notification-badge')?.textContent",
            cx
        )
        .await?
            == "1",
        "Destination total lost moved count"
    );
    ensure!(
        view.update_in(cx, |app, _, _| app.space_notifications(space))? == 1,
        "Space total lost notification when moving between panels"
    );
    // OS foreground focus cannot be delivered while macOS loginwindow owns
    // the session. Keep that check explicit rather than simulating an OS event.
    if foreground {
        host.focus(first)?;
        pause(cx).await;
        ensure!(
            view.update_in(cx, |app, _, _| app.active_tab() == first
                && !app.notifications.contains_key(&first))?,
            "Native page focus did not clear count"
        );
    } else {
        view.update_in(cx, |app, window, cx| {
            app.focus_pane(destination, window, cx)
        })?;
        ensure!(
            view.update_in(cx, |app, _, _| !app.notifications.contains_key(&first))?,
            "Panel focus did not clear count"
        );
    }
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(pane, second), window, cx)
    })?;
    pause(cx).await;
    eval(&host, first, "new Notification('close me'); true", cx).await?;
    pause(cx).await;
    view.update_in(cx, |app, _, cx| {
        app.model.close_tab(destination, first);
        cx.notify();
    })?;
    pause(cx).await;
    // Closing a native page rebuilds floating surfaces above the retained pages.
    // Allow the new control document to load and receive its ready-state update.
    pause(cx).await;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| !app.notifications.contains_key(&first))?,
        "Closed tab retained unread state"
    );
    ensure!(
        bar_eval(
            view,
            destination,
            "document.querySelectorAll('.notification-badge').length",
            cx
        )
        .await?
            == 0,
        "Closed tab retained panel total"
    );
    ensure!(
        view.update_in(cx, |app, _, _| app.space_notifications(space))? == 0,
        "Space total retained closed/read tab counts"
    );
    view.update_in(cx, |app, window, cx| {
        app.command(Command::NewSpace, window, cx)
    })?;
    pause(cx).await;
    eval(&host, second, "new Notification('hidden space'); true", cx).await?;
    pause(cx).await;
    view.update_in(cx, |app, _, _| {
        ensure!(
            app.space_notifications(space) == 1,
            "Hidden space notification missing"
        );
        ensure!(
            app.space_notifications(app.model.active_space) == 0,
            "Notification leaked into another space"
        );
        Ok(())
    })??;
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Space(space), window, cx)
    })?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.space_notifications(space))? == 0,
        "Space selection did not clear its selected tab notification"
    );
    same_origin_selection(view, cx).await?;
    Ok(vec![
        "same-origin selection clears sibling badges even without a source badge; unrelated origin and panel/space totals retained",
        "space badges sum all panels; totals follow moves, reads and closes; hidden spaces accumulate independently",
        "page Notification API: hidden counted; native red/white rounded badges in compact and expanded bars; selection clears; moved/closed tabs update panel totals",
        if foreground {
            "foreground notifications ignored; native page focus clears unread"
        } else {
            "inactive window notifications counted; panel focus clears unread; SKIPPED foreground suppression/native OS focus (window inactive)"
        },
    ])
}

/// Serve real same-origin pages without depending on external network access.
fn origin_fixture() -> Result<String> {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", listener.local_addr()?);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let body = "<!doctype html><title>Origin badge fixture</title><body style='font:24px system-ui;padding:48px;background:#eef5f5'><h1>Same-origin notifications</h1><p>Selecting this tab clears its sibling badges.</p><p>The unrelated tab keeps its badge.</p>";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Ok(origin)
}

async fn evidence_snapshot(
    view: &WeakEntity<BrowserApp>,
    pane: workspace_core::PaneId,
    anchor: workspace_core::TabId,
    cx: &mut AsyncWindowContext,
    stage: &str,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    if let Some(prefix) = std::env::args().find_map(|arg| {
        arg.strip_prefix("--smoke-notifications-snapshot=")
            .map(str::to_owned)
    }) {
        // Let the expansion animation finish before framing the two badge chips.
        for _ in 0..5 {
            pause(cx).await;
        }
        bar_eval(view, pane, &format!("document.querySelector('[data-id=\"{anchor}\"]').scrollIntoView({{block:'nearest',inline:'start'}}); true"), cx).await?;
        pause(cx).await;
        pause(cx).await;
        let path = format!("{prefix}-{stage}.png");
        let previous = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        view.update_in(cx, |app, _, _| {
            app.bars[&pane].native_snapshot(path.clone())
        })?;
        for _ in 0..25 {
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0 && m.modified().ok() != previous)
            {
                return Ok(());
            }
            pause(cx).await;
        }
        anyhow::bail!("Notification snapshot missing: {path}");
    }
    let _ = (view, pane, anchor, cx, stage);
    Ok(())
}

async fn same_origin_selection(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    let origin = origin_fixture()?;
    let other = origin_fixture()?;
    let mut tabs = Vec::new();
    for (url, title) in [
        (format!("{origin}/source"), "Origin source"),
        (format!("{origin}/sibling"), "Same origin"),
        (format!("{other}/unrelated"), "Unrelated origin"),
        ("about:blank".into(), "Neutral tab"),
    ] {
        let id = view.update_in(cx, |app, window, cx| {
            app.command(Command::NewTab, window, cx);
            app.navigate(&url, window, cx);
            let id = app.active_tab();
            app.model.tab_mut(id).unwrap().custom_title = Some(title.into());
            id
        })?;
        pause(cx).await;
        let host = view.update_in(cx, |app, _, _| app.host.clone())?;
        wait_title(
            &host,
            id,
            if url == "about:blank" {
                "New tab · Axolotl"
            } else {
                "Origin badge fixture"
            },
            cx,
        )
        .await?;
        tabs.push(id);
    }
    let (source, sibling, unrelated) = (tabs[0], tabs[1], tabs[2]);
    let (host, pane, space) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.model.focused_pane().id,
            app.model.active_space,
        )
    })?;
    for tab in [sibling, unrelated] {
        eval(&host, tab, "new Notification('unread'); true", cx).await?;
    }
    pause(cx).await;
    bar_eval(view, pane, "expand(true); true", cx).await?;
    pause(cx).await;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            !app.notifications.contains_key(&source),
            "Source should have no badge"
        );
        ensure!(
            app.notifications.get(&sibling) == Some(&1)
                && app.notifications.get(&unrelated) == Some(&1),
            "Native fixture badges missing"
        );
        Ok(())
    })??;
    evidence_snapshot(view, pane, sibling, cx, "before").await?;
    bar_eval(
        view,
        pane,
        &format!("document.querySelector('[data-id=\"{source}\"]').click(); true"),
        cx,
    )
    .await?;
    pause(cx).await;
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.active_tab() == source && !app.notifications.contains_key(&sibling),
            "Same-origin sibling badge survived selection"
        );
        ensure!(
            app.notifications.get(&unrelated) == Some(&1),
            "Unrelated badge was cleared"
        );
        ensure!(
            app.space_notifications(space) == 1,
            "Space total did not update"
        );
        Ok(())
    })??;
    let badges = bar_eval(view, pane, &format!("({{sibling:document.querySelector('[data-id=\"{sibling}\"] .notification-badge')===null,unrelated:document.querySelector('[data-id=\"{unrelated}\"] .notification-badge')?.textContent,total:document.querySelector('#count .notification-badge')?.textContent}})"), cx).await?;
    ensure!(
        badges["sibling"] == true && badges["unrelated"] == "1" && badges["total"] == "1",
        "Native badge totals stale: {badges}"
    );
    evidence_snapshot(view, pane, sibling, cx, "after").await?;
    Ok(())
}
