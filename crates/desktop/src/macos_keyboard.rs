//! Keep WebKit navigation keys out of GPUI's key-equivalent/input-context path.
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSView};
use objc2_web_kit::WKWebView;

fn is_arrow(key: &str) -> bool {
    matches!(key, "\u{f700}" | "\u{f701}" | "\u{f702}" | "\u{f703}")
}

/// Called by the local key-down monitor before AppKit tries key equivalents.
/// Wry child views return NO from performKeyEquivalent:, letting GPUI interpret
/// navigation keys through its own text input context or browser key bindings.
/// Deliver the original event once to WebKit instead, retaining native editing,
/// IME handling, modifiers, repeat state and cancelable DOM keyboard events.
pub fn route_arrow(event: &NSEvent, key: &str) -> bool {
    if !is_arrow(key) {
        return false;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(responder) = event.window(mtm).and_then(|window| window.firstResponder()) else {
        return false;
    };
    // WebKit currently makes WKWebView first responder. Also allow its native
    // subviews, without capturing arrows while a GPUI shell editor has focus.
    let mut current = Some(responder.clone());
    while let Some(node) = current {
        if node.downcast_ref::<WKWebView>().is_some() {
            responder.keyDown(event);
            return true;
        }
        if node.downcast_ref::<NSView>().is_none() {
            break;
        }
        // AppKit responder links are read on the main thread; each node is retained.
        current = unsafe { node.nextResponder() };
    }
    false
}
