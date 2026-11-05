use crate::{app::BrowserApp, smoke::pause};
use anyhow::{Context as _, Result};
use gpui::*;
use serde_json::Value;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
};

impl crate::link_popup::LinkPopup {
    pub(crate) fn evaluate(
        &self,
        script: &str,
        callback: impl Fn(String) + Send + 'static,
    ) -> Result<()> {
        self.backdrop
            .as_ref()
            .context("Missing native popup backdrop")?
            .evaluate_script_with_callback(script, callback)?;
        Ok(())
    }

    pub(super) fn test_key(
        &self,
        key: &str,
        flags: objc2_app_kit::NSEventModifierFlags,
        code: u16,
    ) -> Result<()> {
        use objc2_app_kit::{NSEvent, NSEventType};
        use objc2_foundation::{NSPoint, NSString};
        use wry::WebViewExtMacOS;
        let window = self
            .backdrop
            .as_ref()
            .context("Missing native popup backdrop")?
            .ns_window();
        window.makeKeyWindow();
        let characters = NSString::from_str(key);
        let plain = NSString::from_str(&key.to_lowercase());
        for kind in [NSEventType::KeyDown, NSEventType::KeyUp] {
            let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                kind, NSPoint::new(0.,0.), flags, 0., window.windowNumber(), None, &characters, &plain, false, code,
            ).context("create native popup key event")?;
            window.sendEvent(&event);
        }
        Ok(())
    }

    pub(super) fn test_click(&self, x: f64, y: f64) -> Result<()> {
        use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
        use objc2_foundation::NSPoint;
        use wry::WebViewExtMacOS;
        let view = self
            .backdrop
            .as_ref()
            .context("Missing native popup backdrop")?;
        let webview = view.webview();
        let window = view.ns_window();
        let p = NSPoint::new(
            x,
            if webview.isFlipped() {
                y
            } else {
                webview.frame().size.height - y
            },
        );
        let p = webview.convertPoint_toView(p, None);
        for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
            let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind, p, NSEventModifierFlags::empty(), 0., window.windowNumber(), None, 0, 1, 1.,
            ).context("create native popup chrome click")?;
            window.sendEvent(&event);
        }
        Ok(())
    }
}

pub(super) fn fixture() -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let base = format!("http://{}", listener.local_addr()?);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                let mut request = [0; 4096];
                let count = stream.read(&mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (title, target) = match path {
                    "/source" => ("Popup source", "/a"),
                    "/a" => ("Popup A", "/b"),
                    "/b" => ("Popup B", "/c"),
                    _ => ("Popup C", "/a"),
                };
                let html = format!(
                    r#"<!doctype html><title>{title}</title><style>body{{margin:0;background:#f2f4f8}}a{{position:fixed;left:20px;top:20px;width:220px;height:80px;display:block;background:#ccddff}}input{{position:fixed;left:20px;top:120px}}</style><a href="{target}"><span>{title} link</span></a><input value="preserved form"><script>window.documentIdentity={{}};window.wasTrusted=false;document.addEventListener('click',e=>window.wasTrusted=e.isTrusted,true)</script>"#
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = stream.write_all(response.as_bytes());
            });
        }
    });
    Ok(base)
}

pub(super) async fn popup_eval(
    view: &WeakEntity<BrowserApp>,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> Result<Value> {
    let response = Arc::new(Mutex::new(None));
    let target = response.clone();
    view.update_in(cx, |app, _, _| {
        app.link_popup
            .as_ref()
            .context("Missing popup")?
            .evaluate(script, move |v| *target.lock().unwrap() = Some(v))
    })??;
    for _ in 0..50 {
        if let Some(value) = response.lock().unwrap().take() {
            return serde_json::from_str(&value)
                .with_context(|| format!("Popup chrome evaluation: {value}"));
        }
        pause(cx).await;
    }
    anyhow::bail!("Popup chrome evaluation timed out")
}

/// A policy update replaces WKUserScripts and reloads pages. The IPC/bridge
/// scripts must survive both directions, not only the original document.
pub(super) async fn check_protection_bridge(
    view: &WeakEntity<BrowserApp>,
    host: &crate::smoke::Host,
    source: workspace_core::TabId,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    use crate::{link_popup::POPUP_TAB, smoke::eval};
    use anyhow::ensure;
    let source_url = host.current_url(source)?;
    for enabled in [false, true] {
        eval(host, source, "window.protectionReloadMarker=true;true", cx).await?;
        host.protection().borrow_mut().set_enabled(enabled)?;
        let mut reloaded = false;
        for _ in 0..100 {
            pause(cx).await;
            if host.protection().borrow().native.ready()
                && host.is_loaded(source)
                && eval(host, source, "window.protectionReloadMarker!==true", cx).await? == true
            {
                reloaded = true;
                break;
            }
        }
        ensure!(
            reloaded,
            "Protection policy {enabled} did not reload the fixture"
        );
        ensure!(
            eval(host, source, "typeof window.ipc?.postMessage", cx).await? == "function",
            "Protection policy {enabled} removed native IPC on reload"
        );
        eval(
            host,
            source,
            "window.protectionSourceState='retained';true",
            cx,
        )
        .await?;
        host.focus(source)?;
        host.test_option_click(source)?;
        super::wait_popup(view, host, "Popup A", cx).await?;
        ensure!(
            host.has_view(POPUP_TAB),
            "Protection policy {enabled} prevented native popup creation"
        );
        ensure!(
            host.current_url(source)? == source_url,
            "Protection policy {enabled} let Option-click navigate the source"
        );
        ensure!(
            eval(host, source, "window.protectionSourceState", cx).await? == "retained",
            "Protection policy {enabled} lost the source document"
        );
        view.update_in(cx, |app, window, cx| app.close_link_popup(window, cx))?;
        pause(cx).await;
    }
    Ok(())
}
