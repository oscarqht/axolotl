//! Native popup construction and script-close notifications. WebKit keeps the
//! opener, message origins, COOP policy and script-close permissions authoritative.
use anyhow::{Context as _, Result};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel},
};
use objc2_app_kit::NSView;
use objc2_web_kit::{WKUIDelegate, WKUserContentController, WKWebView, WKWebViewConfiguration};
use wry::{
    WebViewBuilder, WebViewBuilderExtMacos, WebViewExtMacOS,
    raw_window_handle::{
        AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
    },
};

pub(crate) struct PopupDelegateIvars {
    original: Retained<ProtocolObject<dyn WKUIDelegate>>,
    closed: Box<dyn Fn()>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PopupDelegateIvars]
    pub(crate) struct PopupDelegate;

    unsafe impl NSObjectProtocol for PopupDelegate {
        #[unsafe(method(respondsToSelector:))]
        fn responds_to_selector(&self, selector: Sel) -> bool {
            // Report Wry's optional methods so WebKit continues to dispatch
            // popup creation, file selection and its existing permission UI.
            let own: bool = unsafe { msg_send![super(self), respondsToSelector: selector] };
            own || self.ivars().original.respondsToSelector(selector)
        }
    }

    impl PopupDelegate {
        #[unsafe(method(forwardingTargetForSelector:))]
        fn forwarding_target(&self, _selector: Sel) -> *mut AnyObject {
            Retained::as_ptr(&self.ivars().original).cast_mut().cast()
        }
    }

    unsafe impl WKUIDelegate for PopupDelegate {
        #[unsafe(method(webViewDidClose:))]
        fn web_view_did_close(&self, _view: &WKWebView) {
            // The callback must enqueue an event, never destroy this delegate
            // or mutate the workspace from inside WebKit's delegate invocation.
            (self.ivars().closed)();
        }
    }
);

/// Keep the returned guard alive for the view's lifetime: WKWebView's UI delegate
/// property is weak. All methods except script close continue through Wry.
pub(crate) fn install(
    view: &wry::WebView,
    closed: impl Fn() + 'static,
) -> Result<Retained<PopupDelegate>> {
    let mtm = MainThreadMarker::new().context("Popup delegate requires the UI thread")?;
    unsafe {
        let original = view
            .webview()
            .UIDelegate()
            .context("Missing Wry UI delegate")?;
        let delegate = mtm.alloc::<PopupDelegate>().set_ivars(PopupDelegateIvars {
            original,
            closed: Box::new(closed),
        });
        let delegate: Retained<PopupDelegate> = msg_send![super(delegate), init];
        view.webview()
            .setUIDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        Ok(delegate)
    }
}

struct Parent(Retained<NSView>);
impl HasWindowHandle for Parent {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let handle = AppKitWindowHandle::new(std::ptr::NonNull::from(&*self.0).cast());
        // The retained parent remains alive throughout synchronous construction
        // on the UI thread, and is the opener's actual native parent view.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle)) })
    }
}

/// Build synchronously with WebKit's exact target configuration. Do not load a
/// URL afterward: WebKit loads the original request (including POST bodies).
pub(crate) fn build_child(
    builder: WebViewBuilder<'static>,
    opener: &WKWebView,
    configuration: Retained<WKWebViewConfiguration>,
) -> Result<wry::WebView> {
    let mtm = MainThreadMarker::new().context("Popup construction requires the UI thread")?;
    let parent =
        Parent(unsafe { opener.superview() }.context("Popup opener has no native parent")?);
    unsafe {
        // WebKit's target configuration can share the opener's controller.
        // Give the child its own scripts/IPC handlers while retaining the exact
        // configuration's process pool, data store and related-view linkage.
        configuration.setUserContentController(&WKUserContentController::new(mtm));
    }
    Ok(builder
        .with_webview_configuration(configuration)
        .build_as_child(&parent)?)
}
