//! Implementasi mesin di atas WKWebView.

mod blocker;
mod delegates;
mod download_names;
mod downloads;
mod emitter;
mod observer;
mod page;
mod panel;
mod passwords;
mod scripts;
pub mod user_agent;

use block2::RcBlock;
use blocker::Blocker;
use browser_core::engine::{Engine, PageEvent};
use downloads::DownloadCenter;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_foundation::{NSDate, NSSet, NSString};
use objc2_web_kit::WKWebsiteDataStore;
use std::path::Path;
use std::rc::Rc;

pub use page::{WebKitPage, dropped_webviews_alive};
pub use panel::PanelView;

pub struct WebKitEngine {
    mtm: MainThreadMarker,
    app_name: Retained<NSString>,
    mobile_user_agent: Retained<NSString>,
    blocker: Rc<Blocker>,
    downloads: Retained<DownloadCenter>,
}

impl WebKitEngine {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let version =
            user_agent::installed_safari_version().unwrap_or_else(|| user_agent::FALLBACK_SAFARI_VERSION.to_string());
        Self {
            mtm,
            app_name: NSString::from_str(&user_agent::safari_application_name(&version)),
            mobile_user_agent: NSString::from_str(&user_agent::iphone_user_agent(&version)),
            blocker: Rc::default(),
            downloads: DownloadCenter::new(mtm),
        }
    }

    /// Memuat `blocklist-*.json` dari `dir` (asinkron). Page yang sudah dan akan
    /// dibuat dipasangi daftar begitu siap.
    pub fn load_blocklists(&self, dir: &Path) {
        self.blocker.load(self.mtm, dir);
    }
}

impl Engine for WebKitEngine {
    type Page = WebKitPage;

    fn create_page(&self, on_event: Box<dyn Fn(PageEvent)>) -> WebKitPage {
        WebKitPage::new(
            self.mtm,
            &self.app_name,
            self.mobile_user_agent.clone(),
            self.blocker.clone(),
            self.downloads.clone(),
            on_event,
        )
    }

    fn set_blocker_exceptions(&self, hosts: &[String]) {
        self.blocker.set_exceptions(hosts);
    }

    fn set_global_sink(&self, sink: Box<dyn Fn(PageEvent)>) {
        self.downloads.set_sink(sink);
    }

    /// Menghapus data "Alternative Services" (petunjuk HTTP/3 dari header
    /// `Alt-Svc`) di data store bawaan, di memory proses jaringan maupun di
    /// `AlternativeService.sqlite`. Tipe datanya SPI WebKit, tetapi namanya
    /// stabil dan diterima `removeDataOfTypes`.
    fn forget_http3_hints(&self) {
        let store = unsafe { WKWebsiteDataStore::defaultDataStore(self.mtm) };
        let kind = NSString::from_str(ALTERNATIVE_SERVICES);
        let types = NSSet::from_slice(&[&*kind]);
        let done = RcBlock::new(|| {});
        // SAFETY: dipanggil di main thread; blok penyelesaian tidak memakai apa pun.
        unsafe { store.removeDataOfTypes_modifiedSince_completionHandler(&types, &NSDate::distantPast(), &done) };
    }
}

/// Nama tipe data WebKit untuk petunjuk HTTP/3 (`_WKWebsiteDataTypeAlternativeServices`).
const ALTERNATIVE_SERVICES: &str = "_WKWebsiteDataTypeAlternativeServices";
