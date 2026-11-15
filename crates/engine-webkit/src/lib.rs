//! Implementasi mesin di atas WKWebView.

mod delegates;
mod emitter;
mod observer;
mod page;
pub mod user_agent;

use browser_core::engine::{Engine, PageEvent};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::rc::Rc;

pub use page::WebKitPage;

pub struct WebKitEngine {
    mtm: MainThreadMarker,
    app_name: Retained<NSString>,
    exceptions: Rc<RefCell<Vec<String>>>,
}

impl WebKitEngine {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let version = user_agent::installed_safari_version()
            .unwrap_or_else(|| user_agent::FALLBACK_SAFARI_VERSION.to_string());
        Self {
            mtm,
            app_name: NSString::from_str(&user_agent::safari_application_name(&version)),
            exceptions: Rc::default(),
        }
    }
}

impl Engine for WebKitEngine {
    type Page = WebKitPage;

    fn create_page(&self, on_event: Box<dyn Fn(PageEvent)>) -> WebKitPage {
        WebKitPage::new(self.mtm, &self.app_name, on_event)
    }

    fn set_blocker_exceptions(&self, hosts: &[String]) {
        *self.exceptions.borrow_mut() = hosts.to_vec();
    }
}
