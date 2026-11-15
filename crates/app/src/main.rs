//! my-browser: browser macOS hemat memory di atas WebKit.

mod actions;
mod memory_pressure;
mod menu;
mod nav_bar;
mod persistence;
mod state;
mod tab_strip;
mod timer;
mod window;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol};
use state::AppState;
use std::cell::RefCell;
use std::rc::Rc;

struct DelegateIvars {
    state: RefCell<Option<Rc<AppState>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBAppDelegate"]
    #[ivars = DelegateIvars]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window_closed(&self, _sender: &NSApplication) -> bool {
            true
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _notification: &NSNotification) {
            if let Some(state) = self.ivars().state.borrow().as_ref() {
                state.save_all();
            }
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars { state: RefCell::new(None) });
        unsafe { msg_send![super(this), init] }
    }
}

fn main() {
    let mtm = MainThreadMarker::new().expect("harus berjalan di main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    // Semua persiapan dibungkus autorelease pool. Di luar app.run() tidak ada
    // pool, dan WebKit meng-autorelease WKWebView puluhan kali saat page dibuat;
    // tanpa pool, referensi itu bocor dan tab sesi yang dipulihkan tidak pernah
    // bisa melepas proses WebContent-nya saat ditidurkan.
    let (_state, _timer, _pressure) = objc2::rc::autoreleasepool(|_| {
        let (settings, session) = persistence::load();
        let state = AppState::new(mtm, settings, session.as_ref());
        *delegate.ivars().state.borrow_mut() = Some(state.clone());
        app.setMainMenu(Some(&menu::build(mtm, &state.ui.target)));
        state.refresh();
        let urls: Vec<String> = std::env::args().skip(1).filter(|a| a.starts_with("http")).collect();
        if !urls.is_empty() {
            state.open_urls(&urls);
        }
        let timer = timer::start(&state);
        let pressure = memory_pressure::start(&state);
        state.ui.window.makeKeyAndOrderFront(None);
        (state, timer, pressure)
    });
    app.activate();
    app.run();
}
