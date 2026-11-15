//! Navigation delegate dan UI delegate WKWebView.

use crate::blocker::Blocker;
use crate::download_names;
use crate::emitter::Emitter;
use browser_core::engine::PageEvent;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSData, NSError, NSObject, NSObjectProtocol, NSString, NSURL, NSURLResponse};
use objc2::runtime::ProtocolObject;
use objc2_web_kit::{
    WKDownload, WKDownloadDelegate, WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate,
    WKNavigationResponse, WKNavigationResponsePolicy, WKUIDelegate, WKUserContentController, WKWebView,
    WKWebViewConfiguration, WKWindowFeatures,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

fn downloads_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Downloads")
}

fn key(download: &WKDownload) -> usize {
    download as *const WKDownload as usize
}

pub(crate) struct DelegateIvars {
    emitter: Emitter,
    blocker: Rc<Blocker>,
    controller: Retained<WKUserContentController>,
    blocking: Rc<Cell<bool>>,
    /// Jumlah download aktif dari page ini; dibagi dengan WebKitPage supaya
    /// tab tidak ditidurkan selama download berjalan.
    active_downloads: Rc<Cell<usize>>,
    destinations: RefCell<HashMap<usize, PathBuf>>,
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
            let policy = if unsafe { action.shouldPerformDownload() } {
                WKNavigationActionPolicy::Download
            } else {
                WKNavigationActionPolicy::Allow
            };
            handler.call((policy,));
        }

        /// Respons yang tidak bisa ditampilkan WebKit (zip, dmg, dan sejenisnya) diunduh.
        #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
        fn decide_response_policy(
            &self,
            _webview: &WKWebView,
            response: &WKNavigationResponse,
            handler: &block2::DynBlock<dyn Fn(WKNavigationResponsePolicy)>,
        ) {
            let policy = if unsafe { response.canShowMIMEType() } {
                WKNavigationResponsePolicy::Allow
            } else {
                WKNavigationResponsePolicy::Download
            };
            handler.call((policy,));
        }

        #[unsafe(method(webView:navigationAction:didBecomeDownload:))]
        fn action_became_download(&self, _webview: &WKWebView, _action: &WKNavigationAction, download: &WKDownload) {
            self.start_download(download);
        }

        #[unsafe(method(webView:navigationResponse:didBecomeDownload:))]
        fn response_became_download(
            &self,
            _webview: &WKWebView,
            _response: &WKNavigationResponse,
            download: &WKDownload,
        ) {
            self.start_download(download);
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

    unsafe impl WKDownloadDelegate for Delegate {
        #[unsafe(method(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:))]
        fn decide_destination(
            &self,
            download: &WKDownload,
            _response: &NSURLResponse,
            suggested: &NSString,
            handler: &block2::DynBlock<dyn Fn(*mut NSURL)>,
        ) {
            let path = download_names::reserve_on_disk(&downloads_dir(), &suggested.to_string());
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            self.ivars().destinations.borrow_mut().insert(key(download), path);
            handler.call((Retained::as_ptr(&url).cast_mut(),));
        }

        #[unsafe(method(downloadDidFinish:))]
        fn download_did_finish(&self, download: &WKDownload) {
            let iv = self.ivars();
            iv.active_downloads.set(iv.active_downloads.get().saturating_sub(1));
            if let Some(path) = iv.destinations.borrow_mut().remove(&key(download)) {
                download_names::release(&path);
                iv.emitter.emit(PageEvent::DownloadFinished(path));
            }
        }

        #[unsafe(method(download:didFailWithError:resumeData:))]
        fn download_did_fail(&self, download: &WKDownload, error: &NSError, _resume: Option<&NSData>) {
            let iv = self.ivars();
            iv.active_downloads.set(iv.active_downloads.get().saturating_sub(1));
            if let Some(path) = iv.destinations.borrow_mut().remove(&key(download)) {
                download_names::release(&path);
            }
            iv.emitter.emit(PageEvent::DownloadFailed(error.localizedDescription().to_string()));
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
    fn start_download(&self, download: &WKDownload) {
        let iv = self.ivars();
        iv.active_downloads.set(iv.active_downloads.get() + 1);
        unsafe { download.setDelegate(Some(ProtocolObject::from_ref(self))) };
        iv.emitter.emit(PageEvent::DownloadStarted);
    }

    pub(crate) fn new(
        mtm: MainThreadMarker,
        emitter: Emitter,
        blocker: Rc<Blocker>,
        controller: Retained<WKUserContentController>,
        blocking: Rc<Cell<bool>>,
        active_downloads: Rc<Cell<usize>>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars {
            emitter,
            blocker,
            controller,
            blocking,
            active_downloads,
            destinations: RefCell::default(),
        });
        unsafe { msg_send![super(this), init] }
    }
}
