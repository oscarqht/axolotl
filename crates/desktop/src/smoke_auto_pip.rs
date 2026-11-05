//! Real audible H.264/AAC playback in the native webview and a native PiP window.
use crate::{
    app::{BrowserApp, TabTarget},
    smoke::{Host, eval, pause},
};
use anyhow::{Result, ensure};
use base64::Engine as _;
use gpui::*;
use workspace_core::{Axis, TabId};

async fn state(
    view: &WeakEntity<BrowserApp>,
    tab: Option<TabId>,
    cx: &mut AsyncWindowContext,
) -> Result<serde_json::Value> {
    let mut stable = 0;
    for _ in 0..35 {
        pause(cx).await;
        let value = view.update_in(cx, |app, _, cx| app.host.picture_in_picture_state(cx))?;
        if value["tab"].as_u64() == tab {
            stable += 1;
            if tab.is_some() || stable >= 3 {
                return Ok(value);
            }
        } else {
            stable = 0;
        }
    }
    let value = view.update_in(cx, |app, _, cx| app.host.picture_in_picture_state(cx))?;
    anyhow::bail!("PiP did not settle to {tab:?}: {value}")
}
async fn select(
    view: &WeakEntity<BrowserApp>,
    pane: u64,
    tab: TabId,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(pane, tab), window, cx)
    })?;
    pause(cx).await;
    Ok(())
}
async fn videos(host: &Host, tab: TabId, cx: &mut AsyncWindowContext) -> Result<()> {
    let api = eval(host, tab, "({api:typeof window.__axolotlAutoPip,crypto:typeof crypto.randomUUID,ipc:typeof window.ipc?.postMessage})", cx).await?;
    ensure!(
        api["api"] == "function",
        "PiP bridge was not installed: {api}"
    );
    let data = base64::engine::general_purpose::STANDARD
        .encode(include_bytes!("../assets/fixtures/pip.mp4"));
    let src = serde_json::to_string(&format!("data:video/mp4;base64,{data}"))?;
    eval(host, tab, &format!(r#"(()=>{{document.body.innerHTML='<h1>Native PiP fixture</h1><video id="small" style="width:120px;height:68px"></video><video id="large" style="width:320px;height:180px"></video><video id="muted" style="width:600px;height:200px"></video>';for(const v of document.querySelectorAll('video')){{v.src={src};v.loop=true;v.volume=0.08;v.muted=v.id==='muted';v.play().catch(e=>window.fixtureError=String(e))}}return true}})()"#), cx).await?;
    for _ in 0..40 {
        pause(cx).await;
        let playing = eval(host, tab, "(()=>{const v=document.querySelector('#large');return {ready:v.readyState,playing:!v.paused,audio:v.webkitAudioDecodedByteCount,tracks:v.audioTracks?.length,error:window.fixtureError||null}})()", cx).await?;
        if playing["playing"] == true
            && (playing["audio"].as_u64().unwrap_or(0) > 0
                || playing["tracks"].as_u64().unwrap_or(0) > 0)
        {
            return Ok(());
        }
    }
    anyhow::bail!("Audible fixture did not start")
}
pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, first, pane, space) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.active_tab(),
            app.model.focused_pane().id,
            app.model.active_space,
        )
    })?;
    videos(&host, first, cx).await?;
    let second = view.update_in(cx, |app, _, cx| {
        let id = app.model.add_tab("about:blank");
        cx.notify();
        id
    })?;
    let native = state(view, Some(first), cx).await?;
    ensure!(
        native["native"]["visible"] == true && native["native"]["topmost"] == true,
        "Native PiP window isn't visible and on top: {native}"
    );
    ensure!(
        eval(
            &host,
            first,
            "document.querySelector('[data-axolotl-pip=target]')?.id",
            cx
        )
        .await?
            == "large",
        "Largest audible video was not selected"
    );
    let before = eval(
        &host,
        first,
        "document.querySelector('#large').currentTime",
        cx,
    )
    .await?
    .as_f64()
    .unwrap();
    let mut after = before;
    for _ in 0..20 {
        pause(cx).await;
        after = eval(
            &host,
            first,
            "document.querySelector('#large').currentTime",
            cx,
        )
        .await?
        .as_f64()
        .unwrap();
        if (after - before).abs() > 0.25 {
            break;
        }
    }
    ensure!((after-before).abs() > 0.25, "PiP playback clock stopped before={before} after={after}; native={}; {}", view.update_in(cx,|app,_,cx|app.host.picture_in_picture_state(cx))?, eval(&host,first,"(()=>{const v=document.querySelector('#large');return {paused:v.paused,ready:v.readyState,ended:v.ended,hidden:document.hidden,time:v.currentTime,error:v.error?.message,frames:v.getVideoPlaybackQuality?.().totalVideoFrames}})()",cx).await?);
    #[cfg(target_os = "macos")]
    host.save_test_snapshot(first, "/tmp/axolotl-auto-pip.png")?;
    if std::env::args().any(|a| a == "--pip-inspect") {
        println!("AXOLOTL_PIP_INSPECT ready {native}");
        for _ in 0..75 {
            pause(cx).await;
        }
    }
    select(view, pane, first, cx).await?;
    state(view, None, cx).await?;
    ensure!(eval(&host,first,"document.querySelectorAll('[data-axolotl-pip]').length===0 && !document.querySelector('#large').controls",cx).await? == true, "Return did not restore page/controls");
    select(view, pane, second, cx).await?;
    state(view, Some(first), cx).await?;
    // Exercise the operating system's close action, not the automatic return path.
    view.update_in(cx, |app, _, cx| app.host.test_close_picture_in_picture(cx))??;
    state(view, None, cx).await?;
    for _ in 0..5 {
        pause(cx).await;
    }
    state(view, None, cx).await?;
    select(view, pane, first, cx).await?;
    select(view, pane, second, cx).await?;
    state(view, Some(first), cx).await?;
    // Preserve the first floating video while another audible source is hidden.
    videos(&host, second, cx).await?;
    view.update_in(cx, |app, _, cx| {
        app.model.add_tab("about:blank");
        cx.notify();
    })?;
    for _ in 0..4 {
        pause(cx).await;
    }
    state(view, Some(first), cx).await?;
    host.set_automatic_picture_in_picture(false)?;
    state(view, None, cx).await?;
    select(view, pane, first, cx).await?;
    select(view, pane, second, cx).await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    state(view, None, cx).await?;
    eval(
        &host,
        second,
        "document.querySelectorAll('video').forEach(v=>v.pause());true",
        cx,
    )
    .await?;
    host.set_automatic_picture_in_picture(true)?;
    // Re-enabling while hidden does not override a dismissal or start PiP.
    state(view, None, cx).await?;
    select(view, pane, first, cx).await?;
    view.update_in(cx, |app, _, cx| {
        app.model.split_focused(Axis::Horizontal, "about:blank");
        cx.notify();
    })?;
    pause(cx).await;
    state(view, None, cx).await?;
    view.update_in(cx, |app, _, cx| {
        app.model.focus_pane(pane);
        cx.notify();
    })?;
    state(view, None, cx).await?;
    // OS app deactivation/minimization are not inputs to workspace visibility.
    view.update_in(cx, |_, _, cx| cx.hide())?;
    for _ in 0..3 {
        pause(cx).await;
    }
    state(view, None, cx).await?;
    view.update_in(cx, |_, window, cx| {
        cx.activate(true);
        window.activate_window();
    })?;
    view.update_in(cx, |_, window, _| window.minimize_window())?;
    for _ in 0..3 {
        pause(cx).await;
    }
    state(view, None, cx).await?;
    view.update_in(cx, |_, window, _| window.activate_window())?;
    view.update_in(cx, |app, window, cx| {
        app.model.add_space("Other", "about:blank");
        app.sync_address(window, cx);
        cx.notify();
    })?;
    state(view, Some(first), cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Space(space), window, cx)
    })?;
    state(view, None, cx).await?;
    // Unload removes the live source and its floating surface.
    select(view, pane, second, cx).await?;
    state(view, Some(first), cx).await?;
    host.unload(first);
    state(view, None, cx).await?;
    let mut checks = vec![
        "visible native always-on-top PiP with advancing audible playback",
        "largest audible video; muted larger video excluded",
        "automatic return restores source DOM and controls",
        "native close stays dismissed until revisit and leave",
        "existing PiP preserved; toggle closes and prevents entry",
        "visible split focus and minimization do not trigger",
        "space switch entry/return; source unload cleanup",
    ];
    checks.extend(crate::smoke_auto_pip_frames::run(view, cx).await?);
    Ok(checks)
}
