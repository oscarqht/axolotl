//! Native AppKit wheel input through the app's WKWebView and authenticated history bridge.
use crate::{
    app::BrowserApp,
    smoke::{eval, pause, wait_title},
};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
};

/// Native responder input, shared by page and floating-bar acceptance checks.
pub(crate) fn scroll_view(view: &wry::WebView, x: f64, y: f64, dx: i32, dy: i32) -> Result<()> {
    use objc2_foundation::NSPoint;
    use std::ffi::c_void;
    use wry::WebViewExtMacOS;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateScrollWheelEvent2(
            source: *const c_void,
            units: u32,
            count: u32,
            dy: i32,
            dx: i32,
            dz: i32,
        ) -> *mut c_void;
        fn CGEventSetLocation(event: *mut c_void, point: NSPoint);
        fn CGMainDisplayID() -> u32;
        fn CGDisplayPixelsHigh(display: u32) -> usize;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: *const c_void);
    }
    let webview = view.webview();
    let window = view.ns_window();
    window.makeKeyAndOrderFront(None);
    let point = NSPoint::new(
        x,
        if webview.isFlipped() {
            y
        } else {
            webview.frame().size.height - y
        },
    );
    let point = webview.convertPoint_toView(point, None);
    unsafe {
        let event = CGEventCreateScrollWheelEvent2(std::ptr::null(), 0, 2, dy, dx, 0);
        anyhow::ensure!(!event.is_null(), "create native scroll event");
        // eventWithCGEvent has no NSWindow. Supply window-local coordinates
        // in CoreGraphics' flipped coordinate space for WebKit's responder.
        CGEventSetLocation(
            event,
            NSPoint::new(
                point.x,
                CGDisplayPixelsHigh(CGMainDisplayID()) as f64 - point.y,
            ),
        );
        let native: Option<objc2::rc::Retained<objc2_app_kit::NSEvent>> =
            objc2::msg_send![objc2::class!(NSEvent), eventWithCGEvent: event];
        CFRelease(event);
        let native = native.context("create AppKit scroll event")?;
        // Feed WebKit's real native responder, not an untrusted JS event.
        webview.scrollWheel(&native);
    }
    Ok(())
}

fn fixture() -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/", listener.local_addr()?);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let _ = stream.read(&mut [0; 4096]);
            let body = include_str!("../assets/smoke-horizontal-scroll.html");
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Ok(url)
}

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, tab) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    host.navigate(tab, &fixture()?)?;
    wait_title(&host, tab, "Horizontal scrolling acceptance", cx).await?;
    host.focus(tab)?;
    for (name, setup, expression, x, y) in [
        (
            "root",
            "document.body.style.width='2400px'; scrollTo(200,0)",
            "scrollX",
            300.,
            100.,
        ),
        (
            "nested",
            "document.body.style.width='auto'; scrollTo(0,0); nested.scrollLeft=200",
            "nested.scrollLeft",
            300.,
            220.,
        ),
        (
            "shadow",
            "shadow.scroller.scrollLeft=200",
            "shadow.scroller.scrollLeft",
            300.,
            400.,
        ),
        ("RTL", "rtl.scrollLeft=-200", "rtl.scrollLeft", 300., 580.),
    ] {
        eval(&host, tab, &format!("(()=>{{{setup}; return true}})()"), cx).await?;
        for _ in 0..3 {
            pause(cx).await;
        }
        let before = eval(&host, tab, expression, cx).await?.as_f64().unwrap();
        // CoreGraphics wheel deltas are opposite DOM wheel deltas.
        host.test_scroll(tab, x, y, -80, 0)?;
        for _ in 0..3 {
            pause(cx).await;
        }
        let right = eval(&host, tab, expression, cx).await?.as_f64().unwrap();
        ensure!(
            right > before,
            "{name}: did not scroll right: {before} -> {right}; events {}",
            eval(&host, tab, "events", cx).await?
        );
        host.test_scroll(tab, x, y, 80, 0)?;
        for _ in 0..3 {
            pause(cx).await;
        }
        let left = eval(&host, tab, expression, cx).await?.as_f64().unwrap();
        ensure!(
            left < right,
            "{name}: did not scroll left: {right} -> {left}"
        );
    }
    host.test_scroll(tab, 900., 100., 0, -80)?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        eval(&host, tab, "scrollY > 0", cx).await? == true,
        "vertical scrolling blocked"
    );
    eval(&host, tab, "(()=>{document.body.style.width='2400px';scrollTo(0,0); history.pushState({},'', '/one'); history.pushState({},'', '/two'); return true})()", cx).await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    host.test_scroll(tab, 900., 100., 100, 0)?;
    for _ in 0..5 {
        pause(cx).await;
    }
    ensure!(
        eval(&host, tab, "location.pathname", cx).await? == "/one",
        "left boundary did not go back"
    );
    eval(
        &host,
        tab,
        "(()=>{scrollTo(document.documentElement.scrollWidth,0);return true})()",
        cx,
    )
    .await?;
    pause(cx).await;
    host.test_scroll(tab, 900., 100., -100, 0)?;
    for _ in 0..5 {
        pause(cx).await;
    }
    ensure!(
        eval(&host, tab, "location.pathname", cx).await? == "/two",
        "right boundary did not go forward"
    );
    ensure!(
        eval(
            &host,
            tab,
            "events.length > 0 && events.every(e => e.trusted)",
            cx
        )
        .await?
            == true,
        "page did not receive genuine wheel events"
    );
    eval(
        &host,
        tab,
        "(()=>{document.body.style.width='auto';scrollTo(0,0);return true})()",
        cx,
    )
    .await?;
    if let Ok(path) = std::env::var("AXOLOTL_SCROLL_SNAPSHOT") {
        pause(cx).await;
        let _ = std::fs::remove_file(&path);
        host.save_test_snapshot(tab, &path)?;
        for _ in 0..10 {
            pause(cx).await;
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
                break;
            }
        }
        ensure!(
            std::fs::metadata(path).is_ok_and(|m| m.len() > 0),
            "snapshot missing"
        );
    }
    let (pane, following) = view.update_in(cx, |app, window, cx| {
        let pane = app.model.focused_pane().id;
        let following = app.model.add_tab("about:blank");
        app.select_target(crate::app::TabTarget::Page(pane, tab), window, cx);
        (pane, following)
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    for (delta, expected) in [(-100, following), (100, tab)] {
        view.update_in(cx, |app, _, _| app.bars[&pane].test_scroll(delta))??;
        for _ in 0..5 {
            pause(cx).await;
        }
        ensure!(
            view.update_in(cx, |app, _, _| app.active_tab())? == expected,
            "floating-bar wheel failed to switch tabs"
        );
    }
    ensure!(
        eval(&host, tab, "location.pathname", cx).await? == "/two",
        "floating-bar gesture changed page history"
    );
    Ok(vec![
        "AppKit wheel: root, nested, shadow DOM and RTL scroll both ways; vertical scrolling; authenticated back/forward at boundaries; trusted page handlers receive scrolling input; floating-bar wheel still switches tabs without changing history",
    ])
}
