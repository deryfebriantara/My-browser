//! Satu halaman: WKWebView beserta observer dan delegate-nya.

use crate::blocker::Blocker;
use crate::delegates::Delegate;
use crate::emitter::Emitter;
use crate::observer::{KEYS, Observer};
use block2::RcBlock;
use browser_core::engine::{JsTag, Page, PageEvent};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
use objc2_foundation::{
    NSError, NSKeyValueObservingOptions, NSNumber, NSObjectNSKeyValueObserverRegistration, NSRect, NSString, NSURL,
    NSURLRequest,
};
use objc2_web_kit::{
    WKMediaCaptureState, WKMediaPlaybackState, WKUserContentController, WKWebView, WKWebViewConfiguration,
    WKWebsiteDataStore,
};
use std::cell::RefCell;
use std::rc::Rc;
use url::Url;

thread_local! {
    /// Referensi lemah ke WKWebView dari page yang sudah dilepas, untuk diagnosis.
    static DROPPED: RefCell<Vec<objc2::rc::Weak<WKWebView>>> = const { RefCell::new(Vec::new()) };
}

/// Jumlah WKWebView dari page yang sudah dilepas tetapi masih hidup.
pub fn dropped_webviews_alive() -> usize {
    DROPPED.with(|d| {
        let mut d = d.borrow_mut();
        d.retain(|w| w.load().is_some());
        d.len()
    })
}

pub struct WebKitPage {
    webview: Retained<WKWebView>,
    observer: Retained<Observer>,
    _delegate: Retained<Delegate>,
    emitter: Emitter,
}

impl WebKitPage {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        app_name: &NSString,
        blocker: Rc<Blocker>,
        on_event: Box<dyn Fn(PageEvent)>,
    ) -> Self {
        let emitter = Emitter::new(on_event);
        let config = unsafe { WKWebViewConfiguration::new(mtm) };
        unsafe {
            config.preferences().setElementFullscreenEnabled(true);
            config.setApplicationNameForUserAgent(Some(app_name));
            config.setWebsiteDataStore(&WKWebsiteDataStore::defaultDataStore(mtm));
        }
        let controller = unsafe { WKUserContentController::new(mtm) };
        unsafe { config.setUserContentController(&controller) };
        let blocking = blocker.register(&controller);
        let webview =
            unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), NSRect::ZERO, &config) };
        webview.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        #[cfg(debug_assertions)]
        unsafe {
            webview.setInspectable(true)
        };

        let delegate = Delegate::new(mtm, emitter.clone(), blocker, controller, blocking);
        unsafe {
            webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            webview.setUIDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        }

        let observer = Observer::new(mtm, emitter.clone(), &webview);
        for key in KEYS {
            unsafe {
                webview.addObserver_forKeyPath_options_context(
                    &observer,
                    &NSString::from_str(key),
                    NSKeyValueObservingOptions::New,
                    std::ptr::null_mut(),
                );
            }
        }

        Self { webview, observer, _delegate: delegate, emitter }
    }
}

impl Page for WebKitPage {
    type View = Retained<NSView>;

    fn load(&self, url: &Url) {
        let Some(nsurl) = NSURL::URLWithString(&NSString::from_str(url.as_str())) else {
            eprintln!("my-browser: URL tidak bisa dimuat: {url}");
            return;
        };
        unsafe { self.webview.loadRequest(&NSURLRequest::requestWithURL(&nsurl)) };
    }

    fn go_back(&self) {
        unsafe { self.webview.goBack() };
    }

    fn go_forward(&self) {
        unsafe { self.webview.goForward() };
    }

    fn reload(&self) {
        unsafe { self.webview.reload() };
    }

    fn stop(&self) {
        unsafe { self.webview.stopLoading() };
    }

    fn request_media_state(&self) {
        let capturing = unsafe {
            self.webview.cameraCaptureState() == WKMediaCaptureState::Active
                || self.webview.microphoneCaptureState() == WKMediaCaptureState::Active
        };
        let emitter = self.emitter.clone();
        let block = RcBlock::new(move |state: WKMediaPlaybackState| {
            emitter.emit(PageEvent::MediaState(capturing || state == WKMediaPlaybackState::Playing));
        });
        unsafe { self.webview.requestMediaPlaybackStateWithCompletionHandler(&block) };
    }

    fn request_number(&self, tag: JsTag, script: &str) {
        let emitter = self.emitter.clone();
        let block = RcBlock::new(move |result: *mut AnyObject, _error: *mut NSError| {
            let value = unsafe { result.as_ref() }
                .and_then(|obj| obj.downcast_ref::<NSNumber>())
                .map(|n| n.doubleValue());
            emitter.emit(PageEvent::Number { tag, value });
        });
        unsafe {
            self.webview.evaluateJavaScript_completionHandler(&NSString::from_str(script), Some(&block));
        }
    }

    fn run_script(&self, script: &str) {
        unsafe { self.webview.evaluateJavaScript_completionHandler(&NSString::from_str(script), None) };
    }

    fn view(&self) -> Retained<NSView> {
        Retained::into_super(self.webview.clone())
    }
}

impl Drop for WebKitPage {
    fn drop(&mut self) {
        if std::env::var_os("MYBROWSER_DEBUG").is_some() {
            let count: usize = unsafe { objc2::msg_send![&*self.webview, retainCount] };
            eprintln!("[debug] WebKitPage::drop retainCount WKWebView = {count}");
        }
        self.emitter.kill();
        unsafe {
            self.webview.stopLoading();
            for key in KEYS {
                self.webview.removeObserver_forKeyPath(&self.observer, &NSString::from_str(key));
            }
            self.webview.setNavigationDelegate(None);
            self.webview.setUIDelegate(None);
        }
        self.webview.removeFromSuperview();
        DROPPED.with(|d| d.borrow_mut().push(objc2::rc::Weak::from(&*self.webview)));
    }
}
