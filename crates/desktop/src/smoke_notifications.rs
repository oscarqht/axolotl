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
    Ok(vec![
        "page Notification API: hidden counted; native red/white rounded badges in compact and expanded bars; selection clears; moved/closed tabs update panel totals",
        if foreground {
            "foreground notifications ignored; native page focus clears unread"
        } else {
            "inactive window notifications counted; panel focus clears unread; SKIPPED foreground suppression/native OS focus (window inactive)"
        },
    ])
}
