//! Keadaan aplikasi: `Browser` plus view-nya. Aksi UI dan event mesin masuk
//! lewat sini, lalu `refresh()` menggambar ulang dari `take_changes()`.
//!
//! Aturan: borrow `browser` tidak boleh ditahan saat memanggil AppKit, karena
//! AppKit bisa memanggil balik handler aksi secara sinkron.

use crate::actions::ActionTarget;
use crate::persistence;
use crate::tab_strip::TabInfo;
use crate::window::Ui;
use browser_core::browser::{Browser, SinkFactory, UiChange};
use browser_core::session::SessionFile;
use browser_core::settings::Settings;
use browser_core::sleep::Pressure;
use browser_core::tabs::{Tab, TabId};
use engine_webkit::WebKitEngine;
use objc2::MainThreadMarker;
use objc2_foundation::{NSBundle, NSString};
use std::path::PathBuf;
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

/// `Contents/Resources/blocklist` di dalam bundle aplikasi.
fn blocklist_dir() -> PathBuf {
    let resources = NSBundle::mainBundle().resourcePath().map(|p| p.to_string()).unwrap_or_default();
    PathBuf::from(resources).join("blocklist")
}

fn debug_enabled() -> bool {
    std::env::var_os("MYBROWSER_DEBUG").is_some()
}

fn sink_factory(weak: Weak<AppState>) -> SinkFactory {
    Box::new(move |id| {
        let weak = weak.clone();
        Box::new(move |event| {
            if debug_enabled() {
                eprintln!("[debug] tab {} event {:?}", id.0, event);
            }
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
            let engine = WebKitEngine::new(mtm);
            engine.load_blocklists(&blocklist_dir());
            let browser = Browser::new(engine, settings, session, sink_factory(weak.clone()), Instant::now());
            AppState { mtm, browser: RefCell::new(browser), ui }
        })
    }

    fn tab_infos(&self) -> Vec<TabInfo> {
        let b = self.browser.borrow();
        let active = b.tabs().active();
        b.tabs()
            .iter()
            .map(|t| TabInfo {
                id: t.id.0,
                title: display_title(t),
                active: t.id == active,
                sleeping: !t.awake && t.url.is_some(),
            })
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
        if has(UiChange::Settings) {
            let settings = self.browser.borrow().settings().clone();
            persistence::save_settings(&settings);
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

    /// Dipanggil timer (Normal) dan sinyal memory pressure (Warning/Critical).
    pub fn tick(&self, pressure: Pressure) {
        self.browser.borrow_mut().tick(Instant::now(), pressure);
        if debug_enabled() {
            eprintln!("[debug] WKWebView terlepas yang masih hidup: {}", engine_webkit::dropped_webviews_alive());
            let b = self.browser.borrow();
            for t in b.tabs().iter() {
                eprintln!(
                    "[debug] tick {:?}: tab {} awake={} pending={} media={} idle={:?}",
                    pressure,
                    t.id.0,
                    t.awake,
                    t.sleep_pending,
                    t.media_active,
                    t.last_viewed.elapsed()
                );
            }
        }
        self.refresh();
        let session = self.browser.borrow_mut().session_if_dirty();
        if let Some(session) = session {
            persistence::save_session(&session);
        }
    }

    /// Dipanggil saat aplikasi akan keluar: sesi dan pengaturan selalu disimpan.
    pub fn save_all(&self) {
        let (session, settings) = {
            let b = self.browser.borrow();
            (b.tabs().to_session(), b.settings().clone())
        };
        persistence::save_session(&session);
        persistence::save_settings(&settings);
    }

    /// URL dari argumen baris perintah. URL pertama memakai tab aktif kalau
    /// tab itu masih kosong.
    pub fn open_urls(&self, urls: &[String]) {
        for (i, url) in urls.iter().enumerate() {
            let blank = self.nav_info().blank;
            if !(i == 0 && blank) {
                self.browser.borrow_mut().new_tab(Instant::now());
            }
            self.browser.borrow_mut().navigate(url, Instant::now());
        }
        self.refresh();
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

    /// Status menu pemblokir: `None` kalau tab aktif tidak punya host (menu
    /// dinonaktifkan), `Some(true)` kalau pemblokir dimatikan untuk situs ini.
    pub fn blocker_menu_state(&self) -> Option<bool> {
        let b = self.browser.borrow();
        let tab = b.tabs().get(b.tabs().active())?;
        tab.url.as_ref()?.host_str()?;
        Some(b.active_host_excepted())
    }

    pub fn toggle_blocker(&self) {
        self.browser.borrow_mut().toggle_blocker_exception();
        self.refresh();
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
