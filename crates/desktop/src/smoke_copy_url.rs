//! Clipboard regression through AppKit's event queue and real native web views.
use crate::{
    app::{BrowserApp, TabTarget},
    smoke::{Host, bar_eval, eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use objc2_app_kit::NSEventModifierFlags as Flags;
use workspace_core::TabId;

fn fixture_url() -> Result<String> {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let url = format!("http://{}/", listener.local_addr()?);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let _ = stream.read(&mut [0; 4096]);
            let body = "<!doctype html><title>Copy URL fixture</title><input value='selected webpage text'><script>window.copyUrlKeys=0;addEventListener('keydown',e=>{if(e.metaKey&&e.shiftKey&&e.key.toLowerCase()==='c'){copyUrlKeys++;e.preventDefault();}},true);</script>";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'none'\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Ok(url)
}

async fn copy_and_check(
    host: &Host,
    tab: TabId,
    expected: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into())))?;
    host.test_key(tab, 8, "C", Flags::Command | Flags::Shift, false)?;
    for _ in 0..20 {
        pause(cx).await;
        if cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))?
            == Some(expected.to_string())
        {
            check_toast(host, tab, cx).await?;
            return Ok(());
        }
    }
    anyhow::bail!("Cmd+Shift+C did not copy {expected} for tab {tab}")
}

async fn check_toast(host: &Host, tab: TabId, cx: &mut AsyncWindowContext) -> Result<()> {
    let valid = eval(
        host,
        tab,
        r#"(() => {
        const t = document.getElementById('axolotl-copy-url-toast');
        if (!t) return false;
        const r = t.getBoundingClientRect(), s = getComputedStyle(t);
        return t.shadowRoot.textContent === 'URL copied' &&
            document.querySelectorAll('#axolotl-copy-url-toast').length === 1 &&
            t.getAttribute('role') === 'status' && s.backgroundColor === 'rgb(17, 17, 17)' &&
            s.color === 'rgb(255, 255, 255)' && s.pointerEvents === 'none' &&
            s.position === 'fixed' && Math.abs(r.top - 16) < 1 &&
            Math.abs(innerWidth - r.right - 16) < 1 &&
            r.width > 50 && r.width < 160 && r.height > 20 && r.height < 50;
    })()"#,
        cx,
    )
    .await?;
    ensure!(
        valid == true,
        "Missing or misplaced black URL copied toast in tab {tab}"
    );
    Ok(())
}

pub async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let previous = cx.update(|_, cx| cx.read_from_clipboard())?;
    let result = checks(view, cx).await;
    cx.update(|_, cx| {
        cx.write_to_clipboard(previous.unwrap_or_else(|| ClipboardItem::new_string(String::new())))
    })?;
    result
}

async fn checks(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, first) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    let origin = fixture_url()?;
    let initial = format!("{origin}first?full=a%20b&other=1#fragment");
    host.navigate(first, &initial)?;
    wait_title(&host, first, "Copy URL fixture", cx).await?;
    host.focus(first)?;
    copy_and_check(&host, first, &initial, cx).await?;
    ensure!(host.has_native_focus(first)?, "URL copy changed page focus");
    ensure!(
        eval(&host, first, "copyUrlKeys", cx).await? == 0,
        "Reserved URL shortcut reached the webpage"
    );
    if let Ok(path) = std::env::var("AXOLOTL_SMOKE_TOAST_SNAPSHOT") {
        host.save_test_snapshot(first, &path)?;
        for _ in 0..10 {
            pause(cx).await;
            if std::path::Path::new(&path).is_file() {
                break;
            }
        }
        ensure!(
            std::path::Path::new(&path).is_file(),
            "Toast snapshot was not saved"
        );
    }
    // Re-copying refreshes one toast; the first timeout cannot remove the new one.
    copy_and_check(&host, first, &initial, cx).await?;
    eval(
        &host,
        first,
        "window.oldCopyToast=document.getElementById('axolotl-copy-url-toast');true",
        cx,
    )
    .await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    copy_and_check(&host, first, &initial, cx).await?;
    ensure!(
        eval(&host, first, "!oldCopyToast.isConnected", cx).await? == true,
        "Repeated copies stacked toasts"
    );
    for _ in 0..4 {
        pause(cx).await;
    }
    check_toast(&host, first, cx).await?;
    for _ in 0..20 {
        pause(cx).await;
        if eval(
            &host,
            first,
            "!document.getElementById('axolotl-copy-url-toast')",
            cx,
        )
        .await?
            == true
        {
            break;
        }
    }
    ensure!(
        eval(
            &host,
            first,
            "!document.getElementById('axolotl-copy-url-toast')",
            cx
        )
        .await?
            == true,
        "Copy toast did not dismiss automatically"
    );

    // A real new document, followed by SPA navigation, must each copy the live URL.
    let navigated = format!("{origin}navigated?query=full%2Fpath#next");
    eval(
        &host,
        first,
        &format!("location.href={};true", serde_json::to_string(&navigated)?),
        cx,
    )
    .await?;
    for _ in 0..30 {
        pause(cx).await;
        if host.current_url(first)? == navigated && host.is_loaded(first) {
            break;
        }
    }
    ensure!(host.current_url(first)? == navigated, "Navigation failed");
    copy_and_check(&host, first, &navigated, cx).await?;
    let spa = format!("{origin}spa?state=one%20two#live");
    eval(
        &host,
        first,
        &format!(
            "history.pushState({{}},'',{});true",
            serde_json::to_string(&spa)?
        ),
        cx,
    )
    .await?;
    copy_and_check(&host, first, &spa, cx).await?;

    // Standard Cmd+C still copies the webpage's selected text.
    cx.update(|_, cx| cx.activate(true))?;
    for _ in 0..20 {
        pause(cx).await;
        if eval(&host, first, "document.hasFocus()", cx).await? == true {
            break;
        }
    }
    ensure!(
        eval(&host, first, "document.hasFocus()", cx).await? == true,
        "Copy selection check requires the test app in the foreground"
    );
    eval(
        &host,
        first,
        "document.querySelector('input').focus();document.querySelector('input').select();true",
        cx,
    )
    .await?;
    host.focus(first)?;
    host.test_key(first, 8, "c", Flags::Command, false)?;
    for _ in 0..20 {
        pause(cx).await;
        if cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))?
            == Some("selected webpage text".into())
        {
            break;
        }
    }
    ensure!(
        cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))?
            == Some("selected webpage text".into()),
        "Ordinary Cmd+C no longer copies the webpage selection"
    );

    let second_url = format!("{origin}second?tab=2#other");
    let second = view.update_in(cx, |app, window, cx| {
        let pane = app.model.focused_pane().id;
        let tab = app.model.add_tab(&second_url);
        app.select_target(TabTarget::Page(pane, tab), window, cx);
        tab
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    wait_title(&host, second, "Copy URL fixture", cx).await?;
    host.focus(second)?;
    copy_and_check(&host, second, &second_url, cx).await?;
    view.update_in(cx, |app, window, cx| {
        let pane = app.model.focused_pane().id;
        app.select_target(TabTarget::Page(pane, first), window, cx);
    })?;
    pause(cx).await;
    host.focus(first)?;
    copy_and_check(&host, first, &spa, cx).await?;

    // The native monitor also handles chrome focus, preserving the address draft.
    let draft = "https://unsubmitted.invalid/draft";
    view.update_in(cx, |app, window, cx| {
        let target = TabTarget::Space(app.model.active_space);
        app.begin_edit(target, window, cx);
        app.address.update(cx, |input, cx| {
            input.set_value(draft, window, cx);
            input.focus(window, cx);
        });
    })?;
    pause(cx).await;
    copy_and_check(&host, first, &spa, cx).await?;
    view.update_in(cx, |app, window, cx| -> Result<()> {
        ensure!(app.editing.is_some(), "Copy dismissed the address editor");
        ensure!(
            app.address.read(cx).value().as_str() == draft,
            "Copy changed the address draft"
        );
        ensure!(
            app.address.read(cx).focus_handle(cx).is_focused(window),
            "Copy changed editor focus"
        );
        // Exercise the GPUI action route as well as the native monitor.
        cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into()));
        window.dispatch_action(Box::new(crate::CopyUrl), cx);
        Ok(())
    })??;
    pause(cx).await;
    ensure!(
        cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))?
            == Some(spa.clone()),
        "Chrome action did not copy the current URL"
    );
    let pane = view.update_in(cx, |app, window, cx| {
        let pane = app.model.focused_pane().id;
        app.begin_edit(TabTarget::Page(pane, first), window, cx);
        pane
    })?;
    for _ in 0..20 {
        pause(cx).await;
        if bar_eval(view, pane, "!!document.querySelector('input')", cx).await? == true {
            break;
        }
    }
    bar_eval(view, pane, "document.querySelector('input').value='native address draft';document.querySelector('input').focus();document.querySelector('input').select();true", cx).await?;
    copy_and_check(&host, first, &spa, cx).await?;
    ensure!(
        bar_eval(view, pane, "document.querySelector('input').value", cx).await?
            == "native address draft",
        "URL copy changed the native address draft"
    );

    // Native popup pages are the current page while they are open.
    view.update_in(cx, |app, window, cx| {
        app.cancel_edit(window, cx);
        let pane = app.model.focused_pane().id;
        app.pin_page(pane, first, window, cx);
        app.toggle_pin(first, window, cx);
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    host.focus(first)?;
    copy_and_check(&host, first, &spa, cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.dismiss_pin(window, cx);
        app.open_link_popup(second, format!("{origin}preview?full=1#popup"), window, cx);
    })?;
    let popup = crate::link_popup::POPUP_TAB;
    for _ in 0..5 {
        pause(cx).await;
    }
    wait_title(&host, popup, "Copy URL fixture", cx).await?;
    host.focus(popup)?;
    copy_and_check(&host, popup, &format!("{origin}preview?full=1#popup"), cx).await?;
    ensure!(
        view.update_in(cx, |app, _, _| app.link_popup.is_some())?,
        "URL copy dismissed the preview"
    );
    Ok(vec![
        "Cmd+Shift+C copies full current URL with native webpage focus",
        "URL copy follows document and SPA navigation and active tab switching",
        "URL copy reserves the shortcut before page handlers and preserves focus/address drafts",
        "Chrome action and native address focus copy current URL; ordinary Cmd+C copies selected page text",
        "Pinned pages and temporary previews copy their own current URL",
        "Black success toast appears at the copied page's top-right, works under strict CSP, refreshes on repeat and dismisses automatically",
    ])
}
