//! Exercise the shipped settings document, native surface and host IPC.
use crate::{
    app::{BrowserApp, Command},
    smoke::{eval, pause},
};
use anyhow::{Result, ensure};
use gpui::*;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use workspace_core::SearchEngine;

async fn dialog_eval(
    view: &WeakEntity<BrowserApp>,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> Result<Value> {
    let response = Arc::new(Mutex::new(None));
    let target = response.clone();
    view.update_in(cx, |app, _, _| {
        app.settings
            .as_ref()
            .and_then(|d| d.view.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Settings not mounted"))?
            .evaluate_script_with_callback(script, move |value| {
                *target.lock().unwrap() = Some(value)
            })
            .map_err(anyhow::Error::from)
    })??;
    for _ in 0..50 {
        if let Some(value) = response.lock().unwrap().take() {
            return Ok(serde_json::from_str(&value)?);
        }
        pause(cx).await;
    }
    anyhow::bail!("Settings evaluation timed out")
}

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, tab, before) = view.update_in(cx, |app, window, cx| {
        app.settings_focus.focus(window);
        let snapshot = app.model.to_json();
        app.open_settings(false, window, cx);
        (app.host.clone(), app.active_tab(), snapshot)
    })?;
    pause(cx).await;
    pause(cx).await;
    let state = dialog_eval(view, "({role:document.querySelector('#settings').getAttribute('role'),modal:document.querySelector('#settings').getAttribute('aria-modal'),focused:document.activeElement===select,value:select.value,options:[...select.options].map(o=>o.textContent),fits:document.querySelector('#settings').getBoundingClientRect().right<=innerWidth})", cx).await?;
    ensure!(
        state["role"] == "dialog"
            && state["modal"] == "true"
            && state["focused"] == true
            && state["value"] == "google"
            && state["options"] == serde_json::json!(["Google", "DuckDuckGo", "Bing"])
            && state["fits"] == true,
        "Incorrect settings dialog: {state}"
    );
    #[cfg(target_os = "macos")]
    {
        for dark in [true, false] {
            host.set_test_appearance(tab, Some(dark))?;
            pause(cx).await;
            let scheme =
                dialog_eval(view, "document.documentElement.style.colorScheme", cx).await?;
            ensure!(
                scheme == if dark { "dark" } else { "light" },
                "Settings did not follow native appearance"
            );
        }
        host.set_test_appearance(tab, None)?;
        pause(cx).await;
    }
    ensure!(
        dialog_eval(view, "homepage.value", cx).await? == "about:blank",
        "Incorrect homepage default"
    );
    for shift in [false, true] {
        let expected = if shift {
            ["close", "homepage-url", "search-engine"]
        } else {
            ["homepage-url", "close", "search-engine"]
        };
        for id in expected {
            let focused = dialog_eval(view, &format!("document.dispatchEvent(new KeyboardEvent('keydown',{{key:'Tab',shiftKey:{shift},bubbles:true,cancelable:true}}));document.activeElement.id"), cx).await?;
            ensure!(
                focused == id,
                "Tab failed to cycle inside dialog: {focused}"
            );
        }
    }
    dialog_eval(view, "homepage.value='https://example.com';homepage.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));true", cx).await?;
    pause(cx).await;
    ensure!(
        dialog_eval(view, "homepage.value", cx).await? == "https://example.com/",
        "Homepage was not normalized"
    );
    view.update_in(cx, |app, _, _| {
        ensure!(
            app.preferences.homepage_url == "https://example.com/",
            "Homepage IPC did not apply"
        );
        ensure!(
            app.model.to_json() == before,
            "Homepage changed existing tabs"
        );
        Ok(())
    })??;
    for value in [
        serde_json::json!("javascript:alert(1)"),
        serde_json::json!("not a URL"),
        serde_json::json!(false),
    ] {
        dialog_eval(view, &format!("send('homepage',{value});true"), cx).await?;
        pause(cx).await;
        ensure!(
            view.update_in(cx, |app, _, _| app.preferences.homepage_url
                == "https://example.com/")?,
            "Invalid homepage IPC changed preference"
        );
    }
    ensure!(dialog_eval(view, "!document.querySelector('#homepage-error').hidden && homepage.getAttribute('aria-invalid')==='true'", cx).await? == true, "Homepage validation error not displayed");
    dialog_eval(
        view,
        "homepage.value='';homepage.dispatchEvent(new Event('change'));true",
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        dialog_eval(
            view,
            "homepage.value==='about:blank' && document.querySelector('#homepage-error').hidden",
            cx
        )
        .await?
            == true,
        "Empty homepage did not reset or clear error"
    );
    dialog_eval(
        view,
        "homepage.value='https://example.com';homepage.dispatchEvent(new Event('change'));true",
        cx,
    )
    .await?;
    pause(cx).await;
    for engine in [
        SearchEngine::Bing,
        SearchEngine::DuckDuckGo,
        SearchEngine::Google,
    ] {
        dialog_eval(
            view,
            &format!(
                "select.value='{engine_id}';select.dispatchEvent(new Event('change'));true",
                engine_id = engine.id()
            ),
            cx,
        )
        .await?;
        pause(cx).await;
        view.update_in(cx, |app, window, cx| {
            ensure!(
                app.preferences.search_engine == engine,
                "Preference did not apply immediately"
            );
            app.command(Command::NewTab, window, cx);
            ensure!(
                app.model.to_json() == before,
                "Settings changed the workspace or allowed background commands"
            );
            Ok(())
        })??;
    }
    dialog_eval(
        view,
        "window.ipc.postMessage(JSON.stringify({type:'engine',value:'invalid'}));true",
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.preferences.search_engine
            == SearchEngine::Google)?,
        "Invalid IPC changed the preference"
    );
    // A live document must retain JS state while the dialog is open and closes.
    eval(&host, tab, "window.settingsSmokeMarker=42;true", cx).await?;
    dialog_eval(view, "closeButton.click();true", cx).await?;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        ensure!(
            app.settings.is_none() && app.settings_focus.is_focused(window),
            "Close failed to restore the invoking control's focus"
        );
        // Exercise the same action used by the menu and shell shortcut.
        window.dispatch_action(Box::new(crate::Settings), cx);
        Ok(())
    })??;
    pause(cx).await;
    pause(cx).await;
    ensure!(
        dialog_eval(view, "homepage.value", cx).await? == "https://example.com/",
        "Reopened dialog lost homepage"
    );
    dialog_eval(
        view,
        "document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));true",
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.settings.is_none()
            && app.model.to_json() == before)?,
        "Escape failed or changed existing tabs"
    );
    ensure!(
        eval(&host, tab, "window.settingsSmokeMarker", cx).await? == 42,
        "Existing page reloaded"
    );
    view.update_in(cx, |app, window, cx| {
        let original = app.model.clone();
        app.command(Command::NewTab, window, cx);
        ensure!(
            app.model.focused_pane().active_tab().url == "https://example.com/",
            "New tab ignored homepage"
        );
        app.command(Command::NewSpace, window, cx);
        ensure!(
            app.model.focused_pane().active_tab().url == "https://example.com/",
            "New space ignored homepage"
        );
        app.command(Command::CloseTab, window, cx);
        ensure!(
            app.model.focused_pane().active_tab().url == "https://example.com/",
            "Replacement tab ignored homepage"
        );
        app.tab_action(
            crate::app::TabTarget::Page(app.model.focused_pane().id, app.active_tab()),
            crate::app::TabAction::Close,
            window,
            cx,
        );
        ensure!(
            app.model.focused_pane().active_tab().url == "https://example.com/",
            "Close button replacement ignored homepage"
        );
        app.model = original;
        app.preferences.set_homepage_url("about:blank")?;
        app.sync_address(window, cx);
        Ok(())
    })??;
    Ok(vec![
        "native settings: homepage editing, validation, reset, new tabs/spaces and replacement tabs, Google default, three providers, immediate changes, invalid IPC rejection, live theme updates, modal focus wrap, close/Escape dismissal, focus restoration, menu action, unchanged workspace and live page",
    ])
}
