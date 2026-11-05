//! Real attachment clicks through native pages, including retained pins and previews.
use crate::smoke::eval;
use crate::{
    app::BrowserApp,
    downloads::{DownloadItem, DownloadStatus},
    smoke::{Host, pause},
};
use anyhow::{Context as _, Result};
use gpui::*;
use serde_json::Value;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    time::Duration,
};
use workspace_core::TabId;

#[cfg(target_os = "macos")]
impl crate::downloads_ui::DownloadPopup {
    pub(super) fn test_click(&self, x: f64, y: f64) -> Result<()> {
        use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
        use objc2_foundation::NSPoint;
        use wry::WebViewExtMacOS;
        let view = self.view.as_ref().context("Missing downloads overlay")?;
        let native = view.webview();
        let window = view.ns_window();
        window.makeKeyWindow();
        view.focus()?;
        let point = NSPoint::new(
            x,
            if native.isFlipped() {
                y
            } else {
                native.frame().size.height - y
            },
        );
        let point = native.convertPoint_toView(point, None);
        for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
            let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind,point,NSEventModifierFlags::empty(),0.,window.windowNumber(),None,0,1,1.,
            ).context("Create native download control click")?;
            window.sendEvent(&event);
        }
        Ok(())
    }
}

pub(super) struct Cleanup {
    pub(super) host: Host,
    pub(super) prefix: String,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let items = self.host.downloads().borrow().items();
        for item in items {
            if item.filename.starts_with(&self.prefix) {
                let _ = self.host.downloads().borrow_mut().cancel(&item.id);
                let _ = std::fs::remove_file(&item.path);
                if let Some(parent) = item.path.parent() {
                    let _ =
                        std::fs::remove_file(parent.join(format!(".axolotl-{}.download", item.id)));
                }
            }
        }
    }
}

pub(super) fn fixture(prefix: &str) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", listener.local_addr()?);
    let prefix = prefix.to_owned();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let prefix = prefix.clone();
            std::thread::spawn(move || -> std::io::Result<()> {
                stream.set_read_timeout(Some(Duration::from_secs(3)))?;
                stream.set_write_timeout(Some(Duration::from_secs(3)))?;
                let mut request = [0u8; 4096];
                let count = stream.read(&mut request)?;
                let request = String::from_utf8_lossy(&request[..count]);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                if path == "/page" {
                    let html = format!(
                        r#"<!doctype html><title>Download fixture</title><style>body{{margin:0}}a{{position:absolute;left:20px;width:220px;height:32px;background:#e4eaff}}a:nth-of-type(1){{top:20px}}a:nth-of-type(2){{top:60px}}a:nth-of-type(3){{top:100px}}a:nth-of-type(4){{top:140px}}a:nth-of-type(5){{top:180px}}a:nth-of-type(6){{top:220px}}</style><a href="/same">Attachment</a><a href="/known">Known size</a><a href="/unknown">Unknown size</a><a href="/broken">Interrupted</a><a href="/pin">Pinned attachment</a><a href="/preview">Preview attachment</a><a id="blob" download="{prefix}-blob.txt" style="top:260px">Blob attachment</a><a href="/preview-slow" style="top:300px">Preview long attachment</a><script>blob.href=URL.createObjectURL(new Blob(['native blob contents'],{{type:'text/plain'}}));window.trustedClicks=0;document.addEventListener('click',e=>{{if(e.isTrusted)trustedClicks++}},true)</script>"#
                    );
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
                        html.len()
                    )?;
                    return Ok(());
                }
                let suffix = path.trim_start_matches('/');
                let name = format!("{prefix}-{suffix}.txt");
                let slow = matches!(path, "/known" | "/unknown" | "/preview-slow");
                let broken = path == "/broken";
                let body = if slow {
                    vec![b'x'; 16 * 1024]
                } else {
                    format!("native attachment contents: {suffix}").into_bytes()
                };
                let chunks = if path == "/known" { 256 } else { 128 };
                let length = if slow {
                    body.len() * chunks
                } else if broken {
                    body.len() + 1048576
                } else {
                    body.len()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"{name}\"\r\nConnection: close\r\n"
                )?;
                if path != "/unknown" {
                    write!(stream, "Content-Length: {length}\r\n")?;
                }
                write!(stream, "\r\n")?;
                for _ in 0..if slow { chunks } else { 1 } {
                    stream.write_all(&body)?;
                    stream.flush()?;
                    if slow {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
                Ok(())
            });
        }
    });
    Ok(origin)
}

pub(super) async fn click(
    view: &WeakEntity<BrowserApp>,
    host: &Host,
    tab: TabId,
    row: usize,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let before = eval(host, tab, "window.trustedClicks", cx)
            .await?
            .as_u64()
            .context("Missing trusted click fixture counter")?;
        for _ in 0..2 {
            view.update_in(cx, |_, window, cx| {
                window.activate_window();
                cx.activate(true);
            })?;
            host.focus(tab)?;
            pause(cx).await;
            host.test_click_at(tab, 100., 36. + row as f64 * 40., false)?;
            pause(cx).await;
            if eval(host, tab, "window.trustedClicks", cx)
                .await?
                .as_u64()
                .unwrap_or(0)
                > before
            {
                return Ok(());
            }
            // Retry only an event that never reached the page, so a download
            // which started successfully can never be duplicated by the test.
        }
        anyhow::bail!("Native click row {row} never reached tab {tab}");
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = view;
        eval(
            host,
            tab,
            &format!("document.querySelectorAll('a')[{row}].click();true"),
            cx,
        )
        .await?;
        pause(cx).await;
        Ok(())
    }
}

pub(super) async fn wait_item(
    host: &Host,
    suffix: &str,
    status: DownloadStatus,
    cx: &mut AsyncWindowContext,
) -> Result<DownloadItem> {
    for _ in 0..150 {
        if let Some(item) = host
            .downloads()
            .borrow()
            .items()
            .into_iter()
            .rev()
            .find(|item| item.filename.ends_with(suffix) && item.status == status)
        {
            return Ok(item);
        }
        pause(cx).await;
    }
    anyhow::bail!(
        "Download {suffix} did not become {status:?}: {:?}",
        host.downloads().borrow().items()
    )
}

pub(super) async fn popup_read(
    view: &WeakEntity<BrowserApp>,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> Result<Value> {
    // Wry queues only JS strings before didFinish; callbacks passed while the
    // document loads are discarded. Wait for the native document before eval.
    #[cfg(target_os = "macos")]
    {
        use wry::WebViewExtMacOS;
        for _ in 0..50 {
            let loading = view.update_in(cx, |app, _, _| {
                app.downloads_popup
                    .as_ref()
                    .and_then(|popup| popup.view.as_ref())
                    .is_none_or(|view| unsafe { view.webview().isLoading() })
            })?;
            if !loading {
                break;
            }
            pause(cx).await;
        }
    }
    let response = Arc::new(Mutex::new(None));
    let target = response.clone();
    view.update_in(cx, |app, _, _| {
        app.downloads_popup
            .as_ref()
            .context("Missing downloads popup")?
            .evaluate(script, move |v| *target.lock().unwrap() = Some(v))
    })??;
    for _ in 0..50 {
        if let Some(value) = response.lock().unwrap().take() {
            return serde_json::from_str(&value)
                .with_context(|| format!("Download popup evaluation: {value}"));
        }
        cx.background_executor()
            .timer(Duration::from_millis(20))
            .await;
    }
    let retained = view.update_in(cx, |app, _, _| app.downloads_popup.is_some())?;
    anyhow::bail!("Download popup evaluation timed out (retained={retained}): {script}")
}

#[cfg(target_os = "macos")]
pub(super) async fn snapshot(
    view: &WeakEntity<BrowserApp>,
    path: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    use block2::RcBlock;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use wry::WebViewExtMacOS;
    let output = path.to_owned();
    let callback = RcBlock::new(move |snapshot: *mut NSImage, _error: *mut NSError| unsafe {
        if let Some(snapshot) = snapshot.as_ref()
            && let Some(tiff) = snapshot.TIFFRepresentation()
            && let Some(bitmap) = NSBitmapImageRep::imageRepWithData(&tiff)
            && let Some(png) = bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )
        {
            let _ = std::fs::write(&output, png.to_vec());
        }
    });
    view.update_in(cx, |app, _, _| {
        let popup = app
            .downloads_popup
            .as_ref()
            .context("Missing downloads snapshot popup")?;
        let native = popup
            .view
            .as_ref()
            .context("Missing downloads snapshot view")?;
        unsafe {
            native
                .webview()
                .takeSnapshotWithConfiguration_completionHandler(None, &callback);
        }
        Ok::<_, anyhow::Error>(())
    })??;
    for _ in 0..50 {
        if std::path::Path::new(path).is_file() {
            eprintln!("Download smoke: saved native dropdown snapshot {path}");
            return Ok(());
        }
        pause(cx).await;
    }
    anyhow::bail!("Downloads snapshot did not finish")
}
