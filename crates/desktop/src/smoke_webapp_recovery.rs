//! Isolated native warning/cancel/accept and page recreation without a JavaScript response.
use crate::{
    app::BrowserApp,
    smoke::{eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use objc2::{MainThreadMarker, Message, rc::Retained};
use objc2_app_kit::{NSApplication, NSButton, NSView};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn fixture() -> Result<(String, Arc<AtomicBool>, Arc<AtomicUsize>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/page", listener.local_addr()?);
    let busy = Arc::new(AtomicBool::new(false));
    let started = busy.clone();
    let pages = Arc::new(AtomicUsize::new(0));
    let count = pages.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = [0; 4096];
            let Ok(n) = stream.read(&mut request) else {
                continue;
            };
            let request = String::from_utf8_lossy(&request[..n]);
            let path = request.split_whitespace().nth(1).unwrap_or("");
            if path == "/page" {
                count.fetch_add(1, Ordering::Relaxed);
            }
            let body = if path == "/busy" {
                started.store(true, Ordering::Relaxed);
                "ok"
            } else {
                "<!doctype html><meta charset='utf-8'><title>Recovery fixture</title><input value='unsaved edit'><script>window.recoveryIdentity=Math.random().toString()</script>"
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Ok((url, busy, pages))
}

fn find_button(view: &NSView, label: &str) -> Option<Retained<NSButton>> {
    if let Some(button) = view.downcast_ref::<NSButton>()
        && button.title().to_string() == label
    {
        return Some(button.retain());
    }
    for child in view.subviews() {
        if let Some(button) = find_button(&child, label) {
            return Some(button);
        }
    }
    None
}

async fn answer(label: &str, cx: &mut AsyncWindowContext) -> Result<()> {
    for _ in 0..25 {
        let button = cx.update(|_, _| {
            let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
            app.windows().iter().find_map(|window| {
                if !window.isVisible() {
                    return None;
                }
                window
                    .contentView()
                    .and_then(|root| find_button(&root, label))
            })
        })?;
        if let Some(button) = button {
            // Invoke the actual AppKit alert button outside any GPUI update borrow.
            unsafe {
                button.performClick(None);
            }
            pause(cx).await;
            return Ok(());
        }
        pause(cx).await;
    }
    anyhow::bail!("Native recovery warning button {label} did not appear")
}

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, tab, pane) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.active_tab(),
            app.model.focused_pane().id,
        )
    })?;
    let (url, busy, pages) = fixture()?;
    host.navigate(tab, &url)?;
    wait_title(&host, tab, "Recovery fixture", cx).await?;
    view.update_in(cx, |app, _, _| app.toggle_keep_alive(tab))?;
    let identity = eval(&host, tab, "document.cookie='recovery=keep; path=/';localStorage.recovery='keep';document.querySelector('input').value='keep this edit';window.recoveryIdentity", cx).await?;
    // Opening a warning must not replace the document or discard edits.
    view.update_in(cx, |app, window, cx| {
        app.confirm_tab_recovery(tab, window, cx)
    })?;
    pause(cx).await;
    // A reserved shortcut must not reload behind its own discard warning.
    host.test_key(
        tab,
        15,
        "r",
        objc2_app_kit::NSEventModifierFlags::Command,
        false,
    )?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        eval(&host, tab, "recoveryIdentity", cx).await? == identity,
        "Prompt recreated page before confirmation"
    );
    answer("Cancel", cx).await?;
    ensure!(
        eval(&host, tab, "recoveryIdentity", cx).await? == identity
            && eval(&host, tab, "document.querySelector('input').value", cx).await?
                == "keep this edit",
        "Cancel discarded page state"
    );
    ensure!(
        !view.update_in(cx, |app, _, _| app.recovery_prompt)?,
        "Cancel left warning pending"
    );

    // Navigating while confirmation is pending invalidates its target.
    let stale = view.update_in(cx, |app, _, _| app.test_recovery_target(tab))??;
    view.update_in(cx, |app, _, _| {
        app.model.tab_mut(tab).unwrap().url = format!("{url}?changed")
    })?;
    view.update_in(cx, |app, window, cx| {
        ensure!(
            app.recreate_tab(&stale, window, cx).is_err(),
            "Stale confirmation recreated a navigated page"
        );
        Ok::<_, anyhow::Error>(())
    })??;
    view.update_in(cx, |app, _, _| {
        app.model.tab_mut(tab).unwrap().url = url.clone()
    })?;
    ensure!(
        eval(&host, tab, "recoveryIdentity", cx).await? == identity,
        "Stale confirmation discarded page"
    );

    #[cfg(target_os = "macos")]
    view.update_in(cx, |app, _, _| {
        app.native_popup_sources.insert(u64::MAX - 17, tab);
        ensure!(
            app.test_recovery_target(tab).is_err(),
            "Recovery allowed a live popup opener"
        );
        app.native_popup_sources.clear();
        Ok::<_, anyhow::Error>(())
    })??;

    // Prove a quiet page is never recreated automatically while a short block runs.
    view.update_in(cx, |app, window, cx| {
        app.confirm_tab_recovery(tab, window, cx)
    })?;
    eval(&host, tab, "setTimeout(()=>{fetch('/busy');const end=Date.now()+10000;while(Date.now()<end){}},100);true", cx).await?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while !busy.load(Ordering::Relaxed) && Instant::now() < deadline {
        pause(cx).await;
    }
    ensure!(
        busy.load(Ordering::Relaxed),
        "Busy recovery fixture did not start"
    );
    for _ in 0..2 {
        pause(cx).await;
    }
    ensure!(
        pages.load(Ordering::Relaxed) == 1,
        "Page was automatically recreated without confirmation"
    );
    let accepted = Instant::now();
    answer("Recreate Tab", cx).await?;
    wait_title(&host, tab, "Recovery fixture", cx).await?;
    ensure!(
        eval(&host, tab, "recoveryIdentity", cx).await? != identity,
        "Confirmed recovery kept the blocked document"
    );
    ensure!(
        eval(
            &host,
            tab,
            "document.cookie.includes('recovery=keep')&&localStorage.recovery==='keep'",
            cx
        )
        .await?
            == true,
        "Recovery discarded website session storage"
    );
    ensure!(
        accepted.elapsed() < Duration::from_secs(3),
        "Fresh native page waited for the old ten-second JavaScript stall"
    );
    ensure!(
        view.update_in(cx, |app, _, _| app.sleep.keep_alive(tab)
            && app.model.tab(tab).is_some())?,
        "Recovery changed the tab identity or Keep Alive choice"
    );
    eprintln!(
        "Recovery: recreated blocked page in {:?}",
        accepted.elapsed()
    );

    let active_identity = eval(&host, tab, "recoveryIdentity", cx).await?;
    let background = view.update_in(cx, |app, window, cx| {
        let background = app.model.add_tab(&url);
        app.model.switch_tab(pane, tab);
        app.host.ensure(background, &url, window, cx)?;
        Ok::<_, anyhow::Error>(background)
    })??;
    wait_title(&host, background, "Recovery fixture", cx).await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    host.focus(tab)?;
    let target = view.update_in(cx, |app, _, _| app.test_recovery_target(background))??;
    view.update_in(cx, |app, window, cx| app.recreate_tab(&target, window, cx))??;
    wait_title(&host, background, "Recovery fixture", cx).await?;
    ensure!(
        view.update_in(cx, |app, _, _| app.active_tab() == tab)?
            && host.focused_page() == Some(tab)
            && eval(&host, tab, "recoveryIdentity", cx).await? == active_identity,
        "Background recovery changed foreground tab, focus or document"
    );

    // A closed target cannot be recovered by a previously captured confirmation.
    let closed = view.update_in(cx, |app, _, _| app.test_recovery_target(tab))??;
    view.update_in(cx, |app, window, cx| {
        app.model.close_tab(pane, tab);
        ensure!(
            app.recreate_tab(&closed, window, cx).is_err(),
            "Closed target was recreated"
        );
        Ok::<_, anyhow::Error>(())
    })??;
    Ok(vec![
        "native recovery warning Cancel preserves page identity and edits",
        "explicit confirmed recreation replaces blocked page and preserves cookies/local storage",
        "stale or closed confirmations and live auth popup pairs cannot recreate pages",
        "background recovery preserves foreground tab, native focus and document",
    ])
}
