//! Script pengguna yang dipasang di setiap page:
//! - pencatat console dan network untuk alat developer, selalu;
//! - skip iklan YouTube, hanya selama pemblokir aktif untuk page itu;
//! - penangkap login untuk pengelola password, di content world terpisah.
//!
//! WKUserContentController tidak punya API publik untuk melepas satu script,
//! jadi setiap perubahan memasang ulang seluruh set.

use browser_core::page_scripts::DEV_CAPTURE;
use browser_core::password_scripts::{CAPTURE, PASSWORD_WORLD};
use browser_core::youtube::AD_SKIP_JS;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_foundation::NSString;
use objc2_web_kit::{WKContentWorld, WKUserContentController, WKUserScript, WKUserScriptInjectionTime};

fn add(controller: &WKUserContentController, source: &str, time: WKUserScriptInjectionTime, main_frame_only: bool) {
    let mtm = MainThreadMarker::from(controller);
    unsafe {
        let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
            WKUserScript::alloc(mtm),
            &NSString::from_str(source),
            time,
            main_frame_only,
        );
        controller.addUserScript(&script);
    }
}

/// Script di content world terpisah: berbagi DOM dengan halaman, tetapi
/// variabel, fungsi, dan handler pesannya tidak terlihat oleh script halaman.
fn add_isolated(controller: &WKUserContentController, source: &str, world_name: &str) {
    let mtm = MainThreadMarker::from(controller);
    unsafe {
        let world = WKContentWorld::worldWithName(&NSString::from_str(world_name), mtm);
        let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
            WKUserScript::alloc(mtm),
            &NSString::from_str(source),
            WKUserScriptInjectionTime::AtDocumentEnd,
            true,
            &world,
        );
        controller.addUserScript(&script);
    }
}

/// Memasang ulang semua script pengguna page. Berlaku untuk dokumen
/// berikutnya yang dimuat.
pub(crate) fn install(controller: &WKUserContentController, ad_skip: bool) {
    unsafe { controller.removeAllUserScripts() };
    add(controller, DEV_CAPTURE, WKUserScriptInjectionTime::AtDocumentStart, true);
    add_isolated(controller, CAPTURE, PASSWORD_WORLD);
    if ad_skip {
        add(controller, AD_SKIP_JS, WKUserScriptInjectionTime::AtDocumentEnd, false);
    }
}
