//! Navigation delegate dan UI delegate WKWebView.

use crate::blocker::Blocker;
use crate::downloads::DownloadCenter;
use crate::emitter::Emitter;
use browser_core::engine::PageEvent;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSModalResponseOK, NSOpenPanel, NSTextField};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::{
    WKDownload, WKFrameInfo, WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate,
    WKNavigationResponse, WKNavigationResponsePolicy, WKOpenPanelParameters, WKUIDelegate, WKUserContentController,
    WKWebView, WKWebViewConfiguration, WKWindowFeatures,
};
use std::cell::Cell;
use std::rc::Rc;

/// NSURLErrorCancelled: navigasi dibatalkan (mis. diganti navigasi lain).
const URL_ERROR_CANCELLED: isize = -999;
/// WebKitErrorFrameLoadInterruptedByPolicyChange: navigasi berubah menjadi download.
const WEBKIT_ERROR_POLICY_CHANGE: isize = 102;

pub(crate) struct DelegateIvars {
    emitter: Emitter,
    blocker: Rc<Blocker>,
    controller: Retained<WKUserContentController>,
    blocking: Rc<Cell<bool>>,
    downloads: Retained<DownloadCenter>,
    /// Dibagi dengan WebKitPage: selama > 0 (download atau dialog terbuka),
    /// tab dilaporkan sibuk dan tidak ditidurkan.
    busy: Rc<Cell<usize>>,
}

fn frame_host(frame: &WKFrameInfo) -> String {
    unsafe { frame.request() }.URL().and_then(|u| u.host()).map(|h| h.to_string()).unwrap_or_default()
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

        /// Respons main frame yang tidak bisa ditampilkan (zip, dmg, dan
        /// sejenisnya) diunduh. Subframe (mis. iframe iklan) tidak boleh
        /// memicu download.
        #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
        fn decide_response_policy(
            &self,
            _webview: &WKWebView,
            response: &WKNavigationResponse,
            handler: &block2::DynBlock<dyn Fn(WKNavigationResponsePolicy)>,
        ) {
            let policy = if unsafe { response.canShowMIMEType() } {
                WKNavigationResponsePolicy::Allow
            } else if unsafe { response.isForMainFrame() } {
                WKNavigationResponsePolicy::Download
            } else {
                WKNavigationResponsePolicy::Cancel
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

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(&self, _webview: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            self.report_failure(error);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(&self, _webview: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            self.report_failure(error);
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

        /// `<input type=file>`.
        #[unsafe(method(webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:))]
        fn run_open_panel(
            &self,
            _webview: &WKWebView,
            parameters: &WKOpenPanelParameters,
            _frame: &WKFrameInfo,
            handler: &block2::DynBlock<dyn Fn(*mut NSArray<NSURL>)>,
        ) {
            let mtm = MainThreadMarker::from(self);
            let panel = NSOpenPanel::openPanel(mtm);
            unsafe {
                panel.setAllowsMultipleSelection(parameters.allowsMultipleSelection());
                panel.setCanChooseDirectories(parameters.allowsDirectories());
            }
            panel.setCanChooseFiles(true);
            let response = self.while_busy(|| panel.runModal());
            if response == NSModalResponseOK {
                let urls = panel.URLs();
                handler.call((Retained::as_ptr(&urls).cast_mut(),));
            } else {
                handler.call((std::ptr::null_mut(),));
            }
        }

        #[unsafe(method(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn run_alert(
            &self,
            _webview: &WKWebView,
            message: &NSString,
            frame: &WKFrameInfo,
            handler: &block2::DynBlock<dyn Fn()>,
        ) {
            let alert = self.alert(&frame_host(frame), message, &["OK"]);
            self.while_busy(|| alert.runModal());
            handler.call(());
        }

        #[unsafe(method(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn run_confirm(
            &self,
            _webview: &WKWebView,
            message: &NSString,
            frame: &WKFrameInfo,
            handler: &block2::DynBlock<dyn Fn(Bool)>,
        ) {
            let alert = self.alert(&frame_host(frame), message, &["OK", "Batal"]);
            let response = self.while_busy(|| alert.runModal());
            handler.call((Bool::new(response == NSAlertFirstButtonReturn),));
        }

        #[unsafe(method(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:))]
        fn run_prompt(
            &self,
            _webview: &WKWebView,
            prompt: &NSString,
            default_text: Option<&NSString>,
            frame: &WKFrameInfo,
            handler: &block2::DynBlock<dyn Fn(*mut NSString)>,
        ) {
            let mtm = MainThreadMarker::from(self);
            let alert = self.alert(&frame_host(frame), prompt, &["OK", "Batal"]);
            let field = NSTextField::new(mtm);
            field.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(260.0, 24.0)));
            field.setStringValue(default_text.unwrap_or(&NSString::from_str("")));
            alert.setAccessoryView(Some(&field));
            let response = self.while_busy(|| alert.runModal());
            if response == NSAlertFirstButtonReturn {
                let text = field.stringValue();
                handler.call((Retained::as_ptr(&text).cast_mut(),));
            } else {
                handler.call((std::ptr::null_mut(),));
            }
        }
    }
);

impl Delegate {
    fn start_download(&self, download: &WKDownload) {
        let iv = self.ivars();
        iv.downloads.start(download, iv.busy.clone());
        iv.emitter.emit(PageEvent::DownloadStarted);
    }

    fn report_failure(&self, error: &NSError) {
        let code = error.code();
        let domain = error.domain().to_string();
        if code == URL_ERROR_CANCELLED || (domain == "WebKitErrorDomain" && code == WEBKIT_ERROR_POLICY_CHANGE) {
            return;
        }
        self.ivars().emitter.emit(PageEvent::LoadFailed(error.localizedDescription().to_string()));
    }

    /// Menjalankan dialog modal sambil menandai page sibuk, supaya tab tidak
    /// ditidurkan (dan delegate tidak dilepas) selama dialog terbuka.
    fn while_busy<T>(&self, run: impl FnOnce() -> T) -> T {
        let busy = &self.ivars().busy;
        busy.set(busy.get() + 1);
        let result = run();
        busy.set(busy.get().saturating_sub(1));
        result
    }

    fn alert(&self, host: &str, message: &NSString, buttons: &[&str]) -> Retained<NSAlert> {
        let alert = NSAlert::new(MainThreadMarker::from(self));
        let title = if host.is_empty() { "Halaman ini".to_string() } else { host.to_string() };
        alert.setMessageText(&NSString::from_str(&title));
        alert.setInformativeText(message);
        for button in buttons {
            alert.addButtonWithTitle(&NSString::from_str(button));
        }
        alert
    }

    pub(crate) fn new(
        mtm: MainThreadMarker,
        emitter: Emitter,
        blocker: Rc<Blocker>,
        controller: Retained<WKUserContentController>,
        blocking: Rc<Cell<bool>>,
        downloads: Retained<DownloadCenter>,
        busy: Rc<Cell<usize>>,
    ) -> Retained<Self> {
        let this =
            Self::alloc(mtm).set_ivars(DelegateIvars { emitter, blocker, controller, blocking, downloads, busy });
        unsafe { msg_send![super(this), init] }
    }
}
