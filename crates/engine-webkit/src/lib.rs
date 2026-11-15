//! Implementasi mesin di atas WKWebView.

mod blocker;
mod delegates;
mod download_names;
mod emitter;
mod observer;
mod page;
pub mod user_agent;

use browser_core::engine::{Engine, PageEvent};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_foundation::NSString;
use blocker::Blocker;
use std::path::Path;
use std::rc::Rc;

pub use page::{WebKitPage, dropped_webviews_alive};

pub struct WebKitEngine {
    mtm: MainThreadMarker,
    app_name: Retained<NSString>,
    blocker: Rc<Blocker>,
}

impl WebKitEngine {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let version = user_agent::installed_safari_version()
            .unwrap_or_else(|| user_agent::FALLBACK_SAFARI_VERSION.to_string());
        Self {
            mtm,
            app_name: NSString::from_str(&user_agent::safari_application_name(&version)),
            blocker: Rc::default(),
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
        WebKitPage::new(self.mtm, &self.app_name, self.blocker.clone(), on_event)
    }

    fn set_blocker_exceptions(&self, hosts: &[String]) {
        self.blocker.set_exceptions(hosts);
    }
}
