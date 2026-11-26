//! Keadaan aplikasi: `Browser` plus view-nya. Aksi UI dan event mesin masuk
//! lewat sini, lalu `refresh()` menggambar ulang dari `take_changes()`.
//!
//! Aturan: borrow `browser` tidak boleh ditahan saat memanggil AppKit, karena
//! AppKit bisa memanggil balik handler aksi secara sinkron.

use crate::actions::ActionTarget;
use crate::address_bar::AddressBar;
use crate::chat::Chat;
use crate::favicons::Favicons;
use crate::history_window::HistoryWindow;
use crate::mcp_access::McpAccess;
use crate::password_manager::PasswordManager;
use crate::passwords::Passwords;
use crate::persistence::{self, Loaded};
use crate::window::Ui;
use block2::RcBlock;
use browser_core::browser::{Browser, SinkFactory, UiChange};
use browser_core::engine::Engine;
use browser_core::engine::{Emulation, Page};
use browser_core::page_scripts::ELEMENT_PICKER;
use browser_core::sleep::Pressure;
use browser_core::suggest::Suggestion;
use browser_core::tabs::TabId;
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
    /// Lebar sidebar saat pegangan ubah lebar mulai diseret.
    resize_from: Cell<Option<f64>>,
    /// `spaces.json` dari versi lebih baru (versi formatnya): status tab
    /// tidak disimpan supaya berkas itu tidak tertimpa.
    newer_spaces: Option<u32>,
    /// Favicon untuk sidebar (cache di `<data>/favicons`).
    pub favicons: Favicons,
}

/// Aksi menu konteks tab di sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    TogglePin,
    ToggleFavorite,
    SetPinnedUrl,
    Sleep,
    CopyLink,
}

struct NavInfo {
    can_back: bool,
    can_forward: bool,
    loading: bool,
    url: String,
    title: String,
    blank: bool,
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
    pub fn new(mtm: MainThreadMarker, loaded: Loaded) -> Rc<AppState> {
        let Loaded { settings, spaces, newer_spaces, history } = loaded;
        let state = Rc::new_cyclic(|weak: &Weak<AppState>| {
            let target = ActionTarget::new(mtm, weak.clone());
            let theme = browser_core::theme::theme(
                &spaces
                    .spaces
                    .get(spaces.active_space)
                    .or(spaces.spaces.first())
                    .map_or("personal".to_string(), |s| s.theme.clone()),
            );
            let ui = Ui::build(mtm, target, theme, settings.sidebar_width, settings.sidebar_visible);
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
            let browser = Browser::new(engine, settings, &spaces, history, sink_factory(weak.clone()), Instant::now());
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
                newer_spaces,
                resize_from: Cell::new(None),
                favicons: Favicons::new(persistence::data_dir().join("favicons")),
            }
        });
        state.ui.sidebar.install_drops(Rc::downgrade(&state));
        state.install_window_hooks();
        if state.newer_spaces.is_some() {
            state.ui.show_notice("Tabs won't be saved: spaces.json was written by a newer version of my-browser.");
        }
        state
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
            self.layout_sidebar();
            self.update_window_title(&nav.title);
        }
        if has(UiChange::Active) {
            self.address.end();
            let (view, viewport) = {
                let b = self.browser.borrow();
                (b.active_view(), b.active_viewport())
            };
            self.ui.show_content(view, viewport);
            let space = self.browser.borrow().space().name.clone();
            self.ui.card.show_empty(nav.blank.then_some(space.as_str()));
            // Berpindah tab mengakhiri pengeditan address bar (juga fokus
            // otomatis saat jendela aktif), supaya field menampilkan alamat
            // tab yang baru.
            if !nav.blank && self.ui.sidebar.is_editing() {
                self.ui.sidebar.address.abortEditing();
            }
            if nav.blank {
                self.ui.sidebar.address.setStringValue(&NSString::new());
                self.focus_address();
            }
        } else if has(UiChange::Layout) {
            let viewport = self.browser.borrow().active_viewport();
            self.ui.layout_content(viewport);
        }
        if has(UiChange::Nav) || has(UiChange::Active) {
            self.ui.sidebar.update_nav(nav.can_back, nav.can_forward, nav.loading, &nav.url);
        }
        if has(UiChange::Settings) {
            let settings = self.browser.borrow().settings().clone();
            persistence::save_settings(&settings);
        }
        if has(UiChange::History) {
            self.history_window.changed(self);
        }
        for change in changes {
            match change {
                UiChange::Notice(message) => self.ui.show_notice(&message),
                UiChange::Loaded(id) => {
                    if let Some(me) = self.me.upgrade() {
                        self.favicons.page_loaded(&me, id);
                    }
                }
                _ => {}
            }
        }
    }

    /// Judul window mengikuti tab aktif, ditambah penanda selama Claude boleh
    /// berinteraksi.
    fn update_window_title(&self, tab_title: &str) {
        let mut title = if tab_title.is_empty() { "my-browser".to_string() } else { tab_title.to_string() };
        if self.claude_interact.get() {
            title.push_str(" — Claude can interact");
        }
        self.ui.window.setTitle(&NSString::from_str(&title));
    }

    pub fn toggle_claude_interact(&self) {
        let on = !self.claude_interact.get();
        self.claude_interact.set(on);
        let title = self.nav_info().title;
        self.update_window_title(&title);
        self.ui.card.set_outline(on);
        self.ui.sidebar.set_interact(on);
        self.ui.show_notice(if on {
            "Claude can now click, type, and run JavaScript"
        } else {
            "Claude interaction turned off"
        });
    }

    /// Mode Tunjuk Elemen di tab aktif: pengguna mengklik satu elemen, lalu
    /// Claude membacanya lewat get_selection.
    pub fn start_pick(&self) {
        let id = self.browser.borrow().tabs().active();
        let started = self.browser.borrow().page(id).map(|page| page.run_script(ELEMENT_PICKER)).is_some();
        if !started {
            self.ui.show_notice("Open a page first to pick an element");
            return;
        }
        self.picked_tab.set(Some(id));
        // Fokus ke halaman supaya Esc sampai ke mode pilih, bukan ke address bar.
        if let Some(view) = self.ui.shown_view() {
            self.ui.window.makeFirstResponder(Some(&view));
        }
        self.ui.show_notice("Click the element to show Claude (Esc to cancel)");
    }

    /// Rc dari state ini (untuk pengait uji yang butuh `&Rc<AppState>`).
    #[cfg(debug_assertions)]
    pub fn me(&self) -> Option<Rc<AppState>> {
        self.me.upgrade()
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
        self.ui.show_notice("Back to normal view");
    }

    /// Menjadwalkan penggambaran ulang sidebar di putaran run loop berikutnya.
    /// Tidak langsung, karena aksi baris tab sendiri yang sering memicunya, dan
    /// membuang baris itu di tengah aksinya berisiko crash.
    pub fn layout_sidebar(&self) {
        if self.tabs_layout_pending.replace(true) {
            return;
        }
        let me = self.me.clone();
        let block = RcBlock::new(move || {
            if let Some(state) = me.upgrade() {
                state.tabs_layout_pending.set(false);
                let model = browser_core::sidebar::sidebar_model(state.browser.borrow().tabs());
                state.ui.sidebar.render(&model, &state.favicons, &state.ui.target);
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
        let spaces = self.browser.borrow_mut().session_if_dirty();
        if let Some(spaces) = spaces
            && self.newer_spaces.is_none()
        {
            persistence::save_spaces(&spaces);
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
        if self.newer_spaces.is_none() {
            let spaces = self.browser.borrow().spaces_file();
            persistence::save_spaces(&spaces);
        }
        let bytes = self.browser.borrow_mut().history_bytes_if_dirty();
        if let Some(bytes) = bytes {
            persistence::save_history(&bytes);
        }
    }

    /// URL dari argumen baris perintah. URL pertama memakai tab aktif kalau
    /// tab itu masih kosong.
    pub fn open_urls(&self, urls: &[String]) {
        // Tab baru masuk di paling atas Today, jadi URL dibuka dari belakang
        // supaya urutan di sidebar sama dengan urutan argumen (yang pertama aktif).
        for (i, url) in urls.iter().rev().enumerate() {
            let blank = self.nav_info().blank;
            if !(i == 0 && blank) {
                self.browser.borrow_mut().new_tab(Instant::now());
            }
            self.browser.borrow_mut().navigate(url, Instant::now());
        }
        // Tab kosong saat aplikasi dibuka memfokuskan address bar; setelah
        // URL dibuka, field itu harus menampilkan alamat halaman lagi.
        if !urls.is_empty() && !self.nav_info().blank {
            self.ui.sidebar.address.abortEditing();
        }
        self.refresh();
    }

    pub fn new_tab(&self) {
        self.browser.borrow_mut().new_tab(Instant::now());
        self.refresh();
        // ⌘T di tab yang masih kosong memakai tab itu lagi: cukup fokus ke address bar.
        let blank = {
            let b = self.browser.borrow();
            b.tabs().get(b.tabs().active()).is_some_and(|t| t.is_blank())
        };
        if blank {
            self.focus_address();
        }
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

    /// Aksi pada satu tab dari menu konteks sidebar atau menu File (`None`: tab aktif).
    pub fn tab_action(&self, id: Option<u64>, action: TabAction) {
        let id = id.map_or_else(|| self.browser.borrow().tabs().active(), TabId);
        match action {
            TabAction::TogglePin => {
                self.browser.borrow_mut().toggle_pin(id);
            }
            TabAction::ToggleFavorite => {
                self.browser.borrow_mut().toggle_favorite(id);
            }
            TabAction::SetPinnedUrl => {
                self.browser.borrow_mut().set_pinned_url(id);
            }
            TabAction::Sleep => {
                self.browser.borrow_mut().request_sleep(id);
            }
            TabAction::CopyLink => {
                let url = self.browser.borrow().tabs().get(id).and_then(|t| t.url.as_ref().map(|u| u.to_string()));
                if let Some(url) = url {
                    let board = objc2_app_kit::NSPasteboard::generalPasteboard();
                    board.clearContents();
                    // SAFETY: tipe string standar pasteboard.
                    board
                        .setString_forType(&NSString::from_str(&url), unsafe { objc2_app_kit::NSPasteboardTypeString });
                }
            }
        }
        self.refresh();
    }

    fn install_window_hooks(&self) {
        let (edge, sidebar, resize) = (self.me.clone(), self.me.clone(), self.me.clone());
        self.ui.install_hooks(
            Rc::new(move |inside| {
                if let Some(s) = edge.upgrade() {
                    s.edge_hover(inside);
                }
            }),
            Rc::new(move |inside| {
                if let Some(s) = sidebar.upgrade() {
                    s.sidebar_hover(inside);
                }
            }),
            Rc::new(move |dx, done| {
                if let Some(s) = resize.upgrade() {
                    s.resize_sidebar(dx, done);
                }
            }),
        );
    }

    /// ⌘S: sembunyikan atau tampilkan sidebar (disimpan di pengaturan).
    pub fn toggle_sidebar(&self) {
        let visible = self.ui.sidebar_mode() != crate::layout::SidebarMode::Docked;
        self.ui.set_sidebar_visible(visible);
        self.browser.borrow_mut().update_settings(|s| s.sidebar_visible = visible);
        self.layout_sidebar();
        self.refresh();
    }

    /// Kursor masuk/keluar pita tepi kiri saat sidebar tersembunyi; setelah
    /// diam 150 ms di sana, sidebar muncul melayang.
    pub fn edge_hover(&self, inside: bool) {
        self.ui.set_edge_hovered(inside);
        if !inside || self.ui.sidebar_mode() != crate::layout::SidebarMode::Hidden {
            return;
        }
        let me = self.me.clone();
        let show = RcBlock::new(move |_timer: std::ptr::NonNull<objc2_foundation::NSTimer>| {
            if let Some(s) = me.upgrade()
                && s.ui.edge_hovered()
            {
                s.ui.show_overlay();
                s.layout_sidebar();
            }
        });
        let _ = unsafe { objc2_foundation::NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.15, false, &show) };
    }

    /// Kursor keluar dari sidebar yang melayang: sidebar kembali tersembunyi.
    pub fn sidebar_hover(&self, inside: bool) {
        if !inside && self.ui.sidebar_mode() == crate::layout::SidebarMode::Overlay {
            self.ui.hide_overlay();
        }
    }

    /// Pegangan ubah lebar diseret sejauh `dx`; saat dilepas, lebarnya disimpan.
    pub fn resize_sidebar(&self, dx: f64, done: bool) {
        let from = match self.resize_from.get() {
            Some(from) => from,
            None => {
                let from = self.ui.sidebar_width();
                self.resize_from.set(Some(from));
                from
            }
        };
        self.ui.set_sidebar_width(from + dx);
        if done {
            self.resize_from.set(None);
            let width = self.ui.sidebar_width();
            self.browser.borrow_mut().update_settings(|s| s.sidebar_width = width);
            self.refresh();
        }
        self.layout_sidebar();
    }

    /// Memindahkan tab ke bagian sidebar `kind` di posisi `at` (seret-lepas).
    pub fn place_tab(&self, id: u64, kind: browser_core::tabs::TabKind, at: usize) {
        self.browser.borrow_mut().place_tab(TabId(id), kind, at);
        self.refresh();
    }

    /// Bagian tab aktif dan apakah tab itu kosong (untuk judul menu Pin/Favorit).
    pub fn active_tab_kind(&self) -> Option<(browser_core::tabs::TabKind, bool)> {
        let b = self.browser.borrow();
        b.tabs().get(b.tabs().active()).map(|t| (t.kind, t.is_blank()))
    }

    pub fn active_saved_differs(&self) -> bool {
        let b = self.browser.borrow();
        b.tabs().get(b.tabs().active()).is_some_and(|t| {
            t.kind != browser_core::tabs::TabKind::Today && t.saved_url.is_some() && t.saved_url != t.url
        })
    }

    pub fn select_tab(&self, id: u64) {
        self.browser.borrow_mut().activate(TabId(id), Instant::now());
        self.refresh();
        self.layout_sidebar();
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
        self.ui.window.makeFirstResponder(Some(&self.ui.sidebar.address));
        unsafe { self.ui.sidebar.address.selectText(None) };
        self.ui.sidebar.style_editor();
    }

    pub fn revert_address(&self) {
        self.address.end();
        let url = self.nav_info().url;
        self.ui.sidebar.address.abortEditing();
        self.ui.sidebar.address.setStringValue(&NSString::from_str(&url));
        self.focus_address();
    }

    /// Enter di address bar tanpa saran yang dipilih.
    pub fn navigate_from_address(&self) {
        self.address.end();
        // Selama diedit, teks terbaru ada di field editor.
        let text = match self.ui.sidebar.address.currentEditor() {
            Some(editor) => editor.string().to_string(),
            None => self.ui.sidebar.address.stringValue().to_string(),
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
        self.ui.sidebar.address.abortEditing();
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
