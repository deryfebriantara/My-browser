//! Prototipe minimal: satu window berisi satu page WebKitEngine yang memuat YouTube.

use browser_core::engine::{Engine, Page};
use engine_webkit::WebKitEngine;
use objc2::rc::Retained;
use objc2::runtime::{ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType, NSMenu, NSMenuItem, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

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

fn menu_item(mtm: MainThreadMarker, title: &str, action: Option<Sel>, key: &str) -> Retained<NSMenuItem> {
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    }
}

fn submenu(mtm: MainThreadMarker, title: &str, items: &[Retained<NSMenuItem>]) -> Retained<NSMenuItem> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for item in items {
        menu.addItem(item);
    }
    let holder = NSMenuItem::new(mtm);
    holder.setSubmenu(Some(&menu));
    holder
}

fn build_main_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let main = NSMenu::new(mtm);
    main.addItem(&submenu(mtm, "my-browser", &[menu_item(mtm, "Keluar my-browser", Some(sel!(terminate:)), "q")]));
    main.addItem(&submenu(
        mtm,
        "Edit",
        &[
            menu_item(mtm, "Urungkan", Some(sel!(undo:)), "z"),
            menu_item(mtm, "Ulangi", Some(sel!(redo:)), "Z"),
            menu_item(mtm, "Potong", Some(sel!(cut:)), "x"),
            menu_item(mtm, "Salin", Some(sel!(copy:)), "c"),
            menu_item(mtm, "Tempel", Some(sel!(paste:)), "v"),
            menu_item(mtm, "Pilih Semua", Some(sel!(selectAll:)), "a"),
        ],
    ));
    main
}

fn main() {
    let mtm = MainThreadMarker::new().expect("harus berjalan di main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.setMainMenu(Some(&build_main_menu(mtm)));

    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 800.0));
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("my-browser"));
    window.center();

    let engine = WebKitEngine::new(mtm);
    let page = engine.create_page(Box::new(|_| {}));
    window.setContentView(Some(&page.view()));
    page.load(&url::Url::parse("https://www.youtube.com").expect("URL valid"));

    window.makeKeyAndOrderFront(None);
    app.activate();
    app.run();
}
