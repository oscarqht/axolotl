//! Real AppKit key dispatch into native pages, not synthetic DOM shortcuts.
use crate::{
    app::Command,
    page_tools::Operation,
    smoke::{Host, eval, pause, wait_title},
};
use anyhow::{Result, ensure};
use gpui::*;
use objc2_app_kit::NSEventModifierFlags as Flags;
use workspace_core::{Axis, TabId};

fn fixture() -> Result<String> {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let server = TcpListener::bind(("127.0.0.1", 0))?;
    let url = format!("http://{address}/", address = server.local_addr()?);
    std::thread::spawn(move || {
        for mut stream in server.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let _ = stream.read(&mut [0; 4096]);
            let body = "<!doctype html><title>Page tools fixture</title><body><p>Alpha alpha ALPHA</p><p>Cross <b>node</b> match</p><p>Literal [a].*</p><p hidden>alpha</p><input value='preserve me'><script>window.toolKeys=0;addEventListener('keydown',e=>{if(e.metaKey&&['f','=','+','-','0'].includes(e.key))toolKeys++})</script></body>";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Security-Policy: style-src 'none'\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{body}",
                length = body.len()
            );
        }
    });
    Ok(url)
}
async fn key(
    host: &Host,
    tab: TabId,
    code: u16,
    text: &str,
    flags: Flags,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    host.test_key(tab, code, text, flags, false)?;
    for _ in 0..3 {
        pause(cx).await;
    }
    Ok(())
}
async fn count(host: &Host, tab: TabId, expected: &str, cx: &mut AsyncWindowContext) -> Result<()> {
    let value = eval(host, tab, "document.querySelector('#axolotl-page-find')?.shadowRoot.querySelector('output').textContent ?? 'closed'", cx).await?;
    ensure!(
        value == expected,
        "Find count expected {expected}, got {value}"
    );
    Ok(())
}
async fn query(
    host: &Host,
    tab: TabId,
    query: &str,
    expected: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    let script = format!(
        "(()=>{{const input=document.querySelector('#axolotl-page-find').shadowRoot.querySelector('input');input.value={query};input.dispatchEvent(new Event('input'));return true}})()",
        query = serde_json::to_string(query)?
    );
    eval(host, tab, &script, cx).await?;
    pause(cx).await;
    count(host, tab, expected, cx).await
}
pub async fn run(
    view: &WeakEntity<crate::app::BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let (host, first) = view.update_in(cx, |app, _, _| (app.host.clone(), app.active_tab()))?;
    let url = fixture()?;
    host.navigate(first, &url)?;
    wait_title(&host, first, "Page tools fixture", cx).await?;
    host.focus(first)?;
    key(&host, first, 24, "=", Flags::Command, cx).await?;
    ensure!(
        (host.test_zoom(first)? - 1.1).abs() < 0.001,
        "Cmd+= did not zoom native page"
    );
    key(&host, first, 24, "+", Flags::Command | Flags::Shift, cx).await?;
    ensure!(
        (host.test_zoom(first)? - 1.25).abs() < 0.001,
        "Cmd++ did not advance zoom"
    );
    key(&host, first, 27, "-", Flags::Command, cx).await?;
    ensure!(
        (host.test_zoom(first)? - 1.1).abs() < 0.001,
        "Cmd+- did not reduce zoom"
    );
    key(&host, first, 29, "0", Flags::Command, cx).await?;
    ensure!(
        (host.test_zoom(first)? - 1.).abs() < 0.001,
        "Cmd+0 did not reset zoom"
    );
    key(&host, first, 24, "=", Flags::Command, cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.command(Command::Split(Axis::Horizontal), window, cx)
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    let second = view.update_in(cx, |app, _, _| app.active_tab())?;
    host.navigate(second, &url)?;
    wait_title(&host, second, "Page tools fixture", cx).await?;
    ensure!(
        (host.test_zoom(second)? - 1.1).abs() < 0.001,
        "New tab missed hostname zoom"
    );
    host.focus(second)?;
    key(&host, second, 24, "=", Flags::Command, cx).await?;
    ensure!(
        (host.test_zoom(first)? - 1.25).abs() < 0.001,
        "Same hostname sibling not updated"
    );
    let other = url.replace("127.0.0.1", "localhost");
    host.navigate(second, &other)?;
    wait_title(&host, second, "Page tools fixture", cx).await?;
    ensure!(
        (host.test_zoom(second)? - 1.).abs() < 0.001,
        "Navigation leaked previous hostname zoom"
    );
    host.focus(first)?;
    for _ in 0..3 {
        pause(cx).await;
    }
    key(&host, first, 3, "f", Flags::Command, cx).await?;
    count(&host, first, "0/0", cx).await?;
    count(&host, second, "closed", cx).await?;
    query(&host, first, "alpha", "1/3", cx).await?;
    ensure!(eval(&host, first, "getComputedStyle(document.querySelector('#axolotl-page-find').shadowRoot.querySelector('form')).display==='flex'", cx).await? == true, "Strict CSP broke find styling");
    eval(&host, first, "window.addEventListener('keydown',e=>{if(e.key==='Enter')e.stopImmediatePropagation()},true);true", cx).await?;

    ensure!(eval(&host, first, "CSS.highlights.has('axolotl-find-matches') && CSS.highlights.get('axolotl-find-matches').size===3", cx).await? == true, "Matches not highlighted");
    key(&host, first, 36, "\r", Flags::empty(), cx).await?;
    count(&host, first, "2/3", cx).await?;
    key(&host, first, 36, "\r", Flags::Shift, cx).await?;
    count(&host, first, "1/3", cx).await?;
    key(&host, first, 36, "\r", Flags::Shift, cx).await?;
    count(&host, first, "3/3", cx).await?;
    query(&host, first, "Cross node match", "1/1", cx).await?;
    query(&host, first, "[a].*", "1/1", cx).await?;
    query(&host, first, "missing", "0/0", cx).await?;
    key(&host, first, 53, "\u{1b}", Flags::empty(), cx).await?;
    count(&host, first, "closed", cx).await?;
    ensure!(eval(&host, first, "!CSS.highlights.has('axolotl-find-matches') && document.querySelector('input').value==='preserve me' && toolKeys===0", cx).await? == true, "Find cleanup/editing/shortcut regression");
    // Also verify chrome action routing rather than only native event dispatch.
    view.update_in(cx, |_, window, cx| {
        window.dispatch_action(
            Box::new(crate::page_tools::PageTool {
                operation: Operation::ZoomReset,
            }),
            cx,
        )
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        (host.test_zoom(first)? - 1.).abs() < 0.001,
        "Chrome zoom reset targeted wrong split"
    );
    // Find from chrome must transfer native focus to the field.
    view.update_in(cx, |app, window, cx| {
        app.focus.focus(window);
        window.dispatch_action(
            Box::new(crate::page_tools::PageTool {
                operation: Operation::Find,
            }),
            cx,
        );
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    let active = view.update_in(cx, |app, _, _| app.active_tab())?;
    count(&host, active, "0/0", cx).await?;
    ensure!(
        host.has_native_focus(active)?,
        "Chrome find did not focus the native page"
    );
    key(&host, active, 53, "\u{1b}", Flags::empty(), cx).await?;

    // Pins and previews bypass normal pane routing. Escape closes only find.
    view.update_in(cx, |app, window, cx| {
        let pane = app
            .model
            .active_space()
            .panes
            .iter()
            .find(|p| p.active_tab == first)
            .unwrap()
            .id;
        app.pin_page(pane, first, window, cx);
        app.toggle_pin(first, window, cx);
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    host.focus(first)?;
    key(&host, first, 3, "f", Flags::Command, cx).await?;
    query(&host, first, "alpha", "1/3", cx).await?;
    key(&host, first, 53, "\u{1b}", Flags::empty(), cx).await?;
    ensure!(
        view.update_in(cx, |app, _, _| app.pins.open == Some(first))?,
        "Closing find dismissed the pinned page"
    );
    view.update_in(cx, |app, window, cx| {
        app.dismiss_pin(window, cx);
        app.open_link_popup(second, url.clone(), window, cx);
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    let popup = crate::link_popup::POPUP_TAB;
    wait_title(&host, popup, "Page tools fixture", cx).await?;
    host.focus(popup)?;
    key(&host, popup, 3, "f", Flags::Command, cx).await?;
    query(&host, popup, "alpha", "1/3", cx).await?;
    key(&host, popup, 53, "\u{1b}", Flags::empty(), cx).await?;
    ensure!(
        view.update_in(cx, |app, _, _| app.link_popup.is_some())?,
        "Closing find dismissed preview"
    );
    key(&host, popup, 24, "=", Flags::Command, cx).await?;
    ensure!(
        (host.test_zoom(popup)? - 1.1).abs() < 0.001,
        "Preview zoom targeted underlying page"
    );
    view.update_in(cx, |app, window, cx| {
        app.close_link_popup(window, cx);
        let pane = app
            .model
            .active_space()
            .panes
            .iter()
            .find(|p| p.active_tab == second)
            .unwrap()
            .id;
        app.model.float_pane(
            pane,
            workspace_core::Rect {
                x: 80.,
                y: 100.,
                width: 500.,
                height: 400.,
            },
            None,
        );
        app.model.pin_floating(pane);
        app.reveal_floating(pane);
        cx.notify();
    })?;
    for _ in 0..5 {
        pause(cx).await;
    }
    host.focus(second)?;
    key(&host, second, 3, "f", Flags::Command, cx).await?;
    query(&host, second, "alpha", "1/3", cx).await?;
    key(&host, second, 53, "\u{1b}", Flags::empty(), cx).await?;
    key(&host, second, 24, "=", Flags::Command, cx).await?;
    ensure!(
        (host.test_zoom(second)? - 1.1).abs() < 0.001,
        "Floating page zoom targeted wrong hostname"
    );
    host.navigate(second, "about:blank")?;
    wait_title(&host, second, "New tab · Axolotl", cx).await?;
    host.page_tool(second, Operation::ZoomIn)?;
    host.page_tool(second, Operation::ZoomIn)?;
    ensure!(
        (host.test_zoom(second)? - 1.25).abs() < 0.001,
        "Hostname-free zoom did not accumulate"
    );
    host.page_tool(second, Operation::ZoomReset)?;
    Ok(vec![
        "native Cmd plus/minus; hostname sharing and navigation isolation; split-targeted find; plain text, counts/highlights, Enter/Shift+Enter and Escape; chrome reset/find; CSP; pins, previews and floating pages",
    ])
}
