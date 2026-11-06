//! Native lifecycle regression: uses the shipped menus and real engine disposal.
use crate::{
    app::{BrowserApp, TabTarget},
    smoke::{bar_eval, eval, pause},
};
use anyhow::{Result, ensure};
use gpui::*;
use std::collections::HashSet;
use workspace_core::Axis;

async fn settled(
    view: &WeakEntity<BrowserApp>,
    id: u64,
    sleeping: bool,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    for _ in 0..35 {
        pause(cx).await;
        if view.update_in(cx, |app, _, _| {
            app.sleep.is_sleeping(id) == sleeping
                && app.host.has_view(id) != sleeping
                && (sleeping || app.host.is_loaded(id))
        })? {
            return Ok(());
        }
    }
    anyhow::bail!("Tab {id} did not settle sleeping={sleeping}")
}

pub(crate) async fn session(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let restore = std::env::args().any(|a| a == "--sleep-restore");
    let (host, id) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    if restore {
        view.update_in(cx, |app, _, _| {
            ensure!(
                app.sleep.is_sleeping(id) && app.sleep.keep_alive(id) && !host.has_view(id),
                "Sleep state was not restored without a native view"
            );
            ensure!(
                app.sleep.image(id).is_some(),
                "Cached native snapshot missing after restart"
            );
            ensure!(
                !app.preferences.auto_sleep && app.preferences.sleep_minutes == 7,
                "Settings did not survive restart"
            );
            Ok(())
        })??;
        view.update_in(cx, |app, window, cx| {
            app.select_target(TabTarget::Page(app.model.focused_pane().id, id), window, cx)
        })?;
        settled(view, id, false, cx).await?;
        view.update_in(cx, |app, _, _| {
            ensure!(
                app.sleep.keep_alive(id) && app.sleep.image(id).is_none(),
                "Wake did not preserve Keep Alive/clear cache"
            );
            Ok(())
        })??;
    } else {
        // Protection compilation can defer the first native view beyond two seconds.
        crate::smoke::wait_title(&host, id, "New tab · Axolotl", cx).await?;
        view.update_in(cx, |app, window, cx| {
            app.sleep.toggle_keep_alive(id);
            app.preferences.set_sleep(false, 7).unwrap();
            app.sleep_tab(id, window, cx);
        })?;
        settled(view, id, true, cx).await?;
        view.update_in(cx, |app, _, _| {
            ensure!(app.sleep.image(id).is_some(), "Native snapshot failed");
            Ok(())
        })??;
    }
    Ok(vec![
        "native sleep snapshot and state across process restart",
    ])
}

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
    // Invoke actual context menu actions; opening it must not select/wake the target.
    bar_eval(
        view,
        pane,
        &format!("menu={{tab:{first},x:30}};send({{type:'menu-open',tab:{first}}});true"),
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        bar_eval(
            view,
            pane,
            "[...document.querySelectorAll('#menu button')].some(b=>b.textContent==='Sleep Tab')",
            cx
        )
        .await?
            == true,
        "Sleep menu missing"
    );
    bar_eval(view,pane,"[...document.querySelectorAll('#menu button')].find(b=>b.textContent==='Keep Alive').click();true",cx).await?;
    pause(cx).await;
    view.update_in(cx, |app, _, _| {
        ensure!(app.sleep.keep_alive(first), "Keep Alive IPC failed");
        Ok(())
    })??;
    bar_eval(
        view,
        pane,
        &format!("menu={{tab:{first},x:30}};send({{type:'menu-open',tab:{first}}});true"),
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        bar_eval(
            view,
            pane,
            "document.querySelector('[role=menuitemcheckbox]').getAttribute('aria-checked')",
            cx
        )
        .await?
            == "true",
        "Keep Alive check missing"
    );
    bar_eval(view,pane,"[...document.querySelectorAll('#menu button')].find(b=>b.textContent==='Sleep Tab').click();true",cx).await?;
    settled(view, first, true, cx).await?;
    for _ in 0..5 {
        pause(cx).await;
    }
    ensure!(!host.has_view(first), "Visible sleeper recreated itself");
    if std::env::args().any(|a| a == "--sleep-inspect") {
        println!("AXOLOTL_SLEEP_INSPECT ready");
        for _ in 0..300 {
            pause(cx).await;
            if host.has_view(first) {
                return Ok(vec!["OS click on sleeping pane recreated native page"]);
            }
        }
        anyhow::bail!("Sleeping pane was not clicked during inspection");
    }

    view.update_in(cx, |app, window, cx| {
        ensure!(
            app.sleep.image(first).is_some(),
            "Native screenshot not captured"
        );
        app.hover_sleep(Some(first), point(px(100.), px(100.)), window, cx);
        Ok(())
    })??;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, _| app.sleep.check_hover())??;
    ensure!(bar_eval(view,pane,"({text:document.querySelector('.label').textContent,opacity:document.querySelector('.label').style.opacity,icon:document.querySelector('.sleep-icon svg')?.dataset.heroicon})",cx).await? == serde_json::json!({"text":"New tab · Axolotl","opacity":"0.5","icon":"moon"}), "Sleeping title style missing");
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(pane, first), window, cx)
    })?;
    settled(view, first, false, cx).await?;
    ensure!(
        eval(&host, first, "document.title", cx).await? == "New tab · Axolotl",
        "Wake failed to reload URL"
    );
    let (hidden, split) = view.update_in(cx, |app, _, cx| {
        let hidden = app.model.add_tab("about:blank");
        app.model.switch_tab(pane, first);
        app.model.split_focused(Axis::Horizontal, "about:blank");
        let split = app.active_tab();
        app.sleep.toggle_keep_alive(first); // allow later auto sleep
        cx.notify();
        (hidden, split)
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    view.update_in(cx, |app, window, cx| {
        // Explicitly selecting then hiding creates a real native hidden view.
        app.select_target(TabTarget::Page(pane, hidden), window, cx);
    })?;
    for _ in 0..4 {
        pause(cx).await;
    }
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(pane, first), window, cx);
        app.sleep.age(hidden);
        app.sleep.age(first);
        app.sleep.age(split);
        app.preferences.set_sleep(true, 1).unwrap();
    })?;
    // Activity exemption, including a live media track and dirty editor probe.
    eval(&host, hidden, "window.__axolotlSleepBusy=()=>true;true", cx).await?;
    for _ in 0..5 {
        pause(cx).await;
    }
    ensure!(host.has_view(hidden), "Busy hidden tab slept");
    eval(
        &host,
        hidden,
        "window.__axolotlSleepBusy=()=>false;true",
        cx,
    )
    .await?;
    view.update_in(cx, |app, _, _| app.sleep.retry(hidden))?;
    settled(view, hidden, true, cx).await?;
    ensure!(
        host.has_view(first) && host.has_view(split),
        "Visible split slept automatically"
    );
    view.update_in(cx, |app, window, cx| {
        app.sleep.toggle_keep_alive(first);
        app.open_space_menu(app.model.active_space, point(px(100.), px(38.)), window, cx);
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, _| {
        app.space_menu.as_ref().unwrap().evaluate(
            "document.querySelector('#sleep-space').click();true",
            |_| {},
        )
    })??;
    settled(view, split, true, cx).await?;
    ensure!(
        host.has_view(first) && !host.has_view(hidden),
        "Sleep Space ignored Keep Alive or woke hidden tab"
    );
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Space(app.model.active_space), window, cx)
    })?;
    settled(view, split, false, cx).await?;
    let space = view.update_in(cx, |app, _, cx| {
        let space = app.model.active_space;
        app.model.add_space("Other", "about:blank");
        cx.notify();
        space
    })?;
    for _ in 0..4 {
        pause(cx).await;
    }
    view.update_in(cx, |app, window, cx| {
        app.sleep_tab(first, window, cx);
        app.sleep_tab(split, window, cx);
    })?;
    settled(view, first, true, cx).await?;
    settled(view, split, true, cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Space(space), window, cx)
    })?;
    settled(view, first, false, cx).await?;
    settled(view, split, false, cx).await?;
    ensure!(!host.has_view(hidden), "Returning space woke hidden tab");
    view.update_in(cx, |app, _, _| {
        app.sleep.retain(&HashSet::new());
    })?;
    Ok(vec![
        "native menu sleep and Keep Alive",
        "native snapshot and hover preview without waking",
        "50% sleeping title opacity",
        "unloaded visible page stays asleep",
        "wake reloads saved URL",
        "hidden auto-sleep and activity exemption",
        "visible split panes stay awake",
        "returning space wakes only visible tabs",
    ])
}
