//! Asynchronous, window-owned JavaScript sheets. Never enter a nested modal loop
//! inside a WebKit callback; page removal must still be able to cancel the sheet.
use block2::RcBlock;
use objc2::{MainThreadOnly, rc::Retained};
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSTextField};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{WKFrameInfo, WKWebView};
use std::{cell::RefCell, rc::Rc};

type Completion = Box<dyn FnOnce(bool, Option<Retained<NSString>>)>;
type Reply = Rc<RefCell<Option<Completion>>>;

struct Pending {
    alert: Retained<NSAlert>,
    reply: Reply,
}

#[derive(Default)]
pub(crate) struct Dialogs {
    pending: Rc<RefCell<Vec<Pending>>>,
}

impl Dialogs {
    pub(crate) fn show(
        &self,
        view: &WKWebView,
        frame: &WKFrameInfo,
        message: &NSString,
        default_text: Option<&NSString>,
        cancellable: bool,
        completion: Completion,
    ) {
        let Some(window) = view.window() else {
            completion(false, None);
            return;
        };
        let alert = NSAlert::new(view.mtm());
        // Attribute subframe messages to their initiating origin, not the tab URL.
        let host = unsafe { frame.securityOrigin().host() };
        let title = if host.is_empty() {
            NSString::from_str("This page says")
        } else {
            NSString::from_str(&format!("{} says", host))
        };
        alert.setMessageText(&title);
        alert.setInformativeText(message);
        alert
            .addButtonWithTitle(&NSString::from_str("OK"))
            .setKeyEquivalent(&NSString::from_str("\r"));
        if cancellable {
            let cancel = alert.addButtonWithTitle(&NSString::from_str("Cancel"));
            cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
        }
        let input = default_text.map(|text| {
            let input = NSTextField::initWithFrame(
                NSTextField::alloc(view.mtm()),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(320.0, 24.0)),
            );
            input.setStringValue(text);
            alert.setAccessoryView(Some(&input));
            alert.window().setInitialFirstResponder(Some(&input));
            input
        });
        alert.layout();
        let reply = Rc::new(RefCell::new(Some(completion)));
        self.pending.borrow_mut().push(Pending {
            alert: alert.clone(),
            reply: reply.clone(),
        });
        #[cfg(feature = "smoke")]
        TEST_DIALOGS.with(|states| {
            let mut states = states.borrow_mut();
            states.retain(|state| state.strong_count() > 0);
            if !states
                .iter()
                .any(|state| state.ptr_eq(&Rc::downgrade(&self.pending)))
            {
                states.push(Rc::downgrade(&self.pending));
            }
        });
        let pending = Rc::downgrade(&self.pending);
        let handler = RcBlock::new(move |response| {
            // Take before calling WebKit: cancellation and sheet completion may
            // race/reenter, but the engine's completion must run exactly once.
            let completion = reply.borrow_mut().take();
            if let Some(pending) = pending.upgrade() {
                pending
                    .borrow_mut()
                    .retain(|p| !Rc::ptr_eq(&p.reply, &reply));
            }
            if let Some(completion) = completion {
                let accepted = response == NSAlertFirstButtonReturn;
                completion(accepted, input.as_ref().map(|field| field.stringValue()));
            }
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
    }
}

impl Drop for Dialogs {
    fn drop(&mut self) {
        // Drain before ending sheets, since AppKit may invoke their handlers
        // synchronously. The callbacks hold only a weak reference to this list.
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for pending in pending {
            let completion = pending.reply.borrow_mut().take();
            let sheet = pending.alert.window();
            if let Some(parent) = sheet.sheetParent() {
                parent.endSheet_returnCode(&sheet, 1001);
            }
            sheet.orderOut(None);
            if let Some(completion) = completion {
                completion(false, None);
            }
        }
    }
}

#[cfg(feature = "smoke")]
thread_local! {
    static TEST_DIALOGS: RefCell<Vec<std::rc::Weak<RefCell<Vec<Pending>>>>> = const { RefCell::new(Vec::new()) };
}

#[cfg(feature = "smoke")]
pub(crate) fn test_alert() -> Option<Retained<NSAlert>> {
    TEST_DIALOGS.with(|states| {
        states
            .borrow()
            .iter()
            .filter_map(|state| state.upgrade())
            .find_map(|state| {
                state
                    .borrow()
                    .iter()
                    .find(|p| p.alert.window().isVisible())
                    .map(|p| p.alert.clone())
            })
    })
}
