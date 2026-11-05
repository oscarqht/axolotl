//! Offline native OAuth popup lifecycle regression; no accounts or credentials.
use crate::{
    app::{BrowserApp, TabTarget},
    smoke::{Host, eval, pause, wait_title},
};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};
use workspace_core::{PaneId, TabId};

fn fixture() -> Result<(String, String)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let source = format!("http://127.0.0.1:{port}");
    let provider = format!("http://localhost:{port}");
    let a = source.clone();
    let b = provider.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let a = a.clone();
            let b = b.clone();
            std::thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut request = [0; 8192];
                let count = stream.read(&mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                if path == "/redirect" {
                    let response = format!(
                        "HTTP/1.1 302 Found\r\nLocation: {a}/callback\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = stream.write_all(response.as_bytes());
                    return;
                }
                let a = serde_json::to_string(&a).unwrap();
                let b = serde_json::to_string(&b).unwrap();
                let body = if path == "/source" {
                    format!(
                        r#"<!doctype html><title>OAuth source</title>
<style>button,a{{position:fixed;left:20px;top:20px;display:block;width:220px;height:80px}}a{{top:120px}}</style>
<button onclick="openAuth()">Open sign in</button><a id="ordinary" href={b} target="_blank" rel="noopener">Ordinary new tab</a>
<script>
window.messages=[];window.marker='preserved';window.mode='oauth';document.cookie='axolotlOAuthFixture=shared; path=/; SameSite=Lax';
window.openAuth=()=>{{
 if(mode==='blank'){{window.child=window.open('', 'blank-auth');child.location={b}+'/provider';}}
 else if(mode==='reuse'){{window.reused=window.open({b}+'/provider','oauth-auth');}}
 else{{window.child=window.open({b}+'/provider','oauth-auth');}}
}};
window.addEventListener('message',e=>messages.push({{origin:e.origin,source:e.source===child,data:e.data}}));
document.querySelector('#ordinary').href={b}+'/ordinary';
</script>"#
                    )
                } else if path == "/callback" {
                    format!(
                        r#"<!doctype html><title>OAuth callback</title><script>
opener.postMessage({{kind:'complete',session:document.cookie.includes('axolotlOAuthFixture=shared')}},{a});setTimeout(()=>window.close(),100);
</script>"#
                    )
                } else if path == "/ordinary" {
                    "<!doctype html><title>Ordinary target</title>Ordinary new tab".into()
                } else {
                    format!(
                        r#"<!doctype html><title>OAuth provider</title>
<style>button{{position:fixed;left:20px;top:20px;width:220px;height:80px}}</style>
<button onclick="location.href='/redirect'">Select account</button><script>
window.domDenied=false;try{{void opener.document.body}}catch(e){{domDenied=e.name==='SecurityError'}}
if(opener){{opener.postMessage({{kind:'wrong-target'}},{b});opener.postMessage({{kind:'ready',domDenied}}, {a});}}
</script>"#
                    )
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            });
        }
    });
    Ok((source, provider))
}

async fn select(
    view: &WeakEntity<BrowserApp>,
    pane: PaneId,
    tab: TabId,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(pane, tab), window, cx)
    })?;
    pause(cx).await;
    Ok(())
}

async fn wait_new_tab(
    view: &WeakEntity<BrowserApp>,
    pane: PaneId,
    count: usize,
    cx: &mut AsyncWindowContext,
) -> Result<TabId> {
    for _ in 0..50 {
        pause(cx).await;
        if let Some(tab) = view.update_in(cx, |app, _, _| {
            let pane = app.model.pane(pane).unwrap();
            (pane.tabs.len() == count + 1).then_some(pane.active_tab)
        })? {
            return Ok(tab);
        }
    }
    anyhow::bail!("Native popup did not create exactly one tab")
}

async fn wait_value(
    host: &Host,
    tab: TabId,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    for _ in 0..50 {
        if eval(host, tab, script, cx).await? == true {
            return Ok(());
        }
        pause(cx).await;
    }
    anyhow::bail!("Native popup condition failed: {script}")
}

/// A native popup must keep an Option-click preview's actual opener alive.
async fn preview_source(
    view: &WeakEntity<BrowserApp>,
    host: &Host,
    original: TabId,
    pane: PaneId,
    origin: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    use crate::link_popup::POPUP_TAB;
    select(view, pane, original, cx).await?;
    let before = view.update_in(cx, |app, window, cx| {
        let before = app.model.pane(pane).unwrap().tabs.len();
        app.open_link_popup(original, format!("{origin}/source"), window, cx);
        before
    })?;
    for _ in 0..50 {
        pause(cx).await;
        if view.update_in(cx, |app, _, _| {
            app.link_popup
                .as_ref()
                .is_some_and(|p| p.ready && p.presented)
        })? {
            break;
        }
    }
    wait_title(host, POPUP_TAB, "OAuth source", cx)
        .await
        .context("preview source did not load")?;
    eval(host, POPUP_TAB, "marker='promoted-preview';true", cx).await?;
    host.focus(POPUP_TAB)?;
    host.test_click_at(POPUP_TAB, 40., 40., false)?;
    let mut pair = None;
    for _ in 0..50 {
        pause(cx).await;
        pair = view.update_in(cx, |app, _, _| {
            let p = app.model.pane(pane).unwrap();
            if p.tabs.len() != before + 2 || app.link_popup.is_some() {
                return None;
            }
            let child = p.active_tab;
            app.native_popup_sources
                .get(&child)
                .map(|source| (*source, child))
        })?;
        if pair.is_some() {
            break;
        }
    }
    let (promoted, child) =
        pair.context("preview sign-in did not promote its source and adopt exactly one child")?;
    ensure!(
        promoted != original
            && promoted != POPUP_TAB
            && host.has_view(promoted)
            && !host.has_view(POPUP_TAB),
        "Preview promotion did not transfer its original native view"
    );
    wait_title(host, child, "OAuth provider", cx)
        .await
        .context("preview child did not load provider")?;
    ensure!(
        eval(
            host,
            promoted,
            "marker==='promoted-preview' && child!==null && !child.closed",
            cx
        )
        .await?
            == true,
        "Promoting the preview reloaded its document or lost its child WindowProxy"
    );
    ensure!(
        eval(host, child, "opener!==null && domDenied", cx).await? == true,
        "Preview-source popup lost its native opener or cross-origin restriction"
    );
    wait_value(
        host,
        promoted,
        "messages.some(e=>e.source&&e.data.kind==='ready'&&e.data.domDenied)",
        cx,
    )
    .await
    .context("preview child callback did not reach promoted opener")?;
    // Both members of a pending auth interaction must resist sleep, including
    // a hidden opener; recreating either native page loses their relationship.
    view.update_in(cx, |app, window, cx| {
        app.sleep_tab(promoted, window, cx);
        app.sleep_tab(child, window, cx);
    })?;
    for _ in 0..10 {
        pause(cx).await;
    }
    ensure!(
        view.update_in(cx, |app, _, _| {
            !app.sleep.is_sleeping(promoted)
                && !app.sleep.is_sleeping(child)
                && host.has_view(promoted)
                && host.has_view(child)
        })?,
        "Sleeping an active OAuth pair unloaded its opener or child"
    );
    ensure!(
        eval(
            host,
            promoted,
            "marker==='promoted-preview' && child!==null && !child.closed",
            cx
        )
        .await?
            == true,
        "Sleep attempt changed the promoted source document or child proxy"
    );
    ensure!(
        eval(host, child, "opener!==null && domDenied", cx).await? == true,
        "Sleep attempt detached the popup's native opener"
    );
    // Pinning a live popup preserves its view; script closure must still remove
    // it without stealing keyboard focus from the user's chosen ordinary tab.
    view.update_in(cx, |app, window, cx| app.pin_page(pane, child, window, cx))?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app
            .model
            .pinned_tabs
            .iter()
            .any(|p| p.tab.id == child))?,
        "Popup did not move into pinned tabs"
    );
    select(view, pane, original, cx).await?;
    host.focus(original)?;
    eval(host, child, "window.close();true", cx).await?;
    wait_value(host, promoted, "child.closed", cx)
        .await
        .context("background preview child did not close its WindowProxy")?;
    for _ in 0..50 {
        pause(cx).await;
        if view.update_in(cx, |app, _, _| app.model.tab(child).is_none())? {
            break;
        }
    }
    ensure!(
        view.update_in(cx, |app, _, _| {
            app.model.tab(child).is_none()
                && app.active_tab() == original
                && app.model.pane(pane).unwrap().tabs.len() == before + 1
                && !app.native_popup_sources.contains_key(&child)
        })?,
        "Background popup closure stole selection or retained its tab/source relationship"
    );
    ensure!(
        !host.has_view(child) && host.has_native_focus(original)?,
        "Background popup closure retained its native view or stole native focus"
    );
    ensure!(
        eval(host, promoted, "marker==='promoted-preview'", cx).await? == true,
        "Background popup closure changed the promoted source document"
    );
    Ok(())
}

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, source, pane, count) = view.update_in(cx, |app, _, _| {
        let pane = app.model.focused_pane();
        (app.host.clone(), pane.active_tab, pane.id, pane.tabs.len())
    })?;
    let (origin, provider) = fixture()?;
    host.navigate(source, &format!("{origin}/source"))?;
    wait_title(&host, source, "OAuth source", cx).await?;
    host.focus(source)?;
    host.test_click_at(source, 40., 40., false)?;
    let popup = wait_new_tab(view, pane, count, cx).await?;
    wait_title(&host, popup, "OAuth provider", cx).await?;
    ensure!(
        eval(&host, source, "child !== null", cx).await? == true,
        "window.open returned null instead of the native popup WindowProxy"
    );
    ensure!(
        eval(&host, popup, "opener !== null && domDenied", cx).await? == true,
        "Native popup lost its opener or permitted cross-origin DOM access"
    );
    let expected = serde_json::to_string(&provider)?;
    wait_value(&host, source, &format!("messages.some(e=>e.origin==={expected}&&e.source&&e.data.kind==='ready'&&e.data.domDenied)"), cx).await?;
    ensure!(
        eval(
            &host,
            source,
            "!messages.some(e=>e.data.kind==='wrong-target')",
            cx
        )
        .await?
            == true,
        "postMessage delivered a message with the wrong targetOrigin"
    );
    // Reopening a named native window must reuse its WindowProxy and tab.
    select(view, pane, source, cx).await?;
    eval(&host, source, "mode='reuse';true", cx).await?;
    host.focus(source)?;
    host.test_click_at(source, 40., 40., false)?;
    pause(cx).await;
    ensure!(
        eval(&host, source, "reused===child", cx).await? == true,
        "Named window.open did not reuse its WindowProxy"
    );
    ensure!(
        view.update_in(cx, |app, _, _| app.model.pane(pane).unwrap().tabs.len())? == count + 1,
        "Named popup reuse created a duplicate tab"
    );
    select(view, pane, popup, cx).await?;
    wait_title(&host, popup, "OAuth provider", cx).await?;
    host.focus(popup)?;
    host.test_click_at(popup, 40., 40., false)?;
    wait_value(
        &host,
        source,
        "messages.some(e=>e.source&&e.data.kind==='complete'&&e.data.session) && child.closed",
        cx,
    )
    .await?;
    for _ in 0..50 {
        pause(cx).await;
        if view.update_in(cx, |app, _, _| app.model.tab(popup).is_none())? {
            break;
        }
    }
    ensure!(
        view.update_in(cx, |app, _, _| app.model.tab(popup).is_none()
            && app.active_tab() == source
            && app.model.pane(pane).unwrap().tabs.len() == count)?,
        "window.close did not remove the popup tab and restore source selection"
    );
    for _ in 0..50 {
        if host.has_native_focus(source)? {
            break;
        }
        pause(cx).await;
    }
    ensure!(
        host.has_native_focus(source)?,
        "Closing the popup did not restore native source focus"
    );
    ensure!(
        eval(
            &host,
            source,
            "marker==='preserved' && location.pathname==='/source'",
            cx
        )
        .await?
            == true,
        "OAuth changed the source document"
    );
    // A normal target=_blank noopener link remains independent.
    select(view, pane, source, cx).await?;
    host.focus(source)?;
    host.test_click_at(source, 40., 140., false)?;
    let ordinary = wait_new_tab(view, pane, count, cx)
        .await
        .context("ordinary noopener link")?;
    wait_title(&host, ordinary, "Ordinary target", cx).await?;
    ensure!(
        eval(&host, ordinary, "opener===null", cx).await? == true,
        "An explicit noopener link acquired an opener"
    );
    view.update_in(cx, |app, window, cx| {
        app.command(crate::app::Command::CloseTab, window, cx)
    })?;
    pause(cx).await;
    // OAuth providers often open about:blank and assign its URL afterward.
    select(view, pane, source, cx).await?;
    eval(&host, source, "mode='blank';messages=[];true", cx).await?;
    host.focus(source)?;
    host.test_click_at(source, 40., 40., false)?;
    let blank = wait_new_tab(view, pane, count, cx)
        .await
        .context("blank open then navigate")?;
    wait_title(&host, blank, "OAuth provider", cx).await?;
    ensure!(
        eval(&host, source, "child!==null && !child.closed", cx).await? == true,
        "Opening about:blank then setting location lost its WindowProxy"
    );
    ensure!(
        eval(&host, blank, "opener!==null && domDenied", cx).await? == true,
        "Opening about:blank then setting location lost opener security"
    );
    eval(&host, blank, "location.href='file:///etc/passwd';true", cx).await?;
    pause(cx).await;
    ensure!(
        host.current_url(blank)?.starts_with(&provider),
        "Popup navigation allowed a forbidden file URL"
    );
    // Closing the shell tab must also close the WindowProxy watched by OAuth
    // callers. Merely removing the native view leaves their polling loop alive.
    ensure!(
        eval(&host, source, "!child.closed", cx).await? == true,
        "Popup was already closed before the manual-close check"
    );
    select(view, pane, blank, cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.command(crate::app::Command::CloseTab, window, cx)
    })?;
    wait_value(&host, source, "child.closed", cx).await?;
    ensure!(
        view.update_in(cx, |app, _, _| app.model.tab(blank).is_none())? && !host.has_view(blank),
        "Manual popup closure retained its workspace tab or native view"
    );
    ensure!(
        eval(
            &host,
            source,
            "marker==='preserved' && location.pathname==='/source'",
            cx
        )
        .await?
            == true,
        "Manual popup closure changed the source document"
    );
    preview_source(view, &host, source, pane, &origin, cx).await?;
    Ok(vec![
        "native window.open returns a linked WindowProxy in exactly one Axolotl tab, including named reuse and about:blank navigation",
        "cross-origin postMessage preserves event.origin/source and targetOrigin checks while same-origin DOM access remains blocked",
        "provider account selection redirects to callback, messages the opener, closes the native popup tab and restores its source",
        "ordinary target=_blank noopener links remain independent and popup file navigation stays blocked",
        "manually closing a popup tab closes its WindowProxy and removes its native view",
        "an authenticating preview promotes its existing native opener; opener and child resist sleep, and background popup closure preserves the selected tab and native focus",
    ])
}
