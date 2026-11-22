//! Observer KVO untuk properti WKWebView yang ditampilkan UI.

use crate::emitter::Emitter;
use browser_core::engine::PageEvent;
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSObject, NSString};
use objc2_web_kit::WKWebView;
use std::ffi::c_void;

pub(crate) const KEYS: [&str; 5] = ["title", "URL", "loading", "canGoBack", "canGoForward"];

pub(crate) struct ObserverIvars {
    emitter: Emitter,
    webview: Weak<WKWebView>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBPageObserver"]
    #[ivars = ObserverIvars]
    pub(crate) struct Observer;

    impl Observer {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe(
            &self,
            key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&AnyObject>,
            _context: *mut c_void,
        ) {
            let (Some(key), Some(webview)) = (key_path, self.ivars().webview.load()) else { return };
            let emitter = &self.ivars().emitter;
            match key.to_string().as_str() {
                "title" => {
                    let title = unsafe { webview.title() }.map(|t| t.to_string()).unwrap_or_default();
                    emitter.emit(PageEvent::TitleChanged(title));
                }
                "URL" => {
                    let url = unsafe { webview.URL() }
                        .and_then(|u| u.absoluteString())
                        .and_then(|s| url::Url::parse(&s.to_string()).ok());
                    // Saat notifikasi URL datang, WebKit sudah memperbarui
                    // `isLoading`: navigasi yang memuat dokumen baru selalu
                    // sedang loading, pushState dan #fragment tidak.
                    let loading = unsafe { webview.isLoading() };
                    if let Some(url) = url {
                        emitter.emit(if loading { PageEvent::UrlChanged(url) } else { PageEvent::SameDocumentNavigation(url) });
                    }
                }
                "loading" => emitter.emit(PageEvent::LoadingChanged(unsafe { webview.isLoading() })),
                "canGoBack" | "canGoForward" => emitter.emit(PageEvent::NavState {
                    can_back: unsafe { webview.canGoBack() },
                    can_forward: unsafe { webview.canGoForward() },
                }),
                _ => {}
            }
        }
    }
);

impl Observer {
    pub(crate) fn new(mtm: MainThreadMarker, emitter: Emitter, webview: &WKWebView) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ObserverIvars { emitter, webview: Weak::from(webview) });
        unsafe { msg_send![super(this), init] }
    }
}
