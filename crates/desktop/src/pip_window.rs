//! A native, always-on-top home for the original page surface. Reparenting keeps
//! media streams, DRM sessions and playback position in the source web engine.
use crate::{native_webview::NativeWebView, pip_geometry::Geometry};
use anyhow::Result;
use gpui::{prelude::*, *};
use std::{cell::RefCell, rc::Rc};

struct Attachment {
    source: Rc<NativeWebView>,
    #[cfg(target_os = "macos")]
    parent: objc2::rc::Retained<objc2_app_kit::NSView>,
    #[cfg(target_os = "macos")]
    _window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
    #[cfg(target_os = "windows")]
    parent: isize,
    #[cfg(all(feature = "smoke", target_os = "windows"))]
    target: isize,
}

impl Attachment {
    fn new(source: Rc<NativeWebView>, window: &Window, bounds: Bounds<Pixels>) -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            let _ = bounds;
            use wry::{
                WebViewExtMacOS,
                raw_window_handle::{HasWindowHandle, RawWindowHandle},
            };
            let RawWindowHandle::AppKit(target) = HasWindowHandle::window_handle(window)?.as_raw()
            else {
                anyhow::bail!("PiP requires an AppKit window");
            };
            // GPUI owns the target view for the lifetime of this window.
            let target = unsafe { &*target.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
            let target = target
                .window()
                .ok_or_else(|| anyhow::anyhow!("PiP window is detached"))?;
            target.setHidesOnDeactivate(false);
            let webview = source.webview();
            let parent = unsafe { webview.superview() }
                .ok_or_else(|| anyhow::anyhow!("PiP source is detached"))?;
            let original_window = source.ns_window();
            #[cfg(feature = "smoke")]
            eprintln!("PiP native: reparenting WKWebView");
            source.reparent(objc2::rc::Retained::as_ptr(&target).cast_mut())?;
            #[cfg(feature = "smoke")]
            eprintln!("PiP native: reparented WKWebView");
            Ok(Self {
                source,
                parent,
                _window: original_window,
            })
        }
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::{
                Foundation::HWND,
                UI::WindowsAndMessaging::{
                    GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, HWND_TOPMOST, SWP_FRAMECHANGED,
                    SWP_NOACTIVATE, SetWindowLongPtrW, SetWindowPos, WS_CAPTION, WS_EX_APPWINDOW,
                    WS_EX_TOOLWINDOW, WS_SYSMENU, WS_THICKFRAME,
                },
            };
            use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let RawWindowHandle::Win32(target) = HasWindowHandle::window_handle(window)?.as_raw()
            else {
                anyhow::bail!("PiP requires a Win32 window");
            };
            let scale = window.scale_factor();
            unsafe {
                let native = HWND(target.hwnd.get() as _);
                // GPUI's Normal Windows style supplies resize/system-menu bits
                // without WS_CAPTION. Keep the OS titlebar and Close button.
                let style = GetWindowLongPtrW(native, GWL_STYLE) as u32;
                SetWindowLongPtrW(
                    native,
                    GWL_STYLE,
                    (style | WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0) as isize,
                );
                let extended = GetWindowLongPtrW(native, GWL_EXSTYLE) as u32;
                SetWindowLongPtrW(
                    native,
                    GWL_EXSTYLE,
                    ((extended & !WS_EX_APPWINDOW.0) | WS_EX_TOOLWINDOW.0) as isize,
                );
                SetWindowPos(
                    native,
                    Some(HWND_TOPMOST),
                    (f32::from(bounds.origin.x) * scale) as i32,
                    (f32::from(bounds.origin.y) * scale) as i32,
                    (f32::from(bounds.size.width) * scale) as i32,
                    (f32::from(bounds.size.height) * scale) as i32,
                    SWP_NOACTIVATE | SWP_FRAMECHANGED,
                )?;
            }
            let parent = source.reparent_for_picture_in_picture(window)?;
            Ok(Self {
                source,
                parent,
                #[cfg(feature = "smoke")]
                target: target.hwnd.get(),
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (source, window, bounds);
            anyhow::bail!("PiP requires macOS or Windows")
        }
    }

    fn geometry(&self, window: &Window) -> Result<Geometry> {
        #[cfg(target_os = "macos")]
        let anchor = objc2::rc::Retained::as_ptr(&self._window) as isize;
        #[cfg(target_os = "windows")]
        let anchor = self.parent;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let anchor = 0;
        Geometry::open(window, anchor)
    }

    fn restore(&self) -> Result<()> {
        // Hide before returning to the shell; the shell alone decides when this
        // tab should become visible again. Never let native window teardown own it.
        self.source.set_visible(false)?;
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS;
            self.parent.addSubview(&self.source.webview());
        }
        #[cfg(target_os = "windows")]
        self.source.restore_picture_in_picture_parent(self.parent)?;
        Ok(())
    }
}

#[derive(Default)]
struct State {
    attachment: Option<Attachment>,
    closed: bool,
    geometry: Option<Geometry>,
}

impl State {
    fn restore(&mut self) -> Result<()> {
        if let Some(geometry) = &mut self.geometry {
            geometry.remember();
        }
        if let Some(attachment) = &self.attachment {
            attachment.restore()?;
        }
        self.attachment = None;
        self.closed = true;
        Ok(())
    }
}

struct Surface(Rc<RefCell<State>>);
impl Render for Surface {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if let Some(attachment) = &self.0.borrow().attachment {
            let bounds = Bounds {
                origin: point(px(0.), px(0.)),
                size: window.viewport_size(),
            };
            if let Err(error) = attachment
                .source
                .set_bounds(crate::browser::native_bounds(bounds))
            {
                eprintln!("PiP size: {error}");
            }
        }
        div().size_full().bg(rgb(0x000000))
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        if let Err(error) = self.0.borrow_mut().restore() {
            eprintln!("PiP restore during teardown: {error}");
        }
    }
}

pub struct PipWindow {
    handle: WindowHandle<Surface>,
    state: Rc<RefCell<State>>,
    app: AsyncApp,
}

impl PipWindow {
    pub fn new(source: Rc<NativeWebView>, aspect: f64, cx: &mut App) -> Result<Self> {
        let aspect = if aspect.is_finite() && aspect > 0. {
            aspect.clamp(0.4, 3.)
        } else {
            16. / 9.
        };
        let initial_size = size(px(360.), px((360. / aspect) as f32));
        let bounds = Bounds::centered(None, initial_size, cx);
        let state = Rc::new(RefCell::new(State::default()));
        let surface_state = state.clone();
        let error = Rc::new(RefCell::new(None));
        let setup_error = error.clone();
        #[cfg(feature = "smoke")]
        eprintln!("PiP native: opening GPUI window");
        let handle = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Picture in Picture".into()),
                    appears_transparent: false,
                    ..Default::default()
                }),
                focus: false,
                // Position before showing, so initial placement never flashes
                // at the temporary construction bounds or activates the window.
                show: false,
                kind: if cfg!(target_os = "macos") {
                    WindowKind::PopUp
                } else {
                    WindowKind::Normal
                },
                is_resizable: true,
                is_minimizable: false,
                window_min_size: Some(size(px(180.), px(100.))),
                ..Default::default()
            },
            move |window, cx| {
                #[cfg(feature = "smoke")]
                eprintln!("PiP native: attaching source");
                match Attachment::new(source, window, bounds) {
                    Ok(attachment) => {
                        match attachment.geometry(window) {
                            Ok(geometry) => surface_state.borrow_mut().geometry = Some(geometry),
                            Err(error) => *setup_error.borrow_mut() = Some(error),
                        }
                        surface_state.borrow_mut().attachment = Some(attachment);
                    }
                    Err(error) => *setup_error.borrow_mut() = Some(error),
                }
                let closing = surface_state.clone();
                window.on_window_should_close(cx, move |_, _| {
                    match closing.borrow_mut().restore() {
                        Ok(()) => true,
                        Err(error) => {
                            eprintln!("PiP restore: {error}");
                            false
                        }
                    }
                });
                #[cfg(feature = "smoke")]
                eprintln!("PiP native: creating surface root");
                cx.new(|cx| {
                    cx.observe_window_bounds(window, |this: &mut Surface, _, _| {
                        if let Some(geometry) = &mut this.0.borrow_mut().geometry {
                            geometry.remember();
                        }
                    })
                    .detach();
                    Surface(surface_state)
                })
            },
        )?;
        #[cfg(feature = "smoke")]
        eprintln!("PiP native: opened window");
        let pip = Self {
            handle,
            state,
            app: cx.to_async(),
        };
        if let Some(error) = error.borrow_mut().take() {
            pip.close(cx)?;
            return Err(error);
        }
        // Moving a previously hidden child must not activate its new window.
        let visible = pip
            .state
            .borrow()
            .attachment
            .as_ref()
            .map(|attachment| attachment.source.set_visible(true));
        if let Some(Err(error)) = visible {
            pip.close(cx)?;
            return Err(error.into());
        }
        Ok(pip)
    }

    pub fn is_closed(&self, cx: &App) -> bool {
        self.state.borrow().closed || self.handle.read(cx).is_err()
    }

    pub fn close(&self, cx: &mut App) -> Result<()> {
        self.state.borrow_mut().restore()?;
        if self.handle.read(cx).is_ok() {
            self.handle
                .update(cx, |_, window, _| window.remove_window())?;
        }
        Ok(())
    }

    #[cfg(feature = "smoke")]
    pub fn test_adjust_geometry(&self, cx: &App) -> Result<()> {
        let adjust = self
            .state
            .borrow()
            .geometry
            .as_ref()
            .map(Geometry::test_adjust)
            .transpose()?;
        if let Some(adjust) = adjust {
            cx.foreground_executor()
                .spawn(async move {
                    adjust();
                })
                .detach();
        }
        Ok(())
    }

    #[cfg(feature = "smoke")]
    pub fn test_close(&self, cx: &mut App) -> Result<()> {
        let state = self.state.borrow();
        let Some(attachment) = &state.attachment else {
            return Ok(());
        };
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS;
            let native = attachment.source.ns_window();
            // Native close delegates enter GPUI. Run outside the current App
            // update, exactly as an OS close-button event would arrive.
            cx.foreground_executor()
                .spawn(async move {
                    native.performClose(None);
                })
                .detach();
        }
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::{
                Foundation::{HWND, LPARAM, WPARAM},
                UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE},
            };
            unsafe {
                PostMessageW(
                    Some(HWND(attachment.target as _)),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                )?;
            }
            let _ = cx;
        }
        Ok(())
    }

    #[cfg(feature = "smoke")]
    pub fn test_state(&self, cx: &App) -> serde_json::Value {
        let state = self.state.borrow();
        let Some(attachment) = &state.attachment else {
            return serde_json::json!({"closed":true});
        };
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS;
            let view = attachment.source.webview();
            let native = attachment.source.ns_window();
            let frame = view.frame();
            serde_json::json!({
                "geometry":state.geometry.as_ref().map(Geometry::test_state),
                "closed":self.is_closed(cx), "visible":native.isVisible(),
                "topmost":native.level() > 0, "title":native.title().to_string(),
                "sourceAttached":view.window().as_ref() == Some(&native),
                "width":frame.size.width, "height":frame.size.height,
            })
        }
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::{
                Foundation::{HWND, RECT},
                UI::WindowsAndMessaging::{
                    GWL_EXSTYLE, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, IsWindowVisible,
                    WS_EX_TOPMOST,
                },
            };
            let native = HWND(attachment.target as _);
            let mut bounds = RECT::default();
            let mut title = [0u16; 128];
            let title_len = unsafe { GetWindowTextW(native, &mut title) } as usize;
            let (visible, topmost) = unsafe {
                let _ = GetWindowRect(native, &mut bounds);
                (
                    IsWindowVisible(native).as_bool(),
                    (GetWindowLongPtrW(native, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0) != 0,
                )
            };
            let source = attachment.source.test_picture_in_picture_state();
            serde_json::json!({"geometry":state.geometry.as_ref().map(Geometry::test_state),"closed":self.is_closed(cx), "visible":visible, "topmost":topmost,
                "title":String::from_utf16_lossy(&title[..title_len]), "width":bounds.right-bounds.left,"height":bounds.bottom-bounds.top,
                "sourceAttached":source["parent"].as_i64() == Some(attachment.target as i64),
                "source":source})
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        serde_json::json!({"closed":self.is_closed(cx),"unsupported":true})
    }
}

impl Drop for PipWindow {
    fn drop(&mut self) {
        // The main BrowserApp can be released while this second GPUI window is
        // still alive. Restore the child immediately, then remove the empty PiP
        // window on the next foreground turn (Drop may run inside an App update).
        if let Err(error) = self.state.borrow_mut().restore() {
            eprintln!("PiP owner teardown: {error}");
        }
        let handle = self.handle;
        let app = self.app.clone();
        app.foreground_executor()
            .clone()
            .spawn(async move {
                let _ = app.update(|cx| {
                    if handle.read(cx).is_ok() {
                        let _ = handle.update(cx, |_, window, _| window.remove_window());
                    }
                });
            })
            .detach();
    }
}
