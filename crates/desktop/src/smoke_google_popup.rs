//! Opt-in live Google diagnosis. Run only when investigating real search links.
use crate::{
    app::BrowserApp,
    link_popup::POPUP_TAB,
    smoke::{eval, pause},
};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
use serde_json::Value;

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, source) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    let search = "https://www.google.com/search?q=axolotl+browser+github&hl=en";
    host.navigate(source, search)?;
    let mut link = Value::Null;
    for _ in 0..100 {
        pause(cx).await;
        link = eval(&host, source, r#"(()=>{
            const h=[...document.querySelectorAll('h3')].find(h=>h.closest('a[href]')?.href.startsWith('http'));
            if(!h)return {url:location.href,title:document.title,blocked:!!document.querySelector('#captcha-form'),text:document.body?.innerText.slice(0,800)};
            const a=h.closest('a[href]');a.scrollIntoView({block:'center'});
            const b=h.getBoundingClientRect();return {url:location.href,title:document.title,href:a.href,heading:h.innerText,anchorTag:a.tagName,x:b.x+b.width/2,y:b.y+b.height/2};
        })()"#, cx).await?;
        if link["href"].is_string() || link["blocked"] == true {
            break;
        }
    }
    println!("AXOLOTL_GOOGLE_RUNTIME {}", eval(&host, source, "({ipc:typeof window.ipc,post:typeof window.ipc?.postMessage,elementType:typeof Element,matchesType:typeof Element.prototype.matches})", cx).await?);
    println!("AXOLOTL_GOOGLE_RESULT {}", link);
    ensure!(
        link["href"].is_string(),
        "No Google result available: {link}"
    );
    host.save_test_snapshot(source, "/tmp/axolotl-google-before.png")?;
    eval(&host, source, r#"(()=>{
        window.axolotlGoogleEvents=[];
        sessionStorage.removeItem('__axolotlGoogleDiag');
        const record=e=>{
            const path=e.composedPath().filter(n=>n instanceof Element);
            const a=path.find(n=>n.matches('a[href],area[href]'));
            window.axolotlGoogleEvents.push({type:e.type,trusted:e.isTrusted,alt:e.altKey,meta:e.metaKey,ctrl:e.ctrlKey,shift:e.shiftKey,button:e.button,prevented:e.defaultPrevented,href:a?.href,path:path.slice(0,8).map(n=>n.tagName+'.'+n.className),x:e.clientX,y:e.clientY});
            sessionStorage.setItem('__axolotlGoogleDiag',JSON.stringify(window.axolotlGoogleEvents));
        };
        for(const type of ['pointerdown','mousedown','pointerup','mouseup','click','auxclick'])window.addEventListener(type,record,true);
        for(const type of ['beforeunload','pagehide'])window.addEventListener(type,()=>{
            window.axolotlGoogleEvents.push({type,url:location.href});sessionStorage.setItem('__axolotlGoogleDiag',JSON.stringify(window.axolotlGoogleEvents));
        });
        return true;
    })()"#, cx).await?;
    host.focus(source)?;
    let before = host.current_url(source)?;
    host.test_option_click_at(
        source,
        link["x"].as_f64().context("Missing result x")?,
        link["y"].as_f64().context("Missing result y")?,
    )?;
    for _ in 0..20 {
        pause(cx).await;
    }
    let source_url = host.current_url(source)?;
    let popup = view.update_in(cx, |app, _, _| {
        app.link_popup.as_ref().map(
            |p| serde_json::json!({"url":p.url,"source":p.source,"pane":p.pane,"title":p.title}),
        )
    })?;
    println!(
        "AXOLOTL_GOOGLE_AFTER {}",
        serde_json::json!({"before":before,"source":source_url,"popup":popup,"popup_native":host.has_view(POPUP_TAB)})
    );
    if source_url.starts_with("https://www.google.") {
        println!("AXOLOTL_GOOGLE_EVENTS {}", eval(&host, source, "window.axolotlGoogleEvents||JSON.parse(sessionStorage.getItem('__axolotlGoogleDiag')||'[]')", cx).await?);
    } else {
        host.back(source)?;
        for _ in 0..30 {
            pause(cx).await;
            if host.current_url(source)?.starts_with("https://www.google.") {
                break;
            }
        }
        println!(
            "AXOLOTL_GOOGLE_EVENTS {}",
            eval(
                &host,
                source,
                "JSON.parse(sessionStorage.getItem('__axolotlGoogleDiag')||'[]')",
                cx
            )
            .await?
        );
    }
    ensure!(
        popup.is_some(),
        "Trusted Option-click did not open Google result popup"
    );
    ensure!(
        source_url == before,
        "Google result navigated source instead of preserving search"
    );
    Ok(vec![
        "live Google result trusted native Option-click opens popup and preserves search source",
    ])
}
