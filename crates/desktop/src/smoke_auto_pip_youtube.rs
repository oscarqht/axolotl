//! Opt-in live-site regression. Never runs as part of the offline smoke suite.
use crate::{
    app::{BrowserApp, TabTarget},
    smoke::{eval, pause},
};
use anyhow::{Result, ensure};
use gpui::*;

const DIAGNOSTICS: &str = r#"(()=>{const rect=e=>{const r=e.getBoundingClientRect();return {x:r.x,y:r.y,w:r.width,h:r.height}};const style=e=>{const s=getComputedStyle(e);return {display:s.display,visibility:s.visibility,opacity:s.opacity,z:s.zIndex,position:s.position,contentVisibility:s.contentVisibility,transform:s.transform,background:s.backgroundColor}};return {title:document.title,hidden:document.hidden,size:[innerWidth,innerHeight],video:[...document.querySelectorAll('video')].map(v=>{let ancestors=[];for(let e=v;e;e=e.parentElement){ancestors.push({tag:e.tagName,id:e.id,class:e.className,pip:e.getAttribute('data-axolotl-pip'),rect:rect(e),style:style(e)})}const hit=document.elementFromPoint(innerWidth/2,innerHeight/2);return {time:v.currentTime,paused:v.paused,ready:v.readyState,ended:v.ended,error:v.error?.message,decoded:v.webkitDecodedFrameCount,quality:v.getVideoPlaybackQuality?.().totalVideoFrames,audio:v.webkitAudioDecodedByteCount,tracks:v.audioTracks?.length,controls:v.controls,width:v.videoWidth,height:v.videoHeight,rect:rect(v),style:style(v),hit:{tag:hit?.tagName,id:hit?.id,class:hit?.className,pip:hit?.getAttribute('data-axolotl-pip')},ancestors}}),text:document.body.innerText.slice(0,600)}})()"#;

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, source, pane) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.active_tab(),
            app.model.focused_pane().id,
        )
    })?;
    host.navigate(source, "https://www.youtube.com/watch?v=aqz-KE-bpKQ")?;
    let mut ready = false;
    for attempt in 0..150 {
        #[cfg(not(target_os = "macos"))]
        let _ = attempt;
        pause(cx).await;
        let state=eval(&host,source,"(()=>{const v=document.querySelector('video');if(v){v.volume=0.03;if(v.paused)v.play().catch(()=>{});}return !!v&&v.readyState>=2&&!v.paused&&v.videoWidth>0})()",cx).await?;
        if state == true {
            ready = true;
            break;
        }
        #[cfg(target_os = "macos")]
        if attempt == 15 {
            let button=eval(&host,source,"(()=>{const b=document.querySelector('.ytp-large-play-button');const r=b?.getBoundingClientRect();return r?[r.x+r.width/2,r.y+r.height/2]:null})()",cx).await?;
            if let (Some(x), Some(y)) = (button[0].as_f64(), button[1].as_f64()) {
                host.test_click_at(source, x, y, false)?;
            }
        }
    }
    let before = eval(&host, source, DIAGNOSTICS, cx).await?;
    println!("AXOLOTL_YOUTUBE_PIP_BEFORE {before}");
    ensure!(ready, "Live YouTube did not start: {before}");
    #[cfg(target_os = "macos")]
    host.save_test_snapshot(source, "/tmp/axolotl-youtube-pip-before.png")?;
    view.update_in(cx, |app, _, cx| {
        app.model.add_tab("about:blank");
        cx.notify();
    })?;
    let mut opened = false;
    for _ in 0..40 {
        pause(cx).await;
        if view.update_in(cx, |app, _, cx| {
            app.host.picture_in_picture_state(cx)["tab"].as_u64() == Some(source)
        })? {
            opened = true;
            break;
        }
    }
    ensure!(opened, "Live YouTube PiP did not open");
    for _ in 0..5 {
        pause(cx).await;
    }
    let after = eval(&host, source, DIAGNOSTICS, cx).await?;
    println!("AXOLOTL_YOUTUBE_PIP_AFTER {after}");
    #[cfg(target_os = "macos")]
    host.save_test_snapshot(source, "/tmp/axolotl-youtube-pip-after.png")?;
    for _ in 0..5 {
        pause(cx).await;
    }
    ensure!(eval(&host,source,"(()=>{const v=document.querySelector('[data-axolotl-pip=target]');return !!v&&document.elementFromPoint(innerWidth/2,innerHeight/2)===v})()",cx).await?==true,"Live YouTube PiP video is covered: {after}");
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(pane, source), window, cx)
    })?;
    for _ in 0..4 {
        pause(cx).await;
    }
    Ok(vec![
        "live YouTube audible video enters PiP with unobscured video and returns to source",
    ])
}
