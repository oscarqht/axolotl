//! Native outer-window rectangles and usable screen areas. macOS uses Cocoa
//! global coordinates; Windows uses physical desktop pixels throughout.
use super::Rect;
use anyhow::{Result, anyhow};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(super) struct Native {
    window: objc2::rc::Retained<objc2::runtime::AnyObject>,
    anchor: objc2::rc::Retained<objc2::runtime::AnyObject>,
}

#[cfg(target_os = "macos")]
fn rect(frame: objc2_foundation::NSRect) -> Rect {
    Rect {
        x: frame.origin.x,
        y: frame.origin.y,
        width: frame.size.width,
        height: frame.size.height,
    }
}

#[cfg(target_os = "macos")]
impl Native {
    pub(super) fn new(window: &gpui::Window, anchor: isize) -> Result<Self> {
        use objc2::{msg_send, rc::Retained, runtime::AnyObject};
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)?.as_raw()
        else {
            return Err(anyhow!("PiP geometry requires an AppKit window"));
        };
        // GPUI owns the target view; keep both windows alive while geometry
        // observers can run, including the final callback before native close.
        unsafe {
            let view = handle.ns_view.as_ptr().cast::<AnyObject>();
            let native: *mut AnyObject = msg_send![view, window];
            Ok(Self {
                window: Retained::retain(native)
                    .ok_or_else(|| anyhow!("PiP window is detached"))?,
                anchor: Retained::retain(anchor as *mut AnyObject)
                    .ok_or_else(|| anyhow!("PiP source window is detached"))?,
            })
        }
    }

    pub(super) fn capture(&self) -> Result<Rect> {
        use objc2::msg_send;
        Ok(rect(unsafe { msg_send![&*self.window, frame] }))
    }

    pub(super) fn area(&self, saved: Option<Rect>) -> Result<Rect> {
        use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
        unsafe {
            let source: *mut AnyObject = msg_send![&*self.anchor, screen];
            let screen = if source.is_null() {
                let main: *mut AnyObject = msg_send![class!(NSScreen), mainScreen];
                main
            } else {
                source
            };
            if screen.is_null() {
                return Err(anyhow!("PiP cannot find an available screen"));
            }
            let mut area = rect(msg_send![screen, visibleFrame]);
            if let Some(saved) = saved {
                let screens: Retained<AnyObject> = msg_send![class!(NSScreen), screens];
                let count: usize = msg_send![&*screens, count];
                let mut best = 0.;
                for index in 0..count {
                    let screen: *mut AnyObject = msg_send![&*screens, objectAtIndex: index];
                    let candidate = rect(msg_send![screen, visibleFrame]);
                    let overlap = saved.overlap(candidate);
                    if overlap > best {
                        area = candidate;
                        best = overlap;
                    }
                }
            }
            Ok(area)
        }
    }

    pub(super) fn apply(&self, bounds: Rect) -> Result<()> {
        use objc2::msg_send;
        use objc2_foundation::{NSPoint, NSRect, NSSize};
        let frame = NSRect::new(
            NSPoint::new(bounds.x, bounds.y),
            NSSize::new(bounds.width, bounds.height),
        );
        unsafe {
            let level: isize = msg_send![&*self.window, level];
            if level < 3 {
                let _: () = msg_send![&*self.window, setLevel: 3isize];
            }
            // Changing frame does not make this nonactivating panel key.
            let _: () = msg_send![&*self.window, setFrame: frame, display: true];
            let _: () = msg_send![&*self.window, orderFrontRegardless];
        }
        Ok(())
    }

    pub(super) fn padding(&self) -> f64 {
        24.
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
pub(super) struct Native {
    window: windows::Win32::Foundation::HWND,
    anchor: windows::Win32::Foundation::HWND,
}

#[cfg(target_os = "windows")]
fn rect(frame: windows::Win32::Foundation::RECT) -> Rect {
    Rect {
        x: frame.left as f64,
        y: frame.top as f64,
        width: (frame.right - frame.left) as f64,
        height: (frame.bottom - frame.top) as f64,
    }
}

#[cfg(target_os = "windows")]
impl Native {
    pub(super) fn new(window: &gpui::Window, anchor: isize) -> Result<Self> {
        use windows::Win32::Foundation::HWND;
        let RawWindowHandle::Win32(handle) = HasWindowHandle::window_handle(window)?.as_raw()
        else {
            return Err(anyhow!("PiP geometry requires a Win32 window"));
        };
        Ok(Self {
            window: HWND(handle.hwnd.get() as _),
            anchor: HWND(anchor as _),
        })
    }

    pub(super) fn capture(&self) -> Result<Rect> {
        use windows::Win32::{Foundation::RECT, UI::WindowsAndMessaging::GetWindowRect};
        let mut bounds = RECT::default();
        unsafe { GetWindowRect(self.window, &mut bounds)? };
        Ok(rect(bounds))
    }

    pub(super) fn area(&self, saved: Option<Rect>) -> Result<Rect> {
        use windows::Win32::{
            Foundation::RECT,
            Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
                MonitorFromWindow,
            },
        };
        let monitor = unsafe {
            if let Some(saved) = saved {
                let bounds = RECT {
                    left: saved.x.round() as i32,
                    top: saved.y.round() as i32,
                    right: (saved.x + saved.width).round() as i32,
                    bottom: (saved.y + saved.height).round() as i32,
                };
                MonitorFromRect(&bounds, MONITOR_DEFAULTTONEAREST)
            } else {
                MonitorFromWindow(self.anchor, MONITOR_DEFAULTTONEAREST)
            }
        };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetMonitorInfoW(monitor, &mut info).ok()? };
        Ok(rect(info.rcWork))
    }

    pub(super) fn apply(&self, bounds: Rect) -> Result<()> {
        use windows::Win32::UI::WindowsAndMessaging::{
            HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos,
        };
        unsafe {
            SetWindowPos(
                self.window,
                Some(HWND_TOPMOST),
                bounds.x.round() as i32,
                bounds.y.round() as i32,
                bounds.width.round() as i32,
                bounds.height.round() as i32,
                SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
            )?;
        }
        Ok(())
    }

    pub(super) fn padding(&self) -> f64 {
        use windows::Win32::UI::HiDpi::GetDpiForWindow;
        let dpi = unsafe { GetDpiForWindow(self.anchor) };
        24. * if dpi == 0 { 1. } else { dpi as f64 / 96. }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[derive(Clone)]
pub(super) struct Native;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Native {
    pub(super) fn new(_: &gpui::Window, _: isize) -> Result<Self> {
        Err(anyhow!("PiP geometry requires macOS or Windows"))
    }
    pub(super) fn capture(&self) -> Result<Rect> {
        Err(anyhow!("PiP geometry requires macOS or Windows"))
    }
    pub(super) fn area(&self, _: Option<Rect>) -> Result<Rect> {
        self.capture()
    }
    pub(super) fn apply(&self, _: Rect) -> Result<()> {
        self.capture().map(|_| ())
    }
    pub(super) fn padding(&self) -> f64 {
        24.
    }
}
