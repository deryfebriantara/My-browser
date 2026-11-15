//! my-browser: browser macOS hemat memory di atas WebKit.

mod actions;
mod menu;
mod nav_bar;
mod state;
mod tab_strip;
mod window;

use browser_core::settings::Settings;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};
use objc2_foundation::{NSObject, NSObjectProtocol};
use state::AppState;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBAppDelegate"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window_closed(&self, _sender: &NSApplication) -> bool {
            true
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn main() {
    let mtm = MainThreadMarker::new().expect("harus berjalan di main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    let state = AppState::new(mtm, Settings::default(), None);
    app.setMainMenu(Some(&menu::build(mtm, &state.ui.target)));
    state.refresh();
    state.ui.window.makeKeyAndOrderFront(None);
    app.activate();
    app.run();
}
