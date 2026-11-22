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

use blocker::Blocker;
use browser_core::engine::{Engine, PageEvent};
use downloads::DownloadCenter;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_foundation::NSString;
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
}
