//! Keadaan aplikasi: `Browser` plus view-nya. Aksi UI dan event mesin masuk
//! lewat sini, lalu `refresh()` menggambar ulang dari `take_changes()`.
//!
//! Aturan: borrow `browser` tidak boleh ditahan saat memanggil AppKit, karena
//! AppKit bisa memanggil balik handler aksi secara sinkron.

use crate::actions::ActionTarget;
use crate::address_bar::AddressBar;
use crate::chat::Chat;
use crate::history_window::HistoryWindow;
use crate::mcp_access::McpAccess;
use crate::password_manager::PasswordManager;
use crate::passwords::Passwords;
use crate::persistence;
use crate::tab_strip::TabInfo;
use crate::window::Ui;
use block2::RcBlock;
use browser_core::browser::{Browser, SinkFactory, UiChange};
use browser_core::engine::Engine;
use browser_core::engine::{Emulation, Page};
use browser_core::history::History;
use browser_core::page_scripts::ELEMENT_PICKER;
use browser_core::session::SessionFile;
use browser_core::settings::Settings;
use browser_core::sleep::Pressure;
use browser_core::suggest::Suggestion;
use browser_core::tabs::{Tab, TabId};
use engine_webkit::WebKitEngine;
use objc2::MainThreadMarker;
use objc2_foundation::NSRunLoop;
use objc2_foundation::{NSBundle, NSString};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

/// Jeda paling pendek antara dua penyimpanan riwayat saat browsing.
const HISTORY_SAVE_INTERVAL: Duration = Duration::from_secs(120);

pub struct AppState {
    mtm: MainThreadMarker,
    me: Weak<AppState>,
    pub browser: RefCell<Browser<WebKitEngine>>,
    pub ui: Ui,
    /// Penggambaran ulang tab strip sudah dijadwalkan untuk putaran run loop berikutnya.
    tabs_layout_pending: Cell<bool>,
    /// Claude boleh klik, mengetik, dan menjalankan JavaScript lewat MCP.
    /// Selalu mati saat aplikasi dibuka.
    pub claude_interact: Cell<bool>,
    /// Tab tempat pengguna terakhir memakai Pick Element for Claude.
    pub picked_tab: Cell<Option<TabId>>,
    /// Panel Tanya Claude.
    pub chat: Chat,
    /// Vault password dan tawaran simpan/isi login.
    pub passwords: Passwords,
    /// Jendela Kelola Password.
    pub password_manager: PasswordManager,
    /// Izin program untuk socket kontrol (MCP).
    pub mcp_access: McpAccess,
    /// Pelengkapan dan dropdown saran address bar.
    pub address: AddressBar,
    /// Jendela Riwayat (⌘Y).
    pub history_window: HistoryWindow,
    /// Terakhir kali riwayat disimpan.
    last_history_save: Cell<Instant>,
}

struct NavInfo {
    can_back: bool,
    can_forward: bool,
    loading: bool,
    url: String,
    title: String,
    blank: bool,
}

pub(crate) fn display_title(tab: &Tab) -> String {
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
            let Some(state) = weak.upgrade() else { return };
            // Password tidak pernah masuk ke Browser (core): langsung ke
            // pengelola password.
            if let browser_core::engine::PageEvent::CredentialSubmitted { origin, username, password } = event {
                state.passwords.on_credential(&state, origin, username, password);
                return;
            }
            state.browser.borrow_mut().handle_event(id, event, Instant::now());
            state.refresh();
        })
    })
}

impl AppState {
    pub fn new(
        mtm: MainThreadMarker,
        settings: Settings,
        session: Option<&SessionFile>,
        history: History,
    ) -> Rc<AppState> {
        Rc::new_cyclic(|weak: &Weak<AppState>| {
            let target = ActionTarget::new(mtm, weak.clone());
            let ui = Ui::build(mtm, target);
            let engine = WebKitEngine::new(mtm);
            engine.load_blocklists(&blocklist_dir());
            let global = weak.clone();
            engine.set_global_sink(Box::new(move |event| {
                if debug_enabled() {
                    eprintln!("[debug] global event {:?}", event);
                }
                if let Some(state) = global.upgrade() {
                    state.browser.borrow_mut().handle_global_event(event);
                    state.refresh();
                }
            }));
            let browser = Browser::new(engine, settings, session, history, sink_factory(weak.clone()), Instant::now());
            AppState {
                mtm,
                me: weak.clone(),
                browser: RefCell::new(browser),
                ui,
                tabs_layout_pending: Cell::new(false),
                claude_interact: Cell::new(false),
                picked_tab: Cell::new(None),
                chat: Chat::default(),
                passwords: Passwords::default(),
                password_manager: PasswordManager::default(),
                mcp_access: McpAccess::default(),
                last_history_save: Cell::new(Instant::now()),
                address: AddressBar::default(),
                history_window: HistoryWindow::default(),
            }
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
            self.update_window_title(&nav.title);
        }
        if has(UiChange::Active) {
            self.address.end();
            let (view, viewport) = {
                let b = self.browser.borrow();
                (b.active_view(), b.active_viewport())
            };
            self.ui.show_content(view, viewport);
            if nav.blank {
                self.focus_address();
            }
        } else if has(UiChange::Layout) {
            let viewport = self.browser.borrow().active_viewport();
            self.ui.layout_content(viewport);
        }
        if has(UiChange::Nav) || has(UiChange::Active) {
            self.ui.nav.update(nav.can_back, nav.can_forward, nav.loading, &nav.url);
        }
        if has(UiChange::Settings) {
            let settings = self.browser.borrow().settings().clone();
            persistence::save_settings(&settings);
        }
        if has(UiChange::History) {
            self.history_window.changed(self);
        }
        for change in changes {
            if let UiChange::Notice(message) = change {
                self.ui.nav.show_notice(&message);
            }
        }
    }

    /// Judul window mengikuti tab aktif, ditambah penanda selama Claude boleh
    /// berinteraksi.
    fn update_window_title(&self, tab_title: &str) {
        let mut title = if tab_title.is_empty() { "my-browser".to_string() } else { tab_title.to_string() };
        if self.claude_interact.get() {
            title.push_str(" — Claude dapat berinteraksi");
        }
        self.ui.window.setTitle(&NSString::from_str(&title));
    }

    pub fn toggle_claude_interact(&self) {
        let on = !self.claude_interact.get();
        self.claude_interact.set(on);
        let title = self.nav_info().title;
        self.update_window_title(&title);
        self.ui.nav.show_notice(if on {
            "Claude sekarang boleh klik, mengetik, dan menjalankan JavaScript"
        } else {
            "Interaksi Claude dimatikan"
        });
    }

    /// Mode Tunjuk Elemen di tab aktif: pengguna mengklik satu elemen, lalu
    /// Claude membacanya lewat get_selection.
    pub fn start_pick(&self) {
        let id = self.browser.borrow().tabs().active();
        let started = self.browser.borrow().page(id).map(|page| page.run_script(ELEMENT_PICKER)).is_some();
        if !started {
            self.ui.nav.show_notice("Buka halaman dulu untuk menunjuk elemen");
            return;
        }
        self.picked_tab.set(Some(id));
        // Fokus ke halaman supaya Esc sampai ke mode pilih, bukan ke address bar.
        if let Some(view) = self.ui.shown_view() {
            self.ui.window.makeFirstResponder(Some(&view));
        }
        self.ui.nav.show_notice("Klik elemen yang ingin ditunjukkan ke Claude (Esc untuk batal)");
    }

    pub fn mtm(&self) -> MainThreadMarker {
        self.mtm
    }

    /// Jendela untuk dialog: jendela yang sedang aktif (mis. Kelola Password),
    /// kalau tidak ada, jendela browser.
    pub fn dialog_window(&self) -> objc2::rc::Retained<objc2_app_kit::NSWindow> {
        objc2_app_kit::NSApplication::sharedApplication(self.mtm).keyWindow().unwrap_or_else(|| self.ui.window.clone())
    }

    /// Tab aktif sedang memakai emulasi perangkat dari Claude.
    pub fn active_emulated(&self) -> bool {
        let b = self.browser.borrow();
        b.tabs().get(b.tabs().active()).is_some_and(|t| t.emulation != Emulation::default())
    }

    pub fn reset_emulation(&self) {
        let id = self.browser.borrow().tabs().active();
        self.browser.borrow_mut().set_emulation(id, Emulation::default());
        self.refresh();
        self.ui.nav.show_notice("Tampilan kembali normal");
    }

    /// Menjadwalkan penggambaran ulang tab strip di putaran run loop berikutnya.
    /// Tidak langsung, karena aksi tombol tab sendiri yang sering memicunya, dan
    /// membuang tombol itu di tengah aksinya berisiko crash.
    pub fn layout_tabs(&self) {
        if self.tabs_layout_pending.replace(true) {
            return;
        }
        let me = self.me.clone();
        let block = RcBlock::new(move || {
            if let Some(state) = me.upgrade() {
                state.tabs_layout_pending.set(false);
                let infos = state.tab_infos();
                state.ui.tabs.render(state.mtm, &infos, &state.ui.target);
            }
        });
        // SAFETY: dijadwalkan dari main thread ke run loop main thread.
        unsafe { NSRunLoop::mainRunLoop().performBlock(&block) };
    }

    /// Dipanggil timer (Normal) dan sinyal memory pressure (Warning/Critical).
    pub fn tick(&self, pressure: Pressure) {
        self.browser.borrow_mut().tick(Instant::now(), pressure);
        self.passwords.tick(self);
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
        // Riwayat bisa sekitar 1,5 MB: disimpan paling sering sekali per 2 menit.
        if self.last_history_save.get().elapsed() >= HISTORY_SAVE_INTERVAL {
            let bytes = self.browser.borrow_mut().history_bytes_if_dirty();
            if let Some(bytes) = bytes {
                persistence::save_history(&bytes);
                self.last_history_save.set(Instant::now());
            }
        }
    }

    /// Menyimpan riwayat sekarang (setelah dihapus pengguna).
    pub fn save_history_now(&self) {
        let bytes = self.browser.borrow_mut().history_bytes();
        persistence::save_history(&bytes);
        self.last_history_save.set(Instant::now());
    }

    /// Dipanggil saat aplikasi akan keluar: sesi selalu disimpan. Pengaturan
    /// hanya disimpan saat berubah (UiChange::Settings), supaya editan tangan
    /// di settings.json tidak ditimpa nilai lama di memory.
    pub fn save_all(&self) {
        let session = self.browser.borrow().tabs().to_session();
        persistence::save_session(&session);
        let bytes = self.browser.borrow_mut().history_bytes_if_dirty();
        if let Some(bytes) = bytes {
            persistence::save_history(&bytes);
        }
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
        self.address.begin();
        self.ui.window.makeFirstResponder(Some(&self.ui.nav.address));
        unsafe { self.ui.nav.address.selectText(None) };
    }

    pub fn revert_address(&self) {
        self.address.end();
        let url = self.nav_info().url;
        self.ui.nav.address.abortEditing();
        self.ui.nav.address.setStringValue(&NSString::from_str(&url));
        self.focus_address();
    }

    /// Enter di address bar tanpa saran yang dipilih.
    pub fn navigate_from_address(&self) {
        self.address.end();
        // Selama diedit, teks terbaru ada di field editor.
        let text = match self.ui.nav.address.currentEditor() {
            Some(editor) => editor.string().to_string(),
            None => self.ui.nav.address.stringValue().to_string(),
        };
        let ok = self.browser.borrow_mut().navigate_from_address(&text, Instant::now());
        self.refresh();
        if ok && let Some(view) = self.ui.shown_view() {
            self.ui.window.makeFirstResponder(Some(&view));
        }
    }

    /// Membuka saran dari dropdown address bar.
    pub fn activate_suggestion(&self, suggestion: &Suggestion) {
        self.address.end();
        self.ui.nav.address.abortEditing();
        self.browser.borrow_mut().open_suggestion(suggestion, Instant::now());
        self.refresh();
        if let Some(view) = self.ui.shown_view() {
            self.ui.window.makeFirstResponder(Some(&view));
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
