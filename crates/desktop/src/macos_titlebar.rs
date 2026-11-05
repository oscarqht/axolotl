//! Route window movement exclusively through the empty custom titlebar region.
//! Native titlebar dragging happens before GPUI event propagation, so stopping a
//! space card's mouse event does not prevent AppKit from moving the window.
use anyhow::{Context as _, Result};
use gpui::{App, Window};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSView, NSWindow};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

fn native_window(window: &Window) -> Result<Retained<NSWindow>> {
    let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)?.as_raw() else {
        anyhow::bail!("Expected an AppKit window");
    };
    // GPUI owns this NSView for the lifetime of the borrowed window. Callers run
    // on the UI thread; retain the NSWindow when scheduling its tracking loop.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    view.window().context("Missing native window")
}

pub fn install(window: &Window) -> Result<()> {
    let window = native_window(window)?;
    window.setMovableByWindowBackground(false);
    window.setMovable(false);
    Ok(())
}

pub fn start_window_move(window: &Window, cx: &mut App) -> Result<()> {
    let mtm = MainThreadMarker::new().context("Window dragging requires the UI thread")?;
    let window = native_window(window)?;
    let event = NSApplication::sharedApplication(mtm)
        .currentEvent()
        .context("Missing titlebar mouse event")?;
    // AppKit runs a tracking loop here; defer until GPUI has released the view's
    // update borrow so native resize/move callbacks can safely reenter the app.
    cx.defer(move |_| {
        let movement = NativeMove::new(window);
        movement.0.performWindowDragWithEvent(&event);
    });
    Ok(())
}

// Restore the lock even if a native tracking callback unwinds or returns early.
struct NativeMove(Retained<NSWindow>);
impl NativeMove {
    fn new(window: Retained<NSWindow>) -> Self {
        window.setMovable(true);
        Self(window)
    }
}
impl Drop for NativeMove {
    fn drop(&mut self) {
        self.0.setMovable(false);
    }
}

#[cfg(feature = "smoke")]
pub fn check_drag_policy(window: &Window) -> Result<()> {
    let window = native_window(window)?;
    anyhow::ensure!(
        !window.isMovable(),
        "native titlebar drag must be disabled over cards"
    );
    anyhow::ensure!(
        !window.isMovableByWindowBackground(),
        "native background drag must be disabled"
    );
    {
        let movement = NativeMove::new(window.clone());
        anyhow::ensure!(
            movement.0.isMovable(),
            "empty titlebar must allow explicit movement"
        );
    }
    anyhow::ensure!(!window.isMovable(), "window movement lock was not restored");
    Ok(())
}
