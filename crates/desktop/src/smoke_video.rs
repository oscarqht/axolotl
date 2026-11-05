//! Native WKWebView regression, served by scripts/test-webm-alpha.py.
use crate::{
    app::BrowserApp,
    smoke::{eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, tab) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    let url = std::env::var("AXOLOTL_VIDEO_TEST_URL")?;
    host.present_video_test_window(tab)?;
    host.navigate(tab, &url)?;
    wait_title(&host, tab, "WebM alpha regression", cx).await?;
    let baseline = std::env::args().any(|arg| arg == "--smoke-video-baseline");
    let mut ready = false;
    for _ in 0..100 {
        let result = eval(&host, tab, "(()=>{const vs=[...document.querySelectorAll('video')];return {ready:vs.length===3&&vs.every(v=>v.readyState>=2), masks:vs.map(v=>getComputedStyle(v).maskImage!=='none'),secure:isSecureContext,decoder:typeof VideoDecoder}})()", cx).await?;
        if result["ready"] == true
            && (baseline || result["masks"] == serde_json::json!([true, true, false]))
        {
            ready = true;
            break;
        }
        pause(cx).await;
    }
    ensure!(ready, "Video fixture / alpha masks did not become ready");
    if std::env::var_os("AXOLOTL_VIDEO_REQUIRE_INSECURE").is_some() {
        let context = eval(
            &host,
            tab,
            "({secure:isSecureContext,decoder:typeof VideoDecoder,url:location.href})",
            cx,
        )
        .await?;
        ensure!(
            context["secure"] == false,
            "Expected non-secure HTTP context: {context}"
        );
        println!("AXOLOTL_VIDEO_CONTEXT {context}");
    }

    // Native decoded color plane is opaque: reproduces the engine limitation.
    let native = eval(&host, tab, "(()=>{const c=document.createElement('canvas');c.width=160;c.height=120;const x=c.getContext('2d');x.drawImage(document.querySelector('#vp9'),0,0);return [...x.getImageData(0,0,1,1).data]})()", cx).await?;
    ensure!(
        native[3] == if baseline { 255 } else { 0 },
        "Video drawImage alpha incorrect: {native}"
    );
    if !baseline {
        eval(&host, tab, "window.maskChecks=null; (async()=>{const result=[];for(const id of ['vp9','vp8']){const v=document.getElementById(id);const i=new Image();i.src=getComputedStyle(v).maskImage.slice(5,-2);await i.decode();const c=document.createElement('canvas');c.width=160;c.height=120;const x=c.getContext('2d');x.drawImage(i,0,0);result.push([x.getImageData(0,0,1,1).data[3],x.getImageData(125,50,1,1).data[3],x.getImageData(85,50,1,1).data[3]]);}window.maskChecks=result;})();true", cx).await?;
        let mut checked = false;
        for _ in 0..30 {
            let samples = eval(&host, tab, "window.maskChecks", cx).await?;
            if let Some(samples) = samples.as_array() {
                for sample in samples {
                    ensure!(
                        sample[0].as_u64().unwrap() < 8
                            && (110..=145).contains(&sample[1].as_u64().unwrap())
                            && sample[2].as_u64().unwrap() > 245,
                        "Incorrect mask alpha: {sample}"
                    );
                }
                checked = true;
                break;
            }
            pause(cx).await;
        }
        ensure!(checked, "Could not inspect decoded masks");
    }
    eval(
        &host,
        tab,
        "for(const v of document.querySelectorAll('video')) v.play().catch(()=>{}); true",
        cx,
    )
    .await?;
    eval(&host, tab, "window.loopCounts=[0,0,0];window.lastTimes=[...document.querySelectorAll('video')].map(v=>v.currentTime);window.loopTimer=setInterval(()=>{[...document.querySelectorAll('video')].forEach((v,i)=>{if(v.currentTime+0.05<lastTimes[i])loopCounts[i]++;lastTimes[i]=v.currentTime;})},50);window.videoFrames=[]; for(const v of document.querySelectorAll('video')){v.requestVideoFrameCallback((_,m)=>videoFrames.push(m.presentedFrames));} true", cx).await?;
    for _ in 0..15 {
        pause(cx).await;
    }
    let playback = eval(&host, tab, "({loops:loopCounts,frames:videoFrames.length, hidden:document.hidden, videos:[...document.querySelectorAll('video')].map(v=>({paused:v.paused,ended:v.ended,time:v.currentTime,error:v.error?.message,ready:v.readyState}))})", cx).await?;
    ensure!(
        playback["videos"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["ready"] == 4 && v.get("error").is_none()),
        "Video decode failed: {playback}"
    );
    // Other native test windows can occlude this process. WebKit legitimately
    // pauses/throttles autoplay then; do not misreport that as a codec failure.
    let occluded = playback["hidden"] == true;
    if !occluded {
        ensure!(
            playback["frames"] == 3
                && playback["loops"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|n| n.as_u64().unwrap_or(0) > 0)
                && playback["videos"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|v| v["paused"] == false && v["ended"] == false),
            "Animation or loop stopped: {playback}"
        );
    }
    eval(&host, tab, "clearInterval(loopTimer);true", cx).await?;
    if !baseline {
        eval(&host, tab, "window.maskAtStart=''; const v=document.querySelector('#vp9');v.pause();v.currentTime=0;true", cx).await?;
        for _ in 0..3 {
            pause(cx).await;
        }
        eval(&host, tab, "window.maskAtStart=getComputedStyle(document.querySelector('#vp9')).maskImage;document.querySelector('#vp9').currentTime=1.8;true", cx).await?;
        for _ in 0..3 {
            pause(cx).await;
        }
        ensure!(
            eval(
                &host,
                tab,
                "getComputedStyle(document.querySelector('#vp9')).maskImage!==maskAtStart",
                cx
            )
            .await?
                == true,
            "Alpha mask did not follow seeking / animation"
        );
        eval(
            &host,
            tab,
            "for(const v of document.querySelectorAll('video')){v.pause();v.currentTime=0.5;}true",
            cx,
        )
        .await?;
        for _ in 0..3 {
            pause(cx).await;
        }
        eval(&host, tab, "const duplicate=document.querySelector('#vp9').cloneNode();duplicate.id='duplicate';document.body.append(duplicate);true", cx).await?;
        let mut duplicate_ready = false;
        for _ in 0..30 {
            if eval(
                &host,
                tab,
                "getComputedStyle(document.querySelector('#duplicate')).maskImage!=='none'",
                cx,
            )
            .await?
                == true
            {
                duplicate_ready = true;
                break;
            }
            pause(cx).await;
        }
        ensure!(
            duplicate_ready,
            "Repeated source did not acquire shared alpha masks"
        );
        eval(
            &host,
            tab,
            "document.querySelector('#duplicate').remove();true",
            cx,
        )
        .await?;
    }
    if let Ok(path) = std::env::var("AXOLOTL_VIDEO_SNAPSHOT") {
        host.save_test_snapshot(tab, &path)?;
        for _ in 0..10 {
            pause(cx).await;
        }
    }
    if !baseline {
        eval(
            &host,
            tab,
            "document.querySelector('#vp9').src='ordinary.mp4'; true",
            cx,
        )
        .await?;
        for _ in 0..10 {
            pause(cx).await;
        }
        ensure!(
            eval(
                &host,
                tab,
                "getComputedStyle(document.querySelector('#vp9')).maskImage==='none'",
                cx
            )
            .await?
                == true,
            "Old alpha mask retained after source replacement"
        );
    }
    if !baseline {
        let canvas_url = std::env::var("AXOLOTL_VIDEO_CANVAS_URL")?;
        host.navigate(tab, &canvas_url)?;
        wait_title(&host, tab, "WebM texture regression", cx).await?;
        let expected = if canvas_url.contains("real=1") { 9 } else { 2 };
        let mut ready = false;
        let mut last = serde_json::Value::Null;
        for _ in 0..500 {
            let result = eval(
                &host,
                tab,
                "({checks:window.textureChecks,error:window.textureFailure,changes:window.textureChanges||0})",
                cx,
            )
            .await?;
            ensure!(
                result["error"].is_null(),
                "Texture rendering failed: {result}"
            );
            if let Some(samples) = result["checks"].as_array()
                && samples.len() == expected
                && samples.iter().all(|v| {
                    v["canvas"] == 0
                        && v["gl"] == 0
                        && v["gl2"] == 0
                        && v["detached"] == true
                        && v["opaque"].as_u64().unwrap_or(0) > 100
                })
                && result["changes"].as_u64().unwrap_or(0) > 5
            {
                ready = true;
                break;
            }
            last = result;
            pause(cx).await;
        }
        ensure!(ready, "Detached canvas/WebGL alpha failed: {last}");
        if let Ok(path) = std::env::var("AXOLOTL_VIDEO_SNAPSHOT") {
            host.save_test_snapshot(tab, &format!("{path}.textures.png"))?;
            for _ in 0..10 {
                pause(cx).await;
            }
        }
    }
    let mut checks = vec![if baseline {
        "reproduced native WebM opaque background; ordinary MP4 decodes"
    } else {
        "VP8/VP9 transparent, semitransparent and opaque alpha pixels; frame seeking, repeated sources, MP4 controls, source replacement, detached Canvas2D/WebGL/WebGL2 texture uploads; native snapshots"
    }];
    checks.push(if occluded { "SKIPPED uninterrupted playback/loop timing: native document occluded; WebKit throttles autoplay" } else { "VP8, VP9 and MP4 continue playing across the two-second loop boundary" });
    Ok(checks)
}
