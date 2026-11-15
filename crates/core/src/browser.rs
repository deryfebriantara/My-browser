//! Controller browser: menghubungkan daftar tab, kebijakan tidur, dan mesin.
//! Tidak tahu apa pun tentang UI; UI membaca `take_changes()` setelah setiap
//! pemanggilan dan menggambar ulang.

use crate::blocking::{host_matches, normalize_host};
use crate::engine::{Engine, JsTag, Page, PageEvent};
use crate::omnibox::parse_input;
use crate::session::SessionFile;
use crate::settings::Settings;
use crate::sleep::{Pressure, tabs_to_sleep};
use crate::tabs::{TabId, TabList};
use crate::youtube::{VIDEO_TIME_JS, is_youtube_watch, with_resume_time};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const SCROLL_Y_JS: &str = "window.scrollY";

/// Crash kedua dalam jangka ini tidak dimuat ulang otomatis.
const CRASH_RELOAD_WINDOW: Duration = Duration::from_secs(10);

pub type SinkFactory = Box<dyn Fn(TabId) -> Box<dyn Fn(PageEvent)>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiChange {
    /// Daftar tab, judul, atau status tidur berubah.
    Tabs,
    /// Tab aktif berganti; view konten perlu diganti.
    Active,
    /// URL, status loading, atau back/forward tab aktif berubah.
    Nav,
    /// Pengaturan berubah dan perlu disimpan.
    Settings,
    Notice(String),
}

pub struct Browser<E: Engine> {
    engine: E,
    tabs: TabList,
    pages: HashMap<TabId, E::Page>,
    settings: Settings,
    sinks: SinkFactory,
    changes: Vec<UiChange>,
    dirty: bool,
}

impl<E: Engine> Browser<E> {
    /// Memulihkan sesi (semua tab tidur) lalu membangunkan tab aktif.
    pub fn new(engine: E, settings: Settings, session: Option<&SessionFile>, sinks: SinkFactory, now: Instant) -> Self {
        let tabs = match session {
            Some(s) => TabList::from_session(s, now),
            None => TabList::new(now),
        };
        engine.set_blocker_exceptions(&settings.blocker_exceptions);
        let mut b = Self { engine, tabs, pages: HashMap::new(), settings, sinks, changes: Vec::new(), dirty: false };
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
            self.activate(id, now);
        }
    }

    pub fn activate(&mut self, id: TabId, now: Instant) {
        let prev = self.tabs.active();
        self.tabs.activate(id, now);
        self.after_switch(prev);
    }

    pub fn activate_index(&mut self, index: usize, now: Instant) {
        let prev = self.tabs.active();
        self.tabs.activate_index(index, now);
        self.after_switch(prev);
    }

    pub fn activate_next(&mut self, now: Instant) {
        let prev = self.tabs.active();
        self.tabs.activate_next(now);
        self.after_switch(prev);
    }

    pub fn activate_previous(&mut self, now: Instant) {
        let prev = self.tabs.active();
        self.tabs.activate_previous(now);
        self.after_switch(prev);
    }

    /// Memuat input address bar di tab aktif. `false` kalau input kosong.
    pub fn navigate(&mut self, input: &str, _now: Instant) -> bool {
        let Some(url) = parse_input(input, &self.settings.search_template) else { return false };
        let id = self.tabs.active();
        if let Some(tab) = self.tabs.get_mut(id) {
            tab.url = Some(url.clone());
            tab.scroll_y = 0.0;
            tab.restore_scroll = false;
            tab.video_time = None;
        }
        match self.pages.get(&id) {
            Some(page) => page.load(&url),
            None => self.wake(id),
        }
        self.dirty = true;
        self.change(UiChange::Tabs);
        self.change(UiChange::Nav);
        true
    }

    pub fn go_back(&mut self) {
        if let Some(page) = self.pages.get(&self.tabs.active()) {
            page.go_back();
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(page) = self.pages.get(&self.tabs.active()) {
            page.go_forward();
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

    /// Menandai tab yang layak tidur, lalu meminta status media terbaru. Tab
    /// benar-benar ditidurkan saat balasan `MediaState(false)` datang.
    pub fn tick(&mut self, now: Instant, pressure: Pressure) {
        let policy = self.settings.sleep_policy();
        for id in tabs_to_sleep(&self.tabs.snapshots(), now, &policy, pressure) {
            if let Some(tab) = self.tabs.get_mut(id) {
                tab.sleep_pending = true;
            }
        }
        let active = self.tabs.active();
        for tab in self.tabs.iter().filter(|t| t.id != active) {
            let Some(page) = self.pages.get(&tab.id) else { continue };
            page.request_media_state();
            if tab.url.as_ref().is_some_and(is_youtube_watch) {
                page.request_number(JsTag::VideoTime, VIDEO_TIME_JS);
            }
        }
    }

    /// Event dari page milik tab `id`. Event untuk tab yang sudah ditutup atau
    /// sedang tidur diabaikan.
    pub fn handle_event(&mut self, id: TabId, event: PageEvent, now: Instant) {
        if !self.pages.contains_key(&id) {
            return;
        }
        let is_active = self.tabs.active() == id;
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
            PageEvent::TitleChanged(title) => {
                tab.title = title;
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
            PageEvent::MediaState(active_media) => {
                tab.media_active = active_media;
                if tab.sleep_pending {
                    if !active_media && !is_active {
                        self.sleep(id);
                    } else {
                        tab.sleep_pending = false;
                    }
                }
            }
            PageEvent::Number { tag: JsTag::ScrollY, value: Some(y) } => {
                tab.scroll_y = y;
                self.dirty = true;
            }
            PageEvent::Number { tag: JsTag::VideoTime, value: Some(t) } => {
                tab.video_time = Some(t);
            }
            PageEvent::Number { value: None, .. } => {}
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
                tab.url = tab.committed_url.clone();
                tab.loading = false;
                self.dirty = true;
                self.change(UiChange::Tabs);
                if is_active {
                    self.change(UiChange::Nav);
                }
            }
            PageEvent::DownloadFinished(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.change(UiChange::Notice(format!("Selesai diunduh: {name}")));
            }
            PageEvent::DownloadFailed(message) => {
                self.change(UiChange::Notice(format!("Gagal mengunduh: {message}")));
            }
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
    pub fn toggle_blocker_exception(&mut self) -> Option<bool> {
        let host = self.active_host()?;
        let list = &mut self.settings.blocker_exceptions;
        let excepted = if let Some(i) = list.iter().position(|e| *e == host) {
            list.remove(i);
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

    fn after_switch(&mut self, prev: TabId) {
        let now_active = self.tabs.active();
        if now_active == prev {
            return;
        }
        if let Some(page) = self.pages.get(&prev) {
            page.request_number(JsTag::ScrollY, SCROLL_Y_JS);
        }
        self.wake(now_active);
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
        let Some(url) = self.tabs.get(id).and_then(|t| t.url.clone()) else { return };
        let page = self.engine.create_page((self.sinks)(id));
        page.load(&url);
        self.pages.insert(id, page);
        if let Some(tab) = self.tabs.get_mut(id) {
            tab.awake = true;
            tab.loading = true;
        }
        self.change(UiChange::Tabs);
    }

    /// Melepas page tab. Halaman YouTube menyimpan posisi video ke URL.
    fn sleep(&mut self, id: TabId) {
        self.pages.remove(&id);
        if let Some(tab) = self.tabs.get_mut(id) {
            if let (Some(url), Some(t)) = (&tab.url, tab.video_time) {
                if is_youtube_watch(url) {
                    tab.url = Some(with_resume_time(url, t));
                }
            }
            tab.awake = false;
            tab.sleep_pending = false;
            tab.media_active = false;
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
    use crate::blocking::{host_matches, normalize_host};
use crate::engine::{Engine, JsTag, Page, PageEvent};
    use crate::session::{SavedTab, SessionFile, SESSION_VERSION};
    use crate::settings::Settings;
    use crate::sleep::Pressure;
    use crate::tabs::{TabId, Url};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    #[derive(Debug, Clone, PartialEq)]
    enum Call { Load(String), Reload, Stop, Back, Forward, MediaState, Number(JsTag), Script(String) }

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
        fn request_media_state(&self) { self.0.borrow_mut().calls.push(Call::MediaState); }
        fn request_number(&self, tag: JsTag, _script: &str) { self.0.borrow_mut().calls.push(Call::Number(tag)); }
        fn run_script(&self, script: &str) { self.0.borrow_mut().calls.push(Call::Script(script.into())); }
        fn view(&self) {}
    }

    impl Engine for FakeEngine {
        type Page = FakePage;
        fn create_page(&self, _on_event: Box<dyn Fn(PageEvent)>) -> FakePage {
            self.0.borrow_mut().live += 1;
            FakePage(self.0.clone())
        }
        fn set_blocker_exceptions(&self, hosts: &[String]) { self.0.borrow_mut().exceptions = hosts.to_vec(); }
    }

    fn u(s: &str) -> Url { Url::parse(s).unwrap() }
    fn secs(n: u64) -> Duration { Duration::from_secs(n) }

    fn setup(session: Option<&SessionFile>) -> (Browser<FakeEngine>, Shared, Instant) {
        let log = Shared::default();
        let t0 = Instant::now();
        let b = Browser::new(FakeEngine(log.clone()), Settings::default(), session, Box::new(|_| Box::new(|_| {})), t0);
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
        assert!(has(&log, Call::MediaState));
        b.handle_event(a, PageEvent::MediaState(false), t);
        assert_eq!(log.borrow().live, 1);
        assert!(!b.tabs().get(a).unwrap().awake);
    }

    #[test]
    fn media_state_true_cancels_pending_sleep() {
        let (mut b, log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        let t = t0 + secs(301);
        b.tick(t, Pressure::Normal);
        b.handle_event(a, PageEvent::MediaState(true), t);
        assert_eq!(log.borrow().live, 2);
        let tab = b.tabs().get(a).unwrap();
        assert!(tab.awake && !tab.sleep_pending);
    }

    #[test]
    fn waking_restores_scroll_after_load_finished() {
        let (mut b, log, t0) = setup(None);
        let (a, _) = two_tabs(&mut b, t0);
        b.handle_event(a, PageEvent::Number { tag: JsTag::ScrollY, value: Some(640.0) }, t0);
        let t = t0 + secs(301);
        b.tick(t, Pressure::Normal);
        b.handle_event(a, PageEvent::MediaState(false), t);
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
        b.handle_event(yt, PageEvent::MediaState(false), t);
        assert_eq!(b.tabs().get(yt).unwrap().url.as_ref().unwrap().as_str(), "https://www.youtube.com/watch?v=abc&t=125s");
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
        b.handle_event(a, PageEvent::MediaState(false), t0);
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
}
