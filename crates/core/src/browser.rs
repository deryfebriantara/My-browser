//! Controller browser: menghubungkan daftar tab, kebijakan tidur, dan mesin.
//! Tidak tahu apa pun tentang UI; UI membaca `take_changes()` setelah setiap
//! pemanggilan dan menggambar ulang.

use crate::blocking::{host_matches, normalize_host};
use crate::engine::{ColorScheme, Emulation, Engine, JsTag, Page, PageEvent};
use crate::history::{History, note_visit, visit_key};
use crate::omnibox::{Parsed, classify_input, parse_input};
use crate::session::SessionFile;
use crate::settings::Settings;
use crate::sleep::{Pressure, tabs_to_sleep};
use crate::tabs::{Closed, PendingVisit, TabId, TabKind, TabList, Url};
use crate::youtube::{VIDEO_TIME_JS, is_youtube_watch, with_resume_time};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const SCROLL_Y_JS: &str = "window.scrollY";

/// Crash kedua dalam jangka ini tidak dimuat ulang otomatis.
const CRASH_RELOAD_WINDOW: Duration = Duration::from_secs(10);

pub type SinkFactory = Box<dyn Fn(TabId) -> Box<dyn Fn(PageEvent)>>;

fn download_notice(event: &PageEvent) -> Option<String> {
    match event {
        PageEvent::DownloadFinished(path) => {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            Some(format!("Downloaded: {name}"))
        }
        PageEvent::DownloadFailed(message) => Some(format!("Download failed: {message}")),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiChange {
    /// Daftar tab, judul, atau status tidur berubah.
    Tabs,
    /// Tab aktif berganti; view konten perlu diganti.
    Active,
    /// Ukuran viewport emulasi tab aktif berubah; view konten perlu ditata ulang.
    Layout,
    /// URL, status loading, atau back/forward tab aktif berubah.
    Nav,
    /// Pengaturan berubah dan perlu disimpan.
    Settings,
    /// Riwayat berubah (kunjungan baru, judul, atau dihapus).
    History,
    /// Halaman http(s) di tab ini selesai dimuat (mis. untuk mengambil favicon).
    Loaded(TabId),
    Notice(String),
}

/// Detik unix sekarang. Bukan `Instant`, yang berhenti saat Mac tidur.
fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub struct Browser<E: Engine> {
    engine: E,
    tabs: TabList,
    pages: HashMap<TabId, E::Page>,
    settings: Settings,
    sinks: SinkFactory,
    changes: Vec<UiChange>,
    dirty: bool,
    history: History,
    /// Jam dinding untuk riwayat (detik unix); bisa diganti di test.
    clock: Box<dyn Fn() -> u64>,
}

impl<E: Engine> Browser<E> {
    /// Memulihkan sesi (semua tab tidur) lalu membangunkan tab aktif.
    pub fn new(
        engine: E,
        settings: Settings,
        session: Option<&SessionFile>,
        history: History,
        sinks: SinkFactory,
        now: Instant,
    ) -> Self {
        let mut tabs = match session {
            Some(s) => TabList::from_session(s, now),
            None => TabList::new(now),
        };
        // Tab dari sesi sudah tercatat saat dibuka dulu; memuatnya lagi
        // bukan kunjungan baru.
        let ids: Vec<TabId> = tabs.iter().map(|t| t.id).collect();
        for id in ids {
            if let Some(tab) = tabs.get_mut(id) {
                tab.visit_key = tab.url.as_ref().and_then(visit_key);
            }
        }
        engine.set_blocker_exceptions(&settings.blocker_exceptions);
        if !settings.http3_enabled {
            engine.forget_http3_hints();
        }
        let mut b = Self {
            engine,
            tabs,
            pages: HashMap::new(),
            settings,
            sinks,
            changes: Vec::new(),
            dirty: false,
            history,
            clock: Box::new(unix_now),
        };
        let active = b.tabs.active();
        b.wake(active);
        b.change(UiChange::Tabs);
        b.change(UiChange::Active);
        b
    }

    pub fn tabs(&self) -> &TabList {
        &self.tabs
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn active_view(&self) -> Option<<E::Page as Page>::View> {
        self.pages.get(&self.tabs.active()).map(Page::view)
    }

    /// Page milik tab yang sedang bangun.
    pub fn page(&self, id: TabId) -> Option<&E::Page> {
        self.pages.get(&id)
    }

    /// Ukuran viewport emulasi tab aktif. None: seukuran area konten.
    pub fn active_viewport(&self) -> Option<(f64, f64)> {
        self.tabs.get(self.tabs.active()).and_then(|t| t.emulation.viewport)
    }

    /// Mengganti emulasi perangkat tab. Page yang bangun langsung mengikuti;
    /// kalau user agent berganti, halaman dimuat ulang supaya situs ikut
    /// menyajikan versi HP atau desktop. true kalau halaman dimuat ulang.
    pub fn set_emulation(&mut self, id: TabId, emulation: Emulation) -> bool {
        let Some(tab) = self.tabs.get_mut(id) else { return false };
        let old = std::mem::replace(&mut tab.emulation, emulation);
        let mut reloaded = false;
        if let Some(page) = self.pages.get(&id) {
            if old.color_scheme != emulation.color_scheme {
                page.set_color_scheme(emulation.color_scheme);
            }
            if old.mobile != emulation.mobile {
                page.set_mobile_user_agent(emulation.mobile);
                page.reload();
                reloaded = true;
            }
        }
        if old.viewport != emulation.viewport && id == self.tabs.active() {
            self.change(UiChange::Layout);
        }
        reloaded
    }

    /// Membuka tab baru berisi input (URL atau kata pencarian) tepat setelah
    /// tab aktif. Tab latar langsung dimuat. None kalau input kosong.
    pub fn open_tab(&mut self, input: &str, activate: bool, now: Instant) -> Option<TabId> {
        let url = parse_input(input, &self.settings.search_template)?;
        let id = self.tabs.open(Some(url), now, false);
        if activate {
            self.activate(id, now);
        } else {
            self.wake(id);
            self.tick(now, Pressure::Normal);
        }
        self.dirty = true;
        self.change(UiChange::Tabs);
        Some(id)
    }

    pub fn new_tab(&mut self, now: Instant) {
        let id = self.tabs.open(None, now, false);
        self.activate(id, now);
    }

    pub fn close_tab(&mut self, id: TabId, now: Instant) {
        let was_active = self.tabs.active() == id;
        self.pages.remove(&id);
        match self.tabs.close(id, now) {
            None => return,
            // Slot kembali ke alamat simpanannya; membangunkannya lagi nanti
            // bukan kunjungan baru.
            Some(Closed::Unloaded) => {
                if let Some(tab) = self.tabs.get_mut(id) {
                    tab.visit_key = tab.url.as_ref().and_then(visit_key);
                }
            }
            Some(Closed::Removed(_)) => {}
        }
        if was_active {
            let next = self.tabs.active();
            self.wake(next);
            self.change(UiChange::Active);
            self.change(UiChange::Nav);
        }
        self.change(UiChange::Tabs);
        self.dirty = true;
    }

    /// Memindahkan tab ke bagian sidebar `kind` di posisi `at`.
    pub fn place_tab(&mut self, id: TabId, kind: TabKind, at: usize) -> bool {
        if !self.tabs.place(id, kind, at) {
            return false;
        }
        self.dirty = true;
        self.change(UiChange::Tabs);
        true
    }

    /// ⌘D: Today menjadi Pinned (paling bawah); Pinned atau Favorite kembali
    /// ke paling atas Today.
    pub fn toggle_pin(&mut self, id: TabId) -> bool {
        match self.tabs.get(id).map(|t| t.kind) {
            Some(TabKind::Today) => {
                let end = self.tabs.section(TabKind::Pinned).count();
                self.place_tab(id, TabKind::Pinned, end)
            }
            Some(_) => self.place_tab(id, TabKind::Today, 0),
            None => false,
        }
    }

    /// Favorite kembali ke paling atas Today; selain itu menjadi favorit terakhir.
    pub fn toggle_favorite(&mut self, id: TabId) -> bool {
        match self.tabs.get(id).map(|t| t.kind) {
            Some(TabKind::Favorite) => self.place_tab(id, TabKind::Today, 0),
            Some(_) => {
                let end = self.tabs.section(TabKind::Favorite).count();
                self.place_tab(id, TabKind::Favorite, end)
            }
            None => false,
        }
    }

    /// "Set Current Page as Pinned URL".
    pub fn set_pinned_url(&mut self, id: TabId) -> bool {
        if !self.tabs.set_saved_to_current(id) {
            return false;
        }
        self.dirty = true;
        self.change(UiChange::Tabs);
        true
    }

    pub fn reopen_closed_tab(&mut self, now: Instant) {
        if let Some(id) = self.tabs.reopen_closed(now) {
            if let Some(tab) = self.tabs.get_mut(id) {
                tab.visit_key = tab.url.as_ref().and_then(visit_key);
            }
            self.activate(id, now);
        }
    }

    pub fn activate(&mut self, id: TabId, now: Instant) {
        let prev = self.tabs.active();
        self.tabs.activate(id, now);
        self.after_switch(prev, now);
    }

    pub fn activate_index(&mut self, index: usize, now: Instant) {
        let prev = self.tabs.active();
        self.tabs.activate_index(index, now);
        self.after_switch(prev, now);
    }

    pub fn activate_next(&mut self, now: Instant) {
        let prev = self.tabs.active();
        self.tabs.activate_next(now);
        self.after_switch(prev, now);
    }

    pub fn activate_previous(&mut self, now: Instant) {
        let prev = self.tabs.active();
        self.tabs.activate_previous(now);
        self.after_switch(prev, now);
    }

    /// Memuat input address bar di tab aktif. `false` kalau input kosong.
    pub fn navigate(&mut self, input: &str, now: Instant) -> bool {
        let id = self.tabs.active();
        self.navigate_tab(id, input, now)
    }

    /// Memuat input address bar yang diketik pengguna di tab aktif. Alamat
    /// dihitung sebagai ketikan di riwayat (untuk melengkapi alamat), pencarian
    /// tidak. `false` kalau input kosong.
    pub fn navigate_from_address(&mut self, input: &str, now: Instant) -> bool {
        let Some(parsed) = classify_input(input, &self.settings.search_template) else { return false };
        let typed = matches!(parsed, Parsed::Url(_));
        let id = self.tabs.active();
        self.navigate_url(id, parsed.into_url(), typed, now)
    }

    /// Memuat input di tab tertentu. `false` kalau input kosong atau tab tidak ada.
    pub fn navigate_tab(&mut self, id: TabId, input: &str, now: Instant) -> bool {
        let Some(url) = parse_input(input, &self.settings.search_template) else { return false };
        self.navigate_url(id, url, false, now)
    }

    /// Memuat `url` di tab `id`. Kunjungannya dicatat ke riwayat saat selesai
    /// dimuat, walaupun alamatnya sama dengan yang sedang terbuka.
    pub fn navigate_url(&mut self, id: TabId, url: Url, typed: bool, now: Instant) -> bool {
        self.forget_http3_hints_if_disabled();
        let Some(tab) = self.tabs.get_mut(id) else { return false };
        tab.pending_visit = Some(PendingVisit { typed });
        tab.url = Some(url.clone());
        tab.scroll_y = 0.0;
        tab.restore_scroll = false;
        tab.video_time = None;
        match self.pages.get(&id) {
            Some(page) => page.load(&url),
            None => {
                self.wake(id);
                self.tick(now, Pressure::Normal);
            }
        }
        self.dirty = true;
        self.change(UiChange::Tabs);
        self.change(UiChange::Nav);
        true
    }

    pub fn go_back(&mut self) {
        self.go_back_in(self.tabs.active());
    }

    pub fn go_forward(&mut self) {
        self.go_forward_in(self.tabs.active());
    }

    pub fn go_back_in(&mut self, id: TabId) {
        if let Some(page) = self.pages.get(&id) {
            page.go_back();
        }
    }

    pub fn go_forward_in(&mut self, id: TabId) {
        if let Some(page) = self.pages.get(&id) {
            page.go_forward();
        }
    }

    pub fn reload_in(&mut self, id: TabId) {
        if let Some(page) = self.pages.get(&id) {
            page.reload();
        }
    }

    pub fn reload_or_stop(&mut self) {
        let id = self.tabs.active();
        let loading = self.tabs.get(id).is_some_and(|t| t.loading);
        match self.pages.get(&id) {
            Some(page) if loading => page.stop(),
            Some(page) => page.reload(),
            None => self.wake(id),
        }
    }

    /// Menandai tab yang layak tidur, lalu meminta status aktivitas terbaru.
    /// Tab benar-benar ditidurkan saat balasan `Activity` menyatakan aman.
    pub fn tick(&mut self, now: Instant, pressure: Pressure) {
        self.forget_http3_hints_if_disabled();
        let policy = self.settings.sleep_policy();
        for id in tabs_to_sleep(&self.tabs.snapshots(), now, &policy, pressure) {
            if let Some(tab) = self.tabs.get_mut(id) {
                tab.sleep_pending = true;
                tab.sleep_forced |= pressure == Pressure::Critical;
            }
        }
        let active = self.tabs.active();
        let background: Vec<TabId> =
            self.tabs.iter().filter(|t| t.id != active && self.pages.contains_key(&t.id)).map(|t| t.id).collect();
        for id in background {
            self.check_activity(id);
        }
    }

    /// Meminta tab latar yang sedang bangun untuk tidur. Tab tetap bangun
    /// kalau ternyata sedang memutar media atau ada teks yang belum dikirim.
    /// `false` kalau tab aktif atau sudah tidur.
    pub fn request_sleep(&mut self, id: TabId) -> bool {
        if id == self.tabs.active() || !self.pages.contains_key(&id) {
            return false;
        }
        if let Some(tab) = self.tabs.get_mut(id) {
            tab.sleep_pending = true;
        }
        self.check_activity(id);
        true
    }

    /// Meminta status media dan teks yang belum dikirim (dan posisi video
    /// untuk YouTube) tab latar.
    fn check_activity(&mut self, id: TabId) {
        let Some(page) = self.pages.get(&id) else { return };
        let youtube = self.tabs.get(id).is_some_and(|t| t.url.as_ref().is_some_and(is_youtube_watch));
        // Posisi video diminta lebih dulu; tab YouTube baru ditidurkan setelah
        // balasannya datang (lihat sleep_if_ready).
        if youtube {
            page.request_number(JsTag::VideoTime, VIDEO_TIME_JS);
        }
        page.request_activity();
        if let Some(tab) = self.tabs.get_mut(id) {
            tab.awaiting_video_time |= youtube;
        }
    }

    /// Menidurkan tab yang menunggu tidur kalau status aktivitas sudah aman
    /// dan posisi video (khusus YouTube) sudah diterima.
    fn sleep_if_ready(&mut self, id: TabId) {
        let active = self.tabs.active();
        let ready = self
            .tabs
            .get(id)
            .is_some_and(|t| t.id != active && t.sleep_pending && t.activity_clear && !t.awaiting_video_time);
        if ready {
            self.sleep(id);
        }
    }

    /// Event dari page milik tab `id`. Event untuk tab yang sudah ditutup atau
    /// sedang tidur diabaikan.
    pub fn handle_event(&mut self, id: TabId, event: PageEvent, now: Instant) {
        if !self.pages.contains_key(&id) {
            return;
        }
        let is_active = self.tabs.active() == id;
        let record = self.settings.history_enabled;
        let unix = (self.clock)();
        let mut history_changed = false;
        let Some(tab) = self.tabs.get_mut(id) else { return };
        match event {
            PageEvent::UrlChanged(url) => {
                tab.url = Some(url);
                self.dirty = true;
                self.change(UiChange::Tabs);
                if is_active {
                    self.change(UiChange::Nav);
                }
            }
            PageEvent::SameDocumentNavigation(url) => {
                tab.url = Some(url);
                // pushState (mis. klik video di YouTube) tidak memicu
                // LoadFinished, jadi dicatat di sini. #fragment saja tidak.
                history_changed = record && note_visit(&mut self.history, tab, false, false, unix);
                self.dirty = true;
                self.change(UiChange::Tabs);
                if is_active {
                    self.change(UiChange::Nav);
                }
            }
            PageEvent::TitleChanged(title) => {
                tab.title = title;
                // Judul hanya untuk entri halaman yang sedang tampil; selama
                // halaman baru dimuat, URL tab sudah berganti, jadi entri
                // sebelumnya tidak ikut berganti nama.
                if record
                    && let Some(key) = tab.visit_key.as_deref()
                    && tab.url.as_ref().and_then(visit_key).as_deref() == Some(key)
                {
                    history_changed = self.history.set_title(key, &tab.title);
                }
                self.dirty = true;
                self.change(UiChange::Tabs);
            }
            PageEvent::LoadingChanged(loading) => {
                tab.loading = loading;
                if is_active {
                    self.change(UiChange::Nav);
                }
            }
            PageEvent::LoadFinished => {
                tab.committed_url = tab.url.clone();
                tab.loads += 1;
                let pending = tab.pending_visit.take();
                if record {
                    let typed = pending.is_some_and(|p| p.typed);
                    history_changed = note_visit(&mut self.history, tab, typed, pending.is_some(), unix);
                }
                if tab.restore_scroll {
                    tab.restore_scroll = false;
                    let script = format!("window.scrollTo(0, {})", tab.scroll_y.round() as i64);
                    self.pages[&id].run_script(&script);
                }
                // Halaman web selesai dimuat: UI boleh mengambil favicon-nya.
                if tab.url.as_ref().is_some_and(|u| matches!(u.scheme(), "http" | "https")) {
                    self.change(UiChange::Loaded(id));
                }
            }
            PageEvent::NavState { can_back, can_forward } => {
                tab.can_back = can_back;
                tab.can_forward = can_forward;
                if is_active {
                    self.change(UiChange::Nav);
                }
            }
            PageEvent::OpenInNewTab(url) => {
                let new = self.tabs.open(Some(url), now, false);
                self.activate(new, now);
                self.dirty = true;
            }
            PageEvent::Activity { media, draft } => {
                tab.media_active = media;
                tab.draft = draft;
                if tab.sleep_pending {
                    if media || is_active || (draft && !tab.sleep_forced) {
                        tab.sleep_pending = false;
                        tab.sleep_forced = false;
                        tab.activity_clear = false;
                    } else {
                        tab.activity_clear = true;
                        self.sleep_if_ready(id);
                    }
                }
            }
            PageEvent::Number { tag, value } => {
                // Angka tidak terhingga/NaN tidak bisa disimpan di JSON sesi.
                let value = value.filter(|v| v.is_finite());
                match (tag, value) {
                    (JsTag::ScrollY, Some(y)) => {
                        tab.scroll_y = y;
                        self.dirty = true;
                    }
                    (JsTag::VideoTime, v) => {
                        if v.is_some() {
                            tab.video_time = v;
                        }
                        tab.awaiting_video_time = false;
                        self.sleep_if_ready(id);
                    }
                    (JsTag::ScrollY, None) => {}
                }
            }
            PageEvent::ProcessCrashed => {
                let previous = tab.last_crash.replace(now);
                if !is_active {
                    self.sleep(id);
                } else if previous.is_some_and(|p| now.saturating_duration_since(p) < CRASH_RELOAD_WINDOW) {
                    self.change(UiChange::Notice("This page keeps crashing. Press ⌘R to reload.".into()));
                } else {
                    self.pages[&id].reload();
                }
            }
            PageEvent::DownloadStarted => {
                tab.pending_visit = None;
                tab.url = tab.committed_url.clone();
                tab.loading = false;
                self.dirty = true;
                self.change(UiChange::Tabs);
                if is_active {
                    self.change(UiChange::Nav);
                }
            }
            event @ (PageEvent::DownloadFinished(_) | PageEvent::DownloadFailed(_)) => {
                if let Some(message) = download_notice(&event) {
                    self.change(UiChange::Notice(message));
                }
            }
            PageEvent::LoadFailed(message) => {
                tab.loading = false;
                tab.restore_scroll = false;
                tab.pending_visit = None;
                if is_active {
                    self.change(UiChange::Nav);
                    self.change(UiChange::Notice(format!("Couldn't load page: {message}")));
                }
            }
            // Ditangani aplikasi (pengelola password) sebelum sampai ke sini.
            PageEvent::CredentialSubmitted { .. } => {}
        }
        if history_changed {
            self.change(UiChange::History);
        }
    }

    fn active_host(&self) -> Option<String> {
        let tab = self.tabs.get(self.tabs.active())?;
        tab.url.as_ref()?.host_str().map(normalize_host)
    }

    /// Apakah pemblokir dimatikan untuk host tab aktif (untuk centang menu).
    pub fn active_host_excepted(&self) -> bool {
        self.active_host().is_some_and(|h| host_matches(&h, &self.settings.blocker_exceptions))
    }

    /// Menyalakan/mematikan pengecualian pemblokir untuk host tab aktif, lalu
    /// memuat ulang tab itu. Mengembalikan status pengecualian yang baru, atau
    /// `None` kalau tab aktif tidak punya host.
    /// Mengubah pengaturan; perubahannya disimpan oleh UI (`UiChange::Settings`).
    pub fn update_settings(&mut self, change: impl FnOnce(&mut Settings)) {
        change(&mut self.settings);
        self.change(UiChange::Settings);
    }

    pub fn toggle_blocker_exception(&mut self) -> Option<bool> {
        let host = self.active_host()?;
        let list = &mut self.settings.blocker_exceptions;
        let excepted = if host_matches(&host, list) {
            // Hapus semua entri yang menaungi host ini (host sendiri atau domain induknya).
            list.retain(|e| !host_matches(&host, std::slice::from_ref(e)));
            false
        } else {
            list.push(host);
            true
        };
        self.engine.set_blocker_exceptions(&self.settings.blocker_exceptions);
        if let Some(page) = self.pages.get(&self.tabs.active()) {
            page.reload();
        }
        self.change(UiChange::Settings);
        Some(excepted)
    }

    /// Event dari `Engine::set_global_sink`.
    pub fn handle_global_event(&mut self, event: PageEvent) {
        if let Some(message) = download_notice(&event) {
            self.change(UiChange::Notice(message));
        }
    }

    pub fn take_changes(&mut self) -> Vec<UiChange> {
        std::mem::take(&mut self.changes)
    }

    pub fn session_if_dirty(&mut self) -> Option<crate::session::SessionFile> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        Some(self.tabs.to_session())
    }

    /// HTTP/3 dimatikan (bawaan): petunjuk HTTP/3 dihapus sebelum koneksi baru
    /// sempat memakainya (saat dibuka, setiap tick, sebelum navigasi).
    fn forget_http3_hints_if_disabled(&self) {
        if !self.settings.http3_enabled {
            self.engine.forget_http3_hints();
        }
    }

    fn change(&mut self, change: UiChange) {
        if matches!(change, UiChange::Notice(_)) || !self.changes.contains(&change) {
            self.changes.push(change);
        }
    }

    fn after_switch(&mut self, prev: TabId, now: Instant) {
        let now_active = self.tabs.active();
        if now_active == prev {
            return;
        }
        if let Some(page) = self.pages.get(&prev) {
            page.request_number(JsTag::ScrollY, SCROLL_Y_JS);
            if self.tabs.get(prev).is_some_and(|t| t.url.as_ref().is_some_and(is_youtube_watch)) {
                page.request_number(JsTag::VideoTime, VIDEO_TIME_JS);
            }
        }
        self.wake(now_active);
        // Batas tab bangun ditegakkan langsung, bukan menunggu tick berikutnya.
        self.tick(now, Pressure::Normal);
        self.dirty = true;
        self.change(UiChange::Active);
        self.change(UiChange::Tabs);
        self.change(UiChange::Nav);
    }

    /// Membuat page untuk tab tidur yang punya URL, lalu memuatnya.
    fn wake(&mut self, id: TabId) {
        if self.pages.contains_key(&id) {
            return;
        }
        let Some(tab) = self.tabs.get(id) else { return };
        let (Some(url), emulation) = (tab.url.clone(), tab.emulation) else { return };
        self.forget_http3_hints_if_disabled();
        let page = self.engine.create_page((self.sinks)(id));
        // Emulasi dipasang sebelum memuat, supaya request pertama sudah
        // memakai user agent yang benar.
        if emulation.mobile {
            page.set_mobile_user_agent(true);
        }
        if emulation.color_scheme != ColorScheme::System {
            page.set_color_scheme(emulation.color_scheme);
        }
        page.load(&url);
        self.pages.insert(id, page);
        if let Some(tab) = self.tabs.get_mut(id) {
            tab.awake = true;
            tab.loading = true;
        }
        self.change(UiChange::Tabs);
        // View baru untuk tab aktif harus dipasang ke area konten.
        if id == self.tabs.active() {
            self.change(UiChange::Active);
        }
    }

    /// Melepas page tab. Halaman YouTube menyimpan posisi video ke URL.
    fn sleep(&mut self, id: TabId) {
        self.pages.remove(&id);
        if let Some(tab) = self.tabs.get_mut(id) {
            if let (Some(url), Some(t)) = (&tab.url, tab.video_time)
                && is_youtube_watch(url)
            {
                tab.url = Some(with_resume_time(url, t));
            }
            tab.awake = false;
            tab.sleep_pending = false;
            tab.sleep_forced = false;
            tab.activity_clear = false;
            tab.awaiting_video_time = false;
            tab.media_active = false;
            tab.draft = false;
            tab.loading = false;
            tab.restore_scroll = tab.scroll_y > 0.0;
        }
        self.dirty = true;
        self.change(UiChange::Tabs);
    }
}

mod history;
#[cfg(test)]
mod tests;
