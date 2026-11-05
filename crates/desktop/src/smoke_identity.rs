//! Offline regression for the native HTTP and JavaScript browser identities.
use crate::{
    app::{BrowserApp, Command},
    smoke::{Host, eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};
use workspace_core::TabId;

fn fixture() -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", listener.local_addr()?);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.ends_with(b"\r\n\r\n") && request.len() < 32768 {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&chunk[..n]),
                }
            }
            let request = String::from_utf8_lossy(&request);
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            if path == "/redirect" {
                let _ = stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /page\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            }
            let agent = request
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("user-agent").then(|| value.trim())
                })
                .unwrap_or("");
            let agent = serde_json::to_string(agent).unwrap();
            let body = if path == "/frame" {
                format!(
                    "<script>parent.frameIdentity={{header:{agent},js:navigator.userAgent}}</script>"
                )
            } else {
                format!(
                    "<!doctype html><title>{path}</title><script>window.identity={{header:{agent},js:navigator.userAgent}}</script><iframe src='/frame'></iframe>"
                )
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    Ok(origin)
}

async fn check(
    host: &Host,
    tab: TabId,
    title: &str,
    expected: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    wait_title(host, tab, title, cx).await?;
    let expected = serde_json::to_string(expected)?;
    let script = format!(
        "Boolean(window.identity?.header==={expected} && identity.js==={expected} && window.frameIdentity?.header==={expected} && frameIdentity.js==={expected})"
    );
    for _ in 0..50 {
        if eval(host, tab, &script, cx).await? == true {
            return Ok(());
        }
        pause(cx).await;
    }
    anyhow::bail!("Native header, JavaScript or subframe identity differed on {title}")
}

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, first) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    let expected = crate::browser_identity::user_agent()
        .ok_or_else(|| anyhow::anyhow!("Installed Safari version unavailable"))?;
    ensure!(
        eval(&host, first, "navigator.userAgent", cx).await? == expected,
        "First document has incorrect identity"
    );
    let origin = fixture()?;
    host.navigate(first, &format!("{origin}/redirect"))?;
    check(&host, first, "/page", expected, cx).await?;
    host.navigate(first, &format!("{origin}/next"))?;
    check(&host, first, "/next", expected, cx).await?;
    host.back(first)?;
    check(&host, first, "/page", expected, cx).await?;
    host.forward(first)?;
    check(&host, first, "/next", expected, cx).await?;
    eval(&host, first, "window.beforeIdentityReload = true", cx).await?;
    host.reload(first)?;
    let mut reloaded = false;
    for _ in 0..50 {
        pause(cx).await;
        if eval(&host, first, "!window.beforeIdentityReload", cx).await? == true {
            reloaded = true;
            break;
        }
    }
    ensure!(reloaded, "Reload did not replace the document");
    check(&host, first, "/next", expected, cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.command(Command::NewTab, window, cx)
    })?;
    pause(cx).await;
    let second = view.update_in(cx, |app, _, _| app.active_tab())?;
    ensure!(second != first, "New tab was not created");
    host.navigate(second, &format!("{origin}/new-tab"))?;
    check(&host, second, "/new-tab", expected, cx).await?;
    Ok(vec![
        "Safari identity agrees in native HTTP headers, JavaScript and frames across redirects, navigation, history, reload and new tabs",
    ])
}
