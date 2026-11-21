//! Penerima pesan dari penangkap login (content world password).
//!
//! Handler hanya terdaftar di content world password, jadi script halaman
//! tidak bisa mengirim pesan palsu. Origin diambil dari frame pengirim, dan
//! hanya frame utama yang diterima.

use crate::emitter::Emitter;
use browser_core::engine::{PageEvent, Secret};
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
use objc2_web_kit::{WKScriptMessage, WKScriptMessageHandler, WKUserContentController};

pub(crate) struct BridgeIvars {
    emitter: Emitter,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBPasswordBridge"]
    #[ivars = BridgeIvars]
    pub(crate) struct PasswordBridge;

    unsafe impl NSObjectProtocol for PasswordBridge {}

    unsafe impl WKScriptMessageHandler for PasswordBridge {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            let frame = unsafe { message.frameInfo() };
            if !unsafe { frame.isMainFrame() } {
                return;
            }
            let security = unsafe { frame.securityOrigin() };
            let (scheme, host, port) = unsafe { (security.protocol().to_string(), security.host().to_string(), security.port()) };
            let origin = if port == 0 { format!("{scheme}://{host}") } else { format!("{scheme}://{host}:{port}") };
            let Ok(body) = unsafe { message.body() }.downcast::<NSString>() else { return };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&body.to_string()) else { return };
            let Some(password) = value["password"].as_str().filter(|p| !p.is_empty()) else { return };
            let username = value["username"].as_str().unwrap_or_default().to_string();
            self.ivars().emitter.emit(PageEvent::CredentialSubmitted {
                origin,
                username,
                password: Secret::new(password.to_string()),
            });
        }
    }
);

impl PasswordBridge {
    pub(crate) fn new(mtm: MainThreadMarker, emitter: Emitter) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(BridgeIvars { emitter });
        unsafe { msg_send![super(this), init] }
    }
}
