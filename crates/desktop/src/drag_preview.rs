//! A separate native child surface keeps the drag card above native page pixels.
//! It has no page privileges and never navigates. Snapshot callbacks own only the
//! image slot for that drag, so cancelled/older captures cannot affect a new drag.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use anyhow::Result;
use base64::Engine as _;
use gpui::{Bounds, Pixels, Point, Size, Window, px, size};
use wry::{WebView, WebViewBuilder};

type Snapshot = Rc<RefCell<Option<String>>>;

pub struct DragPreview {
    view: WebView,
    image: Snapshot,
    loaded: Rc<Cell<bool>>,
    source: Option<Rc<WebView>>,
    source_ready: Box<dyn Fn() -> bool>,
    capture_started: Cell<bool>,
}

impl DragPreview {
    pub fn new(
        source: Option<Rc<WebView>>,
        source_ready: impl Fn() -> bool + 'static,
        title: &str,
        window: &Window,
    ) -> Result<Self> {
        let title = if title.is_empty() { "New tab" } else { title };
        let title = serde_json::to_string(title)?.replace('<', "\\u003c");
        let loaded = Rc::new(Cell::new(false));
        let ready = loaded.clone();
        let view = WebViewBuilder::new()
            .with_visible(false)
            .with_focused(false)
            .with_transparent(true)
            .with_on_page_load_handler(move |event, _| {
                ready.set(matches!(event, wry::PageLoadEvent::Finished));
            })
            .with_navigation_handler(|url| url == "about:blank")
            .with_html(format!(r#"<!doctype html><meta charset="utf-8">
<style>*{{box-sizing:border-box}}html,body{{margin:0;background:transparent}}body{{color:#dde5ed;font:12px -apple-system,Segoe UI,sans-serif}}.card{{height:100vh;background:#25303d;border:2px solid #8fddc3;border-radius:8px;overflow:hidden}}header{{height:30px;padding:6px 10px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis}}img{{display:block;width:100%;height:146px;object-fit:cover;object-position:top;background:#f5f6f8}}p{{position:absolute;top:85px;width:100%;text-align:center;color:#8491a2}}</style>
<div class="card"><header></header><p>Capturing page…</p><img></div><script>document.querySelector('header').textContent={title};</script>"#))
            .build_as_child(window)?;
        view.focus_parent()?;
        let image = Rc::new(RefCell::new(None));
        Ok(Self {
            view,
            image,
            loaded,
            source,
            source_ready: Box::new(source_ready),
            capture_started: Cell::new(false),
        })
    }

    #[cfg(feature = "smoke")]
    pub fn evaluate(&self, script: &str, callback: impl Fn(String) + Send + 'static) -> Result<()> {
        self.view.evaluate_script_with_callback(script, callback)?;
        Ok(())
    }

    pub fn update(&self, position: Point<Pixels>, viewport: Size<Pixels>) -> Result<()> {
        if !self.capture_started.get() && (self.source_ready)() {
            self.capture_started.set(true);
            if let Some(source) = &self.source
                && let Err(error) = capture(source, self.image.clone())
            {
                eprintln!("Drag snapshot: {error:#}");
                *self.image.borrow_mut() = Some(String::new());
            }
        }
        // Flip at window edges, keeping a gap so the native card never covers
        // the held cursor or intercepts a drop on the tab strip.
        let x = if position.x + px(260.) <= viewport.width {
            position.x + px(16.)
        } else {
            position.x - px(260.)
        };
        let y = if position.y + px(200.) <= viewport.height {
            position.y + px(20.)
        } else {
            position.y - px(200.)
        };
        self.view.set_bounds(crate::browser::native_bounds(Bounds {
            origin: gpui::point(x, y),
            size: size(px(244.), px(180.)),
        }))?;
        self.view.set_visible(true)?;
        if self.loaded.get()
            && let Some(image) = self.image.borrow_mut().take()
        {
            let script = if image.is_empty() {
                "document.querySelector('p').textContent='Page preview unavailable'".to_owned()
            } else {
                format!(
                    "document.querySelector('img').src={};document.querySelector('p').hidden=true",
                    serde_json::to_string(&image)?
                )
            };
            self.view.evaluate_script(&script)?;
        }
        Ok(())
    }
}

impl Drop for DragPreview {
    fn drop(&mut self) {
        let _ = self.view.set_visible(false);
    }
}

#[cfg(target_os = "macos")]
fn capture(source: &WebView, image: Snapshot) -> Result<()> {
    use block2::RcBlock;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError, NSNumber};
    use objc2_web_kit::WKSnapshotConfiguration;
    use wry::WebViewExtMacOS;
    let callback = RcBlock::new(move |snapshot: *mut NSImage, _error: *mut NSError| {
        // WebKit invokes this on the main thread. Encode PNG to keep the overlay
        // small and independent of the source document's lifetime.
        *image.borrow_mut() = Some(String::new());
        unsafe {
            if let Some(snapshot) = snapshot.as_ref()
                && let Some(tiff) = snapshot.TIFFRepresentation()
                && let Some(bitmap) = NSBitmapImageRep::imageRepWithData(&tiff)
                && let Some(png) = bitmap.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            {
                *image.borrow_mut() = Some(format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(png.to_vec())
                ));
            }
        }
    });
    unsafe {
        let config =
            WKSnapshotConfiguration::new(objc2::MainThreadMarker::new().expect("UI thread"));
        config.setSnapshotWidth(Some(&NSNumber::new_f64(488.)));
        source
            .webview()
            .takeSnapshotWithConfiguration_completionHandler(Some(&config), &callback)
    };
    Ok(())
}

#[cfg(target_os = "windows")]
fn capture(source: &WebView, image: Snapshot) -> Result<()> {
    use webview2_com::{
        CapturePreviewCompletedHandler,
        Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
    };
    use windows::Win32::{
        System::Com::{STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET},
        UI::Shell::SHCreateMemStream,
    };
    use wry::WebViewExtWindows;
    unsafe {
        let stream =
            SHCreateMemStream(None).ok_or_else(|| anyhow::anyhow!("Creating snapshot stream"))?;
        let captured = stream.clone();
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
            *image.borrow_mut() = Some(String::new());
            result?;
            let mut stat = STATSTG::default();
            captured.Stat(&mut stat, STATFLAG_NONAME)?;
            // Bound allocations even if the native engine returns unexpected data.
            if stat.cbSize > 32 * 1024 * 1024 {
                return Ok(());
            }
            captured.Seek(0, STREAM_SEEK_SET, None)?;
            let mut bytes = vec![0; stat.cbSize as usize];
            let mut read = 0;
            captured
                .Read(
                    bytes.as_mut_ptr().cast(),
                    bytes.len() as u32,
                    Some(&mut read),
                )
                .ok()?;
            bytes.truncate(read as usize);
            *image.borrow_mut() = Some(format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ));
            Ok(())
        }));
        source.webview().CapturePreview(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
            &stream,
            &handler,
        )?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn capture(_source: &WebView, _image: Snapshot) -> Result<()> {
    Ok(())
}
