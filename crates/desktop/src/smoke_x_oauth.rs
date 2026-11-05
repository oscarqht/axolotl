//! Opt-in live X/Google sign-in probe. Stops before credentials, consent or account choice.
use crate::{
    app::BrowserApp,
    smoke::{eval, pause},
};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
use serde_json::{Value, json};

const GOOGLE_BUTTON: &str = r#"(()=>{
    const visible=e=>{const r=e.getBoundingClientRect();return r.width>10&&r.height>10&&r.bottom>0&&r.right>0&&r.top<innerHeight&&r.left<innerWidth&&getComputedStyle(e).visibility!=='hidden'};
    const frame=[...document.querySelectorAll('iframe')].find(e=>visible(e)&&(/accounts\.google\.com\/gsi\/button/.test(e.src)||/sign in with google/i.test(e.title)));
    const button=frame||[...document.querySelectorAll('button,[role="button"],a')].find(e=>visible(e)&&!e.disabled&&/^(sign in|continue|sign up) with google$/i.test((e.getAttribute('aria-label')||e.innerText||'').trim()));
    const b=button?.getBoundingClientRect();
    return {page:location.origin+location.pathname,google_button:!!button,kind:frame?'google-frame':'button',x:b?Math.max(1,Math.min(innerWidth-1,b.x+b.width/2)):null,y:b?Math.max(1,Math.min(innerHeight-1,b.y+b.height/2)):null,logged_in:!!document.querySelector('[data-testid="SideNav_AccountSwitcher_Button"]'),challenge:!!document.querySelector('iframe[src*="arkoselabs"],iframe[src*="recaptcha"],#captcha-form')};
})()"#;

const PROVIDER_STATE: &str = r#"(()=>{
    const path=location.pathname;
    const visible=e=>{const r=e.getBoundingClientRect();return r.width>0&&r.height>0&&getComputedStyle(e).visibility!=='hidden'};
    const count=new Set([...document.querySelectorAll('[data-identifier]')].filter(visible).map(e=>e.getAttribute('data-identifier'))).size;
    const text=document.body?.innerText||'';
    let stage='loading';
    if(count||/accountchooser/i.test(path))stage='account-choice';
    else if([...document.querySelectorAll('input[type="password"]')].some(visible))stage='password';
    else if([...document.querySelectorAll('input[type="email"],input[name="identifier"]')].some(visible))stage='identifier';
    else if(/\/challenge\//i.test(path))stage='mfa-or-challenge';
    else if(/consent|oauth\/id|oauth\/delegation/i.test(path))stage='consent';
    else if(/rejected|denied|error/i.test(path)||/browser or app may not be secure|browser.*not supported/i.test(text))stage='provider-rejection';
    return {page:location.origin+path,google:location.hostname==='accounts.google.com',opener:opener!==null,stage,account_count:count};
})()"#;

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, source, before) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.active_tab(),
            app.model
                .spaces
                .iter()
                .flat_map(|s| &s.panes)
                .flat_map(|p| &p.tabs)
                .map(|t| t.id)
                .collect::<std::collections::HashSet<_>>(),
        )
    })?;
    host.navigate(source, "https://x.com/i/flow/login")?;
    for _ in 0..150 {
        pause(cx).await;
        if !host.is_loaded(source) {
            continue;
        }
        let button = eval(&host, source, GOOGLE_BUTTON, cx).await?;
        if button["google_button"] == true
            || button["logged_in"] == true
            || button["challenge"] == true
        {
            break;
        }
    }
    // Google's cross-origin button iframe exists before its content finishes
    // loading. Wait for the page and native paint before sending real input.
    view.update_in(cx, |_, window, cx| {
        window.activate_window();
        cx.activate(true);
    })?;
    for _ in 0..20 {
        pause(cx).await;
    }
    let button = eval(&host, source, GOOGLE_BUTTON, cx).await?;
    host.save_test_snapshot(source, "/tmp/axolotl-x-oauth.png")?;
    println!("AXOLOTL_X_OAUTH_SOURCE {}", button);
    if button["logged_in"] == true || button["challenge"] == true {
        println!(
            "AXOLOTL_X_OAUTH {}",
            json!({"status":"blocked","reason":if button["logged_in"]==true {"existing-X-session"} else {"X-challenge"},"flow_complete":false})
        );
        return Ok(vec![
            "Live X login page inspected; OAuth completion remains unverified because the current page is signed in or requires a challenge",
        ]);
    }
    ensure!(
        button["google_button"] == true,
        "Live X login page did not expose an available Google sign-in button; screenshot: /tmp/axolotl-x-oauth.png"
    );
    host.focus(source)?;
    host.test_click_at(
        source,
        button["x"].as_f64().context("Missing Google button x")?,
        button["y"].as_f64().context("Missing Google button y")?,
        false,
    )?;
    let mut popup = None;
    for _ in 0..100 {
        pause(cx).await;
        popup = view.update_in(cx, |app, _, _| {
            app.model
                .spaces
                .iter()
                .flat_map(|s| &s.panes)
                .flat_map(|p| &p.tabs)
                .find(|t| !before.contains(&t.id))
                .map(|t| t.id)
        })?;
        if popup.is_some() {
            break;
        }
    }
    let popup =
        popup.context("Trusted X Google sign-in click did not create a native popup tab")?;
    // Wry queues scripts before the first navigation without preserving their
    // evaluation callbacks. Wait for a finished provider document first.
    for _ in 0..200 {
        pause(cx).await;
        if host.is_loaded(popup)
            && host
                .current_url(popup)
                .ok()
                .and_then(|value| url::Url::parse(&value).ok())
                .is_some_and(|url| url.host_str() == Some("accounts.google.com"))
        {
            break;
        }
    }
    ensure!(
        host.is_loaded(popup),
        "Google popup did not finish its first document load"
    );
    let mut state = Value::Null;
    for _ in 0..150 {
        pause(cx).await;
        if !host.is_loaded(popup) {
            continue;
        }
        state = eval(&host, popup, PROVIDER_STATE, cx).await?;
        if state["google"] == true && state["stage"] != "loading" {
            break;
        }
    }
    let snapshot = "/tmp/axolotl-google-oauth.png";
    let _ = std::fs::remove_file(snapshot);
    host.save_test_snapshot(popup, snapshot)?;
    for _ in 0..30 {
        if std::path::Path::new(snapshot).exists() {
            break;
        }
        pause(cx).await;
    }
    // Return only origin/path and stage booleans/counts; URLs with OAuth state,
    // account names, emails, page text and credential values never enter logs.
    println!("AXOLOTL_X_OAUTH_PROVIDER {}", state);
    ensure!(
        state["google"] == true,
        "X sign-in popup did not reach accounts.google.com"
    );
    ensure!(
        state["opener"] == true,
        "Live Google sign-in popup has a null window.opener"
    );
    let stage = state["stage"].as_str().unwrap_or("unknown");
    println!(
        "AXOLOTL_X_OAUTH {}",
        json!({"status":"blocked","reason":stage,"flow_complete":false,"native_popup":true,"opener":true,"account_count":state["account_count"]})
    );
    Ok(vec![
        "Live X Google sign-in trusted click opened an Axolotl native tab and reached accounts.google.com with window.opener present",
        "Live sign-in remains incomplete: stopped before account selection, credentials, MFA or consent; inspect AXOLOTL_X_OAUTH_PROVIDER for the observed boundary",
    ])
}
