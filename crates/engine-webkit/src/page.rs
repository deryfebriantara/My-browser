//! Satu halaman: WKWebView beserta observer dan delegate-nya.

use crate::blocker::Blocker;
use crate::delegates::Delegate;
use crate::downloads::DownloadCenter;
use crate::emitter::Emitter;
use crate::observer::{KEYS, Observer};
use crate::passwords::PasswordBridge;
use block2::RcBlock;
use browser_core::engine::{ColorScheme, JsTag, Page, PageEvent, Rect, Secret};
use browser_core::password_scripts::{FILL, PASSWORD_HANDLER, PASSWORD_WORLD};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSAutoresizingMaskOptions,
    NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSDeviceRGBColorSpace, NSGraphicsContext, NSImage,
    NSImageCompressionFactor, NSPDFImageRep, NSRectFill, NSView,
};
use objc2_foundation::{
    NSData, NSDictionary, NSError, NSKeyValueObservingOptions, NSNumber, NSObjectNSKeyValueObserverRegistration, NSPoint, NSRect, NSSize,
    NSString, NSURL, NSURLRequest,
};
use objc2_web_kit::{
    WKContentWorld, WKMediaCaptureState, WKMediaPlaybackState, WKPDFConfiguration, WKSnapshotConfiguration, WKUserContentController,
    WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
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
    /// User agent Safari iPhone untuk emulasi HP.
    mobile_user_agent: Retained<NSString>,
    observer: Retained<Observer>,
    _delegate: Retained<Delegate>,
    controller: Retained<WKUserContentController>,
    _password_bridge: Retained<PasswordBridge>,
    emitter: Emitter,
    /// Download aktif atau dialog JavaScript yang sedang terbuka dari page ini.
    busy: Rc<Cell<usize>>,
}

impl WebKitPage {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        app_name: &NSString,
        mobile_user_agent: Retained<NSString>,
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
        let password_bridge = PasswordBridge::new(mtm, emitter.clone());
        unsafe {
            controller.addScriptMessageHandler_contentWorld_name(
                ProtocolObject::from_ref(&*password_bridge),
                &WKContentWorld::worldWithName(&NSString::from_str(PASSWORD_WORLD), mtm),
                &NSString::from_str(PASSWORD_HANDLER),
            );
        }
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
        let delegate = Delegate::new(mtm, emitter.clone(), blocker, controller.clone(), blocking, downloads, busy.clone());
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

        Self {
            webview,
            mobile_user_agent,
            observer,
            _delegate: delegate,
            controller,
            _password_bridge: password_bridge,
            emitter,
            busy,
        }
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

    fn capture(&self, area: Rect, scale: f64, reply: Box<dyn FnOnce(Result<Vec<u8>, String>)>) {
        let mtm = MainThreadMarker::from(&*self.webview);
        // PDF dirender dari pohon render halaman dalam koordinat dokumen, jadi
        // bagian di luar layar ikut tergambar (snapshot biasa hanya viewport).
        let config = unsafe { WKPDFConfiguration::new(mtm) };
        unsafe { config.setRect(NSRect::new(NSPoint::new(area.x, area.y), NSSize::new(area.width, area.height))) };
        let reply = RefCell::new(Some(reply));
        let block = RcBlock::new(move |data: *mut NSData, error: *mut NSError| {
            let Some(reply) = reply.borrow_mut().take() else { return };
            objc2::rc::autoreleasepool(|_| {
                let result = match (unsafe { data.as_ref() }, unsafe { error.as_ref() }) {
                    (Some(data), _) => rasterize(data, scale).ok_or_else(|| "PDF halaman tidak bisa diubah menjadi gambar".to_string()),
                    (None, Some(error)) => Err(error.localizedDescription().to_string()),
                    (None, None) => Err("hasil render kosong".to_string()),
                };
                reply(result);
            });
        });
        unsafe { self.webview.createPDFWithConfiguration_completionHandler(Some(&config), &block) };
    }

    fn set_mobile_user_agent(&self, mobile: bool) {
        let agent = mobile.then_some(&*self.mobile_user_agent);
        unsafe { self.webview.setCustomUserAgent(agent) };
    }

    /// `prefers-color-scheme` di WKWebView mengikuti appearance view-nya.
    fn set_color_scheme(&self, scheme: ColorScheme) {
        let name = match scheme {
            ColorScheme::System => None,
            ColorScheme::Light => Some(unsafe { NSAppearanceNameAqua }),
            ColorScheme::Dark => Some(unsafe { NSAppearanceNameDarkAqua }),
        };
        let appearance = name.and_then(NSAppearance::appearanceNamed);
        self.webview.setAppearance(appearance.as_deref());
    }

    fn fill_login(&self, origin: &str, username: &str, password: &Secret, reply: Box<dyn FnOnce(Result<String, String>)>) {
        let mtm = MainThreadMarker::from(&*self.webview);
        let world = unsafe { WKContentWorld::worldWithName(&NSString::from_str(PASSWORD_WORLD), mtm) };
        // Nilai dikirim sebagai argumen fungsi, bukan disisipkan ke teks script.
        let keys = [NSString::from_str("origin"), NSString::from_str("username"), NSString::from_str("password")];
        let values = [NSString::from_str(origin), NSString::from_str(username), NSString::from_str(password.expose())];
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let value_refs: Vec<&AnyObject> = values.iter().map(|v| v.as_ref()).collect();
        let arguments = NSDictionary::<NSString, AnyObject>::from_slices(&key_refs, &value_refs);
        let block = string_reply(reply);
        unsafe {
            self.webview.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
                &NSString::from_str(FILL),
                Some(&arguments),
                None,
                &world,
                Some(&block),
            );
        }
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
    encode_jpeg(&rep)
}

/// Menggambar PDF satu halaman ke bitmap `scale` piksel per poin (di atas
/// latar putih), lalu mengubahnya menjadi JPEG.
fn rasterize(pdf: &NSData, scale: f64) -> Option<Vec<u8>> {
    let rep = NSPDFImageRep::imageRepWithData(pdf)?;
    let size = rep.bounds().size;
    let width = (size.width * scale).round().max(1.0);
    let height = (size.height * scale).round().max(1.0);
    // SAFETY: planes null meminta AppKit mengalokasikan buffernya sendiri;
    // 8 bit x 4 sampel RGBA adalah format yang didukung NSGraphicsContext.
    let bitmap = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            width as isize,
            height as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
    let target = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    NSColor::whiteColor().setFill();
    NSRectFill(target);
    let drawn = rep.drawInRect(target);
    NSGraphicsContext::restoreGraphicsState_class();
    if !drawn {
        return None;
    }
    encode_jpeg(&bitmap)
}

fn encode_jpeg(rep: &NSBitmapImageRep) -> Option<Vec<u8>> {
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
            let world = WKContentWorld::worldWithName(&NSString::from_str(PASSWORD_WORLD), MainThreadMarker::from(&*self.webview));
            self.controller.removeScriptMessageHandlerForName_contentWorld(&NSString::from_str(PASSWORD_HANDLER), &world);
        }
        self.webview.removeFromSuperview();
        DROPPED.with(|d| d.borrow_mut().push(objc2::rc::Weak::from(&*self.webview)));
    }
}
