//! Satu halaman: WKWebView beserta observer dan delegate-nya.

use crate::blocker::Blocker;
use crate::delegates::Delegate;
use crate::downloads::DownloadCenter;
use crate::emitter::Emitter;
use crate::observer::{KEYS, Observer};
use block2::RcBlock;
use browser_core::engine::{JsTag, Page, PageEvent};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSImageCompressionFactor, NSView,
};
use objc2_foundation::{
    NSDictionary, NSError, NSKeyValueObservingOptions, NSNumber, NSObjectNSKeyValueObserverRegistration, NSPoint, NSRect, NSSize, NSString, NSURL,
    NSURLRequest,
};
use objc2_web_kit::{
    WKContentWorld, WKMediaCaptureState, WKMediaPlaybackState, WKSnapshotConfiguration, WKUserContentController, WKWebView, WKWebViewConfiguration,
    WKWebsiteDataStore,
};
use std::cell::{Cell, RefCell};
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

const INITIAL_WIDTH: f64 = 1280.0;
const INITIAL_HEIGHT: f64 = 720.0;

pub struct WebKitPage {
    webview: Retained<WKWebView>,
    observer: Retained<Observer>,
    _delegate: Retained<Delegate>,
    emitter: Emitter,
    /// Download aktif atau dialog JavaScript yang sedang terbuka dari page ini.
    busy: Rc<Cell<usize>>,
}

impl WebKitPage {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        app_name: &NSString,
        blocker: Rc<Blocker>,
        downloads: Retained<DownloadCenter>,
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
        // Ukuran awal seukuran jendela bawaan. Tab latar yang belum pernah
        // tampil tetap di-layout seperti desktop (bukan lebar nol), jadi
        // halaman tidak berubah tata letak saat dibuka dan elemen bisa dibaca
        // lewat MCP. Saat ditampilkan, ukurannya mengikuti area konten.
        let initial = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(INITIAL_WIDTH, INITIAL_HEIGHT));
        let webview = unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), initial, &config) };
        webview.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        #[cfg(debug_assertions)]
        unsafe {
            webview.setInspectable(true)
        };

        let busy: Rc<Cell<usize>> = Rc::default();
        let delegate = Delegate::new(mtm, emitter.clone(), blocker, controller, blocking, downloads, busy.clone());
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

        Self { webview, observer, _delegate: delegate, emitter, busy }
    }

    /// Memuat HTML dengan alamat dasar tertentu, seolah-olah halaman dari
    /// situs itu. Dipakai test untuk meniru halaman situs tanpa jaringan.
    pub fn load_html(&self, html: &str, base: &Url) {
        let base = NSURL::URLWithString(&NSString::from_str(base.as_str()));
        unsafe { self.webview.loadHTMLString_baseURL(&NSString::from_str(html), base.as_deref()) };
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
        let busy = self.busy.get() > 0
            || unsafe {
                self.webview.cameraCaptureState() == WKMediaCaptureState::Active
                    || self.webview.microphoneCaptureState() == WKMediaCaptureState::Active
            };
        let emitter = self.emitter.clone();
        let block = RcBlock::new(move |state: WKMediaPlaybackState| {
            emitter.emit(PageEvent::MediaState(busy || state == WKMediaPlaybackState::Playing));
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

    fn evaluate(&self, script: &str, reply: Box<dyn FnOnce(Result<String, String>)>) {
        let block = string_reply(reply);
        unsafe {
            self.webview.evaluateJavaScript_completionHandler(&NSString::from_str(script), Some(&block));
        }
    }

    fn evaluate_async(&self, function_body: &str, reply: Box<dyn FnOnce(Result<String, String>)>) {
        let mtm = MainThreadMarker::from(&*self.webview);
        let world = unsafe { WKContentWorld::pageWorld(mtm) };
        let block = string_reply(reply);
        unsafe {
            self.webview.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(function_body),
                None,
                None,
                &world,
                Some(&block),
            );
        }
    }

    fn snapshot(&self, max_width: f64, reply: Box<dyn FnOnce(Result<Vec<u8>, String>)>) {
        let mtm = MainThreadMarker::from(&*self.webview);
        // Lebar snapshot dalam poin; layar Retina menggandakan pikselnya.
        let scale = self.webview.window().map_or(1.0, |w| w.backingScaleFactor());
        let width = (max_width / scale).min(self.webview.bounds().size.width);
        let config = unsafe { WKSnapshotConfiguration::new(mtm) };
        unsafe { config.setSnapshotWidth(Some(&NSNumber::new_f64(width))) };
        let reply = RefCell::new(Some(reply));
        let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            let Some(reply) = reply.borrow_mut().take() else { return };
            objc2::rc::autoreleasepool(|_| {
                let result = match (unsafe { image.as_ref() }, unsafe { error.as_ref() }) {
                    (Some(image), _) => jpeg(image).ok_or_else(|| "gambar tidak bisa diubah ke JPEG".to_string()),
                    (None, Some(error)) => Err(error.localizedDescription().to_string()),
                    (None, None) => Err("snapshot kosong".to_string()),
                };
                reply(result);
            });
        });
        unsafe { self.webview.takeSnapshotWithConfiguration_completionHandler(Some(&config), &block) };
    }

    /// Memakai properti privat `_webProcessIdentifier` (hanya dibaca, dicek
    /// dulu keberadaannya) karena WebKit tidak punya API publik untuk pid.
    fn process_id(&self) -> Option<i32> {
        let responds: bool = unsafe { msg_send![&*self.webview, respondsToSelector: sel!(_webProcessIdentifier)] };
        if !responds {
            return None;
        }
        let pid: i32 = unsafe { msg_send![&*self.webview, _webProcessIdentifier] };
        (pid > 0).then_some(pid)
    }

    fn view(&self) -> Retained<NSView> {
        Retained::into_super(self.webview.clone())
    }
}

/// Completion handler JavaScript yang meneruskan hasil string ke `reply`
/// (sekali saja, di dalam autorelease pool).
fn string_reply(reply: Box<dyn FnOnce(Result<String, String>)>) -> RcBlock<dyn Fn(*mut AnyObject, *mut NSError)> {
    let reply = RefCell::new(Some(reply));
    RcBlock::new(move |result: *mut AnyObject, error: *mut NSError| {
        let Some(reply) = reply.borrow_mut().take() else { return };
        objc2::rc::autoreleasepool(|_| {
            if let Some(error) = unsafe { error.as_ref() } {
                reply(Err(javascript_error(error)));
                return;
            }
            let text = unsafe { result.as_ref() }
                .and_then(|obj| obj.downcast_ref::<NSString>())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "null".into());
            reply(Ok(text));
        });
    })
}

/// Pesan error JavaScript, termasuk pesan exception kalau WebKit menyertakannya.
fn javascript_error(error: &NSError) -> String {
    let message = error
        .userInfo()
        .objectForKey(&NSString::from_str("WKJavaScriptExceptionMessage"))
        .and_then(|m| m.downcast::<NSString>().ok())
        .map(|m| m.to_string());
    match message {
        Some(m) => m,
        None => error.localizedDescription().to_string(),
    }
}

fn jpeg(image: &NSImage) -> Option<Vec<u8>> {
    let tiff = image.TIFFRepresentation()?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
    let quality = NSNumber::new_f64(0.7);
    let value: &AnyObject = quality.as_ref();
    let properties = NSDictionary::<NSString, AnyObject>::from_slices(&[unsafe { NSImageCompressionFactor }], &[value]);
    let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::JPEG, &properties) }?;
    Some(data.to_vec())
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
