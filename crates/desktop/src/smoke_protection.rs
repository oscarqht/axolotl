//! Real native network + cosmetic checks against a counted loopback server.
use crate::{
    app::BrowserApp,
    smoke::{bar_eval, eval, pause},
};
use anyhow::{Result, ensure};
use gpui::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}/", listener.local_addr()?);
    let hits = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    struct Stop(Arc<AtomicBool>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let _stop = Stop(stop.clone());
    let server_hits = hits.clone();
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut bytes = [0; 8192];
                let n = stream.read(&mut bytes).unwrap_or(0);
                let request = String::from_utf8_lossy(&bytes[..n]);
                let ad = request.starts_with("GET /adserver.js");
                if ad {
                    server_hits.fetch_add(1, Ordering::Relaxed);
                }
                let body = if ad {
                    "window.adReached=true"
                } else {
                    "<!doctype html><title>Protection fixture</title><div class='ad-banner-300x250' id='advert'>advert</div><div id='content'>Article</div><script src='/adserver.js'></script><script>window.fixtureReady=true</script>"
                };
                let kind = if ad {
                    "application/javascript"
                } else {
                    "text/html"
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    });
    let (host, tab, pane) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.active_tab(),
            app.model.focused_pane().id,
        )
    })?;
    host.navigate(tab, &url)?;
    for _ in 0..100 {
        pause(cx).await;
        if host.is_loaded(tab)
            && eval(&host, tab, "Boolean(window.fixtureReady)", cx).await? == true
        {
            break;
        }
    }
    let state=eval(&host,tab,"({ad:!!window.adReached,hidden:getComputedStyle(document.querySelector('#advert')).display==='none',content:getComputedStyle(document.querySelector('#content')).display!=='none'})",cx).await?;
    ensure!(
        state["ad"] == false
            && state["hidden"] == true
            && state["content"] == true
            && hits.load(Ordering::Relaxed) == 0,
        "Native protection failed: {state}, server hits={}",
        hits.load(Ordering::Relaxed)
    );
    let other_url = url.replace("127.0.0.1", "localhost");
    let unrelated = view.update_in(cx, |app, window, cx| {
        let other = app.model.add_tab(&other_url);
        app.model.switch_tab(pane, tab);
        app.host.ensure(other, &other_url, window, cx)?;
        anyhow::Ok(other)
    })??;
    for _ in 0..100 {
        pause(cx).await;
        if host.is_loaded(unrelated) {
            break;
        }
    }
    eval(&host, unrelated, "window.unrelatedMarker=91;true", cx).await?;
    ensure!(
        bar_eval(
            view,
            pane,
            "document.querySelector('#shield').dataset.protectionState",
            cx
        )
        .await?
            == "on",
        "Shield does not show protection on"
    );
    let shield_hit=bar_eval(view,pane,"(()=>{const r=document.querySelector('#shield').getBoundingClientRect();return document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)?.closest('button')?.id})()",cx).await?;
    ensure!(
        shield_hit == "shield",
        "Shield is covered by another control: {shield_hit}"
    );
    // Give the native bar actual document focus before testing keyboard focus.
    // focus_editor focuses its native view and has no input to edit here.
    view.update_in(cx, |app, _, _| app.bars[&pane].focus_editor())??;
    bar_eval(view,pane,"(()=>{const s=document.querySelector('#shield');s.dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));s.focus();return true})()",cx).await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        bar_eval(view, pane, "expanded", cx).await? == false,
        "Shield hover/focus expanded compact chips"
    );
    bar_eval(
        view,
        pane,
        "document.activeElement?.blur();expand(true);true",
        cx,
    )
    .await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    bar_eval(view,pane,"(()=>{document.querySelector('#tabs').dispatchEvent(new PointerEvent('pointerleave',{pointerType:'mouse'}));document.querySelector('#shield').dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));return true})()",cx).await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        bar_eval(view, pane, "expanded", cx).await? == true,
        "Pending collapse moved shield during hover"
    );
    view.update_in(cx, |app, _, _| app.bars[&pane].focus_editor())??;
    bar_eval(
        view,
        pane,
        "document.querySelector('#shield').focus();collapse();true",
        cx,
    )
    .await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    // Background smoke windows may lose OS focus; blur is allowed to collapse.
    let focus_state=bar_eval(view,pane,"({expanded,focused:document.hasFocus(),active:document.activeElement?.id,disabled:document.querySelector('#shield').disabled})",cx).await?;
    ensure!(
        focus_state["focused"] == false || focus_state["expanded"] == true,
        "Focus on shield allowed chips to collapse: {focus_state}"
    );
    #[cfg(target_os = "macos")]
    {
        let center=bar_eval(view,pane,"(()=>{const r=document.querySelector('#shield').getBoundingClientRect();return [r.x+r.width/2,r.y+r.height/2]})()",cx).await?;
        view.update_in(cx, |app, _, _| {
            let bar = &app.bars[&pane];
            let x = bar.bounds.x as f64 + center[0].as_f64().unwrap();
            let y = bar.bounds.y as f64 + center[1].as_f64().unwrap();
            bar.test_mouse(objc2_app_kit::NSEventType::LeftMouseDown, x, y)?;
            bar.test_mouse(objc2_app_kit::NSEventType::LeftMouseUp, x, y)
        })??;
    }

    #[cfg(not(target_os = "macos"))]
    bar_eval(
        view,
        pane,
        "document.querySelector('#shield').click();true",
        cx,
    )
    .await?;
    let mut bypass = false;
    for _ in 0..100 {
        pause(cx).await;
        if eval(&host, tab, "Boolean(window.adReached)", cx).await? == true {
            bypass = true;
            break;
        }
    }
    let diagnostic=eval(&host,tab,"({url:location.href,ad:!!window.adReached,body:document.body.innerText,style:!!document.querySelector('[data-axolotl-protection]')})",cx).await?;
    ensure!(
        eval(&host, unrelated, "window.unrelatedMarker", cx).await? == 91,
        "A site toggle reloaded an unrelated website"
    );
    ensure!(bar_eval(view,pane,"(()=>{const s=document.querySelector('#shield');return s.dataset.protectionState==='off'&&s.getAttribute('aria-pressed')==='false'&&getComputedStyle(s.querySelector('.shield-off-mark')).display!=='none'})()",cx).await?==true,"Shield does not visibly show protection off");
    ensure!(
        bypass && hits.load(Ordering::Relaxed) > 0,
        "Site exception did not allow the native request: {diagnostic}, hits={}",
        hits.load(Ordering::Relaxed)
    );
    ensure!(
        eval(
            &host,
            tab,
            "getComputedStyle(document.querySelector('#advert')).display!=='none'",
            cx
        )
        .await?
            == true,
        "Cosmetics remain active on excepted site"
    );
    bar_eval(
        view,
        pane,
        "document.querySelector('#shield').click();true",
        cx,
    )
    .await?;
    let mut resumed = false;
    for _ in 0..100 {
        pause(cx).await;
        if eval(&host,tab,"Boolean(window.fixtureReady)&&!window.adReached&&getComputedStyle(document.querySelector('#advert')).display==='none'",cx).await?==true {resumed=true;break;}
    }
    ensure!(resumed, "Re-enabling protection failed");
    ensure!(
        eval(&host, unrelated, "window.unrelatedMarker", cx).await? == 91,
        "Re-enabling protection reloaded an unrelated website"
    );
    ensure!(
        bar_eval(
            view,
            pane,
            "document.querySelector('#shield').dataset.protectionState",
            cx
        )
        .await?
            == "on",
        "Shield does not return to protection on"
    );

    view.update_in(cx, |app, window, cx| app.open_settings(false, window, cx))?;
    pause(cx).await;
    pause(cx).await;
    ensure!(crate::smoke_settings::dialog_eval(view,"document.querySelector('#protection-enabled').checked && document.querySelector('#update-filters').textContent==='Update now'",cx).await?==true,"Settings protection controls missing");
    crate::smoke_settings::dialog_eval(
        view,
        "document.querySelector('#protection-enabled').click();true",
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        !host.protection().borrow().policy.enabled,
        "Global checkbox IPC did not apply"
    );
    crate::smoke_settings::dialog_eval(view, "closeButton.click();true", cx).await?;
    let mut global_off = false;
    for _ in 0..100 {
        pause(cx).await;
        if eval(&host,tab,"Boolean(window.adReached)&&getComputedStyle(document.querySelector('#advert')).display!=='none'",cx).await?==true {global_off=true;break;}
    }
    let global_state=eval(&host,tab,"({url:location.href,ad:!!window.adReached,style:!!document.querySelector('[data-axolotl-protection]'),hidden:getComputedStyle(document.querySelector('#advert')).display})",cx).await?;
    ensure!(
        global_off,
        "Global disable did not restore native requests and cosmetics: {global_state}"
    );
    host.protection().borrow_mut().set_enabled(true)?;
    let mut global_on = false;
    for _ in 0..100 {
        pause(cx).await;
        if eval(&host,tab,"Boolean(window.fixtureReady)&&!window.adReached&&getComputedStyle(document.querySelector('#advert')).display==='none'",cx).await?==true {global_on=true;break;}
    }
    ensure!(global_on, "Global enable did not restore protection");

    if std::env::args().any(|a| a == "--smoke-update") {
        eval(&host, tab, "window.updateMarker=91;true", cx).await?;
        view.update_in(cx, |app, window, cx| app.open_settings(false, window, cx))?;
        pause(cx).await;
        pause(cx).await;
        crate::smoke_settings::dialog_eval(
            view,
            "document.querySelector('#update-filters').click();true",
            cx,
        )
        .await?;
        crate::smoke_settings::dialog_eval(view, "closeButton.click();true", cx).await?;
        let mut updated = false;
        for _ in 0..450 {
            pause(cx).await;
            let p = host.protection();
            let p = p.borrow();
            if p.last_updated > 0 && p.native.ready() {
                updated = true;
                break;
            }
            if p.status.contains("failed") {
                anyhow::bail!("{}", p.status);
            }
        }
        ensure!(updated, "Filter update did not complete");
        ensure!(
            eval(&host, tab, "window.updateMarker", cx).await? == 91,
            "Automatic rule refresh reloaded the document"
        );
    }
    if std::env::args().any(|a| a == "--smoke-youtube") {
        host.navigate(tab, "https://www.youtube.com/watch?v=9bZkp7q19f0")?;
        for _ in 0..150 {
            pause(cx).await;
        }
        let bounds=eval(&host,tab,"(()=>{const r=document.querySelector('.ytp-large-play-button,video')?.getBoundingClientRect();return r?[r.x+r.width/2,r.y+r.height/2]:null})()",cx).await?;
        #[cfg(target_os = "macos")]
        if let (Some(x), Some(y)) = (bounds[0].as_f64(), bounds[1].as_f64()) {
            host.test_click_at(tab, x, y, false)?;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = bounds;
        for _ in 0..100 {
            pause(cx).await;
        }
        let result=eval(&host,tab,"({title:document.title,url:location.href,video:[...document.querySelectorAll('video')].map(v=>({time:v.currentTime,paused:v.paused,ready:v.readyState,error:v.error?.code})),ad:!!document.querySelector('.ad-showing'),text:document.body.innerText.slice(0,800),bodyDisplay:getComputedStyle(document.body).display,htmlDisplay:getComputedStyle(document.documentElement).display,html:document.body.innerHTML.slice(0,800),resources:performance.getEntriesByType('resource').slice(-10).map(r=>{const u=new URL(r.name);return u.origin+u.pathname}),diagnostics:window.__axolotlProtectionDiagnostics})",cx).await?;
        println!("AXOLOTL_YOUTUBE {result}");
        host.protection().borrow_mut().set_enabled(false)?;
        for _ in 0..100 {
            pause(cx).await;
        }
        let unprotected_bounds=eval(&host,tab,"(()=>{const r=document.querySelector('.ytp-large-play-button,video')?.getBoundingClientRect();return r?[r.x+r.width/2,r.y+r.height/2]:null})()",cx).await?;
        #[cfg(target_os = "macos")]
        if let (Some(x), Some(y)) = (
            unprotected_bounds[0].as_f64(),
            unprotected_bounds[1].as_f64(),
        ) {
            host.test_click_at(tab, x, y, false)?;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = unprotected_bounds;
        for _ in 0..100 {
            pause(cx).await;
        }
        let unprotected=eval(&host,tab,"({title:document.title,video:[...document.querySelectorAll('video')].map(v=>({time:v.currentTime,paused:v.paused,ready:v.readyState,error:v.error?.code})),ad:!!document.querySelector('.ad-showing'),text:document.body.innerText.slice(0,1000)})",cx).await?;
        println!("AXOLOTL_YOUTUBE_UNPROTECTED {unprotected}");
        anyhow::bail!(
            "YouTube live result recorded; absence of an ad on one visit does not verify required ad blocking"
        );
    }
    Ok(vec![
        "shield receives native hit testing; hover retains compact and expanded states",
        "site toggle preserves unrelated tab state and visibly switches shield on/off",
        "native request blocked before reaching server",
        "ordinary document preserved",
        "maintained cosmetic selector hidden",
        "site exception allows request and cosmetics",
        "re-enable reload restores protection",
        "Settings global checkbox disables network and cosmetics",
        "global re-enable restores protection",
    ])
}
