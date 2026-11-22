//! Controller browser: menghubungkan daftar tab, kebijakan tidur, dan mesin.
//! Tidak tahu apa pun tentang UI; UI membaca `take_changes()` setelah setiap
//! pemanggilan dan menggambar ulang.

use crate::blocking::{host_matches, normalize_host};
use crate::engine::{ColorScheme, Emulation, Engine, JsTag, Page, PageEvent};
use crate::history::{History, HistoryEntry, note_visit, visit_key};
use crate::omnibox::{Parsed, classify_input, parse_input};
use crate::session::SessionFile;
use crate::settings::Settings;
use crate::sleep::{Pressure, tabs_to_sleep};
use crate::tabs::{PendingVisit, TabId, TabList, Url};
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
            Some(format!("Selesai diunduh: {name}"))
        }
        PageEvent::DownloadFailed(message) => Some(format!("Gagal mengunduh: {message}")),
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

    /// Mengganti jam dinding riwayat (untuk test).
    pub fn set_clock(&mut self, clock: Box<dyn Fn() -> u64>) {
        self.clock = clock;
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Isi `history.json` kalau riwayat berubah sejak penyimpanan terakhir.
    pub fn history_bytes_if_dirty(&mut self) -> Option<Vec<u8>> {
        if !self.history.take_dirty() {
            return None;
        }
        Some(self.history.to_bytes((self.clock)()))
    }

    /// Isi `history.json` sekarang (saat keluar, atau setelah menghapus).
    pub fn history_bytes(&mut self) -> Vec<u8> {
        self.history.take_dirty();
        self.history.to_bytes((self.clock)())
    }

    pub fn delete_history(&mut self, url: &str) -> bool {
        let deleted = self.history.delete(url);
        if deleted {
            self.forget_visit_keys();
            self.change(UiChange::History);
        }
        deleted
    }

    /// Menghapus riwayat sejak `since` (detik unix; semua kalau `None`).
    pub fn clear_history_since(&mut self, since: Option<u64>) -> usize {
        let removed = self.history.clear_since(since);
        if removed > 0 {
            self.forget_visit_keys();
            self.change(UiChange::History);
        }
        removed
    }

    /// Setelah riwayat dihapus, kunjungan berikutnya ke halaman yang sedang
    /// terbuka boleh dicatat lagi.
    fn forget_visit_keys(&mut self) {
        let ids: Vec<TabId> = self.tabs.iter().map(|t| t.id).collect();
        for id in ids {
            if let Some(tab) = self.tabs.get_mut(id)
                && tab.visit_key.as_deref().is_some_and(|k| !self.history.entries().iter().any(|e| e.url == k))
            {
                tab.visit_key = None;
            }
        }
    }

    /// Pencarian riwayat untuk MCP dan jendela Riwayat.
    pub fn search_history(&self, query: &str, since: Option<u64>, offset: usize, limit: usize) -> (Vec<&HistoryEntry>, usize) {
        self.history.search(query, since, offset, limit)
    }

    /// Detik unix sekarang menurut jam riwayat.
    pub fn unix_now(&self) -> u64 {
        (self.clock)()
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
        if self.tabs.close(id, now).is_none() {
            return;
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
                    self.change(UiChange::Notice(
                        "Halaman ini crash berulang kali. Tekan ⌘R untuk memuat ulang.".into(),
                    ));
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
                    self.change(UiChange::Notice(format!("Gagal memuat halaman: {message}")));
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

#[cfg(test)]
mod tests {
    use super::*;
use crate::engine::{Engine, JsTag, Page, PageEvent};
    use crate::session::{SavedTab, SessionFile, SESSION_VERSION};
    use crate::settings::Settings;
    use crate::sleep::Pressure;
    use crate::tabs::{TabId, Url};
    use crate::engine::Rect;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    #[derive(Debug, Clone, PartialEq)]
    enum Call { Load(String), Reload, Stop, Back, Forward, Activity, Number(JsTag), Script(String), MobileUa(bool), Scheme(ColorScheme) }

    #[derive(Default)]
    struct Log { calls: Vec<Call>, live: usize, exceptions: Vec<String> }
    type Shared = Rc<RefCell<Log>>;

    struct FakeEngine(Shared);
    struct FakePage(Shared);

    impl Drop for FakePage {
        fn drop(&mut self) { self.0.borrow_mut().live -= 1; }
    }

    impl Page for FakePage {
        type View = ();
        fn load(&self, url: &Url) { self.0.borrow_mut().calls.push(Call::Load(url.to_string())); }
        fn go_back(&self) { self.0.borrow_mut().calls.push(Call::Back); }
        fn go_forward(&self) { self.0.borrow_mut().calls.push(Call::Forward); }
        fn reload(&self) { self.0.borrow_mut().calls.push(Call::Reload); }
        fn stop(&self) { self.0.borrow_mut().calls.push(Call::Stop); }
        fn request_activity(&self) { self.0.borrow_mut().calls.push(Call::Activity); }
        fn request_number(&self, tag: JsTag, _script: &str) { self.0.borrow_mut().calls.push(Call::Number(tag)); }
        fn run_script(&self, script: &str) { self.0.borrow_mut().calls.push(Call::Script(script.into())); }
        fn evaluate(&self, script: &str, _reply: Box<dyn FnOnce(Result<String, String>)>) {
            self.0.borrow_mut().calls.push(Call::Script(script.into()));
        }
        fn evaluate_async(&self, body: &str, _reply: Box<dyn FnOnce(Result<String, String>)>) {
            self.0.borrow_mut().calls.push(Call::Script(body.into()));
        }
        fn snapshot(&self, _max_width: f64, _reply: Box<dyn FnOnce(Result<Vec<u8>, String>)>) {}
        fn capture(&self, _area: Rect, _scale: f64, _reply: Box<dyn FnOnce(Result<Vec<u8>, String>)>) {}
        fn set_mobile_user_agent(&self, mobile: bool) { self.0.borrow_mut().calls.push(Call::MobileUa(mobile)); }
        fn set_color_scheme(&self, scheme: ColorScheme) { self.0.borrow_mut().calls.push(Call::Scheme(scheme)); }
        fn fill_login(&self, _origin: &str, _username: &str, _password: &crate::engine::Secret, _reply: Box<dyn FnOnce(Result<String, String>)>) {}
        fn process_id(&self) -> Option<i32> { None }
        fn view(&self) {}
    }

    impl Engine for FakeEngine {
        type Page = FakePage;
        fn create_page(&self, _on_event: Box<dyn Fn(PageEvent)>) -> FakePage {
            self.0.borrow_mut().live += 1;
            FakePage(self.0.clone())
        }
        fn set_blocker_exceptions(&self, hosts: &[String]) { self.0.borrow_mut().exceptions = hosts.to_vec(); }
        fn set_global_sink(&self, _sink: Box<dyn Fn(PageEvent)>) {}
    }

    fn u(s: &str) -> Url { Url::parse(s).unwrap() }
    fn secs(n: u64) -> Duration { Duration::from_secs(n) }

    /// Tab latar tidur setelah 300 detik, paling banyak 3 tab bangun (angka
    /// kecil supaya test mudah dibaca; bawaannya lebih longgar).
    fn setup(session: Option<&SessionFile>) -> (Browser<FakeEngine>, Shared, Instant) {
        let log = Shared::default();
        let t0 = Instant::now();
        let settings = Settings { sleep_idle_secs: 300, max_awake: 3, ..Settings::default() };
        let b = Browser::new(FakeEngine(log.clone()), settings, session, History::new(), Box::new(|_| Box::new(|_| {})), t0);
        (b, log, t0)
    }

    /// Tab A (a.com) di latar, tab B (b.com) aktif. Keduanya bangun.
    fn two_tabs(b: &mut Browser<FakeEngine>, t0: Instant) -> (TabId, TabId) {
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.new_tab(t0);
        assert!(b.navigate("b.com", t0));
        (a, b.tabs().active())
    }

    fn has(log: &Shared, call: Call) -> bool { log.borrow().calls.contains(&call) }

    #[test]
    fn navigate_creates_page_and_loads_parsed_url() {
        let (mut b, log, t0) = setup(None);
        assert!(b.navigate("youtube.com", t0));
        assert_eq!(log.borrow().live, 1);
        assert!(has(&log, Call::Load("https://youtube.com/".into())));
    }

    #[test]
    fn navigating_blank_active_tab_shows_its_page() {
        let (mut b, _log, t0) = setup(None);
        b.take_changes();
        assert!(b.navigate("a.com", t0));
        assert!(b.take_changes().contains(&UiChange::Active));
        b.new_tab(t0);
        b.take_changes();
        assert!(b.navigate("b.com", t0));
        assert!(b.take_changes().contains(&UiChange::Active));
    }

    #[test]
    fn new_tab_has_no_page_until_navigation() {
        let (mut b, log, t0) = setup(None);
        b.new_tab(t0);
        assert_eq!(log.borrow().live, 0);
    }

    #[test]
    fn deactivating_tab_requests_scroll_position() {
        let (mut b, log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        log.borrow_mut().calls.clear();
        b.activate(a, t0);
        assert!(has(&log, Call::Number(JsTag::ScrollY)));
    }

    #[test]
    fn tick_sleeps_idle_tab_after_media_state_false() {
        let (mut b, log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        let t = t0 + secs(301);
        b.tick(t, Pressure::Normal);
        assert!(has(&log, Call::Activity));
        b.handle_event(a, PageEvent::Activity { media: false, draft: false }, t);
        assert_eq!(log.borrow().live, 1);
        assert!(!b.tabs().get(a).unwrap().awake);
    }

    #[test]
    fn media_state_true_cancels_pending_sleep() {
        let (mut b, log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        let t = t0 + secs(301);
        b.tick(t, Pressure::Normal);
        b.handle_event(a, PageEvent::Activity { media: true, draft: false }, t);
        assert_eq!(log.borrow().live, 2);
        let tab = b.tabs().get(a).unwrap();
        assert!(tab.awake && !tab.sleep_pending);
    }

    #[test]
    fn unsent_text_keeps_tab_awake() {
        let (mut b, log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        let t = t0 + secs(301);
        b.tick(t, Pressure::Normal);
        b.handle_event(a, PageEvent::Activity { media: false, draft: true }, t);
        let tab = b.tabs().get(a).unwrap();
        assert!(tab.awake && tab.draft && !tab.sleep_pending);
        assert_eq!(log.borrow().live, 2);
        // Tick berikutnya tidak memilihnya lagi selama draft masih ada.
        b.tick(t + secs(30), Pressure::Normal);
        assert!(!b.tabs().get(a).unwrap().sleep_pending);
        // Teksnya sudah dikirim: tab boleh tidur lagi.
        b.handle_event(a, PageEvent::Activity { media: false, draft: false }, t + secs(30));
        b.tick(t + secs(60), Pressure::Normal);
        b.handle_event(a, PageEvent::Activity { media: false, draft: false }, t + secs(60));
        assert!(!b.tabs().get(a).unwrap().awake);
    }

    #[test]
    fn request_sleep_respects_unsent_text() {
        let (mut b, log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        assert!(b.request_sleep(a));
        b.handle_event(a, PageEvent::Activity { media: false, draft: true }, t0);
        assert!(b.tabs().get(a).unwrap().awake);
        assert_eq!(log.borrow().live, 2);
    }

    #[test]
    fn critical_memory_sleeps_tab_despite_unsent_text() {
        let (mut b, log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        b.handle_event(a, PageEvent::Activity { media: false, draft: true }, t0);
        b.tick(t0 + secs(5), Pressure::Warning);
        assert!(!b.tabs().get(a).unwrap().sleep_pending, "draft masih ditahan saat memory baru menipis");
        b.tick(t0 + secs(5), Pressure::Critical);
        b.handle_event(a, PageEvent::Activity { media: false, draft: true }, t0 + secs(5));
        assert!(!b.tabs().get(a).unwrap().awake);
        assert_eq!(log.borrow().live, 1);
    }

    #[test]
    fn waking_restores_scroll_after_load_finished() {
        let (mut b, log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        b.handle_event(a, PageEvent::Number { tag: JsTag::ScrollY, value: Some(640.0) }, t0);
        let t = t0 + secs(301);
        b.tick(t, Pressure::Normal);
        b.handle_event(a, PageEvent::Activity { media: false, draft: false }, t);
        log.borrow_mut().calls.clear();
        b.activate(a, t);
        assert!(has(&log, Call::Load("https://a.com/".into())));
        b.handle_event(a, PageEvent::LoadFinished, t);
        assert!(has(&log, Call::Script("window.scrollTo(0, 640)".into())));
    }

    #[test]
    fn youtube_tab_resumes_at_cached_video_time() {
        let (mut b, log, t0) = setup(None);
        let yt = b.tabs().active();
        assert!(b.navigate("https://www.youtube.com/watch?v=abc", t0));
        b.new_tab(t0);
        assert!(b.navigate("b.com", t0));
        let t = t0 + secs(301);
        b.tick(t, Pressure::Normal);
        assert!(has(&log, Call::Number(JsTag::VideoTime)));
        b.handle_event(yt, PageEvent::Number { tag: JsTag::VideoTime, value: Some(125.7) }, t);
        b.handle_event(yt, PageEvent::Activity { media: false, draft: false }, t);
        assert_eq!(b.tabs().get(yt).unwrap().url.as_ref().unwrap().as_str(), "https://www.youtube.com/watch?v=abc&t=125s");
    }

    /// (url, title, visit_count, typed_count) semua entri riwayat, terbaru dulu.
    fn hist(b: &Browser<FakeEngine>) -> Vec<(String, String, u32, u32)> {
        b.history().entries().iter().map(|e| (e.url.clone(), e.title.clone(), e.visit_count, e.typed_count)).collect()
    }

    fn visits(b: &Browser<FakeEngine>, url: &str) -> u32 {
        b.history().entries().iter().find(|e| e.url == url).map_or(0, |e| e.visit_count)
    }

    #[test]
    fn load_records_once_and_reload_does_not_count() {
        let (mut b, _log, t0) = setup(None);
        b.set_clock(Box::new(|| 1_000));
        let a = b.tabs().active();
        assert!(b.navigate_from_address("a.com", t0));
        b.handle_event(a, PageEvent::TitleChanged("Situs A".into()), t0);
        b.handle_event(a, PageEvent::LoadFinished, t0);
        assert_eq!(hist(&b), vec![("https://a.com/".into(), "Situs A".into(), 1, 1)]);
        assert!(b.take_changes().contains(&UiChange::History));
        // Muat ulang (tanpa navigasi baru) tidak menambah hitungan.
        b.handle_event(a, PageEvent::LoadFinished, t0);
        assert_eq!(visits(&b, "https://a.com/"), 1);
        assert_eq!(b.history().entries()[0].last_visit, 1_000);
    }

    #[test]
    fn typed_only_for_addresses_not_searches_or_links() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate_from_address("resep nasi", t0));
        b.handle_event(a, PageEvent::LoadFinished, t0);
        b.handle_event(a, PageEvent::UrlChanged(u("https://b.com/x")), t0);
        b.handle_event(a, PageEvent::LoadFinished, t0);
        let h = hist(&b);
        assert_eq!((h[0].0.as_str(), h[0].3), ("https://b.com/x", 0), "link bukan ketikan");
        assert_eq!((h[1].0.as_str(), h[1].3), ("https://www.google.com/search?q=resep+nasi", 0), "pencarian bukan ketikan");
    }

    #[test]
    fn explicit_navigation_to_same_url_counts_again() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        for _ in 0..2 {
            assert!(b.navigate_from_address("a.com", t0));
            b.handle_event(a, PageEvent::LoadFinished, t0);
        }
        assert_eq!(hist(&b), vec![("https://a.com/".into(), String::new(), 2, 2)]);
    }

    #[test]
    fn redirect_records_only_final_url() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate_from_address("a.com/lama", t0));
        b.handle_event(a, PageEvent::UrlChanged(u("https://a.com/baru")), t0);
        b.handle_event(a, PageEvent::LoadFinished, t0);
        assert_eq!(hist(&b), vec![("https://a.com/baru".into(), String::new(), 1, 1)]);
    }

    #[test]
    fn same_document_navigation_records_but_fragment_does_not() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("https://www.youtube.com/", t0));
        b.handle_event(a, PageEvent::LoadFinished, t0);
        b.handle_event(a, PageEvent::SameDocumentNavigation(u("https://www.youtube.com/watch?v=1")), t0);
        b.handle_event(a, PageEvent::TitleChanged("Video Satu - YouTube".into()), t0);
        b.handle_event(a, PageEvent::SameDocumentNavigation(u("https://www.youtube.com/watch?v=1#komentar")), t0);
        let h = hist(&b);
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].0.as_str(), h[0].1.as_str(), h[0].2), ("https://www.youtube.com/watch?v=1", "Video Satu - YouTube", 1));
    }

    #[test]
    fn title_during_load_does_not_rename_previous_entry() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.handle_event(a, PageEvent::TitleChanged("A".into()), t0);
        b.handle_event(a, PageEvent::LoadFinished, t0);
        b.handle_event(a, PageEvent::UrlChanged(u("https://b.com/")), t0);
        b.handle_event(a, PageEvent::TitleChanged("B".into()), t0);
        assert_eq!(hist(&b), vec![("https://a.com/".into(), "A".into(), 1, 0)]);
        b.handle_event(a, PageEvent::LoadFinished, t0);
        assert_eq!(hist(&b)[0], ("https://b.com/".into(), "B".into(), 1, 0));
        b.handle_event(a, PageEvent::TitleChanged(String::new()), t0);
        assert_eq!(hist(&b)[0].1, "B", "judul kosong diabaikan");
    }

    #[test]
    fn session_restore_does_not_count() {
        let saved = |url: &str| SavedTab { url: url.into(), title: String::new(), scroll_y: 0.0 };
        let s = SessionFile { version: SESSION_VERSION, active_index: Some(0), tabs: vec![saved("https://a.com/")] };
        let (mut b, _log, t0) = setup(Some(&s));
        let a = b.tabs().active();
        b.handle_event(a, PageEvent::LoadFinished, t0);
        assert!(b.history().is_empty());
    }

    #[test]
    fn youtube_sleep_and_wake_keep_one_entry_without_resume_time() {
        let (mut b, _log, t0) = setup(None);
        let yt = b.tabs().active();
        assert!(b.navigate("https://www.youtube.com/watch?v=abc", t0));
        b.handle_event(yt, PageEvent::LoadFinished, t0);
        b.new_tab(t0);
        assert!(b.navigate("b.com", t0));
        let t = t0 + secs(301);
        b.tick(t, Pressure::Normal);
        b.handle_event(yt, PageEvent::Number { tag: JsTag::VideoTime, value: Some(125.7) }, t);
        b.handle_event(yt, PageEvent::Activity { media: false, draft: false }, t);
        assert!(!b.tabs().get(yt).unwrap().awake);
        b.activate(yt, t);
        b.handle_event(yt, PageEvent::LoadFinished, t);
        assert_eq!(visits(&b, "https://www.youtube.com/watch?v=abc"), 1);
        assert!(b.history().entries().iter().all(|e| !e.url.contains("t=125")));
    }

    #[test]
    fn reopened_closed_tab_does_not_count() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.handle_event(a, PageEvent::LoadFinished, t0);
        b.new_tab(t0);
        b.close_tab(a, t0);
        b.reopen_closed_tab(t0);
        let reopened = b.tabs().active();
        b.handle_event(reopened, PageEvent::LoadFinished, t0);
        assert_eq!(visits(&b, "https://a.com/"), 1);
    }

    #[test]
    fn failed_load_and_download_clear_pending_visit() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.handle_event(a, PageEvent::LoadFailed("offline".into()), t0);
        assert_eq!(b.tabs().get(a).unwrap().pending_visit, None);
        assert!(b.navigate("a.com/berkas.zip", t0));
        b.handle_event(a, PageEvent::DownloadStarted, t0);
        assert_eq!(b.tabs().get(a).unwrap().pending_visit, None);
        assert!(b.history().is_empty());
    }

    #[test]
    fn history_disabled_records_nothing() {
        let log = Shared::default();
        let t0 = Instant::now();
        let settings = Settings { history_enabled: false, ..Settings::default() };
        let mut b = Browser::new(FakeEngine(log), settings, None, History::new(), Box::new(|_| Box::new(|_| {})), t0);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.handle_event(a, PageEvent::LoadFinished, t0);
        assert!(b.history().is_empty());
    }

    #[test]
    fn dirty_bytes_and_delete_forgets_visit() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.handle_event(a, PageEvent::LoadFinished, t0);
        assert!(b.history_bytes_if_dirty().is_some());
        assert!(b.history_bytes_if_dirty().is_none());
        assert!(b.delete_history("https://a.com/"));
        assert!(b.take_changes().contains(&UiChange::History));
        // Halaman yang masih terbuka tercatat lagi saat dimuat ulang.
        b.handle_event(a, PageEvent::LoadFinished, t0);
        assert_eq!(visits(&b, "https://a.com/"), 1);
        assert_eq!(b.clear_history_since(None), 1);
        assert!(b.history().is_empty());
    }

    #[test]
    fn open_in_new_tab_opens_next_to_opener_and_activates() {
        let (mut b, log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.handle_event(a, PageEvent::OpenInNewTab(u("https://n.com/")), t0);
        let order: Vec<String> = b.tabs().iter().map(|t| t.url.as_ref().unwrap().to_string()).collect();
        assert_eq!(order, vec!["https://a.com/".to_string(), "https://n.com/".to_string()]);
        assert_ne!(b.tabs().active(), a);
        assert!(has(&log, Call::Load("https://n.com/".into())));
    }

    #[test]
    fn crash_in_active_tab_reloads_once_then_notices() {
        let (mut b, log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.take_changes();
        b.handle_event(a, PageEvent::ProcessCrashed, t0);
        b.handle_event(a, PageEvent::ProcessCrashed, t0 + secs(5));
        let reloads = log.borrow().calls.iter().filter(|c| **c == Call::Reload).count();
        assert_eq!(reloads, 1);
        assert!(b.take_changes().iter().any(|c| matches!(c, UiChange::Notice(_))));
    }

    #[test]
    fn crash_in_background_tab_puts_it_to_sleep() {
        let (mut b, log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        b.handle_event(a, PageEvent::ProcessCrashed, t0);
        assert!(!b.tabs().get(a).unwrap().awake);
        assert_eq!(log.borrow().live, 1);
    }

    #[test]
    fn late_events_for_closed_or_sleeping_tab_are_ignored() {
        let (mut b, log, t0) = setup(None);
        let (a, bb) = two_tabs(&mut b, t0);
        b.close_tab(a, t0);
        b.handle_event(a, PageEvent::TitleChanged("x".into()), t0);
        b.handle_event(a, PageEvent::Activity { media: false, draft: false }, t0);
        assert_eq!(b.tabs().iter().count(), 1);
        assert_eq!(log.borrow().live, 1);

        b.new_tab(t0);
        assert!(b.navigate("c.com", t0));
        b.handle_event(bb, PageEvent::ProcessCrashed, t0);
        assert!(!b.tabs().get(bb).unwrap().awake);
        let live = log.borrow().live;
        b.handle_event(bb, PageEvent::Number { tag: JsTag::ScrollY, value: Some(9.0) }, t0);
        assert_eq!(b.tabs().get(bb).unwrap().scroll_y, 0.0);
        assert_eq!(log.borrow().live, live);
    }

    #[test]
    fn title_and_url_events_update_tab_and_dirty_session() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.session_if_dirty();
        b.handle_event(a, PageEvent::TitleChanged("Judul".into()), t0);
        b.handle_event(a, PageEvent::UrlChanged(u("https://a.com/x")), t0);
        let tab = b.tabs().get(a).unwrap();
        assert_eq!(tab.title, "Judul");
        assert_eq!(tab.url.as_ref().unwrap().as_str(), "https://a.com/x");
        assert!(b.session_if_dirty().is_some());
        assert!(b.session_if_dirty().is_none());
    }

    #[test]
    fn toggle_exception_updates_settings_engine_and_reloads() {
        let (mut b, log, t0) = setup(None);
        assert!(b.navigate("https://www.detik.com/x", t0));
        b.take_changes();
        assert_eq!(b.toggle_blocker_exception(), Some(true));
        assert_eq!(b.settings().blocker_exceptions, vec!["detik.com".to_string()]);
        assert_eq!(log.borrow().exceptions, vec!["detik.com".to_string()]);
        assert!(b.active_host_excepted());
        assert!(has(&log, Call::Reload));
        assert!(b.take_changes().contains(&UiChange::Settings));
        assert_eq!(b.toggle_blocker_exception(), Some(false));
        assert!(b.settings().blocker_exceptions.is_empty());
        assert!(log.borrow().exceptions.is_empty());
        assert!(!b.active_host_excepted());
    }

    #[test]
    fn toggle_on_blank_tab_returns_none() {
        let (mut b, _log, _t0) = setup(None);
        assert_eq!(b.toggle_blocker_exception(), None);
    }

    #[test]
    fn download_finished_emits_notice() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.take_changes();
        b.handle_event(a, PageEvent::DownloadFinished("/Users/x/Downloads/a.pdf".into()), t0);
        assert!(b.take_changes().contains(&UiChange::Notice("Selesai diunduh: a.pdf".into())));
    }

    #[test]
    fn download_failed_emits_notice() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.take_changes();
        b.handle_event(a, PageEvent::DownloadFailed("koneksi terputus".into()), t0);
        assert!(b.take_changes().contains(&UiChange::Notice("Gagal mengunduh: koneksi terputus".into())));
    }

    #[test]
    fn download_started_restores_last_committed_url() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        b.handle_event(a, PageEvent::UrlChanged(u("https://a.com/")), t0);
        b.handle_event(a, PageEvent::LoadFinished, t0);
        b.handle_event(a, PageEvent::UrlChanged(u("https://cdn.a.com/file.zip")), t0);
        b.handle_event(a, PageEvent::DownloadStarted, t0);
        assert_eq!(b.tabs().get(a).unwrap().url.as_ref().unwrap().as_str(), "https://a.com/");
    }

    #[test]
    fn download_in_fresh_tab_leaves_it_blank_and_out_of_session() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("https://a.com/file.zip", t0));
        b.handle_event(a, PageEvent::UrlChanged(u("https://cdn.a.com/file.zip")), t0);
        b.handle_event(a, PageEvent::DownloadStarted, t0);
        assert!(b.tabs().get(a).unwrap().url.is_none());
        assert!(b.session_if_dirty().unwrap().tabs.is_empty());
    }

    #[test]
    fn toggle_from_subdomain_turns_blocking_back_on() {
        let (mut b, log, t0) = setup(None);
        assert!(b.navigate("https://www.detik.com/x", t0));
        assert_eq!(b.toggle_blocker_exception(), Some(true));
        assert!(b.navigate("https://news.detik.com/y", t0));
        assert!(b.active_host_excepted());
        assert_eq!(b.toggle_blocker_exception(), Some(false));
        assert!(b.settings().blocker_exceptions.is_empty());
        assert!(log.borrow().exceptions.is_empty());
        assert!(!b.active_host_excepted());
    }

    #[test]
    fn youtube_resume_survives_media_state_arriving_first() {
        let (mut b, _log, t0) = setup(None);
        let yt = b.tabs().active();
        assert!(b.navigate("https://www.youtube.com/watch?v=abc", t0));
        b.new_tab(t0);
        assert!(b.navigate("b.com", t0));
        let t = t0 + secs(301);
        b.tick(t, Pressure::Normal);
        b.handle_event(yt, PageEvent::Activity { media: false, draft: false }, t);
        assert!(b.tabs().get(yt).unwrap().awake, "harus menunggu posisi video dulu");
        b.handle_event(yt, PageEvent::Number { tag: JsTag::VideoTime, value: Some(125.7) }, t);
        let tab = b.tabs().get(yt).unwrap();
        assert!(!tab.awake);
        assert_eq!(tab.url.as_ref().unwrap().as_str(), "https://www.youtube.com/watch?v=abc&t=125s");
    }

    #[test]
    fn youtube_sleep_proceeds_when_video_time_unavailable() {
        let (mut b, _log, t0) = setup(None);
        let yt = b.tabs().active();
        assert!(b.navigate("https://www.youtube.com/watch?v=abc", t0));
        b.new_tab(t0);
        assert!(b.navigate("b.com", t0));
        let t = t0 + secs(301);
        b.tick(t, Pressure::Normal);
        b.handle_event(yt, PageEvent::Activity { media: false, draft: false }, t);
        b.handle_event(yt, PageEvent::Number { tag: JsTag::VideoTime, value: None }, t);
        let tab = b.tabs().get(yt).unwrap();
        assert!(!tab.awake);
        assert_eq!(tab.url.as_ref().unwrap().as_str(), "https://www.youtube.com/watch?v=abc");
    }

    #[test]
    fn deactivating_youtube_tab_requests_video_time() {
        let (mut b, log, t0) = setup(None);
        assert!(b.navigate("https://www.youtube.com/watch?v=abc", t0));
        log.borrow_mut().calls.clear();
        b.new_tab(t0);
        assert!(has(&log, Call::Number(JsTag::VideoTime)));
    }

    #[test]
    fn opening_fourth_awake_tab_sleeps_oldest_background_tab_right_away() {
        let (mut b, log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("a.com", t0));
        for (i, site) in ["b.com", "c.com", "d.com"].iter().enumerate() {
            b.new_tab(t0 + secs(i as u64 + 1));
            assert!(b.navigate(site, t0 + secs(i as u64 + 1)));
        }
        assert_eq!(log.borrow().live, 4);
        b.handle_event(a, PageEvent::Activity { media: false, draft: false }, t0 + secs(4));
        assert!(!b.tabs().get(a).unwrap().awake);
        assert_eq!(log.borrow().live, 3);
    }

    #[test]
    fn non_finite_numbers_are_ignored() {
        let (mut b, _log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        b.handle_event(a, PageEvent::Number { tag: JsTag::ScrollY, value: Some(f64::NAN) }, t0);
        b.handle_event(a, PageEvent::Number { tag: JsTag::VideoTime, value: Some(f64::INFINITY) }, t0);
        let tab = b.tabs().get(a).unwrap();
        assert_eq!(tab.scroll_y, 0.0);
        assert!(tab.video_time.is_none());
    }

    #[test]
    fn load_failure_in_active_tab_emits_notice() {
        let (mut b, _log, t0) = setup(None);
        let a = b.tabs().active();
        assert!(b.navigate("localhost:3000", t0));
        b.take_changes();
        b.handle_event(a, PageEvent::LoadFailed("Could not connect to the server.".into()), t0);
        assert!(!b.tabs().get(a).unwrap().loading);
        assert!(b.take_changes().contains(&UiChange::Notice("Gagal memuat halaman: Could not connect to the server.".into())));
    }

    #[test]
    fn global_download_events_emit_notices() {
        let (mut b, _log, _t0) = setup(None);
        b.take_changes();
        b.handle_global_event(PageEvent::DownloadFinished("/Users/x/Downloads/a.pdf".into()));
        b.handle_global_event(PageEvent::DownloadFailed("koneksi terputus".into()));
        let changes = b.take_changes();
        assert!(changes.contains(&UiChange::Notice("Selesai diunduh: a.pdf".into())));
        assert!(changes.contains(&UiChange::Notice("Gagal mengunduh: koneksi terputus".into())));
    }

    #[test]
    fn session_restore_wakes_only_active_tab() {
        let saved = |url: &str| SavedTab { url: url.into(), title: String::new(), scroll_y: 0.0 };
        let s = SessionFile {
            version: SESSION_VERSION,
            active_index: Some(1),
            tabs: vec![saved("https://a.com/"), saved("https://b.com/"), saved("https://c.com/")],
        };
        let (_b, log, _t0) = setup(Some(&s));
        assert_eq!(log.borrow().live, 1);
        assert_eq!(log.borrow().calls, vec![Call::Load("https://b.com/".into())]);
    }

    #[test]
    fn open_tab_in_background_loads_without_activating() {
        let (mut b, log, t0) = setup(None);
        let first = b.tabs().active();
        let id = b.open_tab("example.com", false, t0).expect("tab dibuat");
        assert_eq!(b.tabs().active(), first);
        assert!(b.page(id).is_some(), "tab latar langsung dimuat");
        assert!(has(&log, Call::Load("https://example.com/".into())));
        assert!(b.open_tab("   ", true, t0).is_none());
    }

    #[test]
    fn open_tab_activated_becomes_active() {
        let (mut b, _log, t0) = setup(None);
        let id = b.open_tab("example.com", true, t0).unwrap();
        assert_eq!(b.tabs().active(), id);
        assert!(b.page(id).is_some());
    }

    #[test]
    fn navigate_tab_loads_in_given_background_tab() {
        let (mut b, log, t0) = setup(None);
        let (a, active) = two_tabs(&mut b, t0);
        assert!(b.navigate_tab(a, "c.com", t0));
        assert_eq!(b.tabs().active(), active);
        assert_eq!(b.tabs().get(a).unwrap().url.as_ref().unwrap().as_str(), "https://c.com/");
        assert!(has(&log, Call::Load("https://c.com/".into())));
        assert!(!b.navigate_tab(TabId(999), "c.com", t0));
    }

    #[test]
    fn load_finished_counts_loads_per_tab() {
        let (mut b, _log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        assert_eq!(b.tabs().get(a).unwrap().loads, 0);
        b.handle_event(a, PageEvent::LoadFinished, t0);
        b.handle_event(a, PageEvent::LoadFinished, t0);
        assert_eq!(b.tabs().get(a).unwrap().loads, 2);
    }

    #[test]
    fn request_sleep_sleeps_background_tab_once_media_is_clear() {
        let (mut b, log, t0) = setup(None);
        let (a, active) = two_tabs(&mut b, t0);
        assert!(!b.request_sleep(active), "tab aktif tidak boleh ditidurkan");
        assert!(b.request_sleep(a));
        assert!(has(&log, Call::Activity));
        b.handle_event(a, PageEvent::Activity { media: false, draft: false }, t0);
        assert!(b.page(a).is_none());
        assert!(!b.tabs().get(a).unwrap().awake);
        assert!(!b.request_sleep(a), "tab yang sudah tidur");
    }

    #[test]
    fn request_sleep_keeps_tab_playing_media() {
        let (mut b, _log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        assert!(b.request_sleep(a));
        b.handle_event(a, PageEvent::Activity { media: true, draft: false }, t0);
        assert!(b.page(a).is_some());
    }

    #[test]
    fn tab_navigation_helpers_target_given_tab() {
        let (mut b, log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        log.borrow_mut().calls.clear();
        b.go_back_in(a);
        b.go_forward_in(a);
        b.reload_in(a);
        assert_eq!(log.borrow().calls, vec![Call::Back, Call::Forward, Call::Reload]);
    }

    fn phone() -> Emulation {
        Emulation { viewport: Some((390.0, 844.0)), mobile: true, color_scheme: ColorScheme::Dark }
    }

    #[test]
    fn emulation_applies_to_awake_page_and_reloads_when_user_agent_changes() {
        let (mut b, log, t0) = setup(None);
        let (_, active) = two_tabs(&mut b, t0);
        b.take_changes();
        log.borrow_mut().calls.clear();
        assert!(b.set_emulation(active, phone()), "ganti user agent harus memuat ulang");
        assert_eq!(log.borrow().calls, vec![Call::Scheme(ColorScheme::Dark), Call::MobileUa(true), Call::Reload]);
        assert!(b.take_changes().contains(&UiChange::Layout));
        assert_eq!(b.active_viewport(), Some((390.0, 844.0)));

        log.borrow_mut().calls.clear();
        let light = Emulation { color_scheme: ColorScheme::Light, ..phone() };
        assert!(!b.set_emulation(active, light), "hanya warna yang berubah: tanpa muat ulang");
        assert_eq!(log.borrow().calls, vec![Call::Scheme(ColorScheme::Light)]);
        assert!(!b.take_changes().contains(&UiChange::Layout), "viewport tidak berubah");
    }

    #[test]
    fn emulation_of_background_tab_does_not_relayout_the_window() {
        let (mut b, _log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        b.take_changes();
        b.set_emulation(a, phone());
        assert!(!b.take_changes().contains(&UiChange::Layout));
        assert_eq!(b.active_viewport(), None);
        b.activate(a, t0);
        assert_eq!(b.active_viewport(), Some((390.0, 844.0)));
    }

    #[test]
    fn emulation_survives_sleep_and_is_applied_before_loading() {
        let (mut b, log, t0) = setup(None);
        let (a, active) = two_tabs(&mut b, t0);
        b.set_emulation(a, phone());
        assert!(b.request_sleep(a));
        b.handle_event(a, PageEvent::Activity { media: false, draft: false }, t0);
        assert!(b.page(a).is_none());
        log.borrow_mut().calls.clear();
        b.activate(a, t0);
        let calls = log.borrow().calls.clone();
        let load = calls.iter().position(|c| matches!(c, Call::Load(_))).expect("tab dimuat ulang");
        let ua = calls.iter().position(|c| *c == Call::MobileUa(true)).expect("user agent HP dipasang lagi");
        let scheme = calls.iter().position(|c| *c == Call::Scheme(ColorScheme::Dark)).expect("mode gelap dipasang lagi");
        assert!(ua < load && scheme < load, "emulasi harus dipasang sebelum memuat: {calls:?}");
        let _ = active;
    }

    #[test]
    fn default_emulation_adds_no_engine_calls_on_wake() {
        let (mut b, log, t0) = setup(None);
        assert!(b.navigate("a.com", t0));
        assert!(!log.borrow().calls.iter().any(|c| matches!(c, Call::MobileUa(_) | Call::Scheme(_))));
    }
}
