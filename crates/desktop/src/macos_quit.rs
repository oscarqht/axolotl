//! Reserve app-level shortcuts before WKWebView key-equivalent handling.
use block2::RcBlock;
use gpui::{App, Global};
use objc2::{rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags as Flags};
use std::{cell::Cell, ptr::NonNull};

struct QuitMonitor(Retained<AnyObject>);
impl Global for QuitMonitor {}
impl Drop for QuitMonitor {
    fn drop(&mut self) {
        // The token was returned by AppKit's event-monitor registration below.
        unsafe { NSEvent::removeMonitor(&self.0) };
    }
}

fn is_quit(key: &str, flags: Flags) -> bool {
    key.eq_ignore_ascii_case("q")
        && flags.contains(Flags::Command)
        && !flags.intersects(Flags::Control | Flags::Option | Flags::Shift)
}

fn is_settings(key: &str, flags: Flags) -> bool {
    key == ","
        && flags.contains(Flags::Command)
        && !flags.intersects(Flags::Control | Flags::Option | Flags::Shift)
}

/// Cmd+9 selects the ninth space, rather than a special last-space shortcut.
fn space_index(key: &str, flags: Flags) -> Option<usize> {
    if !flags.contains(Flags::Command)
        || flags.intersects(Flags::Control | Flags::Option | Flags::Shift)
    {
        return None;
    }
    match key.as_bytes() {
        [digit @ b'1'..=b'9'] => Some(usize::from(digit - b'1')),
        _ => None,
    }
}

pub fn install(cx: &mut App) {
    let app = cx.to_async();
    let executor = cx.foreground_executor().clone();
    let requested = Cell::new(false);
    let handler = RcBlock::new(move |event: NonNull<NSEvent>| {
        // AppKit invokes local monitors on the main thread with a live event.
        let event_ref = unsafe { event.as_ref() };
        let key = event_ref
            .charactersIgnoringModifiers()
            .map(|s| s.to_string());
        let flags = event_ref.modifierFlags();
        if key
            .as_deref()
            .is_some_and(|key| crate::macos_keyboard::route_arrow(event_ref, key))
        {
            return std::ptr::null_mut();
        }

        if let Some(index) = key.as_deref().and_then(|key| space_index(key, flags)) {
            let app = app.clone();
            executor
                .spawn(async move {
                    let _ = app.update(|cx| cx.dispatch_action(&crate::SelectSpace { index }));
                })
                .detach();
            // Reserve the shortcut before any WKWebView, including native controls.
            return std::ptr::null_mut();
        }
        if key.as_deref().is_some_and(|key| is_settings(key, flags)) {
            let app = app.clone();
            executor
                .spawn(async move {
                    let _ = app.update(|cx| cx.dispatch_action(&crate::Settings));
                })
                .detach();
            return std::ptr::null_mut();
        }
        if key.is_some_and(|key| is_quit(&key, flags)) {
            if !requested.replace(true) {
                let app = app.clone();
                // Do not borrow GPUI reentrantly from native event dispatch.
                executor
                    .spawn(async move {
                        let _ = app.update(|cx| cx.quit());
                    })
                    .detach();
            }
            std::ptr::null_mut()
        } else {
            event.as_ptr()
        }
    });
    // Return the original event, or nil after routing a reserved key.
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler)
    }
    .expect("register app shortcut event monitor");
    cx.set_global(QuitMonitor(monitor));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_numbers_select_exact_space_indices_only() {
        for number in 1..=9 {
            let key = number.to_string();
            assert_eq!(space_index(&key, Flags::Command), Some(number - 1));
            assert_eq!(
                space_index(&key, Flags::Command | Flags::CapsLock),
                Some(number - 1)
            );
            for flags in [
                Flags::empty(),
                Flags::Control,
                Flags::Command | Flags::Control,
                Flags::Command | Flags::Option,
                Flags::Command | Flags::Shift,
            ] {
                assert_eq!(space_index(&key, flags), None);
            }
        }
        for key in ["0", "10", "", "a", "!", "１"] {
            assert_eq!(space_index(key, Flags::Command), None);
        }
    }

    #[test]
    fn only_command_comma_opens_settings() {
        assert!(is_settings(",", Flags::Command));
        assert!(is_settings(",", Flags::Command | Flags::CapsLock));
        assert!(!is_settings("q", Flags::Command));
        for flags in [
            Flags::empty(),
            Flags::Control,
            Flags::Command | Flags::Control,
            Flags::Command | Flags::Option,
            Flags::Command | Flags::Shift,
        ] {
            assert!(!is_settings(",", flags));
        }
    }

    #[test]
    fn only_unmodified_command_q_requests_quit() {
        assert!(is_quit("q", Flags::Command));
        assert!(is_quit("Q", Flags::Command | Flags::CapsLock));
        for flags in [
            Flags::empty(),
            Flags::Control,
            Flags::Command | Flags::Control,
            Flags::Command | Flags::Option,
            Flags::Command | Flags::Shift,
        ] {
            assert!(!is_quit("q", flags));
        }
        assert!(!is_quit("w", Flags::Command));
    }
}
