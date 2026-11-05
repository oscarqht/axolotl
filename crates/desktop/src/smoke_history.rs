//! Native acceptance flow, using deterministic HTTP pages and isolated history.
use crate::{
    app::{BrowserApp, TabTarget},
    smoke::{bar_eval, eval, pause, wait_title},
    smoke_settings::dialog_eval,
};
use anyhow::{Result, ensure};
use gpui::*;
use serde_json::Value;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
};

fn fixture() -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let base = format!("http://{}", listener.local_addr()?);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut request = [0; 4096];
            let Ok(size) = stream.read(&mut request) else {
                continue;
            };
            let request = String::from_utf8_lossy(&request[..size]);
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            let (status, headers, body) = match path {
                "/redirect" => ("302 Found", "Location: /one\r\n", ""),
                "/one" => (
                    "200 OK",
                    "",
                    "<title>History One</title><h1>History One</h1><iframe src='/frame'></iframe>",
                ),
                "/frame" => ("200 OK", "", "<title>Excluded subframe</title>Subframe"),
                _ => (
                    "200 OK",
                    "",
                    "<title>History Two</title><h1>History Two</h1>",
                ),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\n{headers}Content-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Ok(base)
}
async fn count(
    view: &WeakEntity<BrowserApp>,
    expected: i64,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    let mut actual = -1;
    for _ in 0..30 {
        pause(cx).await;
        actual = view.update_in(cx, |app, _, _| {
            app.browsing_history.as_ref().unwrap().count()
        })??;
        if actual == expected {
            return Ok(());
        }
    }
    anyhow::bail!("Expected {expected} visits, found {actual}")
}
async fn popup_eval(
    view: &WeakEntity<BrowserApp>,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> Result<Value> {
    let response = Arc::new(Mutex::new(None));
    let target = response.clone();
    view.update_in(cx, |app, _, _| {
        app.history_popup
            .as_ref()
            .unwrap()
            .view
            .evaluate_script_with_callback(script, move |value| {
                *target.lock().unwrap() = Some(value)
            })
    })??;
    for _ in 0..50 {
        if let Some(value) = response.lock().unwrap().take() {
            return Ok(serde_json::from_str(&value)?);
        }
        pause(cx).await;
    }
    anyhow::bail!("Suggestion evaluation timed out")
}
pub(crate) async fn run(
    view: &WeakEntity<BrowserApp>,
    cx: &mut AsyncWindowContext,
) -> Result<Vec<&'static str>> {
    let base = fixture()?;
    let (host, tab, pane, space) = view.update_in(cx, |app, _, _| {
        (
            app.host.clone(),
            app.active_tab(),
            app.model.focused_pane().id,
            app.model.active_space,
        )
    })?;
    count(view, 0, cx).await?;
    host.navigate(tab, &format!("{base}/redirect"))?;
    wait_title(&host, tab, "History One", cx).await?;
    count(view, 1, cx).await?;
    pause(cx).await;
    count(view, 1, cx).await?;
    ensure!(
        view.update_in(cx, |app, _, _| app
            .browsing_history
            .as_ref()
            .unwrap()
            .search("redirect"))??
            .is_empty(),
        "Recorded redirect intermediate"
    );
    ensure!(
        view.update_in(cx, |app, _, _| app
            .browsing_history
            .as_ref()
            .unwrap()
            .search("frame"))??
            .is_empty(),
        "Recorded subframe"
    );
    host.reload(tab)?;
    count(view, 2, cx).await?;
    host.navigate(tab, &format!("{base}/two"))?;
    wait_title(&host, tab, "History Two", cx).await?;
    count(view, 3, cx).await?;
    host.back(tab)?;
    wait_title(&host, tab, "History One", cx).await?;
    count(view, 4, cx).await?;
    host.forward(tab)?;
    wait_title(&host, tab, "History Two", cx).await?;
    count(view, 5, cx).await?;
    eval(&host,tab,"document.title='Late History Two';document.cookie='historySmoke=kept';window.historySmoke=42;true",cx).await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app
            .browsing_history
            .as_ref()
            .unwrap()
            .search("Late History"))??
            .len()
            == 1,
        "Late title missing"
    );
    // A refused local connection must not become a visit.
    let closed = TcpListener::bind("127.0.0.1:0")?;
    let failed = format!("http://{}/failed", closed.local_addr()?);
    drop(closed);
    host.navigate(tab, &failed)?;
    for _ in 0..5 {
        pause(cx).await;
    }
    count(view, 5, cx).await?;
    host.navigate(tab, &format!("{base}/one"))?;
    wait_title(&host, tab, "History One", cx).await?;
    count(view, 6, cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.begin_edit(TabTarget::Space(space), window, cx);
    })?;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        app.address
            .update(cx, |input, cx| input.set_value("Late History", window, cx))
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        view.update_in(cx, |app, _, _| app.history_popup.is_some())?,
        "Main popup missing"
    );
    ensure!(popup_eval(view,"({rows:document.querySelectorAll('button').length,title:document.querySelector('strong')?.textContent,url:document.querySelector('span')?.textContent,height:innerHeight})",cx).await?["rows"]==1,"Main suggestions missing");
    popup_eval(view, "window.historyPopupIdentity='retained';true", cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.address
            .update(cx, |input, cx| input.set_value("late history", window, cx));
    })?;
    for _ in 0..3 {
        pause(cx).await;
    }
    ensure!(
        popup_eval(view, "window.historyPopupIdentity", cx).await? == "retained",
        "typing recreated the native suggestion popup"
    );
    if std::env::args().any(|a| a == "--smoke-hold-history") {
        eprintln!("History preview: main suggestions");
        for _ in 0..100 {
            pause(cx).await;
        }
    }
    cx.update(|window, cx| window.dispatch_keystroke(Keystroke::parse("down").unwrap(), cx))?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.history_selected)? == Some(0),
        "Main ArrowDown did not select"
    );
    cx.update(|window, cx| window.dispatch_keystroke(Keystroke::parse("escape").unwrap(), cx))?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.editing.is_some()
            && app.history_matches.is_empty())?,
        "Escape must dismiss suggestions and retain editing"
    );
    view.update_in(cx, |app, window, cx| {
        app.address.update(cx, |input, cx| {
            input.set_value("late history two", window, cx)
        })
    })?;
    pause(cx).await;
    pause(cx).await;
    cx.update(|window, cx| window.dispatch_keystroke(Keystroke::parse("up").unwrap(), cx))?;
    pause(cx).await;
    cx.update(|window, cx| window.dispatch_keystroke(Keystroke::parse("enter").unwrap(), cx))?;
    wait_title(&host, tab, "History Two", cx).await?;
    ensure!(
        view.update_in(cx, |app, _, _| app.editing.is_none())?,
        "Main selected Enter did not navigate"
    );
    view.update_in(cx, |app, window, cx| {
        app.begin_edit(TabTarget::Space(space), window, cx)
    })?;
    pause(cx).await;
    view.update_in(cx, |app, window, cx| {
        app.address
            .update(cx, |input, cx| input.set_value("one", window, cx))
    })?;
    pause(cx).await;
    pause(cx).await;
    view.update_in(cx, |app, _, _| {
        app.history_popup
            .as_ref()
            .unwrap()
            .view
            .evaluate_script("document.querySelector('button').click()")
    })??;
    wait_title(&host, tab, "History One", cx).await?;
    ensure!(
        view.update_in(cx, |app, _, _| app.editing.is_none())?,
        "Main suggestion click did not navigate"
    );
    view.update_in(cx, |app, _, _| {
        for i in 0..10 {
            app.browsing_history.as_ref().unwrap().record(
                &format!("https://seed.test/{i}"),
                &format!("Seed {i}"),
                crate::browsing_history::now(),
            )?;
        }
        anyhow::Ok(())
    })??;
    view.update_in(cx, |app, window, cx| {
        app.begin_edit(TabTarget::Page(pane, tab), window, cx)
    })?;
    pause(cx).await;
    pause(cx).await;
    bar_eval(view,pane,"(()=>{const input=document.querySelector('input');input.value='seed';input.dispatchEvent(new Event('input',{bubbles:true}));return true})()",cx).await?;
    pause(cx).await;
    pause(cx).await;
    ensure!(
        bar_eval(view, pane, "historyItems.length", cx).await? == 8,
        "Floating history must cap at eight unique results"
    );
    bar_eval(view,pane,"(()=>{const input=document.querySelector('input');input.value='History';input.dispatchEvent(new Event('input',{bubbles:true}));return true})()",cx).await?;
    for _ in 0..3 {
        pause(cx).await;
    }
    let matches = bar_eval(view,pane,"({count:historyItems.length,visible:!document.querySelector('#history').hidden,fits:document.querySelector('#history').getBoundingClientRect().top>=0})",cx).await?;
    ensure!(
        matches["count"] == 2 && matches["visible"] == true && matches["fits"] == true,
        "Floating suggestions not visible: {matches}"
    );
    if std::env::args().any(|a| a == "--smoke-hold-history") {
        eprintln!("History preview: floating suggestions");
        for _ in 0..100 {
            pause(cx).await;
        }
    }
    bar_eval(view,pane,"document.querySelector('input').dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}));true",cx).await?;
    ensure!(
        bar_eval(view, pane, "historySelection", cx).await? == 0,
        "Floating ArrowDown failed"
    );
    bar_eval(view,pane,"document.querySelector('input').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));true",cx).await?;
    ensure!(
        bar_eval(view, pane, "document.querySelector('#history').hidden", cx).await? == true,
        "Floating Escape failed"
    );
    bar_eval(view,pane,"(()=>{const i=document.querySelector('input');i.value='one';i.dispatchEvent(new Event('input',{bubbles:true}));return true})()",cx).await?;
    pause(cx).await;
    pause(cx).await;
    bar_eval(
        view,
        pane,
        "document.querySelector('#history button').click();true",
        cx,
    )
    .await?;
    wait_title(&host, tab, "History One", cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.begin_edit(TabTarget::Page(pane, tab), window, cx)
    })?;
    pause(cx).await;
    pause(cx).await;
    bar_eval(view,pane,"(()=>{const i=document.querySelector('input');i.value='two';i.dispatchEvent(new Event('input',{bubbles:true}));return true})()",cx).await?;
    pause(cx).await;
    pause(cx).await;
    bar_eval(view,pane,"(()=>{const i=document.querySelector('input');for(const key of ['ArrowUp','Enter'])i.dispatchEvent(new KeyboardEvent('keydown',{key,bubbles:true}));return true})()",cx).await?;
    wait_title(&host, tab, "History Two", cx).await?;
    view.update_in(cx, |app, _, _| {
        app.browsing_history.as_ref().unwrap().record(
            "https://expired.test/",
            "Expired",
            crate::browsing_history::now() - 172800,
        )
    })??;
    // Retention and confirmed clearing use shipped settings and real IPC.
    eval(
        &host,
        tab,
        "window.historySmoke=42;document.cookie='historySmoke=kept';true",
        cx,
    )
    .await?;
    view.update_in(cx, |app, window, cx| app.open_settings(false, window, cx))?;
    pause(cx).await;
    pause(cx).await;
    dialog_eval(
        view,
        "retention.value='never';retention.dispatchEvent(new Event('change'));true",
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.preferences.history_retention_days)?
            .is_none(),
        "Never not applied"
    );
    dialog_eval(
        view,
        "retention.value='days';days.value='1';retention.dispatchEvent(new Event('change'));true",
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app.preferences.history_retention_days)? == Some(1),
        "Day retention not applied"
    );
    ensure!(
        view.update_in(cx, |app, _, _| app
            .browsing_history
            .as_ref()
            .unwrap()
            .search("expired"))??
            .is_empty(),
        "Retention change did not prune immediately"
    );
    view.update_in(cx, |app, _, _| {
        app.browsing_history.as_ref().unwrap().record(
            "https://periodic.test/",
            "Periodic",
            crate::browsing_history::now() - 172800,
        )?;
        app.last_history_prune = std::time::Instant::now() - std::time::Duration::from_secs(3601);
        anyhow::Ok(())
    })??;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app
            .browsing_history
            .as_ref()
            .unwrap()
            .search("periodic"))??
            .is_empty(),
        "Periodic pruning did not run"
    );
    let before = view.update_in(cx, |app, _, _| app.model.to_json())?;
    dialog_eval(
        view,
        "document.querySelector('#clear-history').click();true",
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        view.update_in(cx, |app, _, _| app
            .browsing_history
            .as_ref()
            .unwrap()
            .count())??
            > 0,
        "Cleared before confirmation"
    );
    dialog_eval(
        view,
        "document.querySelector('#cancel-clear').click();true",
        cx,
    )
    .await?;
    pause(cx).await;
    dialog_eval(view,"document.querySelector('#clear-history').click();document.querySelector('#confirm-clear').click();true",cx).await?;
    count(view, 0, cx).await?;
    ensure!(
        view.update_in(cx, |app, _, _| app.model.to_json())? == before,
        "Clear changed workspace"
    );
    ensure!(
        eval(
            &host,
            tab,
            "window.historySmoke===42&&document.cookie.includes('historySmoke=kept')",
            cx
        )
        .await?
            == true,
        "Clear changed live page or cookies"
    );
    view.update_in(cx, |app, window, cx| app.close_settings(window, cx))?;
    host.back(tab)?;
    count(view, 1, cx).await?;
    Ok(vec![
        "native history: successful main-frame visits, redirect/subframe/internal/failure exclusion, reload and Back/Forward repeats, late titles, main popup click and ArrowDown/Escape, floating suggestions/click/ArrowDown/Escape, retention IPC, confirmation/cancellation, clear preserves workspace, cookies and Back/Forward",
    ])
}
