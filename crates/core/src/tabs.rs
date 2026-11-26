//! Daftar tab, urutan, tab aktif, dan stack tab yang baru ditutup.
//!
//! Urutan di dalam daftar adalah urutan sidebar, dan bagiannya selalu
//! bersambung: semua Favorite, lalu Pinned, lalu Today.

use crate::engine::Emulation;
use crate::sleep::TabSnapshot;
use serde::{Deserialize, Serialize};
use std::time::Instant;
pub use url::Url;

const MAX_CLOSED: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

/// Bagian sidebar tempat tab berada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabKind {
    /// Ikon di grid favorit. Slot tetap: ditutup berarti dilepas dari memory.
    Favorite,
    /// Tab yang disematkan. Slot tetap seperti favorit, tetapi tampil sebagai baris.
    Pinned,
    /// Tab sehari-hari.
    #[default]
    Today,
}

impl TabKind {
    fn rank(self) -> u8 {
        match self {
            TabKind::Favorite => 0,
            TabKind::Pinned => 1,
            TabKind::Today => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tab {
    pub id: TabId,
    pub url: Option<Url>,
    /// URL halaman terakhir yang selesai dimuat (bukan URL sementara navigasi).
    pub committed_url: Option<Url>,
    pub title: String,
    pub scroll_y: f64,
    pub video_time: Option<f64>,
    pub awake: bool,
    pub last_viewed: Instant,
    pub media_active: bool,
    /// Ada teks yang sudah diketik pengguna tapi belum dikirim (dari balasan
    /// `Activity` terakhir).
    pub draft: bool,
    pub sleep_pending: bool,
    /// Tidur diminta karena memory kritis: teks yang belum dikirim tidak
    /// lagi menahan tab.
    pub sleep_forced: bool,
    /// Balasan `Activity` yang aman untuk tidur sudah datang selama `sleep_pending`.
    pub activity_clear: bool,
    /// Posisi video YouTube sudah diminta dan balasannya belum datang.
    pub awaiting_video_time: bool,
    pub restore_scroll: bool,
    pub loading: bool,
    pub can_back: bool,
    pub can_forward: bool,
    pub last_crash: Option<Instant>,
    /// Jumlah halaman yang selesai dimuat di tab ini (dipakai untuk menunggu
    /// navigasi selesai).
    pub loads: u64,
    /// Emulasi perangkat dari Claude. Tidak disimpan ke sesi.
    pub emulation: Emulation,
    /// Kunci riwayat (`history::visit_key`) kunjungan terakhir yang dicatat
    /// di tab ini. Muat ulang ke URL yang sama tidak dihitung lagi.
    pub visit_key: Option<String>,
    /// Navigasi yang diminta (address bar, saran, MCP) dan belum selesai dimuat.
    pub pending_visit: Option<PendingVisit>,
    pub kind: TabKind,
    /// Alamat saat tab disematkan atau dijadikan favorit. Hanya ada untuk
    /// Favorite dan Pinned; menutup slot mengembalikan tab ke alamat ini.
    pub saved_url: Option<Url>,
    pub saved_title: String,
}

/// Navigasi yang diminta dan akan dicatat ke riwayat saat selesai dimuat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingVisit {
    /// Alamat diketik pengguna (bukan pencarian, bukan link).
    pub typed: bool,
}

impl Tab {
    fn new(id: TabId, url: Option<Url>, now: Instant) -> Self {
        Self {
            id,
            committed_url: None,
            url,
            title: String::new(),
            scroll_y: 0.0,
            video_time: None,
            awake: false,
            last_viewed: now,
            media_active: false,
            draft: false,
            sleep_pending: false,
            sleep_forced: false,
            activity_clear: false,
            awaiting_video_time: false,
            restore_scroll: false,
            loading: false,
            can_back: false,
            can_forward: false,
            last_crash: None,
            loads: 0,
            emulation: Emulation::default(),
            visit_key: None,
            pending_visit: None,
            kind: TabKind::Today,
            saved_url: None,
            saved_title: String::new(),
        }
    }

    /// Tab Today tanpa alamat (baru dibuka, belum diisi). Tidak tampil di sidebar.
    pub fn is_blank(&self) -> bool {
        self.kind == TabKind::Today && self.url.is_none()
    }

    /// Judul untuk sidebar dan judul window: judul halaman, host, atau "New Tab".
    pub fn display_title(&self) -> String {
        if !self.title.is_empty() {
            return self.title.clone();
        }
        match &self.url {
            Some(url) => url.host_str().unwrap_or(url.as_str()).to_string(),
            None => "New Tab".to_string(),
        }
    }
}

/// Hasil menutup tab.
#[derive(Debug)]
pub enum Closed {
    /// Tab Today dibuang dari daftar.
    Removed(Box<Tab>),
    /// Slot Favorite/Pinned tetap di daftar, dilepas dan kembali ke alamat simpanannya.
    Unloaded,
}

#[derive(Debug, Clone)]
struct ClosedTab {
    /// Posisi di dalam bagian Today saat ditutup.
    today_index: usize,
    url: Option<Url>,
    title: String,
    scroll_y: f64,
}

/// Daftar tab. Selalu berisi minimal satu tab, dan selalu ada satu tab aktif.
#[derive(Debug, Clone)]
pub struct TabList {
    tabs: Vec<Tab>,
    active: TabId,
    closed: Vec<ClosedTab>,
    next_id: u64,
}

impl TabList {
    /// Satu tab kosong yang aktif.
    pub fn new(now: Instant) -> Self {
        let mut list = Self { tabs: Vec::new(), active: TabId(0), closed: Vec::new(), next_id: 0 };
        let id = list.alloc_id();
        list.tabs.push(Tab::new(id, None, now));
        list.active = id;
        list
    }

    fn alloc_id(&mut self) -> TabId {
        self.next_id += 1;
        TabId(self.next_id)
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// Awal dan akhir (eksklusif) satu bagian di dalam daftar.
    fn section_bounds(&self, kind: TabKind) -> (usize, usize) {
        let start = self.tabs.iter().filter(|t| t.kind.rank() < kind.rank()).count();
        let len = self.tabs.iter().filter(|t| t.kind == kind).count();
        (start, start + len)
    }

    fn visible_ids(&self) -> Vec<TabId> {
        self.visible().map(|t| t.id).collect()
    }

    /// Membuka tab baru di paling atas bagian Today. `None` saat tab aktif
    /// sudah kosong memakai tab itu lagi, supaya tab kosong tidak menumpuk.
    pub fn open(&mut self, url: Option<Url>, now: Instant, activate: bool) -> TabId {
        if url.is_none() && self.get(self.active).is_some_and(Tab::is_blank) {
            return self.active;
        }
        let id = self.alloc_id();
        let (at, _) = self.section_bounds(TabKind::Today);
        self.tabs.insert(at, Tab::new(id, url, now));
        if activate {
            self.activate(id, now);
        }
        id
    }

    /// Menutup tab. Tab Today dibuang (dan bisa dibuka lagi); slot Favorite
    /// atau Pinned tetap di tempatnya, kembali ke alamat simpanannya. Kalau
    /// tab aktif yang ditutup, tab terlihat di bawahnya (atau di atasnya)
    /// menjadi aktif; kalau tidak ada lagi, satu tab kosong dibuat.
    pub fn close(&mut self, id: TabId, now: Instant) -> Option<Closed> {
        let index = self.index_of(id)?;
        if self.tabs[index].kind != TabKind::Today {
            let tab = &mut self.tabs[index];
            tab.url = tab.saved_url.clone();
            tab.title = tab.saved_title.clone();
            tab.scroll_y = 0.0;
            tab.restore_scroll = false;
            tab.video_time = None;
            tab.committed_url = None;
            tab.can_back = false;
            tab.can_forward = false;
            tab.awake = false;
            tab.loading = false;
            if self.active == id {
                let below = self.tabs[index + 1..].iter().find(|t| !t.is_blank()).map(|t| t.id);
                let above = self.tabs[..index].iter().rev().find(|t| !t.is_blank()).map(|t| t.id);
                match below.or(above) {
                    Some(next) => self.active = next,
                    None => self.push_blank(now),
                }
            }
            return Some(Closed::Unloaded);
        }
        let (today_start, _) = self.section_bounds(TabKind::Today);
        let tab = self.tabs.remove(index);
        if tab.url.is_some() {
            self.closed.push(ClosedTab {
                today_index: index - today_start,
                url: tab.url.clone(),
                title: tab.title.clone(),
                scroll_y: tab.scroll_y,
            });
            if self.closed.len() > MAX_CLOSED {
                self.closed.remove(0);
            }
        }
        if self.tabs.is_empty() {
            self.push_blank(now);
        } else if self.active == id {
            self.active = self.tabs[index.min(self.tabs.len() - 1)].id;
        }
        Some(Closed::Removed(Box::new(tab)))
    }

    fn push_blank(&mut self, now: Instant) {
        let blank = self.alloc_id();
        let (at, _) = self.section_bounds(TabKind::Today);
        self.tabs.insert(at, Tab::new(blank, None, now));
        self.active = blank;
    }

    /// Membuka lagi tab Today yang terakhir ditutup, dalam keadaan tidur, di
    /// posisinya semula di bagian Today.
    pub fn reopen_closed(&mut self, now: Instant) -> Option<TabId> {
        let closed = self.closed.pop()?;
        let id = self.alloc_id();
        let mut tab = Tab::new(id, closed.url, now);
        tab.title = closed.title;
        tab.scroll_y = closed.scroll_y;
        tab.restore_scroll = closed.scroll_y > 0.0;
        let (start, end) = self.section_bounds(TabKind::Today);
        self.tabs.insert(start + closed.today_index.min(end - start), tab);
        Some(id)
    }

    /// Memindahkan tab ke bagian `kind` di posisi `at` (dibatasi panjang
    /// bagian). Masuk ke Favorite/Pinned dari Today menyimpan alamat dan
    /// judulnya; masuk ke Today menghapus simpanan itu. Tab kosong ditolak.
    pub fn place(&mut self, id: TabId, kind: TabKind, at: usize) -> bool {
        let Some(index) = self.index_of(id) else { return false };
        if self.tabs[index].is_blank() {
            return false;
        }
        let mut tab = self.tabs.remove(index);
        match (tab.kind, kind) {
            (_, TabKind::Today) => {
                tab.saved_url = None;
                tab.saved_title.clear();
            }
            (TabKind::Today, _) => {
                tab.saved_url = tab.url.clone();
                tab.saved_title = tab.title.clone();
            }
            _ => {}
        }
        tab.kind = kind;
        let (start, end) = self.section_bounds(kind);
        self.tabs.insert(start + at.min(end - start), tab);
        true
    }

    /// "Set Current Page as Pinned URL": alamat simpanan slot menjadi alamat sekarang.
    pub fn set_saved_to_current(&mut self, id: TabId) -> bool {
        let Some(tab) = self.get_mut(id) else { return false };
        if tab.kind == TabKind::Today || tab.url.is_none() {
            return false;
        }
        tab.saved_url = tab.url.clone();
        tab.saved_title = tab.title.clone();
        true
    }

    /// Mengaktifkan tab. Tab yang sebelumnya aktif mencatat waktu terakhir
    /// dilihat; kalau tab itu kosong, ia dibuang.
    pub fn activate(&mut self, id: TabId, now: Instant) {
        if self.index_of(id).is_none() || self.active == id {
            return;
        }
        let prev = self.active;
        if let Some(tab) = self.get_mut(prev) {
            tab.last_viewed = now;
        }
        self.active = id;
        if let Some(tab) = self.get_mut(id) {
            tab.last_viewed = now;
            tab.sleep_pending = false;
            tab.sleep_forced = false;
            tab.activity_clear = false;
        }
        if self.get(prev).is_some_and(Tab::is_blank) {
            self.tabs.retain(|t| t.id != prev);
        }
    }

    /// ⌘1 sampai ⌘8 memilih tab terlihat ke-n; index 8 (⌘9) selalu memilih
    /// tab terlihat terakhir.
    pub fn activate_index(&mut self, index: usize, now: Instant) {
        let ids = self.visible_ids();
        if ids.is_empty() {
            return;
        }
        let i = if index >= 8 { ids.len() - 1 } else { index };
        if let Some(&id) = ids.get(i) {
            self.activate(id, now);
        }
    }

    pub fn activate_next(&mut self, now: Instant) {
        let ids = self.visible_ids();
        if ids.is_empty() {
            return;
        }
        let next = match ids.iter().position(|&id| id == self.active) {
            Some(p) => ids[(p + 1) % ids.len()],
            None => ids[0],
        };
        self.activate(next, now);
    }

    pub fn activate_previous(&mut self, now: Instant) {
        let ids = self.visible_ids();
        if ids.is_empty() {
            return;
        }
        let prev = match ids.iter().position(|&id| id == self.active) {
            Some(p) => ids[(p + ids.len() - 1) % ids.len()],
            None => ids[ids.len() - 1],
        };
        self.activate(prev, now);
    }

    pub fn active(&self) -> TabId {
        self.active
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter()
    }

    /// Tab yang tampil di sidebar, dalam urutan sidebar (tanpa tab kosong).
    pub fn visible(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter().filter(|t| !t.is_blank())
    }

    /// Tab terlihat di satu bagian.
    pub fn section(&self, kind: TabKind) -> impl Iterator<Item = &Tab> {
        self.visible().filter(move |t| t.kind == kind)
    }

    pub fn snapshots(&self) -> Vec<TabSnapshot> {
        self.tabs
            .iter()
            .map(|t| TabSnapshot {
                id: t.id,
                awake: t.awake,
                active: t.id == self.active,
                last_viewed: t.last_viewed,
                media_active: t.media_active,
                draft: t.draft,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn urls(list: &TabList) -> Vec<Option<String>> {
        list.iter().map(|t| t.url.as_ref().map(|u| u.to_string())).collect()
    }

    fn visible_urls(list: &TabList) -> Vec<String> {
        list.visible().map(|t| t.url.as_ref().unwrap().to_string()).collect()
    }

    /// Daftar dengan tab Today berurutan seperti `names` (tab kosong awal sudah hilang).
    fn today_list(names: &[&str], t0: Instant) -> (TabList, Vec<TabId>) {
        let mut l = TabList::new(t0);
        let mut ids = Vec::new();
        for name in names.iter().rev() {
            ids.push(l.open(Some(u(&format!("https://{name}.com/"))), t0, true));
        }
        ids.reverse();
        (l, ids)
    }

    #[test]
    fn open_puts_new_tab_at_top_of_today() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["p", "a"], t0);
        assert!(l.place(ids[0], TabKind::Pinned, 0));
        let b = l.open(Some(u("https://b.com/")), t0, false);
        assert_eq!(visible_urls(&l), vec!["https://p.com/", "https://b.com/", "https://a.com/"]);
        assert_eq!(l.get(b).unwrap().kind, TabKind::Today);
    }

    #[test]
    fn open_none_reuses_active_blank_tab() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        let first = l.open(None, t0, true);
        let second = l.open(None, t0, true);
        assert_eq!(first, second);
        assert_eq!(l.iter().count(), 1);
        assert!(l.get(first).unwrap().is_blank());
    }

    #[test]
    fn switching_away_from_blank_tab_removes_it() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["a"], t0);
        let blank = l.open(None, t0, true);
        assert_eq!(l.active(), blank);
        l.activate(ids[0], t0);
        assert!(l.get(blank).is_none());
        assert_eq!(l.iter().count(), 1);
        assert!(l.reopen_closed(t0).is_none());
    }

    #[test]
    fn place_into_pinned_saves_url_and_title() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["a", "b"], t0);
        l.get_mut(ids[1]).unwrap().title = "B".into();
        assert!(l.place(ids[1], TabKind::Pinned, 0));
        let t = l.get(ids[1]).unwrap();
        assert_eq!(t.kind, TabKind::Pinned);
        assert_eq!(t.saved_url.as_ref().unwrap().as_str(), "https://b.com/");
        assert_eq!(t.saved_title, "B");
        assert_eq!(l.iter().position(|t| t.id == ids[1]), Some(0));
    }

    #[test]
    fn place_back_to_today_clears_saved_and_uses_index() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["p", "a", "b"], t0);
        l.place(ids[0], TabKind::Pinned, 0);
        assert!(l.place(ids[0], TabKind::Today, 1));
        assert_eq!(visible_urls(&l), vec!["https://a.com/", "https://p.com/", "https://b.com/"]);
        let p = l.get(ids[0]).unwrap();
        assert_eq!(p.kind, TabKind::Today);
        assert!(p.saved_url.is_none());
        assert!(p.saved_title.is_empty());
    }

    #[test]
    fn place_blank_tab_is_refused() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        let blank = l.active();
        assert!(!l.place(blank, TabKind::Pinned, 0));
        assert_eq!(l.get(blank).unwrap().kind, TabKind::Today);
    }

    #[test]
    fn sections_stay_contiguous_after_moves() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["a", "b", "c", "d", "e", "f"], t0);
        let moves = [
            (ids[3], TabKind::Favorite, 0),
            (ids[1], TabKind::Pinned, 5),
            (ids[5], TabKind::Favorite, 9),
            (ids[0], TabKind::Pinned, 0),
            (ids[3], TabKind::Today, 2),
            (ids[2], TabKind::Favorite, 1),
            (ids[1], TabKind::Favorite, 0),
            (ids[5], TabKind::Pinned, 1),
        ];
        let rank = |k: TabKind| match k {
            TabKind::Favorite => 0,
            TabKind::Pinned => 1,
            TabKind::Today => 2,
        };
        for (id, kind, at) in moves {
            assert!(l.place(id, kind, at));
            let kinds: Vec<u8> = l.iter().map(|t| rank(t.kind)).collect();
            let mut sorted = kinds.clone();
            sorted.sort();
            assert_eq!(kinds, sorted, "setelah {id:?} -> {kind:?}");
        }
        assert_eq!(l.iter().count(), 6);
    }

    #[test]
    fn closing_pinned_tab_unloads_and_resets_to_saved_url() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["p", "a"], t0);
        l.get_mut(ids[0]).unwrap().title = "P".into();
        l.place(ids[0], TabKind::Pinned, 0);
        l.activate(ids[0], t0);
        {
            let p = l.get_mut(ids[0]).unwrap();
            p.url = Some(u("https://p.com/elsewhere"));
            p.title = "Elsewhere".into();
            p.scroll_y = 300.0;
            p.awake = true;
            p.can_back = true;
        }
        assert!(matches!(l.close(ids[0], t0), Some(Closed::Unloaded)));
        let p = l.get(ids[0]).unwrap();
        assert_eq!(p.url.as_ref().unwrap().as_str(), "https://p.com/");
        assert_eq!(p.title, "P");
        assert_eq!(p.scroll_y, 0.0);
        assert!(!p.awake);
        assert!(!p.can_back);
        assert_eq!(p.kind, TabKind::Pinned);
        assert_eq!(l.active(), ids[1]);
        assert!(l.reopen_closed(t0).is_none());
    }

    #[test]
    fn closing_only_slot_leaves_blank_today_tab_active() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["f"], t0);
        l.place(ids[0], TabKind::Favorite, 0);
        assert!(matches!(l.close(ids[0], t0), Some(Closed::Unloaded)));
        let active = l.get(l.active()).unwrap();
        assert!(active.is_blank());
        assert!(l.get(ids[0]).is_some());
    }

    #[test]
    fn closing_active_activates_below_else_above() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["a", "b", "c"], t0);
        l.activate(ids[1], t0);
        assert!(matches!(l.close(ids[1], t0), Some(Closed::Removed(_))));
        assert_eq!(l.active(), ids[2]);
        l.close(ids[2], t0);
        assert_eq!(l.active(), ids[0]);
    }

    #[test]
    fn closing_last_tab_leaves_one_blank_tab() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["a"], t0);
        assert!(l.close(ids[0], t0).is_some());
        assert_eq!(l.iter().count(), 1);
        let only = l.iter().next().unwrap();
        assert!(only.is_blank());
        assert_eq!(l.active(), only.id);
    }

    #[test]
    fn reopen_closed_goes_back_into_today_at_its_position() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["p", "a", "b", "c"], t0);
        l.place(ids[0], TabKind::Pinned, 0);
        {
            let b = l.get_mut(ids[2]).unwrap();
            b.title = "B".into();
            b.scroll_y = 640.0;
            b.awake = true;
        }
        l.close(ids[2], t0);
        let r = l.reopen_closed(t0).unwrap();
        let t = l.get(r).unwrap();
        assert_eq!(t.url.as_ref().unwrap().as_str(), "https://b.com/");
        assert_eq!(t.title, "B");
        assert_eq!(t.scroll_y, 640.0);
        assert!(!t.awake);
        assert_eq!(t.kind, TabKind::Today);
        assert_eq!(visible_urls(&l), vec!["https://p.com/", "https://a.com/", "https://b.com/", "https://c.com/"]);
        assert!(l.reopen_closed(t0).is_none());
    }

    #[test]
    fn closed_stack_keeps_at_most_20() {
        let t0 = Instant::now();
        let (mut l, _) = today_list(&["keep"], t0);
        for i in 0..25 {
            let id = l.open(Some(u(&format!("https://t{i}.com/"))), t0, false);
            l.close(id, t0);
        }
        let mut n = 0;
        while l.reopen_closed(t0).is_some() {
            n += 1;
        }
        assert_eq!(n, 20);
    }

    #[test]
    fn activate_index_and_cycling_skip_blank_tab() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["a", "b", "c"], t0);
        l.open(None, t0, true);
        l.activate_index(0, t0);
        assert_eq!(l.active(), ids[0]);
        l.activate_index(8, t0);
        assert_eq!(l.active(), ids[2]);
        // Dari tab kosong: berikutnya = tab pertama, sebelumnya = tab terakhir.
        l.open(None, t0, true);
        l.activate_next(t0);
        assert_eq!(l.active(), ids[0]);
        l.open(None, t0, true);
        l.activate_previous(t0);
        assert_eq!(l.active(), ids[2]);
        l.activate_next(t0);
        assert_eq!(l.active(), ids[0]);
        l.activate_previous(t0);
        assert_eq!(l.active(), ids[2]);
        assert_eq!(l.iter().count(), 3);
    }

    #[test]
    fn display_title_falls_back_to_host_then_new_tab() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["a"], t0);
        assert_eq!(l.get(ids[0]).unwrap().display_title(), "a.com");
        l.get_mut(ids[0]).unwrap().title = "Judul".into();
        assert_eq!(l.get(ids[0]).unwrap().display_title(), "Judul");
        let blank = l.open(None, t0, true);
        assert_eq!(l.get(blank).unwrap().display_title(), "New Tab");
    }

    #[test]
    fn set_saved_to_current_updates_slot() {
        let t0 = Instant::now();
        let (mut l, ids) = today_list(&["p", "a"], t0);
        l.place(ids[0], TabKind::Pinned, 0);
        {
            let p = l.get_mut(ids[0]).unwrap();
            p.url = Some(u("https://p.com/b"));
            p.title = "Page B".into();
        }
        assert!(l.set_saved_to_current(ids[0]));
        let p = l.get(ids[0]).unwrap();
        assert!(p.saved_url.as_ref().unwrap().as_str().ends_with("/b"));
        assert_eq!(p.saved_title, "Page B");
        assert!(!l.set_saved_to_current(ids[1]));
    }

    #[test]
    fn deactivated_tab_gets_last_viewed_now() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(42);
        let (mut l, ids) = today_list(&["a", "b"], t0);
        l.activate(ids[0], t0);
        l.activate(ids[1], t1);
        assert_eq!(l.get(ids[0]).unwrap().last_viewed, t1);
    }

    #[test]
    fn new_list_has_one_blank_active_tab() {
        let t0 = Instant::now();
        let l = TabList::new(t0);
        assert_eq!(urls(&l), vec![None]);
        assert_eq!(l.visible().count(), 0);
    }
}
