//! Reload must survive an unavailable JavaScript shortcut bridge and follow native focus.
use crate::{
    app::Command,
    smoke::{Host, eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use objc2_app_kit::NSEventModifierFlags as Flags;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use workspace_core::{Axis, TabId};

fn fixture() -> Result<(String, Arc<AtomicUsize>, Arc<AtomicBool>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/page", listener.local_addr()?);
    let pages = Arc::new(AtomicUsize::new(0));
    let busy = Arc::new(AtomicBool::new(false));
    let (count, started) = (pages.clone(), busy.clone());
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = [0; 4096];
            let Ok(n) = stream.read(&mut request) else {
                continue;
            };
            let path = String::from_utf8_lossy(&request[..n]);
            let path = path.split_whitespace().nth(1).unwrap_or("");
            let body = match path {
                "/busy" => {
                    started.store(true, Ordering::Relaxed);
                    "ok"
                }
                "/page" => {
                    count.fetch_add(1, Ordering::Relaxed);
                    "<!doctype html><title>Reload fixture</title><input value='preserved until reload'>"
                }
                _ => "",
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Ok((url, pages, busy))
}

async fn busy_reload(
    host: &Host,
    tab: TabId,
    pages: &AtomicUsize,
    busy: &AtomicBool,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    let previous = pages.load(Ordering::Relaxed);
    host.focus(tab)?;
    // The callback responds before a finite four-second page event-loop stall.
    // The local HTTP server observes reload without evaluating the busy page.
    eval(host, tab, "setTimeout(()=>{fetch('/busy');const until=Date.now()+4000;while(Date.now()<until){}},100);true", cx).await?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while !busy.load(Ordering::Relaxed) && Instant::now() < deadline {
        pause(cx).await;
    }
    ensure!(
        busy.load(Ordering::Relaxed),
        "Busy-page fixture did not start"
    );
    host.test_key(tab, 15, "r", Flags::Command, false)?;
    let requested = Instant::now();
    let deadline = requested + Duration::from_secs(8);
    while pages.load(Ordering::Relaxed) == previous && Instant::now() < deadline {
        pause(cx).await;
    }
    ensure!(
        pages.load(Ordering::Relaxed) > previous,
        "Cmd+R did not recover after a finite page JavaScript stall"
    );
    eprintln!(
        "Reload: native Cmd+R navigation after {:?} during a four-second JavaScript stall",
        requested.elapsed()
    );
    wait_title(host, tab, "Reload fixture", cx).await
}

async fn identity(
    host: &Host,
    tab: TabId,
    cx: &mut AsyncWindowContext,
) -> Result<serde_json::Value> {
    eval(
        host,
        tab,
        "window.reloadIdentity ||= Math.random().toString();reloadIdentity",
        cx,
    )
    .await
}

async fn reloaded(
    host: &Host,
    tab: TabId,
    previous: &serde_json::Value,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    for _ in 0..30 {
        pause(cx).await;
        if host.is_loaded(tab) && identity(host, tab, cx).await? != *previous {
            return Ok(());
        }
    }
    anyhow::bail!("Reload did not replace document in tab {tab}")
}

async fn press(
    host: &Host,
    tab: TabId,
    pages: &AtomicUsize,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    host.focus(tab)?;
    let previous = identity(host, tab, cx).await?;
    let requests = pages.load(Ordering::Relaxed);
    // The shipped input bridge exits early when this page helper consumes a
    // key. Reload must still be reserved natively before any JavaScript runs.
    eval(host, tab, "window.__axolotlFindKey=()=>true;true", cx).await?;
    host.test_key(tab, 15, "r", Flags::Command, false)?;
    reloaded(host, tab, &previous, cx).await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        pages.load(Ordering::Relaxed) == requests + 1,
        "Cmd+R requested more than one reload"
    );
    Ok(())
}

pub(crate) async fn run(
    view: &WeakEntity<crate::app::BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, first) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    let (url, pages, busy) = fixture()?;
    host.navigate(first, &url)?;
    wait_title(&host, first, "Reload fixture", cx).await?;
    busy_reload(&host, first, &pages, &busy, cx).await?;
    press(&host, first, &pages, cx).await?;

    view.update_in(cx, |app, window, cx| {
        app.command(Command::Split(Axis::Horizontal), window, cx)
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    let second = view.update_in(cx, |app, _, _| app.active_tab())?;
    host.navigate(second, &url)?;
    wait_title(&host, second, "Reload fixture", cx).await?;
    let sibling = identity(&host, second, cx).await?;
    // Native focus changes before the workspace's queued focus notification.
    host.focus(first)?;
    let previous = identity(&host, first, cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.command(Command::Reload, window, cx)
    })?;
    reloaded(&host, first, &previous, cx).await?;
    ensure!(
        identity(&host, second, cx).await? == sibling,
        "Reload changed sibling split"
    );

    view.update_in(cx, |app, window, cx| {
        let pane = app
            .model
            .active_space()
            .panes
            .iter()
            .find(|p| p.active_tab == first)
            .unwrap()
            .id;
        app.pin_page(pane, first, window, cx);
        app.toggle_pin(first, window, cx);
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    press(&host, first, &pages, cx).await?;
    ensure!(
        identity(&host, second, cx).await? == sibling,
        "Pin reload changed underlying page"
    );
    view.update_in(cx, |app, window, cx| {
        app.dismiss_pin(window, cx);
        app.open_link_popup(second, url, window, cx);
    })?;
    let popup = crate::link_popup::POPUP_TAB;
    wait_title(&host, popup, "Reload fixture", cx).await?;
    press(&host, popup, &pages, cx).await?;
    ensure!(
        identity(&host, second, cx).await? == sibling,
        "Preview reload changed underlying page"
    );
    view.update_in(cx, |app, window, cx| {
        app.close_link_popup(window, cx);
        app.command(Command::Settings, window, cx);
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    let requests = pages.load(Ordering::Relaxed);
    host.test_key(second, 15, "r", Flags::Command, false)?;
    for _ in 0..5 {
        pause(cx).await;
    }
    ensure!(
        pages.load(Ordering::Relaxed) == requests,
        "Settings reloaded a background document"
    );
    Ok(vec![
        "native Cmd+R recovers after a finite page JavaScript stall",
        "native Cmd+R bypasses page JavaScript shortcut handling",
        "reload follows focused split, pin and preview without reloading siblings",
        "one native reload per Cmd+R; Settings does not reload background pages",
    ])
}
