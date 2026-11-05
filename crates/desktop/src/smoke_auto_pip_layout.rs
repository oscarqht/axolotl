//! Responsive players can move their live video subtree when PiP shrinks them.
use crate::{
    app::{BrowserApp, TabTarget},
    smoke::{eval, pause},
};
use anyhow::{Result, ensure};
use base64::Engine as _;
use gpui::*;

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, source, pane) = view.update_in(cx, |app, _, cx| {
        let source = app.model.add_tab("about:blank");
        cx.notify();
        (app.host.clone(), source, app.model.focused_pane().id)
    })?;
    for _ in 0..30 {
        pause(cx).await;
        if host.is_loaded(source) {
            break;
        }
    }
    let data = base64::engine::general_purpose::STANDARD
        .encode(include_bytes!("../assets/fixtures/pip.mp4"));
    let src = serde_json::to_string(&format!("data:video/mp4;base64,{data}"))?;
    eval(&host,source,&format!(r#"(()=>{{document.body.innerHTML='<div id="regular" data-axolotl-pip="regular-original"><div id="player"><video id="responsive-video" style="width:320px;height:180px"></video></div></div><div id="full-bleed"></div>';const player=document.querySelector('#player'),v=document.querySelector('video');const layout=()=>{{const target=document.querySelector(innerWidth<600?'#full-bleed':'#regular');if(player.parentElement!==target)target.appendChild(player)}};addEventListener('resize',layout);v.src={src};v.loop=true;v.volume=0.03;v.play();return true}})()"#),cx).await?;
    let mut playing = false;
    for _ in 0..30 {
        pause(cx).await;
        if eval(&host,source,"(()=>{const v=document.querySelector('video');return v.readyState>=2&&!v.paused&&(v.audioTracks?.length>0||v.webkitAudioDecodedByteCount>0)})()",cx).await?==true{playing=true;break;}
    }
    ensure!(playing, "Responsive fixture did not play");
    view.update_in(cx, |app, _, cx| {
        app.model.add_tab("about:blank");
        cx.notify();
    })?;
    let mut opened = false;
    for _ in 0..35 {
        pause(cx).await;
        if view.update_in(cx, |app, _, cx| {
            app.host.picture_in_picture_state(cx)["tab"].as_u64() == Some(source)
        })? {
            opened = true;
            break;
        }
    }
    ensure!(opened, "Responsive fixture PiP did not open");
    for _ in 0..5 {
        pause(cx).await;
    }
    let result=eval(&host,source,"(()=>{const v=document.querySelector('video'),r=v.getBoundingClientRect();return {parent:document.querySelector('#player').parentElement.id,width:r.width,height:r.height,hit:document.elementFromPoint(innerWidth/2,innerHeight/2)===v,regular:document.querySelector('#regular').getAttribute('data-axolotl-pip'),full:document.querySelector('#full-bleed').getAttribute('data-axolotl-pip'),paused:v.paused}})()",cx).await?;
    println!("AXOLOTL_PIP_RESPONSIVE {result}");
    ensure!(
        result["parent"] == "full-bleed"
            && result["width"].as_f64().unwrap_or(0.) > 0.
            && result["hit"] == true,
        "Responsive PiP player was hidden after relocation: {result}"
    );
    ensure!(
        result["regular"] == "regular-original" && result["full"] == "branch",
        "Responsive ancestor isolation did not follow player: {result}"
    );
    #[cfg(target_os = "macos")]
    host.save_test_snapshot(source, "/tmp/axolotl-pip-responsive.png")?;
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(pane, source), window, cx)
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    ensure!(eval(&host,source,"document.querySelector('#regular').getAttribute('data-axolotl-pip')==='regular-original'&&document.querySelector('#full-bleed').getAttribute('data-axolotl-pip')===null&&!document.querySelector('video').controls&&document.querySelectorAll('[data-axolotl-pip=branch],[data-axolotl-pip=target]').length===0",cx).await?==true,"Responsive return did not restore original attributes/controls");
    // Leave no audible source behind for the following independent frame check.
    eval(
        &host,
        source,
        "document.querySelector('video').pause();true",
        cx,
    )
    .await?;
    Ok(vec![
        "responsive player relocation stays visible in PiP and restores both ancestor paths",
    ])
}
