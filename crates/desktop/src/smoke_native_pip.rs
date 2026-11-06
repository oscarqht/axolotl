//! Opt-in live YouTube test; uses the actual GPUI/Wry visibility path.
use crate::{
    app::{BrowserApp, Command, TabTarget},
    smoke::{Host, eval, pause},
};
use anyhow::{Result, ensure};
use gpui::*;
use workspace_core::TabId;

async fn settle(cx: &mut AsyncWindowContext) {
    for _ in 0..15 {
        pause(cx).await;
    }
}
async fn mode(host: &Host, tab: TabId, expected: &str, cx: &mut AsyncWindowContext) -> Result<()> {
    settle(cx).await;
    let actual = eval(
        host,
        tab,
        "(()=>{const v=document.querySelector('video');return {mode:v.webkitPresentationMode,paused:v.paused,time:v.currentTime,ready:v.readyState,supports:v.webkitSupportsPresentationMode?.('picture-in-picture')}})()",
        cx,
    )
    .await?;
    ensure!(
        actual["mode"] == expected,
        "Expected {expected}, got {actual}"
    );
    println!("PIP_CHECK mode={expected}");
    Ok(())
}
fn cpu_seconds() -> f64 {
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        (usage.ru_utime.tv_sec + usage.ru_stime.tv_sec) as f64
            + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 / 1_000_000.
    }
}
async fn measure(host: &Host, tab: TabId, label: &str, cx: &mut AsyncWindowContext) -> Result<()> {
    let script = "(()=>{const v=document.querySelector('video'),q=v.getVideoPlaybackQuality();return {time:v.currentTime,total:q.totalVideoFrames,dropped:q.droppedVideoFrames,paused:v.paused}})()";
    let before = eval(host, tab, script, cx).await?;
    let cpu = cpu_seconds();
    let start = std::time::Instant::now();
    cx.background_executor()
        .timer(std::time::Duration::from_secs(10))
        .await;
    let elapsed = start.elapsed().as_secs_f64();
    let cpu_percent = (cpu_seconds() - cpu) / elapsed * 100.;
    let after = eval(host, tab, script, cx).await?;
    println!(
        "PIP_MEASURE {}",
        serde_json::json!({"phase":label,"seconds":elapsed,"host_cpu_percent":cpu_percent,"before":before,"after":after})
    );
    Ok(())
}
pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, tab, pane, space) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.active_tab(),
            app.model.focused_pane().id,
            app.model.active_space().id,
        )
    })?;
    host.navigate(tab, "https://www.youtube.com/watch?v=aqz-KE-bpKQ")?;
    for _ in 0..150 {
        pause(cx).await;
        if eval(
            &host,
            tab,
            "Boolean(document.querySelector('video')?.readyState >= 2)",
            cx,
        )
        .await?
            == true
        {
            break;
        }
    }
    eval(&host,tab,"(()=>{const v=document.querySelector('video');if(v){v.muted=true;v.play().catch(e=>window.playError=String(e));}return true})()",cx).await?;
    for _ in 0..35 {
        pause(cx).await;
    }
    ensure!(
        eval(
            &host,
            tab,
            "Boolean(document.querySelector('video') && !document.querySelector('video').paused)",
            cx
        )
        .await?
            == true,
        "YouTube playback unavailable"
    );
    measure(&host, tab, "inline", cx).await?;
    println!("PIP_READY");
    view.update_in(cx, |app, window, cx| {
        app.command(Command::NewTab, window, cx)
    })?;
    let other = view.update_in(cx, |app, _, _| app.active_tab())?;
    mode(&host, tab, "picture-in-picture", cx).await?;
    measure(&host, tab, "hidden-pip", cx).await?;
    let start = eval(
        &host,
        tab,
        "document.querySelector('video').currentTime",
        cx,
    )
    .await?
    .as_f64()
    .unwrap();
    settle(cx).await;
    let end = eval(
        &host,
        tab,
        "document.querySelector('video').currentTime",
        cx,
    )
    .await?
    .as_f64()
    .unwrap();
    ensure!(
        end > start + 1.,
        "Hidden PiP playback stalled: {start} -> {end}"
    );
    for _ in 0..3 {
        view.update_in(cx, |app, window, cx| {
            app.select_target(TabTarget::Page(pane, tab), window, cx)
        })?;
        mode(&host, tab, "picture-in-picture", cx).await?;
        view.update_in(cx, |app, window, cx| {
            app.select_target(TabTarget::Page(pane, other), window, cx)
        })?;
        mode(&host, tab, "picture-in-picture", cx).await?;
    }
    // Native dismissal uses the same presentation events; actual close-button coverage is manual.
    eval(
        &host,
        tab,
        "document.querySelector('video').webkitSetPresentationMode('inline');true",
        cx,
    )
    .await?;
    mode(&host, tab, "inline", cx).await?;
    view.update_in(cx, |_, _, cx| cx.notify())?;
    mode(&host, tab, "inline", cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(pane, tab), window, cx)
    })?;
    eval(
        &host,
        tab,
        "document.querySelector('video').play();true",
        cx,
    )
    .await?;
    settle(cx).await;
    view.update_in(cx, |app, window, cx| {
        app.command(Command::NewSpace, window, cx)
    })?;
    mode(&host, tab, "picture-in-picture", cx).await?;
    println!("PIP_VISUAL_READY");
    if std::env::var_os("AXOLOTL_PIP_HOLD").is_some() {
        for _ in 0..300 {
            pause(cx).await;
        }
    }
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Space(space), window, cx);
        app.tab_action(
            TabTarget::Page(pane, tab),
            crate::app::TabAction::Close,
            window,
            cx,
        );
    })?;
    settle(cx).await;
    ensure!(!host.has_view(tab), "Closed source retained its WKWebView");
    Ok(vec![
        "Live YouTube: tab hide enters native PiP, continued hidden playback, three return/hide cycles preserve PiP, dismissal stays inline until shown then hidden, space hide reenters PiP, source closure removes view",
    ])
}
