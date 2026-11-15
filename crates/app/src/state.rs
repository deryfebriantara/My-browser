//! Keadaan aplikasi: `Browser` plus view-nya. Aksi UI dan event mesin masuk
//! lewat sini, lalu `refresh()` menggambar ulang dari `take_changes()`.
//!
//! Aturan: borrow `browser` tidak boleh ditahan saat memanggil AppKit, karena
//! AppKit bisa memanggil balik handler aksi secara sinkron.

use crate::actions::ActionTarget;
use crate::tab_strip::TabInfo;
use crate::window::Ui;
use browser_core::browser::{Browser, SinkFactory, UiChange};
use browser_core::session::SessionFile;
use browser_core::settings::Settings;
use browser_core::tabs::{Tab, TabId};
use engine_webkit::WebKitEngine;
use objc2::MainThreadMarker;
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Instant;

pub struct AppState {
    mtm: MainThreadMarker,
    pub browser: RefCell<Browser<WebKitEngine>>,
    pub ui: Ui,
}

struct NavInfo {
    can_back: bool,
    can_forward: bool,
    loading: bool,
    url: String,
    title: String,
    blank: bool,
}

fn display_title(tab: &Tab) -> String {
    if !tab.title.is_empty() {
        return tab.title.clone();
    }
    match &tab.url {
        Some(url) => url.host_str().unwrap_or(url.as_str()).to_string(),
        None => "Tab Baru".to_string(),
    }
}

fn sink_factory(weak: Weak<AppState>) -> SinkFactory {
    Box::new(move |id| {
        let weak = weak.clone();
        Box::new(move |event| {
            if let Some(state) = weak.upgrade() {
                state.browser.borrow_mut().handle_event(id, event, Instant::now());
                state.refresh();
            }
        })
    })
}

impl AppState {
    pub fn new(mtm: MainThreadMarker, settings: Settings, session: Option<&SessionFile>) -> Rc<AppState> {
        Rc::new_cyclic(|weak: &Weak<AppState>| {
            let target = ActionTarget::new(mtm, weak.clone());
            let ui = Ui::build(mtm, target);
            let browser =
                Browser::new(WebKitEngine::new(mtm), settings, session, sink_factory(weak.clone()), Instant::now());
            AppState { mtm, browser: RefCell::new(browser), ui }
        })
    }

    fn tab_infos(&self) -> Vec<TabInfo> {
        let b = self.browser.borrow();
        let active = b.tabs().active();
        b.tabs()
            .iter()
            .map(|t| TabInfo { id: t.id.0, title: display_title(t), active: t.id == active, awake: t.awake })
            .collect()
    }

    fn nav_info(&self) -> NavInfo {
        let b = self.browser.borrow();
        let tab = b.tabs().get(b.tabs().active()).expect("tab aktif ada");
        NavInfo {
            can_back: tab.can_back,
            can_forward: tab.can_forward,
            loading: tab.loading,
            url: tab.url.as_ref().map(|u| u.to_string()).unwrap_or_default(),
            title: tab.title.clone(),
            blank: tab.url.is_none(),
        }
    }

    pub fn refresh(&self) {
        let changes = self.browser.borrow_mut().take_changes();
        if changes.is_empty() {
            return;
        }
        let has = |c: UiChange| changes.contains(&c);
        let nav = self.nav_info();

        if has(UiChange::Tabs) || has(UiChange::Active) {
            self.layout_tabs();
            let title = if nav.title.is_empty() { "my-browser".to_string() } else { nav.title.clone() };
            self.ui.window.setTitle(&NSString::from_str(&title));
        }
        if has(UiChange::Active) {
            let view = self.browser.borrow().active_view();
            self.ui.show_content(view);
            if nav.blank {
                self.focus_address();
            }
        }
        if has(UiChange::Nav) || has(UiChange::Active) {
            self.ui.nav.update(nav.can_back, nav.can_forward, nav.loading, &nav.url);
        }
        for change in changes {
            if let UiChange::Notice(message) = change {
                eprintln!("my-browser: {message}");
            }
        }
    }

    pub fn layout_tabs(&self) {
        let infos = self.tab_infos();
        self.ui.tabs.render(self.mtm, &infos, &self.ui.target);
    }

    pub fn new_tab(&self) {
        self.browser.borrow_mut().new_tab(Instant::now());
        self.refresh();
    }

    pub fn close_active_tab(&self) {
        let id = self.browser.borrow().tabs().active();
        self.browser.borrow_mut().close_tab(id, Instant::now());
        self.refresh();
    }

    pub fn close_tab(&self, id: u64) {
        self.browser.borrow_mut().close_tab(TabId(id), Instant::now());
        self.refresh();
    }

    pub fn select_tab(&self, id: u64) {
        self.browser.borrow_mut().activate(TabId(id), Instant::now());
        self.refresh();
        // Tombol on/off berganti status saat diklik; gambar ulang supaya
        // klik pada tab yang sudah aktif tidak mematikan tandanya.
        self.layout_tabs();
    }

    pub fn select_tab_index(&self, index: usize) {
        self.browser.borrow_mut().activate_index(index, Instant::now());
        self.refresh();
    }

    pub fn next_tab(&self) {
        self.browser.borrow_mut().activate_next(Instant::now());
        self.refresh();
    }

    pub fn previous_tab(&self) {
        self.browser.borrow_mut().activate_previous(Instant::now());
        self.refresh();
    }

    pub fn reopen_closed_tab(&self) {
        self.browser.borrow_mut().reopen_closed_tab(Instant::now());
        self.refresh();
    }

    pub fn focus_address(&self) {
        self.ui.window.makeFirstResponder(Some(&self.ui.nav.address));
        unsafe { self.ui.nav.address.selectText(None) };
    }

    pub fn revert_address(&self) {
        let url = self.nav_info().url;
        self.ui.nav.address.abortEditing();
        self.ui.nav.address.setStringValue(&NSString::from_str(&url));
        self.focus_address();
    }

    pub fn navigate_from_address(&self) {
        let text = self.ui.nav.address.stringValue().to_string();
        let ok = self.browser.borrow_mut().navigate(&text, Instant::now());
        self.refresh();
        if ok {
            if let Some(view) = self.ui.shown_view() {
                self.ui.window.makeFirstResponder(Some(&view));
            }
        }
    }

    pub fn reload_or_stop(&self) {
        self.browser.borrow_mut().reload_or_stop();
        self.refresh();
    }

    pub fn go_back(&self) {
        self.browser.borrow_mut().go_back();
        self.refresh();
    }

    pub fn go_forward(&self) {
        self.browser.borrow_mut().go_forward();
        self.refresh();
    }
}
