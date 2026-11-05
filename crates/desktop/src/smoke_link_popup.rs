//! Real WKWebView link clicks against a loopback fixture; no network dependency.
use crate::{
    app::{BrowserApp, Command, TabTarget},
    link_popup::POPUP_TAB,
    smoke::{Host, eval, pause, wait_title},
};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
use serde_json::Value;

#[path = "smoke_link_popup_support.rs"]
mod support;
use support::{fixture, popup_eval};

async fn wait_popup(
    view: &WeakEntity<BrowserApp>,
    host: &Host,
    title: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    for _ in 0..50 {
        pause(cx).await;
        if view.update_in(cx, |app, _, _| app.link_popup.is_some())?
            && host.has_view(POPUP_TAB)
            && host.is_loaded(POPUP_TAB)
        {
            wait_title(host, POPUP_TAB, title, cx).await?;
            return Ok(());
        }
    }
    anyhow::bail!("Popup did not open for {title}")
}

async fn check_center(
    view: &WeakEntity<BrowserApp>,
    host: &Host,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    let geometry = popup_eval(view, r#"(()=>{const b=document.querySelector('#panel').getBoundingClientRect();return {x:b.x,y:b.y,w:b.width,h:b.height,vw:innerWidth,vh:innerHeight}})()"#, cx).await?;
    let n = |name| geometry[name].as_f64().unwrap();
    ensure!(
        (n("x") * 2. + n("w") - n("vw")).abs() < 1.,
        "Popup not centered horizontally: {geometry}"
    );
    ensure!(
        (n("y") * 2. + n("h") - n("vh")).abs() < 1.,
        "Popup not centered vertically: {geometry}"
    );
    let size = eval(host, POPUP_TAB, "[innerWidth,innerHeight]", cx).await?;
    ensure!(
        (size[0].as_f64().unwrap() - n("w")).abs() < 1.,
        "Native popup width diverged from chrome: {size}/{geometry}"
    );
    ensure!(
        (size[1].as_f64().unwrap() + 44. - n("h")).abs() < 1.,
        "Native popup height diverged from chrome: {size}/{geometry}"
    );
    Ok(())
}

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let base = fixture()?;
    let session_cookie = format!("axolotl_popup_session={}", uuid::Uuid::new_v4().simple());
    let (host, source, source_pane, original_size) = view.update_in(cx, |app, window, cx| {
        let host = app.host.clone();
        let source = app.active_tab();
        let pane = app.model.focused_pane().id;
        app.command(Command::Split(workspace_core::Axis::Horizontal), window, cx);
        (host, source, pane, window.viewport_size())
    })?;
    pause(cx).await;
    host.navigate(source, &format!("{base}/source"))?;
    wait_title(&host, source, "Popup source", cx)
        .await
        .with_context(|| {
            format!(
                "source initial navigation URL={:?}",
                host.current_url(source)
            )
        })?;
    support::check_protection_bridge(view, &host, source, cx).await?;
    eval(
        &host,
        source,
        &format!("document.cookie={};document.querySelector('input').value='Source survives popup'; true", serde_json::to_string(&format!("{session_cookie};path=/"))?),
        cx,
    )
    .await?;
    let before = view.update_in(cx, |app, _, _| app.model.to_json())?;
    host.focus(source)?;
    host.test_option_click(source)?;
    wait_popup(view, &host, "Popup A", cx).await?;
    view.update_in(cx, |app, _, _| -> Result<()> {
        let popup = app.link_popup.as_ref().unwrap();
        ensure!(
            popup.source == source && popup.pane == source_pane,
            "Wrong popup origin"
        );
        ensure!(
            app.model.tab(POPUP_TAB).is_none(),
            "Temporary page entered workspace"
        );
        ensure!(
            !app.model.to_json().contains(&POPUP_TAB.to_string()),
            "Popup persisted"
        );
        // Native focus may legitimately change the focused pane, but not tabs.
        let old: Value = serde_json::from_str(&before)?;
        let new: Value = serde_json::from_str(&app.model.to_json())?;
        ensure!(
            old["next_id"] == new["next_id"],
            "Option-click allocated a saved tab"
        );
        Ok(())
    })??;
    ensure!(
        eval(&host, source, "document.querySelector('input').value", cx).await?
            == "Source survives popup",
        "Source page changed"
    );
    ensure!(
        eval(
            &host,
            POPUP_TAB,
            &format!(
                "document.cookie.split('; ').includes({})",
                serde_json::to_string(&session_cookie)?
            ),
            cx
        )
        .await?
            == true,
        "Popup browsing session did not share source cookie"
    );
    check_center(view, &host, cx).await?;
    view.update_in(cx, |_, window, _| window.resize(size(px(1000.), px(700.))))?;
    for _ in 0..4 {
        pause(cx).await;
    }
    check_center(view, &host, cx).await?;
    view.update_in(cx, |_, window, _| window.resize(original_size))?;
    for _ in 0..4 {
        pause(cx).await;
    }
    check_center(view, &host, cx).await?;

    host.test_click(POPUP_TAB, false)?;
    wait_title(&host, POPUP_TAB, "Popup B", cx)
        .await
        .with_context(|| {
            format!(
                "ordinary popup navigation, URL={:?}",
                host.current_url(POPUP_TAB)
            )
        })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.link_popup.as_ref().unwrap().url == format!("{base}/b"),
            "Popup URL failed to follow navigation"
        );
        ensure!(
            app.model.pane(source_pane).unwrap().tabs.len() == 1,
            "Popup navigation created regular tab"
        );
        Ok(())
    })??;
    eval(
        &host,
        POPUP_TAB,
        "document.querySelector('a').target='_blank';true",
        cx,
    )
    .await?;
    host.test_option_click(POPUP_TAB)?;
    wait_title(&host, POPUP_TAB, "Popup C", cx).await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.link_popup.as_ref().unwrap().pane == source_pane,
            "Replacement lost origin"
        );
        ensure!(
            app.model.pane(source_pane).unwrap().tabs.len() == 1,
            "Nested Option-click created regular tab"
        );
        Ok(())
    })??;
    eval(&host, POPUP_TAB, "window.promotionIdentity=window.documentIdentity;document.querySelector('input').value='Promoted live page';true", cx).await?;
    let button = popup_eval(view, "(()=>{const b=document.querySelector('#promote').getBoundingClientRect();return [b.x+b.width/2,b.y+b.height/2]})()", cx).await?;
    view.update_in(cx, |app, _, _| {
        app.link_popup
            .as_ref()
            .unwrap()
            .test_click(button[0].as_f64().unwrap(), button[1].as_f64().unwrap())
    })??;
    for _ in 0..5 {
        pause(cx).await;
    }
    let promoted = view.update_in(cx, |app, _, _| -> Result<_> {
        ensure!(
            app.link_popup.is_none(),
            "Promotion retained popup after native header button click at {button}"
        );
        ensure!(
            app.model.focused_pane().id == source_pane,
            "Promotion targeted wrong pane"
        );
        ensure!(
            app.model.pane(source_pane).unwrap().tabs.len() == 2,
            "Promotion failed to add tab"
        );
        Ok(app.active_tab())
    })??;
    ensure!(
        !host.has_view(POPUP_TAB) && host.has_view(promoted),
        "Promotion failed to transfer native page"
    );
    ensure!(eval(&host, promoted, "window.promotionIdentity===window.documentIdentity&&document.querySelector('input').value==='Promoted live page'", cx).await? == true, "Promotion reloaded document or lost form");
    eval(
        &host,
        promoted,
        "document.title='Promoted callback';true",
        cx,
    )
    .await?;
    for _ in 0..5 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.model.tab(promoted).unwrap().title == "Promoted callback",
            "Promoted callbacks still use temporary ID"
        );
        Ok(())
    })??;
    host.back(promoted)?;
    wait_title(&host, promoted, "Popup B", cx)
        .await
        .with_context(|| {
            format!(
                "promoted history back, URL={:?}",
                host.current_url(promoted)
            )
        })?;
    host.forward(promoted)?;
    wait_title(&host, promoted, "Promoted callback", cx)
        .await
        .context("promoted history forward")?;

    // target=_blank Option-click must suppress Wry's ordinary new-window path.
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(source_pane, source), window, cx)
    })?;
    pause(cx).await;
    eval(
        &host,
        source,
        "document.querySelector('a').target='_blank';true",
        cx,
    )
    .await?;
    host.focus(source)?;
    host.test_option_click(source)?;
    wait_popup(view, &host, "Popup A", cx).await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |app, window, cx| -> Result<()> {
        ensure!(
            app.model.pane(source_pane).unwrap().tabs.len() == 2,
            "Option-click _blank duplicated regular tab"
        );
        let _ = (window, cx);
        app.link_popup.as_ref().unwrap().test_click(5., 80.)?;
        Ok(())
    })??;
    pause(cx).await;
    ensure!(
        eval(&host, source, "document.querySelector('input').value", cx).await?
            == "Source survives popup",
        "Dismissal changed source"
    );

    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.link_popup.is_none() && !app.host.has_view(POPUP_TAB),
            "Outside click did not dispose popup"
        );
        Ok(())
    })??;
    for dismissal in ["escape", "button"] {
        host.focus(source)?;
        host.test_option_click(source)?;
        wait_popup(view, &host, "Popup A", cx).await?;
        if dismissal == "escape" {
            host.focus(POPUP_TAB)?;
            view.update_in(cx, |app, _, _| {
                app.link_popup.as_ref().unwrap().test_key(
                    "\u{1b}",
                    objc2_app_kit::NSEventModifierFlags::empty(),
                    53,
                )
            })??;
        } else {
            let button = popup_eval(view, "(()=>{const b=document.querySelector('#close').getBoundingClientRect();return [b.x+b.width/2,b.y+b.height/2]})()", cx).await?;
            view.update_in(cx, |app, _, _| {
                app.link_popup
                    .as_ref()
                    .unwrap()
                    .test_click(button[0].as_f64().unwrap(), button[1].as_f64().unwrap())
            })??;
        }
        for _ in 0..5 {
            pause(cx).await;
        }
        view.update_in(cx, |app, _, _| -> Result<()> {
            ensure!(
                app.link_popup.is_none() && !app.host.has_view(POPUP_TAB),
                "{dismissal} did not dispose popup"
            );
            Ok(())
        })??;
    }

    host.focus(source)?;
    host.test_click(source, true)?;
    for _ in 0..5 {
        pause(cx).await;
    }
    view.update_in(cx, |app, window, cx| -> Result<()> {
        let pane = app.model.pane(source_pane).unwrap();
        let index = pane.tabs.iter().position(|t| t.id == source).unwrap();
        ensure!(
            pane.tabs.len() == 3 && pane.tabs[index + 1].id == pane.active_tab,
            "Existing Cmd-click stopped opening an adjacent regular tab"
        );
        ensure!(
            app.link_popup.is_none(),
            "Cmd-click opened a temporary popup"
        );
        app.select_target(TabTarget::Page(source_pane, source), window, cx);
        Ok(())
    })??;
    pause(cx).await;
    ensure!(
        eval(&host, source, "document.querySelector('input').value", cx).await?
            == "Source survives popup",
        "Cmd-click navigated the source"
    );

    // Deliver a native Cmd+Shift+T with WKWebView content focused.
    host.focus(source)?;
    host.test_option_click(source)?;
    wait_popup(view, &host, "Popup A", cx).await?;
    let homepage = format!("{base}/source");
    let space_count = view.update_in(cx, |app, _, _| {
        app.preferences.homepage_url = homepage.clone();
        app.model.spaces.len()
    })?;
    host.focus(POPUP_TAB)?;
    view.update_in(cx, |app, _, _| {
        app.link_popup.as_ref().unwrap().test_key(
            "T",
            objc2_app_kit::NSEventModifierFlags::Command
                | objc2_app_kit::NSEventModifierFlags::Shift,
            17,
        )
    })??;
    for _ in 0..5 {
        pause(cx).await;
    }
    view.update_in(cx, |app, _, _| -> Result<()> {
        ensure!(
            app.model.spaces.len() == space_count + 1,
            "Native Cmd+Shift+T did not create space"
        );
        ensure!(
            app.model.focused_pane().active_tab().url == homepage,
            "NewSpace ignored homepage"
        );
        ensure!(
            app.editing == Some(TabTarget::Space(app.model.active_space)),
            "NewSpace did not edit new space"
        );
        Ok(())
    })??;
    for _ in 0..3 {
        pause(cx).await;
    }
    view.update_in(cx, |app, window, cx| -> Result<()> {
        ensure!(
            app.address.read(cx).focus_handle(cx).is_focused(window),
            "NewSpace address lost native focus"
        );
        app.command(Command::NewTab, window, cx);
        ensure!(
            matches!(app.editing, Some(TabTarget::Page(_, _))),
            "Existing NewTab editing changed"
        );
        Ok(())
    })??;
    eval(
        &host,
        source,
        "document.cookie='axolotl_popup_session=;max-age=0;path=/';true",
        cx,
    )
    .await?;
    Ok(vec![
        "native IPC and trusted Option-click survive protection policy off/on updates and document reloads",
        "trusted native Option-click and Option-click target=_blank open one temporary page sharing session cookies without navigating source",
        "popup centered across split panes and after native window resize; page dimensions match chrome",
        "ordinary popup navigation and nested Option-click retain origin and create no saved tabs",
        "promotion keeps DOM, form, history and callback identity in originating pane; dismissal disposes page",
        "native Escape, outside click and close button dispose popup; Cmd-click preserves adjacent tab behavior; native Cmd+Shift+T uses homepage and keeps address focus; NewTab still edits page URL",
    ])
}
