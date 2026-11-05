//! Native download acceptance checks using trusted AppKit clicks on macOS.
use crate::smoke::eval;
use crate::{
    app::BrowserApp,
    downloads::DownloadStatus,
    smoke::{pause, wait_title},
};
use anyhow::{Context as _, Result, ensure};
use gpui::*;
#[path = "smoke_downloads_support.rs"]
mod support;
use support::{Cleanup, click, fixture, popup_read, wait_item};

pub(crate) async fn run(
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
    let prefix = format!("axolotl-smoke-{}", uuid::Uuid::new_v4().simple());
    let _cleanup = Cleanup {
        host: host.clone(),
        prefix: prefix.clone(),
    };
    let origin = fixture(&prefix)?;
    host.navigate(first, &format!("{origin}/page"))?;
    wait_title(&host, first, "Download fixture", cx).await?;
    view.update_in(cx, |app, window, cx| app.toggle_downloads(window, cx))?;
    pause(cx).await;
    ensure!(
        popup_read(view, "!document.querySelector('#empty').hidden", cx).await? == true,
        "Empty download state missing"
    );
    view.update_in(cx, |app, window, cx| app.toggle_downloads(window, cx))?;
    click(view, &host, first, 0, cx).await?;
    let same = wait_item(&host, "-same.txt", DownloadStatus::Completed, cx).await?;
    ensure!(
        std::fs::read(&same.path)? == b"native attachment contents: same",
        "Downloaded bytes differ"
    );
    #[cfg(target_os = "macos")]
    ensure!(
        same.path.parent()
            == Some(
                std::path::Path::new(&std::env::var("HOME")?)
                    .join("Downloads")
                    .as_path()
            ),
        "Download did not use ~/Downloads"
    );
    click(view, &host, first, 0, cx).await?;
    let duplicate = wait_item(&host, "-same (1).txt", DownloadStatus::Completed, cx).await?;
    ensure!(
        duplicate.path != same.path
            && std::fs::read(&duplicate.path)? == std::fs::read(&same.path)?,
        "Duplicate overwrote original or changed contents"
    );
    click(view, &host, first, 1, cx).await?;
    click(view, &host, first, 2, cx).await?;
    let mut progress = None;
    for _ in 0..50 {
        let items = host.downloads().borrow().items();
        let known = items.iter().find(|i| {
            i.filename.ends_with("-known.txt")
                && i.status == DownloadStatus::Downloading
                && i.received > 0
                && i.total.is_some()
        });
        let unknown = items.iter().find(|i| {
            i.filename.ends_with("-unknown.txt")
                && i.status == DownloadStatus::Downloading
                && i.total.is_none()
        });
        if let (Some(known), Some(_)) = (known, unknown) {
            progress = Some(known.clone());
            break;
        }
        pause(cx).await;
    }
    if progress.is_none() {
        let page=eval(&host,first,"({clicks:trustedClicks,known:document.elementFromPoint(100,76)?.outerHTML,unknown:document.elementFromPoint(100,116)?.outerHTML,loading:document.readyState})",cx).await?;
        anyhow::bail!(
            "Concurrent progress did not arrive; page={page}; items={:?}",
            host.downloads().borrow().items()
        );
    }
    let known = progress.context("Concurrent known/unknown progress did not arrive")?;
    view.update_in(cx, |app, window, cx| app.toggle_downloads(window, cx))?;
    pause(cx).await;
    for _ in 0..30 {
        if popup_read(view, "document.querySelectorAll('.item').length", cx)
            .await?
            .as_u64()
            .unwrap_or(0)
            >= 4
        {
            break;
        }
        pause(cx).await;
    }
    ensure!(popup_read(view,"(()=>{const rows=[...document.querySelectorAll('.item')];const unknown=rows.find(r=>r.querySelector('.filename').textContent.endsWith('-unknown.txt'));const known=rows.find(r=>r.querySelector('.filename').textContent.endsWith('-known.txt'));return !!unknown?.querySelector('progress:not([value])')&&!!known?.querySelector('progress[value]')&&rows.some(r=>r.querySelector('[data-action=reveal]'))})()",cx).await?==true,"Known/indeterminate progress or completed reveal action missing");
    let selector = format!(".item[data-id=\"{}\"] button[data-action=cancel]", known.id);
    let selector_json = serde_json::to_string(&selector)?;
    popup_read(view,&format!("window.smokeCancel=document.querySelector({selector_json});window.smokePointer=[];['pointerdown','pointerup','click'].forEach(t=>document.addEventListener(t,e=>smokePointer.push({{type:t,x:e.clientX,y:e.clientY,target:e.target.outerHTML,trusted:e.isTrusted}}),true));true"),cx).await?;
    pause(cx).await;
    pause(cx).await;
    ensure!(
        popup_read(
            view,
            &format!(
                "smokeCancel.isConnected&&document.querySelector({selector_json})===smokeCancel"
            ),
            cx
        )
        .await?
            == true,
        "Progress updates replaced the held Cancel button"
    );
    view.update_in(cx, |_, window, cx| {
        window.activate_window();
        cx.activate(true);
    })?;
    pause(cx).await;
    let button = popup_read(view,&format!("(()=>{{const r=document.querySelector({selector_json}).getBoundingClientRect();return [r.x+r.width/2,r.y+r.height/2]}})()"),cx).await?;
    #[cfg(target_os = "macos")]
    view.update_in(cx, |app, _, _| {
        app.downloads_popup
            .as_ref()
            .unwrap()
            .test_click(button[0].as_f64().unwrap(), button[1].as_f64().unwrap())
    })??;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = button;
        popup_read(
            view,
            &format!("document.querySelector({selector_json}).click();true"),
            cx,
        )
        .await?;
    }
    pause(cx).await;
    #[cfg(target_os = "macos")]
    {
        let delivery = popup_read(view, "window.smokePointer", cx).await?;
        ensure!(
            delivery
                .as_array()
                .is_some_and(|events| events.iter().any(|e| e["type"] == "click"
                    && e["trusted"] == true
                    && e["target"]
                        .as_str()
                        .is_some_and(|s| s.contains("data-action=\"cancel\"")))),
            "Native Cancel click missed overlay: {delivery}"
        );
    }
    let cancelled = wait_item(&host, "-known.txt", DownloadStatus::Cancelled, cx).await?;
    ensure!(
        !cancelled.path.exists()
            && !cancelled
                .path
                .parent()
                .unwrap()
                .join(format!(".axolotl-{}.download", cancelled.id))
                .exists(),
        "Cancelled download left a file"
    );
    let unknown = wait_item(&host, "-unknown.txt", DownloadStatus::Completed, cx).await?;
    ensure!(
        std::fs::metadata(&unknown.path)?.len() == 2 * 1024 * 1024,
        "Unknown-length file truncated"
    );
    view.update_in(cx, |app, window, cx| {
        window.activate_window();
        cx.activate(true);
        if app.downloads_popup.is_none() {
            app.toggle_downloads(window, cx);
        }
    })?;
    pause(cx).await;
    ensure!(
        popup_read(
            view,
            "document.querySelectorAll('#items .item').length>=4",
            cx
        )
        .await?
            == true,
        "Popup omitted session downloads"
    );
    view.update_in(cx, |app, window, cx| app.toggle_downloads(window, cx))?;
    eprintln!("Download smoke: failure click");
    click(view, &host, first, 3, cx).await?;
    let failed = wait_item(&host, "-broken.txt", DownloadStatus::Failed, cx).await?;
    ensure!(
        failed.error.is_some() && !failed.path.exists(),
        "Failed transfer missing error or left partial file"
    );
    click(view, &host, first, 6, cx).await?;
    let blob = wait_item(&host, "-blob.txt", DownloadStatus::Completed, cx).await?;
    ensure!(
        std::fs::read(&blob.path)? == b"native blob contents",
        "Blob download contents differ"
    );
    view.update_in(cx, |app, window, cx| {
        app.pin_page(pane, first, window, cx);
        app.toggle_pin(first, window, cx);
    })?;
    pause(cx).await;
    eprintln!("Download smoke: pin click");
    click(view, &host, first, 4, cx).await?;
    wait_item(&host, "-pin.txt", DownloadStatus::Completed, cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.dismiss_pin(window, cx);
        let source = app.active_tab();
        app.open_link_popup(source, format!("{origin}/page"), window, cx);
    })?;
    wait_title(&host, crate::link_popup::POPUP_TAB, "Download fixture", cx).await?;
    pause(cx).await;
    eprintln!("Download smoke: preview click");
    click(view, &host, crate::link_popup::POPUP_TAB, 5, cx).await?;
    wait_item(&host, "-preview.txt", DownloadStatus::Completed, cx).await?;
    click(view, &host, crate::link_popup::POPUP_TAB, 7, cx).await?;
    wait_item(&host, "-preview-slow.txt", DownloadStatus::Downloading, cx).await?;
    view.update_in(cx, |app, window, cx| {
        app.close_link_popup(window, cx);
        app.toggle_downloads(window, cx);
    })?;
    pause(cx).await;
    let retired = wait_item(&host, "-preview-slow.txt", DownloadStatus::Completed, cx).await?;
    ensure!(
        std::fs::metadata(&retired.path)?.len() == 2 * 1024 * 1024,
        "Closing preview interrupted active transfer"
    );
    ensure!(
        popup_read(
            view,
            "document.querySelectorAll('#items .item').length>=9",
            cx
        )
        .await?
            == true,
        "Closing preview lost session downloads"
    );
    if std::env::args().any(|arg| arg == "--smoke-downloads-actions") {
        for action in ["open", "reveal"] {
            let status = view.update_in(cx, |app, _, _| app.status.clone())?;
            let script = format!(
                "document.querySelector('.item[data-id=\"{}\"] button[data-action={action}]').click()",
                duplicate.id
            );
            view.update_in(cx, |app, _, _| {
                app.downloads_popup
                    .as_ref()
                    .unwrap()
                    .view
                    .as_ref()
                    .unwrap()
                    .evaluate_script(&script)
            })??;
            pause(cx).await;
            pause(cx).await;
            view.update_in(cx, |app, window, cx| {
                ensure!(
                    app.status == status,
                    "Explicit {action} action failed: {}",
                    app.status
                );
                window.activate_window();
                cx.activate(true);
                if app.downloads_popup.is_none() {
                    app.toggle_downloads(window, cx);
                }
                Ok::<_, anyhow::Error>(())
            })??;
            pause(cx).await;
        }
        eprintln!("Download smoke: explicit Open and Reveal launches accepted");
    }
    #[cfg(target_os = "macos")]
    if let Some(path) = std::env::args().find_map(|arg| {
        arg.strip_prefix("--smoke-downloads-snapshot=")
            .map(str::to_owned)
    }) {
        support::snapshot(view, &path, cx).await?;
    }
    if std::env::args().any(|arg| arg == "--smoke-downloads-hold") {
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS;
            let number = view.update_in(cx, |app, _, _| {
                app.downloads_popup
                    .as_ref()
                    .unwrap()
                    .view
                    .as_ref()
                    .unwrap()
                    .ns_window()
                    .windowNumber()
            })?;
            eprintln!(
                "Download smoke: holding native dropdown for visual inspection (20 seconds), window={number}"
            );
        }
        #[cfg(not(target_os = "macos"))]
        eprintln!("Download smoke: holding native dropdown for visual inspection (20 seconds)");
        for _ in 0..100 {
            pause(cx).await;
        }
    }
    // Removing our completed fixture lets the Open action exercise its visible error
    // path without launching another application in an unattended native smoke run.
    std::fs::remove_file(&same.path)?;
    popup_read(
        view,
        &format!(
            "document.querySelector('.item[data-id=\"{}\"] button[data-action=open]').click();true",
            same.id
        ),
        cx,
    )
    .await?;
    pause(cx).await;
    ensure!(
        popup_read(
            view,
            "document.querySelector('#message').textContent.length>0",
            cx
        )
        .await?
            == true,
        "Open action error was not shown"
    );
    #[cfg(target_os = "macos")]
    ensure!(
        eval(&host, first, "trustedClicks>=7", cx).await? == true,
        "Attachment checks did not deliver native trusted clicks"
    );
    Ok(vec![
        "native attachment clicks save suggested filenames and bytes in Downloads; collisions use numbered suffixes",
        "concurrent known/unknown-length downloads report progress; native overlay Cancel reaches handler above pages and cleans partial files",
        "truncated responses fail visibly; blob, pinned-page and preview downloads share session history; preview transfers finish after closing their source",
        "download dropdown retains terminal items and reports explicit Open errors without opening files automatically",
    ])
}
