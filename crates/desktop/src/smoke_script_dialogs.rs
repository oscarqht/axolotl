//! Real WebKit calls, native sheets and AppKit keyboard dispatch, offline.
use crate::{
    app::BrowserApp,
    macos_script_dialogs::test_alert,
    smoke::{eval, pause, wait_title},
};
use anyhow::{Context, Result, ensure};
use gpui::*;
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{
    NSAlert, NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSTextField,
};
use objc2_foundation::{NSPoint, NSString};

async fn sheet(cx: &mut AsyncWindowContext) -> Result<Retained<NSAlert>> {
    for _ in 0..40 {
        pause(cx).await;
        if let Some(alert) = test_alert() {
            return Ok(alert);
        }
    }
    anyhow::bail!("JavaScript did not show a native sheet")
}

fn key(alert: &NSAlert, cancel: bool) -> Result<()> {
    if std::env::args().any(|arg| arg == "--smoke-dialog-direct") {
        // Main thread, live NSAlert-owned button and its AppKit target/action.
        unsafe {
            alert
                .buttons()
                .objectAtIndex(usize::from(cancel))
                .performClick(None);
        }
        return Ok(());
    }
    let window = alert.window();
    window.makeKeyWindow();
    let text = NSString::from_str(if cancel { "\u{1b}" } else { "\r" });
    let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
    for kind in [NSEventType::KeyDown, NSEventType::KeyUp] {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind, NSPoint::new(0.,0.), NSEventModifierFlags::empty(), 0., window.windowNumber(), None,
            &text, &text, false, if cancel { 53 } else { 36 },
        ).context("Create sheet keyboard event")?;
        app.postEvent_atStart(&event, false);
    }
    Ok(())
}

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, tab) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    // A local fixture supplies a genuine origin and a child frame.
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", listener.local_addr()?);
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut request = [0; 4096];
            let count = stream.read(&mut request).unwrap_or(0);
            let child = !String::from_utf8_lossy(&request[..count]).starts_with("GET / HTTP");
            let body = if child {
                "<!doctype html><title>Dialog child</title>Child frame or popup"
            } else {
                "<!doctype html><title>JavaScript dialogs</title><h1>JavaScript dialogs</h1><p>Native alert, confirm and prompt regression</p><iframe src='/frame'></iframe>"
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    host.navigate(tab, &origin)?;
    wait_title(&host, tab, "JavaScript dialogs", cx).await?;
    for (call, expected, cancel, input) in [
        (
            "(alert('Alert from a website'), 'acknowledged')",
            serde_json::json!("acknowledged"),
            false,
            None,
        ),
        (
            "confirm('Confirm this action?')",
            serde_json::json!(true),
            false,
            None,
        ),
        (
            "confirm('Cancel this action?')",
            serde_json::json!(false),
            true,
            None,
        ),
        (
            "prompt('Your name', 'Axolotl')",
            serde_json::json!("Axolotl"),
            false,
            None,
        ),
        ("prompt('No default')", serde_json::json!(""), false, None),
        (
            "prompt('Empty submission', 'default')",
            serde_json::json!(""),
            false,
            Some(""),
        ),
        (
            "prompt('Unicode text', 'default')",
            serde_json::json!("你好 🦎"),
            false,
            Some("你好 🦎"),
        ),
        (
            "prompt('Cancel prompt', 'default')",
            serde_json::Value::Null,
            true,
            None,
        ),
        (
            "frames[0].confirm('Frame dialog')",
            serde_json::json!(true),
            false,
            None,
        ),
    ] {
        eval(&host, tab, &format!("window.answer='pending'; setTimeout(()=>{{answer=(()=>{{return {call}}})()}},0); true"), cx).await?;
        let alert = sheet(cx).await?;
        ensure!(
            alert.messageText().to_string().contains("127.0.0.1"),
            "Missing initiating frame host"
        );
        if let Some(input) = input {
            let field = alert
                .accessoryView()
                .context("Prompt input missing")?
                .downcast::<NSTextField>()
                .map_err(|_| anyhow::anyhow!("Prompt accessory is not a text field"))?;
            field.setStringValue(&NSString::from_str(input));
        }
        if call.contains("Your name") {
            let field = alert
                .accessoryView()
                .unwrap()
                .downcast::<NSTextField>()
                .unwrap();
            ensure!(
                field.stringValue().to_string() == "Axolotl",
                "Prompt lost default text"
            );
            if let Ok(path) = std::env::var("AXOLOTL_DIALOG_SNAPSHOT") {
                ensure!(
                    !std::env::args().any(|arg| arg == "--smoke-dialog-direct"),
                    "OS screenshots require an unlocked graphical session; omit AXOLOTL_DIALOG_SNAPSHOT for direct button tests"
                );
                let status = std::process::Command::new("screencapture")
                    .args([
                        "-x",
                        "-l",
                        &alert.window().windowNumber().to_string(),
                        &path,
                    ])
                    .status()?;
                ensure!(status.success(), "Native dialog screenshot failed");
            }
        }
        key(&alert, cancel)?;
        for _ in 0..30 {
            pause(cx).await;
            if test_alert().is_none() {
                break;
            }
        }
        ensure!(test_alert().is_none(), "Sheet did not dismiss: {call}");
        ensure!(
            eval(&host, tab, "answer", cx).await? == expected,
            "Incorrect dialog result: {call}"
        );
    }
    // window.open still creates a related webview with its own dialog delegate.
    eval(
        &host,
        tab,
        &format!("window.child=open('{origin}/popup'); true"),
        cx,
    )
    .await?;
    for _ in 0..15 {
        pause(cx).await;
    }
    eval(
        &host,
        tab,
        "setTimeout(()=>child.eval(\"opener.popupAnswer=confirm('Popup dialog')\"),0); true",
        cx,
    )
    .await?;
    let alert = sheet(cx).await?;
    key(&alert, false)?;
    for _ in 0..5 {
        pause(cx).await;
    }
    ensure!(
        eval(&host, tab, "popupAnswer", cx).await? == true,
        "Popup dialog result lost"
    );
    // Close a tab while a prompt is pending. This must not strand a sheet or
    // leave a WebKit completion outstanding, and a subsequent dialog must work.
    eval(
        &host,
        tab,
        "setTimeout(()=>child.eval(\"prompt('Close owner', 'pending')\"),0); true",
        cx,
    )
    .await?;
    let _ = sheet(cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.command(crate::app::Command::CloseTab, window, cx)
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    ensure!(
        test_alert().is_none(),
        "Closing owner left a native sheet open"
    );
    eval(
        &host,
        tab,
        "setTimeout(()=>alert('Still responsive'),0); true",
        cx,
    )
    .await?;
    let alert = sheet(cx).await?;
    key(&alert, false)?;
    pause(cx).await;
    Ok(vec![
        "native alert/confirm/prompt values, defaults, empty and Unicode input",
        if std::env::args().any(|arg| arg == "--smoke-dialog-direct") {
            "native NSAlert button actions; foreground keyboard unverified"
        } else {
            "Return/Escape through AppKit event queue"
        },
        "frame and window.open popup dialogs",
        "page-close sheet cleanup and subsequent dialog",
    ])
}
