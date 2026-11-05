//! AppKit notifications avoid sampling the animated full-screen transition.
use super::*;
use block2::RcBlock;
use objc2::{
    MainThreadMarker, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use std::{
    cell::{Cell, RefCell},
    ptr::NonNull,
    rc::Rc,
};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

fn rect(frame: NSRect) -> Rect {
    Rect {
        x: frame.origin.x,
        y: frame.origin.y,
        width: frame.size.width,
        height: frame.size.height,
    }
}
fn frame(rect: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.x, rect.y),
        NSSize::new(rect.width, rect.height),
    )
}

struct Native {
    window: Retained<AnyObject>,
    store: RefCell<Store>,
    transitioning: Cell<bool>,
    candidate: RefCell<Option<(SavedWindow, Instant)>>,
}
impl Native {
    fn capture(&self) {
        if self.transitioning.get() {
            return;
        }
        // All access is on AppKit's main thread; the retained NSWindow belongs to
        // this GPUI window. Native isZoomed includes AppKit's standard-frame zoom.
        unsafe {
            let fullscreen: usize = msg_send![&*self.window, styleMask];
            let mut state = self.store.borrow().state.clone();
            state.version = 1;
            state.fullscreen = fullscreen & (1 << 14) != 0;
            if !state.fullscreen {
                state.maximized = msg_send![&*self.window, isZoomed];
                if !state.maximized {
                    let bounds = rect(msg_send![&*self.window, frame]);
                    if bounds.valid() {
                        state.normal = Some(bounds);
                    }
                }
            }
            let mut candidate = self.candidate.borrow_mut();
            match &*candidate {
                Some((previous, since)) if *previous == state => {
                    if since.elapsed() >= Duration::from_millis(500) {
                        self.store.borrow_mut().set(state);
                    }
                }
                _ => *candidate = Some((state, Instant::now())),
            }
        }
    }
    fn commit(&self) {
        if !self.transitioning.get()
            && let Some((state, _)) = self.candidate.borrow().as_ref()
        {
            self.store.borrow_mut().set(state.clone());
        }
    }
}

pub(crate) struct WindowSaver {
    native: Rc<Native>,
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}
impl WindowSaver {
    pub fn new(saved: SavedWindow, window: &mut Window, _cx: &mut App) -> Self {
        MainThreadMarker::new().expect("window state on UI thread");
        let RawWindowHandle::AppKit(handle) =
            window.window_handle().expect("native handle").as_raw()
        else {
            unreachable!()
        };
        unsafe {
            let view = handle.ns_view.as_ptr().cast::<AnyObject>();
            let nswindow: *mut AnyObject = msg_send![view, window];
            let nswindow = Retained::retain(nswindow).expect("NSWindow");
            if let Some(normal) = saved.normal {
                // Cocoa coordinates remain consistent across displays. Pick the
                // screen with the largest overlap; disconnected screens fall back
                // to the main display. Clamp to visibleFrame (menu bar and Dock).
                let screens: Retained<AnyObject> = msg_send![objc2::class!(NSScreen), screens];
                let count: usize = msg_send![&*screens, count];
                let main: *mut AnyObject = msg_send![objc2::class!(NSScreen), mainScreen];
                let mut area = rect(msg_send![main, visibleFrame]);
                let mut best = 0.;
                for i in 0..count {
                    let screen: *mut AnyObject = msg_send![&*screens, objectAtIndex: i];
                    let candidate = rect(msg_send![screen, visibleFrame]);
                    let overlap = (normal.x + normal.width).min(candidate.x + candidate.width)
                        - normal.x.max(candidate.x);
                    let height = (normal.y + normal.height).min(candidate.y + candidate.height)
                        - normal.y.max(candidate.y);
                    let overlap = overlap.max(0.) * height.max(0.);
                    if overlap > best {
                        area = candidate;
                        best = overlap;
                    }
                }
                let _: () = msg_send![&*nswindow, setFrame: frame(normal.fit(area)), display: true];
            }
            let maximize = saved.maximized;
            let fullscreen = saved.fullscreen;
            let native = Rc::new(Native {
                window: nswindow,
                store: RefCell::new(Store {
                    state: saved,
                    written: None,
                    changed: Instant::now(),
                    path: path(),
                }),
                transitioning: Cell::new(false),
                candidate: RefCell::new(None),
            });
            // Establish normal bounds before changing mode, so AppKit also knows
            // the correct unzoom/exit-full-screen frame after restoration.
            if !maximize {
                native.capture();
                native.commit();
            }
            let center = NSNotificationCenter::defaultCenter();
            let mut observers = Vec::new();
            for name in [
                "NSWindowDidMoveNotification",
                "NSWindowDidResizeNotification",
                "NSWindowWillEnterFullScreenNotification",
                "NSWindowDidEnterFullScreenNotification",
                "NSWindowWillExitFullScreenNotification",
                "NSWindowDidExitFullScreenNotification",
                "NSWindowDidFailToEnterFullScreenNotification",
                "NSWindowDidFailToExitFullScreenNotification",
                "NSWindowWillCloseNotification",
            ] {
                let observed_window = native.window.clone();
                let native = native.clone();
                let block = RcBlock::new(move |_: NonNull<NSNotification>| {
                    match name {
                        "NSWindowWillEnterFullScreenNotification" => {
                            native.capture();
                            native.commit();
                            native.candidate.borrow_mut().take();
                            native.transitioning.set(true);
                            let mut store = native.store.borrow_mut();
                            let mut state = store.state.clone();
                            state.fullscreen = true;
                            store.set(state);
                        }
                        "NSWindowWillExitFullScreenNotification" => native.transitioning.set(true),
                        "NSWindowDidEnterFullScreenNotification"
                        | "NSWindowDidExitFullScreenNotification"
                        | "NSWindowDidFailToEnterFullScreenNotification"
                        | "NSWindowDidFailToExitFullScreenNotification" => {
                            native.transitioning.set(false);
                            native.capture();
                        }
                        _ => native.capture(),
                    }
                    if name == "NSWindowWillCloseNotification" {
                        native.commit();
                        native.store.borrow_mut().flush();
                    }
                });
                observers.push(center.addObserverForName_object_queue_usingBlock(
                    Some(&NSString::from_str(name)),
                    Some(&observed_window),
                    None,
                    &block,
                ));
            }
            if maximize {
                let _: () = msg_send![&*native.window, zoom: std::ptr::null::<AnyObject>()];
            }
            if fullscreen {
                let _: () =
                    msg_send![&*native.window, toggleFullScreen: std::ptr::null::<AnyObject>()];
            }
            native.capture();
            Self { native, observers }
        }
    }
    pub fn tick(&mut self, _window: &Window, _cx: &App) {
        self.native.capture();
        self.native.store.borrow_mut().tick();
    }
    pub fn flush(&mut self) {
        self.native.capture();
        self.native.commit();
        self.native.store.borrow_mut().flush();
    }
}
impl Drop for WindowSaver {
    fn drop(&mut self) {
        self.flush();
        let center = NSNotificationCenter::defaultCenter();
        for observer in &self.observers {
            unsafe {
                let _: () = msg_send![&*center, removeObserver: &**observer];
            }
        }
    }
}

/// Opt-in native relaunch probe. The driver must supply an isolated data dir.
#[cfg(feature = "smoke")]
pub(crate) fn probe(window: &Window, cx: &mut App) {
    let Some(stage) = std::env::args()
        .find_map(|arg| arg.strip_prefix("--window-state-probe=").map(str::to_owned))
    else {
        return;
    };
    assert!(
        std::env::var_os("AXOLOTL_DATA_DIR").is_some(),
        "probe requires isolated data"
    );
    window.spawn(cx, async move |cx| {
        cx.background_executor().timer(Duration::from_secs(2)).await;
        let _ = cx.update(|window, _| unsafe {
            let RawWindowHandle::AppKit(handle) = window.window_handle().unwrap().as_raw() else { unreachable!() };
            let view = handle.ns_view.as_ptr().cast::<AnyObject>();
            let native: *mut AnyObject = msg_send![view, window];
            let bounds = rect(msg_send![native, frame]);
            let zoom: bool = msg_send![native, isZoomed];
            let style: usize = msg_send![native, styleMask];
            println!("AXOLOTL_WINDOW {}", serde_json::json!({"phase":"before", "bounds":bounds, "maximized":zoom, "fullscreen":style & (1 << 14) != 0}));
            match stage.as_str() {
                "seed" => {
                    let screen: *mut AnyObject = msg_send![native, screen];
                    let area = rect(msg_send![screen, visibleFrame]);
                    let target = Rect { x: area.x + 70., y: area.y + 50., width: 900., height: 650. }.fit(area);
                    let _: () = msg_send![native, setFrame: frame(target), display: true];
                }
                "zoom" | "unzoom" => { let _: () = msg_send![native, zoom: std::ptr::null::<AnyObject>()]; }
                "fullscreen" | "exit-fullscreen" => { let _: () = msg_send![native, toggleFullScreen: std::ptr::null::<AnyObject>()]; }
                "none" => {}
                _ => panic!("unknown window probe stage"),
            }
        });
        cx.background_executor().timer(Duration::from_secs(2)).await;
        let _ = cx.update(|window, cx| unsafe {
            let RawWindowHandle::AppKit(handle) = window.window_handle().unwrap().as_raw() else { unreachable!() };
            let view = handle.ns_view.as_ptr().cast::<AnyObject>();
            let native: *mut AnyObject = msg_send![view, window];
            let bounds = rect(msg_send![native, frame]);
            let zoom: bool = msg_send![native, isZoomed];
            let style: usize = msg_send![native, styleMask];
            let screen: *mut AnyObject = msg_send![native, screen];
            let area = rect(msg_send![screen, visibleFrame]);
            println!("AXOLOTL_WINDOW {}", serde_json::json!({"phase":"after", "bounds":bounds, "visible":area, "maximized":zoom, "fullscreen":style & (1 << 14) != 0}));
            cx.quit();
        });
    }).detach();
}
