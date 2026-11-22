//! Panel HTML lokal di samping halaman (chat Claude). Memakai WKWebView
//! terpisah: tanpa pemblokir, tanpa script halaman, dan datanya tidak
//! disimpan ke disk. Halaman panel dan aplikasi bertukar pesan JSON: panel
//! memanggil `webkit.messageHandlers.mb.postMessage(teks)`, aplikasi
//! memanggil `window.mbReceive(objek)`.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::NSView;
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSRunLoop, NSSize, NSString};
use objc2_web_kit::{
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKScriptMessage, WKScriptMessageHandler,
    WKUserContentController, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};
use std::cell::Cell;
use std::rc::Rc;

const HANDLER: &str = "mb";

pub struct BridgeIvars {
    on_message: Rc<dyn Fn(String)>,
    /// Mati saat panel dilepas; pesan yang masih antre dibuang.
    alive: Rc<Cell<bool>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBPanelBridge"]
    #[ivars = BridgeIvars]
    struct Bridge;

    unsafe impl NSObjectProtocol for Bridge {}

    unsafe impl WKScriptMessageHandler for Bridge {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            let Ok(text) = unsafe { message.body() }.downcast::<NSString>() else { return };
            let slot = Cell::new(Some(text.to_string()));
            let on_message = self.ivars().on_message.clone();
            let alive = self.ivars().alive.clone();
            // Ditunda ke putaran run loop berikutnya, supaya penerima boleh
            // menutup panel tanpa melepas webview di tengah callback WebKit.
            let block = RcBlock::new(move || {
                if let Some(text) = slot.take()
                    && alive.get()
                {
                    on_message(text);
                }
            });
            // SAFETY: dipanggil WebKit di main thread, dan blok dijalankan di
            // run loop main thread yang sama.
            unsafe { NSRunLoop::mainRunLoop().performBlock(&block) };
        }
    }

    unsafe impl WKNavigationDelegate for Bridge {
        /// Panel tidak pernah berpindah halaman. Link dibuka aplikasi lewat
        /// pesan dari panel, bukan di dalam panel.
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_policy(
            &self,
            _webview: &WKWebView,
            action: &WKNavigationAction,
            handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let scheme = unsafe { action.request() }.URL().and_then(|u| u.scheme()).map(|s| s.to_string());
            let policy = if scheme.as_deref() == Some("about") {
                WKNavigationActionPolicy::Allow
            } else {
                WKNavigationActionPolicy::Cancel
            };
            handler.call((policy,));
        }
    }
);

impl Bridge {
    fn new(mtm: MainThreadMarker, on_message: Rc<dyn Fn(String)>, alive: Rc<Cell<bool>>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(BridgeIvars { on_message, alive });
        unsafe { msg_send![super(this), init] }
    }
}

pub struct PanelView {
    webview: Retained<WKWebView>,
    controller: Retained<WKUserContentController>,
    _bridge: Retained<Bridge>,
    alive: Rc<Cell<bool>>,
}

impl PanelView {
    /// Panel berisi `html`. `on_message` menerima teks yang dikirim panel,
    /// selalu di putaran run loop berikutnya.
    pub fn new(mtm: MainThreadMarker, html: &str, on_message: Box<dyn Fn(String)>) -> Self {
        let config = unsafe { WKWebViewConfiguration::new(mtm) };
        unsafe { config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm)) };
        let controller = unsafe { WKUserContentController::new(mtm) };
        unsafe { config.setUserContentController(&controller) };
        let alive = Rc::new(Cell::new(true));
        let bridge = Bridge::new(mtm, Rc::from(on_message), alive.clone());
        unsafe {
            controller.addScriptMessageHandler_name(ProtocolObject::from_ref(&*bridge), &NSString::from_str(HANDLER))
        };
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(380.0, 600.0));
        let webview = unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config) };
        unsafe { webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*bridge))) };
        #[cfg(debug_assertions)]
        unsafe {
            webview.setInspectable(true)
        };
        unsafe { webview.loadHTMLString_baseURL(&NSString::from_str(html), None) };
        Self { webview, controller, _bridge: bridge, alive }
    }

    /// Mengirim objek JSON (`message` berupa teks JSON) ke `window.mbReceive`.
    pub fn send(&self, message: &str) {
        let message = message.replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029");
        let script = format!("window.mbReceive&&window.mbReceive({message})");
        unsafe { self.webview.evaluateJavaScript_completionHandler(&NSString::from_str(&script), None) };
    }

    pub fn view(&self) -> Retained<NSView> {
        Retained::into_super(self.webview.clone())
    }
}

impl Drop for PanelView {
    fn drop(&mut self) {
        self.alive.set(false);
        unsafe {
            // Controller menahan bridge; dilepas supaya keduanya bisa bebas.
            self.controller.removeScriptMessageHandlerForName(&NSString::from_str(HANDLER));
            self.webview.stopLoading();
            self.webview.setNavigationDelegate(None);
        }
        self.webview.removeFromSuperview();
    }
}
