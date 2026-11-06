//! Stable session and HTTP fixtures; never use the user's browsing profile.
use crate::{
    app::BrowserApp,
    benchmark::{draw, settle},
    smoke::Host,
};
use anyhow::{Result, ensure};
use gpui::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};
use workspace_core::{Axis, PaneId, SpaceId, TabId, Workspace};

pub struct Session {
    pub host: Host,
    pub spaces: Vec<SpaceId>,
    pub first: PaneId,
    pub second: PaneId,
    pub tabs: Vec<TabId>,
    pub pins: Vec<TabId>,
}
pub async fn session(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
    large: bool,
) -> Result<Session> {
    let session = view.update_in(cx, |app, window, cx| {
        app.cancel_edit(window, cx);
        app.dismiss_pin(window, cx);
        app.close_link_popup(window, cx);
        app.cancel_tab_drag(cx);
        app.cancel_space_drag(cx);
        let mut model = Workspace::new("about:blank");
        for s in 0..if large { 12 } else { 2 } {
            if s > 0 {
                model.add_space(format!("Benchmark {s}"), "about:blank");
            }
            for _ in 1..if large { 40 } else { 4 } {
                model.add_tab("about:blank");
            }
        }
        let spaces: Vec<_> = model.spaces.iter().map(|s| s.id).collect();
        model.active_space = spaces[0];
        let first = model.focused_pane().id;
        for _ in 0..4 {
            let tab = model.add_tab("about:blank");
            model.pin_tab(first, tab);
        }
        let tabs = model.focused_pane().tabs.iter().map(|t| t.id).collect();
        let pins = model.pinned_tabs.iter().map(|p| p.tab.id).collect();
        let second = model.split_focused(Axis::Horizontal, "about:blank");
        model.add_tab("about:blank");
        model.focus_pane(first);
        app.host.retain(&Default::default());
        app.bars.clear();
        app.chrome_views.clear();
        app.model = model.into();
        app.preferences.auto_sleep = false;
        cx.notify();
        Session {
            host: app.host.clone(),
            spaces,
            first,
            second,
            tabs,
            pins,
        }
    })?;
    draw(cx).await?;
    settle(cx).await?;
    ensure!(
        session
            .host
            .has_view(view.update_in(cx, |app, _, _| app.active_tab())?),
        "fixture page missing"
    );
    Ok(session)
}
/// Fixed, offline resources. Query strings change document identities while
/// cacheable JS/CSS/image resources exercise WebKit's resource cache.
pub fn server() -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", listener.local_addr()?);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            // WebKit may open speculative sockets without a request. One idle
            // connection must not hold up unrelated fixture resources.
            std::thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut request = Vec::new();
                let mut buf = [0; 2048];
                while !request.ends_with(b"\r\n\r\n") && request.len() < 32768 {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let request = String::from_utf8_lossy(&request);
                let route = request.split_whitespace().nth(1).unwrap_or("/");
                let (path, query) = route.split_once('?').unwrap_or((route, ""));
                let (kind,cache,mut body)=match path {
                "/style.css"=>("text/css","public, max-age=3600","body{font:14px system-ui;margin:20px}.grid{display:grid;grid-template-columns:repeat(8,1fr);gap:4px}.cell{padding:8px;background:#edf2f6}button{padding:12px}".to_owned()),
                "/image.svg"=>("image/svg+xml","public, max-age=3600","<svg xmlns='http://www.w3.org/2000/svg' width='256' height='128'><rect width='256' height='128' fill='#8fddc3'/><circle cx='128' cy='64' r='40' fill='#25303d'/></svg>".to_owned()),
                _=>("text/html; charset=utf-8","no-store",include_str!("../assets/benchmark-page.html").to_owned()),
            };
                if query.starts_with("cold=") {
                    body = body
                        .replace("/style.css", &format!("/style.css?{query}"))
                        .replace("/image.svg", &format!("/image.svg?{query}"));
                }
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nCache-Control: {cache}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    Ok(origin)
}
