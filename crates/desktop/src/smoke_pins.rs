//! Pinning through the native chip menu, retained WK/WebView2 pages and popup IPC.
use crate::{
    app::{BrowserApp, DragInput},
    smoke::{bar_eval, eval, pause},
};
use anyhow::{Context as _, Result, ensure};
use gpui::*;

async fn popup_action(
    view: &WeakEntity<BrowserApp>,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    // Dismissal destroys this child, so do not wait for a callback from it.
    // The caller checks the resulting application state after polling IPC.
    view.update_in(cx, |app, _, _| app.pins.evaluate(script))??;
    pause(cx).await;
    Ok(())
}

fn fixture() -> Result<String> {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", listener.local_addr()?);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            // WebKit may open an idle preconnect socket alongside a request.
            // Serve each connection independently so that cannot stall another page.
            std::thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
                let mut request = [0u8; 4096];
                let Ok(n) = stream.read(&mut request) else {
                    return;
                };
                let request = String::from_utf8_lossy(&request[..n]);
                let title = if request.starts_with("GET /second ") {
                    "Pin second"
                } else {
                    "Pin first"
                };
                let body = format!(
                    r#"<!doctype html><title>{title}</title><link rel="icon" href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j5n8AAAAASUVORK5CYII="><input><div id="scroll" style="height:100px;overflow:auto"><div style="height:2000px">Scrollable pin fixture</div></div>"#
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            });
        }
    });
    Ok(origin)
}

pub async fn run(
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
    let origin = fixture()?;
    host.navigate(first, &format!("{origin}/first"))?;
    crate::smoke::wait_title(&host, first, "Pin first", cx)
        .await
        .context("initial visible fixture / back navigation")?;
    host.navigate(first, &format!("{origin}/second"))?;
    crate::smoke::wait_title(&host, first, "Pin second", cx).await?;
    host.back(first)?;
    crate::smoke::wait_title(&host, first, "Pin first", cx)
        .await
        .context("initial visible fixture / back navigation")?;
    eval(&host,first,"window.pinIdentity={};window.originalPinIdentity=pinIdentity;document.querySelector('input').value='retained pinned form';document.querySelector('#scroll').scrollTop=350;true",cx).await?;
    bar_eval(view, pane, "pinned=true;expand(true);true", cx).await?;
    pause(cx).await;
    bar_eval(view,pane,"document.querySelector('.chip.active').dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:60}));true",cx).await?;
    pause(cx).await;
    ensure!(bar_eval(view, pane, "(()=>{const r=document.querySelector('#menu').getBoundingClientRect();return r.top>=0 && r.bottom<=innerHeight})()", cx).await? == true, "tab menu is clipped");
    bar_eval(view,pane,"[...document.querySelectorAll('#menu button')].find(b=>b.textContent==='Pin to header').click();true",cx).await?;
    pause(cx).await;
    pause(cx).await;
    view.update_in(cx, |app, _, _| {
        ensure!(
            app.model.pinned_tabs.len() == 1 && app.model.pinned_tabs[0].tab.id == first,
            "native menu did not pin original tab"
        );
        ensure!(
            app.host.has_view(first) && !app.host.is_visible(first),
            "pinning destroyed or left visible the moved page"
        );
        Ok::<_, anyhow::Error>(())
    })??;
    let second = view.update_in(cx, |app, window, cx| {
        let id = app.active_tab();
        // The same completion path as a captured native chip released in the header.
        app.arm_tab_drag(pane, id, point(px(180.), px(300.)));
        app.tab_drag.as_mut().unwrap().input = DragInput::NativeBar;
        app.update_native_tab_drag(pane, point(px(800.), px(20.)), window, cx);
        app.finish_native_tab_drag(pane, Some(point(px(800.), px(20.))), window, cx);
        id
    })?;
    pause(cx).await;
    host.navigate(second, &format!("{origin}/first"))?;
    crate::smoke::wait_title(&host, second, "Pin first", cx)
        .await
        .context("hidden pinned fixture load")?;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        ensure!(
            app.model.pinned_tabs.len() == 2,
            "header drop did not pin second tab"
        );
        app.model.move_pin(second, 0);
        app.model.add_space("Pins across spaces", "about:blank");
        app.toggle_pin(first, window, cx);
        Ok::<_, anyhow::Error>(())
    })??;
    pause(cx).await;
    pause(cx).await;
    let preserved = eval(
        &host,
        first,
        "({identity:pinIdentity===originalPinIdentity,form:document.querySelector('input').value,scroll:document.querySelector('#scroll').scrollTop,url:location.href})",
        cx,
    )
    .await?;
    ensure!(
        preserved["identity"] == true
            && preserved["form"] == "retained pinned form"
            && preserved["scroll"] == 350
            && preserved["url"] == format!("{origin}/first"),
        "pinning reloaded the page: {preserved}"
    );
    view.update_in(cx, |app, window, _| {
        ensure!(
            app.favicons.contains_key(&first) && app.favicons.contains_key(&second),
            "page favicon did not reach the header"
        );
        let r = app.pin_rect(window).unwrap();
        ensure!(
            app.host.is_visible(first) && !app.host.is_visible(second),
            "popup visibility incorrect"
        );
        ensure!(
            r.width == 480.
                && r.height <= 640.
                && r.x >= 0.
                && r.y + r.height <= f32::from(window.viewport_size().height),
            "popup bounds incorrect: {r:?}"
        );
        Ok::<_, anyhow::Error>(())
    })??;
    popup_action(view, "send('resize',{width:560,height:420});true", cx).await?;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        let pin = app
            .model
            .pinned_tabs
            .iter()
            .find(|p| p.tab.id == first)
            .unwrap();
        ensure!(pin.width == 560. && pin.height == 420., "resize not saved");
        let restored = workspace_core::Workspace::from_json(&app.model.to_json())?;
        ensure!(
            restored.pinned_tabs == app.model.pinned_tabs,
            "restart loses pins/order/size"
        );
        app.toggle_pin(second, window, cx);
        Ok::<_, anyhow::Error>(())
    })??;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        ensure!(
            !app.host.is_visible(first) && app.host.is_visible(second),
            "switching popup leaves two visible"
        );
        app.toggle_pin(second, window, cx);
        Ok::<_, anyhow::Error>(())
    })??;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        ensure!(
            !app.host.is_visible(second),
            "repeat click failed to dismiss"
        );
        app.toggle_pin(first, window, cx);
        Ok::<_, anyhow::Error>(())
    })??;
    pause(cx).await;
    pause(cx).await;
    popup_action(
        view,
        "document.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));true",
        cx,
    )
    .await?;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        ensure!(app.pins.open.is_none(), "outside click did not dismiss");
        app.toggle_pin(first, window, cx);
        Ok::<_, anyhow::Error>(())
    })??;
    pause(cx).await;
    pause(cx).await;
    popup_action(
        view,
        "document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));true",
        cx,
    )
    .await?;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        ensure!(app.pins.open.is_none(), "Escape did not dismiss");
        app.pin_menu(Some(first), window, cx);
        Ok::<_, anyhow::Error>(())
    })??;
    pause(cx).await;
    pause(cx).await;
    popup_action(
        view,
        "document.querySelector('#menu button').click();true",
        cx,
    )
    .await?;
    pause(cx).await;
    pause(cx).await;
    view.update_in(cx, |app, _, _| {
        ensure!(
            app.model.focused_pane().active_tab == first && app.host.is_visible(first),
            "unpin did not return to active space focused pane"
        );
        let (visible, width) = crate::pinned_tabs::header_layout(420., 12);
        ensure!(
            visible < 12 && width <= 90.,
            "narrow header has no bounded overflow"
        );
        Ok::<_, anyhow::Error>(())
    })??;
    ensure!(eval(&host,first,"pinIdentity===originalPinIdentity && document.querySelector('input').value==='retained pinned form'",cx).await?==true,"unpin lost document state");
    host.forward(first)?;
    crate::smoke::wait_title(&host, first, "Pin second", cx).await?;
    Ok(vec![
        "native chip menu and captured header-drop completion pin live pages; cross-space popups preserve DOM/form/scroll/history state and page favicon; switching, repeat click, outside click and Escape dismiss; resize/order/session round trip; context-menu unpin targets focused pane; bounded narrow-window overflow",
    ])
}
