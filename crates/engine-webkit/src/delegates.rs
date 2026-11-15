//! Navigation delegate dan UI delegate WKWebView.

use crate::emitter::Emitter;
use browser_core::engine::PageEvent;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol};
use objc2_web_kit::{
    WKNavigation, WKNavigationAction, WKNavigationDelegate, WKUIDelegate, WKWebView, WKWebViewConfiguration,
    WKWindowFeatures,
};

pub(crate) struct DelegateIvars {
    emitter: Emitter,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBPageDelegate"]
    #[ivars = DelegateIvars]
    pub(crate) struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl WKNavigationDelegate for Delegate {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, _webview: &WKWebView, _navigation: Option<&WKNavigation>) {
            self.ivars().emitter.emit(PageEvent::LoadFinished);
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn process_terminated(&self, _webview: &WKWebView) {
            self.ivars().emitter.emit(PageEvent::ProcessCrashed);
        }
    }

    unsafe impl WKUIDelegate for Delegate {
        /// `target=_blank` dan `window.open`: dibuka sebagai tab baru oleh
        /// Browser. Mengembalikan nil berarti WebKit tidak membuat webview.
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn create_webview(
            &self,
            _webview: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            let url = unsafe { action.request() }
                .URL()
                .and_then(|u| u.absoluteString())
                .and_then(|s| url::Url::parse(&s.to_string()).ok());
            if let Some(url) = url {
                self.ivars().emitter.emit(PageEvent::OpenInNewTab(url));
            }
            None
        }
    }
);

impl Delegate {
    pub(crate) fn new(mtm: MainThreadMarker, emitter: Emitter) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars { emitter });
        unsafe { msg_send![super(this), init] }
    }
}
