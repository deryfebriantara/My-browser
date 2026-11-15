//! Navigation delegate dan UI delegate WKWebView.

use crate::blocker::Blocker;
use crate::emitter::Emitter;
use browser_core::engine::PageEvent;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol};
use objc2_web_kit::{
    WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKUIDelegate,
    WKUserContentController, WKWebView, WKWebViewConfiguration, WKWindowFeatures,
};
use std::cell::Cell;
use std::rc::Rc;

pub(crate) struct DelegateIvars {
    emitter: Emitter,
    blocker: Rc<Blocker>,
    controller: Retained<WKUserContentController>,
    blocking: Rc<Cell<bool>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBPageDelegate"]
    #[ivars = DelegateIvars]
    pub(crate) struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl WKNavigationDelegate for Delegate {
        /// Navigasi main frame ke host yang dikecualikan melepas semua daftar
        /// aturan dari page ini; navigasi ke host lain memasangnya lagi.
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_policy(
            &self,
            _webview: &WKWebView,
            action: &WKNavigationAction,
            handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let iv = self.ivars();
            let main_frame = unsafe { action.targetFrame() }.is_some_and(|f| unsafe { f.isMainFrame() });
            if main_frame {
                let host = unsafe { action.request() }.URL().and_then(|u| u.host()).map(|h| h.to_string());
                if let Some(host) = host {
                    iv.blocker.apply(&iv.controller, &iv.blocking, iv.blocker.should_block(&host));
                }
            }
            handler.call((WKNavigationActionPolicy::Allow,));
        }

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
    pub(crate) fn new(
        mtm: MainThreadMarker,
        emitter: Emitter,
        blocker: Rc<Blocker>,
        controller: Retained<WKUserContentController>,
        blocking: Rc<Cell<bool>>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars { emitter, blocker, controller, blocking });
        unsafe { msg_send![super(this), init] }
    }
}
