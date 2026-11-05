//! Cross-origin native playback, frame isolation/return, and source lifecycle.
use crate::{
    app::{BrowserApp, TabAction, TabTarget},
    smoke::{Host, eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
};
use workspace_core::TabId;

fn fixture() -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let top = format!(
        r#"<!doctype html><title>Frame PiP Top</title>
<style id="original-style">body{{margin:0;background:#eed}}#shell{{padding:12px}}iframe{{display:block;width:420px;height:260px;border:0}}video{{width:100px;height:56px}}</style>
<div id="shell" data-axolotl-pip="saved-shell"><video id="small" src="/pip.mp4" autoplay loop playsinline></video><iframe id="embedded" allow="autoplay" src="http://localhost:{port}/child"></iframe></div>
<script>
const frame=document.querySelector('#embedded'),small=document.querySelector('#small');
small.volume=0.08;small.play().catch(e=>window.fixtureError=String(e));
window.fixtureChildState=null;
addEventListener('message',event=>{{if(event.source===frame.contentWindow&&event.data?.pipFixture)window.fixtureChildState=event.data.state}});
window.childCommand=command=>frame.contentWindow.postMessage({{pipFixtureCommand:command}},'*');
</script>"#
    );
    let child = r#"<!doctype html><title>Frame PiP Child</title>
<style id="original-style">body{margin:0;background:#123}#child-shell{padding:4px}video{display:block;width:400px;height:225px}</style>
<div id="child-shell" data-axolotl-pip="saved-child"><video id="large" data-axolotl-pip="saved-video" src="/pip.mp4" autoplay loop playsinline></video></div>
<script>
const video=document.querySelector('#large');video.volume=0.08;video.play().catch(e=>window.fixtureError=String(e));
const report=()=>parent.postMessage({pipFixture:true,state:{playing:!video.paused,ended:video.ended,ready:video.readyState,audio:video.webkitAudioDecodedByteCount||0,tracks:video.audioTracks?.length||0,time:video.currentTime,controls:video.controls,selected:video.getAttribute('data-axolotl-pip')==='target',restored:document.querySelectorAll('[data-axolotl-pip]').length===2&&document.querySelector('#child-shell').getAttribute('data-axolotl-pip')==='saved-child'&&video.getAttribute('data-axolotl-pip')==='saved-video'&&!video.controls&&!!document.querySelector('#original-style')&&!Array.from(document.querySelectorAll('style')).some(s=>s.textContent.includes('[data-axolotl-pip=')),error:window.fixtureError||null}},'*');
addEventListener('message',event=>{if(event.source!==parent)return;if(event.data?.pipFixtureCommand==='end'){video.loop=false;video.currentTime=Math.max(0,video.duration-0.05);video.play()}report()});
setInterval(report,100);report();
</script>"#;
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut request = [0; 8192];
            let Ok(size) = stream.read(&mut request) else {
                continue;
            };
            let request = String::from_utf8_lossy(&request[..size]);
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            let (content_type, body): (&str, &[u8]) = match path {
                "/top" => ("text/html", top.as_bytes()),
                "/child" => ("text/html", child.as_bytes()),
                "/pip.mp4" => ("video/mp4", include_bytes!("../assets/fixtures/pip.mp4")),
                _ => ("text/html", b"<title>Frame PiP Unloaded</title>Unloaded"),
            };
            let range = request.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if !name.eq_ignore_ascii_case("range") {
                    return None;
                }
                let (start, end) = value.trim().strip_prefix("bytes=")?.split_once('-')?;
                let start = start.parse::<usize>().ok()?;
                let end = end.parse::<usize>().ok().unwrap_or(body.len() - 1);
                (start < body.len() && end >= start).then_some((start, end.min(body.len() - 1)))
            });
            let (status, headers, bytes) = if let Some((start, end)) = range {
                (
                    "206 Partial Content",
                    format!("Content-Range: bytes {start}-{end}/{}\r\n", body.len()),
                    &body[start..=end],
                )
            } else {
                ("200 OK", String::new(), body)
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nAccept-Ranges: bytes\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            );
            let _ = stream.write_all(bytes);
        }
    });
    Ok(format!("http://127.0.0.1:{port}"))
}

async fn settled(
    view: &WeakEntity<BrowserApp>,
    tab: Option<TabId>,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    let mut stable = 0;
    for _ in 0..40 {
        pause(cx).await;
        let state = view.update_in(cx, |app, _, cx| app.host.picture_in_picture_state(cx))?;
        if state["tab"].as_u64() == tab {
            stable += 1;
            if tab.is_some() || stable >= 3 {
                if tab.is_some() {
                    ensure!(
                        state["native"]["visible"] == true,
                        "Frame PiP not visible: {state}"
                    );
                }
                return Ok(());
            }
        } else {
            stable = 0;
        }
    }
    anyhow::bail!("Cross-origin PiP did not settle to {tab:?}")
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

async fn child_state(
    host: &Host,
    tab: TabId,
    expected: &str,
    cx: &mut AsyncWindowContext,
) -> Result<serde_json::Value> {
    let mut state = serde_json::Value::Null;
    for _ in 0..50 {
        pause(cx).await;
        state = eval(host, tab, "window.fixtureChildState", cx).await?;
        let matches = match expected {
            "playing" => {
                state["playing"] == true
                    && (state["audio"].as_u64().unwrap_or(0) > 0
                        || state["tracks"].as_u64().unwrap_or(0) > 0)
            }
            "selected" => state["selected"] == true && state["controls"] == true,
            "restored" => state["restored"] == true,
            "ended" => state["ended"] == true,
            _ => false,
        };
        if matches {
            return Ok(state);
        }
    }
    anyhow::bail!("Cross-origin fixture did not become {expected}: {state}")
}

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let base = fixture()?;
    let (host, source, pane) = view.update_in(cx, |app, window, cx| {
        let source = app.model.add_tab(&format!("{base}/top"));
        app.sync_address(window, cx);
        cx.notify();
        (app.host.clone(), source, app.model.focused_pane().id)
    })?;
    host.set_automatic_picture_in_picture(true)?;
    wait_title(&host, source, "Frame PiP Top", cx).await?;
    child_state(&host, source, "playing", cx).await?;
    let hidden_by = view.update_in(cx, |app, window, cx| {
        let tab = app.model.add_tab("about:blank");
        app.sync_address(window, cx);
        cx.notify();
        tab
    })?;
    settled(view, Some(source), cx).await?;
    ensure!(eval(&host,source,"document.querySelector('#embedded').getAttribute('data-axolotl-pip')==='target' && document.querySelector('#small').getAttribute('data-axolotl-pip')!=='target'",cx).await? == true, "Cross-origin larger video lost to top-level small video");
    let before = child_state(&host, source, "selected", cx).await?;
    let mut after = before.clone();
    for _ in 0..20 {
        pause(cx).await;
        after = child_state(&host, source, "selected", cx).await?;
        if (after["time"].as_f64().unwrap() - before["time"].as_f64().unwrap()).abs() > 0.25 {
            break;
        }
    }
    ensure!(
        (after["time"].as_f64().unwrap() - before["time"].as_f64().unwrap()).abs() > 0.25,
        "Cross-origin PiP playback stopped: before={before}, after={after}"
    );
    select(view, pane, source, cx).await?;
    settled(view, None, cx).await?;
    child_state(&host, source, "restored", cx).await?;
    ensure!(eval(&host,source,"document.querySelectorAll('[data-axolotl-pip]').length===1 && document.querySelector('#shell').getAttribute('data-axolotl-pip')==='saved-shell' && !!document.querySelector('#original-style')&&!Array.from(document.querySelectorAll('style')).some(s=>s.textContent.includes('[data-axolotl-pip=')) && !document.querySelector('#small').controls",cx).await? == true, "Frame return did not restore all ancestor attributes/styles/controls");

    // Navigation must tear down the source presentation without a visible revisit.
    select(view, pane, hidden_by, cx).await?;
    settled(view, Some(source), cx).await?;
    host.navigate(source, &format!("{base}/blank"))?;
    settled(view, None, cx).await?;
    select(view, pane, source, cx).await?;
    wait_title(&host, source, "Frame PiP Unloaded", cx).await?;
    host.navigate(source, &format!("{base}/top"))?;
    wait_title(&host, source, "Frame PiP Top", cx).await?;
    child_state(&host, source, "playing", cx).await?;

    // An ended child video closes PiP even though its page remains alive.
    select(view, pane, hidden_by, cx).await?;
    settled(view, Some(source), cx).await?;
    eval(&host, source, "childCommand('end');true", cx).await?;
    child_state(&host, source, "ended", cx).await?;
    settled(view, None, cx).await?;
    select(view, pane, source, cx).await?;
    host.navigate(source, &format!("{base}/top"))?;
    wait_title(&host, source, "Frame PiP Top", cx).await?;
    child_state(&host, source, "playing", cx).await?;

    select(view, pane, hidden_by, cx).await?;
    settled(view, Some(source), cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.tab_action(TabTarget::Page(pane, source), TabAction::Close, window, cx)
    })?;
    settled(view, None, cx).await?;
    ensure!(
        view.update_in(cx, |app, _, _| app.model.tab(source).is_none())?,
        "Source tab did not close"
    );
    Ok(vec![
        "cross-origin audible embedded video beats smaller top-level video and keeps playing",
        "frame return restores every ancestor attribute, injected style, and video controls",
        "source navigation, child video end, and source tab close clean up PiP",
    ])
}
