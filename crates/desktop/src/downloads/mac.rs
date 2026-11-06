use super::*;
use objc2::{
    DeclaredClass, MainThreadOnly, Message, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, ProtocolObject, Sel},
};
use objc2_foundation::{
    MainThreadMarker, NSData, NSError, NSObjectProtocol, NSProgressReporting, NSString, NSURL,
    NSURLResponse,
};
use objc2_web_kit::{
    WKDownload, WKDownloadDelegate, WKNavigationAction, WKNavigationActionPolicy,
    WKNavigationDelegate, WKNavigationResponse, WKWebView,
};
use std::{collections::HashMap, rc::Weak};
use wry::WebViewExtMacOS;

type Outcome = Rc<RefCell<Option<std::result::Result<(), String>>>>;
pub struct Transfer {
    download: Retained<WKDownload>,
    outcome: Outcome,
}
impl Transfer {
    pub fn progress(&self) -> (u64, Option<u64>, Option<std::result::Result<(), String>>) {
        let progress = self.download.progress();
        let total = progress.totalUnitCount();
        (
            progress.completedUnitCount().max(0) as u64,
            (total > 0).then_some(total as u64),
            self.outcome.borrow_mut().take(),
        )
    }
    pub fn cancel(&self) {
        let outcome = self.outcome.clone();
        let completion = block2::RcBlock::new(move |_data: *mut NSData| {
            *outcome.borrow_mut() = Some(Err("Cancelled".into()));
        });
        unsafe {
            self.download.cancel(Some(&completion));
        }
    }
}

pub struct Ivars {
    original: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
    manager: Weak<RefCell<DownloadManager>>,
    outcomes: RefCell<HashMap<usize, Outcome>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub struct NavigationDelegate;

    unsafe impl NSObjectProtocol for NavigationDelegate {
        #[unsafe(method(respondsToSelector:))]
        fn responds(&self, selector: Sel) -> bool {
            self.class().instance_method(selector).is_some() || self.ivars().original.respondsToSelector(selector)
        }
    }
    impl NavigationDelegate {
        // Preserve Wry callbacks except the explicit child-document policy and
        // download conversions below.
        #[unsafe(method(forwardingTargetForSelector:))]
        fn forwarding(&self, _selector: Sel) -> *mut AnyObject {
            Retained::as_ptr(&self.ivars().original).cast_mut().cast()
        }
    }
    unsafe impl WKNavigationDelegate for NavigationDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn policy(&self, webview: &WKWebView, action: &WKNavigationAction, handler: &block2::Block<dyn Fn(WKNavigationActionPolicy)>) {
            unsafe {
                // Wry's URL-only handler cannot distinguish top-level navigation
                // from iframe documents. WebKit reports srcdoc as about:srcdoc;
                // generated previews commonly use blob: or data:. Permit those
                // only for an existing child target, retaining WebKit's origin,
                // sandbox, CSP and X-Frame-Options enforcement. A nil target is
                // a new window and must still pass Wry's ordinary policy.
                let child = action.targetFrame().is_some_and(|frame| !frame.isMainFrame());
                let embedded = action.request().URL().and_then(|url| url.absoluteString())
                    .is_some_and(|url| crate::browser::embedded_document_url(&url.to_string()));
                let download = action.respondsToSelector(objc2::sel!(shouldPerformDownload))
                    && action.shouldPerformDownload();
                if child && embedded && !download {
                    handler.call((WKNavigationActionPolicy::Allow,));
                } else {
                    self.ivars().original.webView_decidePolicyForNavigationAction_decisionHandler(webview, action, handler);
                }
            }
        }
        #[unsafe(method(webView:navigationAction:didBecomeDownload:))]
        fn action(&self, _webview: &WKWebView, _action: &WKNavigationAction, download: &WKDownload) {
            unsafe { download.setDelegate(Some(ProtocolObject::from_ref(self))); }
        }
        #[unsafe(method(webView:navigationResponse:didBecomeDownload:))]
        fn response(&self, _webview: &WKWebView, _response: &WKNavigationResponse, download: &WKDownload) {
            unsafe { download.setDelegate(Some(ProtocolObject::from_ref(self))); }
        }
    }
    unsafe impl WKDownloadDelegate for NavigationDelegate {
        #[unsafe(method(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:))]
        fn destination(&self, download: &WKDownload, _response: &NSURLResponse, filename: &NSString, handler: &block2::Block<dyn Fn(*mut NSURL)>) {
            let result = self.ivars().manager.upgrade().context("Download manager has closed")
                .and_then(|manager| {
                    let (id, path) = manager.borrow_mut().begin(&filename.to_string())?;
                    let outcome = Rc::new(RefCell::new(None));
                    self.ivars().outcomes.borrow_mut().insert(download as *const _ as usize, outcome.clone());
                    let mut state = manager.borrow_mut();
                    let transfer = state.transfers.iter_mut().find(|t| t.item.id == id).unwrap();
                    transfer.native = Some(Transfer { download: download.retain(), outcome });
                    Ok(path)
                });
            match result {
                Ok(path) => {
                    let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&path.to_string_lossy()), false);
                    handler.call((Retained::as_ptr(&url).cast_mut(),));
                }
                Err(error) => { eprintln!("Download destination: {error}"); handler.call((std::ptr::null_mut(),)); }
            }
        }
        #[unsafe(method(downloadDidFinish:))]
        fn finished(&self, download: &WKDownload) { self.complete(download, Ok(())); }
        #[unsafe(method(download:didFailWithError:resumeData:))]
        fn failed(&self, download: &WKDownload, error: &NSError, _data: Option<&NSData>) {
            self.complete(download, Err(error.localizedDescription().to_string()));
        }
    }
);

impl NavigationDelegate {
    fn complete(&self, download: &WKDownload, outcome: std::result::Result<(), String>) {
        if let Some(state) = self
            .ivars()
            .outcomes
            .borrow_mut()
            .remove(&(download as *const _ as usize))
        {
            *state.borrow_mut() = Some(outcome);
        }
    }
}

pub fn install(view: &wry::WebView, manager: Rc<RefCell<DownloadManager>>) -> Result<()> {
    let webview = view.webview();
    let original =
        unsafe { webview.navigationDelegate() }.context("Missing Wry navigation delegate")?;
    let mtm = MainThreadMarker::new().context("Download setup requires UI thread")?;
    let delegate = mtm.alloc::<NavigationDelegate>().set_ivars(Ivars {
        original,
        manager: Rc::downgrade(&manager),
        outcomes: RefCell::new(HashMap::new()),
    });
    let delegate: Retained<NavigationDelegate> = unsafe { msg_send![super(delegate), init] };
    unsafe {
        webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    }
    // WKWebView's delegate is weak. An associated object keeps the proxy alive
    // exactly as long as this native view (including retired active transfers).
    static DELEGATE_KEY: u8 = 0;
    unsafe {
        objc2::ffi::objc_setAssociatedObject(
            Retained::as_ptr(&webview).cast_mut().cast(),
            (&DELEGATE_KEY as *const u8).cast(),
            Retained::as_ptr(&delegate).cast_mut().cast(),
            objc2::ffi::OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );
    }
    Ok(())
}

/// Preserve WebKit's origin/quarantine metadata even on filesystems without links.
pub fn copy_new_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let input = std::fs::File::open(source)?;
    let output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let result = unsafe {
        libc::fcopyfile(
            input.as_raw_fd(),
            output.as_raw_fd(),
            std::ptr::null_mut(),
            libc::COPYFILE_ACL | libc::COPYFILE_STAT | libc::COPYFILE_XATTR | libc::COPYFILE_DATA,
        )
    };
    let error = (result != 0).then(std::io::Error::last_os_error);
    drop(output);
    if let Some(error) = error {
        // Exclusive creation above proves this is our partial file.
        let _ = std::fs::remove_file(destination);
        return Err(error);
    }
    Ok(())
}
