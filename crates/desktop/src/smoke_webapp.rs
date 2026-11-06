//! Native background scheduling and sleep safety with an isolated HTTP/SSE app.
use crate::{
    app::{BrowserApp, TabTarget},
    smoke::{eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

type Heartbeats = Arc<Mutex<Vec<Instant>>>;

fn fixture() -> Result<(String, Heartbeats)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", listener.local_addr()?);
    let heartbeats = Arc::new(Mutex::new(Vec::new()));
    let received = heartbeats.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let received = received.clone();
            std::thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let mut request = Vec::new();
                let mut chunk = [0; 1024];
                while !request.ends_with(b"\r\n\r\n") && request.len() < 32768 {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => request.extend_from_slice(&chunk[..n]),
                    }
                }
                if request.starts_with(b"GET /heartbeat ") {
                    received.lock().unwrap().push(Instant::now());
                    let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n");
                } else if request.starts_with(b"GET /events ") {
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n");
                    for count in 1..=4000 {
                        if write!(stream, "data: {count}\n\n").is_err() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                } else {
                    let body = r#"<!doctype html><meta charset="utf-8"><title>Webapp reliability fixture</title>
<style>body{font:20px system-ui;padding:36px;background:#eaf7f3;color:#163b32}input,button{font:inherit;padding:12px}output{display:block;margin:24px 0}</style>
<h1>Webapp reliability fixture</h1><p>Native WebKit · isolated local test</p>
<form onsubmit="event.preventDefault()"><input aria-label="Unsaved draft" value="draft"><button>Submit without saving</button></form>
<output id="status">Connecting…</output><script>
window.documentIdentity=Math.random();window.messages=0;window.ticks=0;
setInterval(()=>ticks++,100);
setInterval(()=>fetch('/heartbeat').catch(()=>{}),1000);
window.connection=new EventSource('/events');
connection.onmessage=()=>{messages++;document.querySelector('#status').textContent=`Received ${messages} live messages · ${ticks} timer ticks`};
</script>"#;
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
            });
        }
    });
    Ok((origin, heartbeats))
}

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, first, pane) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.active_tab(),
            app.model.focused_pane().id,
        )
    })?;
    ensure!(
        host.test_background_policy(first)? == 2,
        "Retained content still permits native WebKit background suspension"
    );
    let (origin, heartbeats) = fixture()?;
    host.navigate(first, &origin)?;
    wait_title(&host, first, "Webapp reliability fixture", cx).await?;
    for _ in 0..20 {
        if eval(&host, first, "messages>0", cx).await? == true {
            break;
        }
        pause(cx).await;
    }
    ensure!(
        eval(&host, first, "messages>0 && __axolotlSleepBusy()", cx).await? == true,
        "Live EventSource missing from sleep activity"
    );
    let identity = eval(&host, first, "documentIdentity", cx).await?;
    view.update_in(cx, |app, _, cx| {
        app.toggle_keep_alive(first);
        app.model.add_tab("about:blank");
        cx.notify();
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    let seconds = std::env::args()
        .find_map(|arg| {
            arg.strip_prefix("--webapp-background-seconds=")
                .and_then(|s| s.parse::<u64>().ok())
        })
        .unwrap_or(5);
    let start = Instant::now();
    let mut next_report = Duration::from_secs(10);
    while start.elapsed() < Duration::from_secs(seconds) {
        view.update_in(cx, |app, _, _| app.sleep.age(first))?;
        pause(cx).await;
        if start.elapsed() >= next_report {
            let received = heartbeats.lock().unwrap();
            let recent: Vec<_> = received.iter().filter(|time| **time >= start).collect();
            let gap_ms = recent
                .windows(2)
                .map(|pair| pair[1].duration_since(*pair[0]).as_millis())
                .max()
                .unwrap_or(0);
            println!(
                "AXOLOTL_WEBAPP_PROGRESS elapsed_s={} heartbeats={} last_age_ms={} max_gap_ms={gap_ms}",
                start.elapsed().as_secs(),
                recent.len(),
                recent
                    .last()
                    .map(|time| time.elapsed().as_millis())
                    .unwrap_or(0)
            );
            next_report += Duration::from_secs(10);
        }
    }
    // Read server-side evidence before evaluating the hidden page: evaluation
    // itself could wake a suspended engine and make buffered SSE look healthy.
    {
        let received = heartbeats.lock().unwrap();
        let recent: Vec<_> = received.iter().filter(|time| **time >= start).collect();
        let gap_ms = recent
            .windows(2)
            .map(|pair| pair[1].duration_since(*pair[0]).as_millis())
            .max()
            .unwrap_or(0);
        println!(
            "AXOLOTL_WEBAPP_HEARTBEATS count={} last_age_ms={} max_gap_ms={gap_ms}",
            recent.len(),
            recent
                .last()
                .map(|time| time.elapsed().as_millis())
                .unwrap_or(0)
        );
        ensure!(
            // WebKit coalesces the one-second interval to roughly two seconds
            // when hidden. Require continuity, not foreground timer cadence.
            recent.len() as u64 >= seconds / 3,
            "Hidden page sent too few background heartbeats"
        );
        ensure!(
            recent
                .last()
                .is_some_and(|last| last.elapsed() < Duration::from_secs(3)),
            "Hidden page heartbeat stale before evaluation"
        );
        ensure!(
            recent
                .windows(2)
                .all(|pair| pair[1].duration_since(*pair[0]) < Duration::from_secs(3)),
            "Hidden page heartbeat gap exceeded three seconds"
        );
    }
    ensure!(
        !host.is_visible(first) && host.has_view(first),
        "Hidden Keep Alive page was unloaded"
    );
    ensure!(
        eval(&host, first, "documentIdentity", cx).await? == identity,
        "Background check replaced the document"
    );
    let messages = eval(&host, first, "messages", cx).await?;
    let ticks = eval(&host, first, "ticks", cx).await?;
    ensure!(
        messages.as_u64().unwrap_or(0) >= seconds * 5 && ticks.as_u64().unwrap_or(0) >= seconds / 2,
        "Hidden page stopped receiving events/timers: {messages} messages, {ticks} ticks in {seconds}s"
    );
    // Live connections protect ordinary tabs too, after Keep Alive is disabled.
    view.update_in(cx, |app, _, _| {
        app.toggle_keep_alive(first);
        app.sleep.retry(first);
    })?;
    for _ in 0..10 {
        pause(cx).await;
    }
    ensure!(host.has_view(first), "Auto sleep discarded live connection");
    // Once explicitly closed, the ordinary idle page may sleep as before.
    eval(&host, first, "connection.close();true", cx).await?;
    view.update_in(cx, |app, _, _| app.sleep.retry(first))?;
    let mut slept = false;
    for _ in 0..35 {
        pause(cx).await;
        if !host.has_view(first) {
            slept = true;
            break;
        }
    }
    ensure!(slept, "Idle closed-connection page no longer sleeps");
    view.update_in(cx, |app, window, cx| {
        app.select_target(TabTarget::Page(pane, first), window, cx)
    })?;
    wait_title(&host, first, "Webapp reliability fixture", cx).await?;
    host.focus(first)?;
    eval(
        &host,
        first,
        "connection.close();document.querySelector('input').focus();true",
        cx,
    )
    .await?;
    host.test_key(
        first,
        0,
        "a",
        objc2_app_kit::NSEventModifierFlags::empty(),
        false,
    )?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        eval(&host, first, "__axolotlSleepBusy()", cx).await? == true,
        "Trusted edit did not protect draft"
    );
    eval(
        &host,
        first,
        "document.querySelector('form').requestSubmit();true",
        cx,
    )
    .await?;
    ensure!(
        eval(&host, first, "__axolotlSleepBusy()", cx).await? == true,
        "Prevented submit discarded unsaved-edit protection"
    );
    if let Some(path) =
        std::env::args().find_map(|a| a.strip_prefix("--webapp-snapshot=").map(str::to_owned))
    {
        let previous_snapshot = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let status = serde_json::to_string(&format!(
            "Passed: {seconds}s hidden, {messages} live messages, {ticks} timer ticks. Unsaved submit protected."
        ))?;
        eval(
            &host,
            first,
            &format!("document.querySelector('#status').textContent={status};true"),
            cx,
        )
        .await?;
        host.save_test_snapshot(first, &path)?;
        for _ in 0..20 {
            pause(cx).await;
            if std::fs::metadata(&path)
                .is_ok_and(|m| m.len() > 0 && m.modified().ok() != previous_snapshot)
            {
                break;
            }
        }
        ensure!(
            std::fs::metadata(&path)
                .is_ok_and(|m| m.len() > 0 && m.modified().ok() != previous_snapshot),
            "Native snapshot was not written"
        );
    }
    println!("AXOLOTL_WEBAPP background_seconds={seconds} messages={messages} timer_ticks={ticks}");
    // Opt-in, read-only reachability/auth-gate checks. Never log page contents,
    // private board names, messages, credentials, or authenticated URLs.
    if std::env::args().any(|a| a == "--webapp-live-check") {
        for url in ["https://seatalkweb.com", "https://trello.com"] {
            host.navigate(first, url)?;
            for _ in 0..100 {
                pause(cx).await;
                if host.is_loaded(first) {
                    break;
                }
            }
            let result = eval(&host, first, "({host:location.hostname,hasPasswordField:!!document.querySelector('input[type=password]'),hasLoginLink:!!document.querySelector('a[href*=login],a[href*=signin]'),hasLoginText:/log in|sign in|qr code/i.test(document.body?.innerText||''),hasAppRoot:!!document.querySelector('#app,#root,#board')})", cx).await?;
            println!("AXOLOTL_WEBAPP_LIVE target={url} checks={result}");
        }
    }
    Ok(vec![
        "native inactive scheduling disabled",
        "hidden Keep Alive page receives SSE and timers without document replacement",
        "live connection prevents auto sleep until closed",
        "trusted unsaved edit survives prevented submit",
    ])
}
