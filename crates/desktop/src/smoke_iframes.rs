//! Offline iframe rendering and native pointer regression in the real browser host.
use crate::{
    app::BrowserApp,
    smoke::{eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

fn fixture() -> Result<String> {
    let first = TcpListener::bind("127.0.0.1:0")?;
    let second = TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", first.local_addr()?);
    let cross = format!("http://{}", second.local_addr()?);
    for listener in [first, second] {
        let cross = cross.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut bytes = [0; 8192];
                let n = stream.read(&mut bytes).unwrap_or(0);
                let request = String::from_utf8_lossy(&bytes[..n]);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (body, headers) = if path == "/denied" {
                    (
                        "<script>parent.postMessage({id:'denied'},'*')</script>Must not render"
                            .into(),
                        "X-Frame-Options: DENY\r\nContent-Security-Policy: frame-ancestors 'none'\r\n",
                    )
                } else if path.starts_with("/frame?id=") {
                    let id = if path.ends_with("cross") {
                        "cross"
                    } else if path.ends_with("dynamic") {
                        "dynamic"
                    } else {
                        "same"
                    };
                    (
                        format!(
                            r#"<!doctype html><style>body{{margin:0;padding:14px;background:#d8f5e5;color:#123e2b;font:15px system-ui}}button{{display:block;margin-top:12px;padding:8px 12px;border:0;border-radius:5px;background:#166541;color:white}}</style><strong>{id}: rendered</strong><button onclick="this.textContent='Clicked ✓';parent.postMessage({{id:'{id}',clicked:true,trusted:event.isTrusted}},'*')">Click {id}</button><script>parent.postMessage({{id:'{id}'}},'*')</script>"#
                        ),
                        "",
                    )
                } else {
                    (
                        include_str!("../assets/smoke-iframes.html")
                            .replace("__CROSS_ORIGIN__", &cross),
                        "",
                    )
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n{headers}Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
    }
    Ok(origin)
}

pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, tab) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    let origin = fixture()?;
    host.navigate(tab, &origin)?;
    wait_title(&host, tab, "Iframe acceptance · Axolotl", cx).await?;
    for _ in 0..50 {
        if eval(
            &host,
            tab,
            "Object.keys(window.results || {}).length === 8",
            cx,
        )
        .await?
            == true
        {
            break;
        }
        pause(cx).await;
    }
    let state = eval(&host, tab, "window.results", cx).await?;
    eprintln!("Iframe loaded state: {state}");
    // Capture failures too, so the same fixture supplies before/after evidence.
    if let Ok(path) = std::env::var("AXOLOTL_IFRAME_SNAPSHOT") {
        host.save_test_snapshot(tab, &path)?;
        for _ in 0..20 {
            pause(cx).await;
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
                break;
            }
        }
    }
    ensure!(
        state.as_object().is_some_and(|r| r.len() == 8),
        "Iframe content missing: {state}"
    );
    for id in [
        "same", "cross", "srcdoc", "dynamic", "blob", "data", "blank", "sandbox",
    ] {
        let point = eval(&host, tab, &format!(r#"(()=>{{const f=document.getElementById('{id}');f.scrollIntoView({{block:'nearest'}});const r=f.getBoundingClientRect();return [r.x+60,r.y+70]}})()"#), cx).await?;
        host.test_click_at(
            tab,
            point[0].as_f64().unwrap(),
            point[1].as_f64().unwrap(),
            false,
        )?;
        let script = format!("results.{id}.clicked && results.{id}.trusted");
        for _ in 0..30 {
            pause(cx).await;
            if eval(&host, tab, &script, cx).await? == true {
                break;
            }
        }
        ensure!(
            eval(&host, tab, &script, cx).await? == true,
            "Native click failed in {id}"
        );
    }
    ensure!(eval(&host, tab, "(()=>{try{document.getElementById('cross').contentWindow.document;return false}catch(e){return e.name==='SecurityError'}})()", cx).await? == true, "Cross-origin DOM became accessible");
    ensure!(eval(&host, tab, "(()=>{try{document.getElementById('sandbox').contentWindow.document;return false}catch(e){return e.name==='SecurityError'}})()", cx).await? == true, "Sandbox origin restriction lost");
    ensure!(eval(&host, tab, "document.getElementById('denied').contentDocument?.body?.textContent.includes('Must not render') !== true", cx).await? == true, "Embedding restrictions lost");
    ensure!(
        host.current_url(tab)? == origin || host.current_url(tab)? == format!("{origin}/"),
        "Subframe changed top-level URL"
    );
    // The source is a child but the target is the main frame. Checking the
    // source frame instead of targetFrame would accidentally allow this blob.
    let point = eval(&host, tab, r#"(()=>{
        const f=document.getElementById('same');
        const d=f.contentDocument, b=d.querySelector('button');
        window.savedButton=b.outerHTML;
        window.topProbe=URL.createObjectURL(new Blob(['<title>UNSAFE top navigation</title>'],{type:'text/html'}));
        const a=d.createElement('a');a.href=topProbe;a.target='_top';a.textContent='Top navigation probe';
        a.style='display:block;margin-top:12px;padding:8px';b.replaceWith(a);
        f.scrollIntoView({block:'nearest'});const r=f.getBoundingClientRect();return [r.x+60,r.y+70];
    })()"#, cx).await?;
    host.test_click_at(
        tab,
        point[0].as_f64().unwrap(),
        point[1].as_f64().unwrap(),
        false,
    )?;
    for _ in 0..5 {
        pause(cx).await;
    }
    ensure!(
        eval(
            &host,
            tab,
            "document.title === 'Iframe acceptance · Axolotl' && !!window.savedButton",
            cx
        )
        .await?
            == true,
        "Child link bypassed top-level navigation policy"
    );
    eval(&host, tab, "document.getElementById('same').contentDocument.querySelector('a').outerHTML=savedButton;URL.revokeObjectURL(topProbe);true", cx).await?;
    if let Ok(path) = std::env::var("AXOLOTL_IFRAME_SNAPSHOT") {
        eval(&host, tab, "scrollTo(0,0);true", cx).await?;
        pause(cx).await;
        host.save_test_snapshot(tab, &path)?;
        for _ in 0..5 {
            pause(cx).await;
        }
    }
    Ok(vec![
        "eight iframe types render and receive trusted native clicks; cross-origin, sandbox and embedding restrictions preserved",
    ])
}
