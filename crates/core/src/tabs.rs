//! Daftar tab, urutan, tab aktif, dan stack tab yang baru ditutup.

use crate::sleep::TabSnapshot;
use std::time::Instant;
pub use url::Url;

const MAX_CLOSED: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

#[derive(Debug, Clone)]
pub struct Tab {
    pub id: TabId,
    pub url: Option<Url>,
    pub title: String,
    pub scroll_y: f64,
    pub video_time: Option<f64>,
    pub awake: bool,
    pub last_viewed: Instant,
    pub media_active: bool,
    pub sleep_pending: bool,
    pub restore_scroll: bool,
    pub loading: bool,
    pub can_back: bool,
    pub can_forward: bool,
    pub last_crash: Option<Instant>,
}

impl Tab {
    fn new(id: TabId, url: Option<Url>, now: Instant) -> Self {
        Self {
            id,
            url,
            title: String::new(),
            scroll_y: 0.0,
            video_time: None,
            awake: false,
            last_viewed: now,
            media_active: false,
            sleep_pending: false,
            restore_scroll: false,
            loading: false,
            can_back: false,
            can_forward: false,
            last_crash: None,
        }
    }
}

#[derive(Debug, Clone)]
struct ClosedTab {
    index: usize,
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

    /// Membuka tab baru tepat setelah tab aktif.
    pub fn open(&mut self, url: Option<Url>, now: Instant, activate: bool) -> TabId {
        let id = self.alloc_id();
        let at = self.index_of(self.active).map_or(self.tabs.len(), |i| i + 1);
        self.tabs.insert(at, Tab::new(id, url, now));
        if activate {
            self.activate(id, now);
        }
        id
    }

    /// Menutup tab. Kalau tab aktif yang ditutup, tetangga kanan (atau kiri)
    /// menjadi aktif. Kalau tab terakhir yang ditutup, satu tab kosong dibuat.
    pub fn close(&mut self, id: TabId, now: Instant) -> Option<Tab> {
        let index = self.index_of(id)?;
        let tab = self.tabs.remove(index);
        if tab.url.is_some() {
            self.closed.push(ClosedTab { index, url: tab.url.clone(), title: tab.title.clone(), scroll_y: tab.scroll_y });
            if self.closed.len() > MAX_CLOSED {
                self.closed.remove(0);
            }
        }
        if self.tabs.is_empty() {
            let blank = self.alloc_id();
            self.tabs.push(Tab::new(blank, None, now));
            self.active = blank;
        } else if self.active == id {
            let next = self.tabs[index.min(self.tabs.len() - 1)].id;
            self.active = next;
        }
        Some(tab)
    }

    /// Membuka lagi tab yang terakhir ditutup, dalam keadaan tidur, di posisi semula.
    pub fn reopen_closed(&mut self, now: Instant) -> Option<TabId> {
        let closed = self.closed.pop()?;
        let id = self.alloc_id();
        let mut tab = Tab::new(id, closed.url, now);
        tab.title = closed.title;
        tab.scroll_y = closed.scroll_y;
        tab.restore_scroll = closed.scroll_y > 0.0;
        self.tabs.insert(closed.index.min(self.tabs.len()), tab);
        Some(id)
    }

    /// Mengaktifkan tab. Tab yang sebelumnya aktif mencatat waktu terakhir dilihat.
    pub fn activate(&mut self, id: TabId, now: Instant) {
        if self.index_of(id).is_none() || self.active == id {
            return;
        }
        if let Some(prev) = self.get_mut(self.active) {
            prev.last_viewed = now;
        }
        self.active = id;
        if let Some(tab) = self.get_mut(id) {
            tab.last_viewed = now;
            tab.sleep_pending = false;
        }
    }

    /// ⌘1 sampai ⌘8 memilih tab ke-n; index 8 (⌘9) selalu memilih tab terakhir.
    pub fn activate_index(&mut self, index: usize, now: Instant) {
        let i = if index >= 8 { self.tabs.len() - 1 } else { index };
        if let Some(tab) = self.tabs.get(i) {
            let id = tab.id;
            self.activate(id, now);
        }
    }

    pub fn activate_next(&mut self, now: Instant) {
        let i = self.index_of(self.active).unwrap_or(0);
        let id = self.tabs[(i + 1) % self.tabs.len()].id;
        self.activate(id, now);
    }

    pub fn activate_previous(&mut self, now: Instant) {
        let i = self.index_of(self.active).unwrap_or(0);
        let id = self.tabs[(i + self.tabs.len() - 1) % self.tabs.len()].id;
        self.activate(id, now);
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

    pub fn snapshots(&self) -> Vec<TabSnapshot> {
        self.tabs
            .iter()
            .map(|t| TabSnapshot {
                id: t.id,
                awake: t.awake,
                active: t.id == self.active,
                last_viewed: t.last_viewed,
                media_active: t.media_active,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn u(s: &str) -> Url { Url::parse(s).unwrap() }
    fn urls(list: &TabList) -> Vec<Option<String>> {
        list.iter().map(|t| t.url.as_ref().map(|u| u.to_string())).collect()
    }

    #[test]
    fn open_inserts_after_active_and_activates() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        let a = l.active();
        l.get_mut(a).unwrap().url = Some(u("https://a.com/"));
        let b = l.open(Some(u("https://b.com/")), t0, false);
        assert_eq!(l.active(), a);
        let n = l.open(Some(u("https://n.com/")), t0, true);
        assert_eq!(urls(&l), vec![Some("https://a.com/".into()), Some("https://n.com/".into()), Some("https://b.com/".into())]);
        assert_eq!(l.active(), n);
        assert_ne!(n, b);
    }

    #[test]
    fn closing_active_activates_right_neighbor_else_left() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        let a = l.active();
        let b = l.open(Some(u("https://b.com/")), t0, true);
        let c = l.open(Some(u("https://c.com/")), t0, false);
        l.close(b, t0);
        assert_eq!(l.active(), c);
        l.close(c, t0);
        assert_eq!(l.active(), a);
    }

    #[test]
    fn closing_last_tab_leaves_one_blank_tab() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        let a = l.active();
        l.get_mut(a).unwrap().url = Some(u("https://a.com/"));
        let closed = l.close(a, t0);
        assert!(closed.is_some());
        assert_eq!(l.iter().count(), 1);
        let only = l.iter().next().unwrap();
        assert!(only.url.is_none());
        assert_eq!(l.active(), only.id);
    }

    #[test]
    fn reopen_closed_restores_url_title_scroll_asleep() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        let b = l.open(Some(u("https://b.com/")), t0, false);
        let _c = l.open(Some(u("https://c.com/")), t0, false);
        {
            let tb = l.get_mut(b).unwrap();
            tb.title = "B".into();
            tb.scroll_y = 640.0;
            tb.awake = true;
        }
        l.close(b, t0);
        let r = l.reopen_closed(t0).unwrap();
        let t = l.get(r).unwrap();
        assert_eq!(t.url.as_ref().unwrap().as_str(), "https://b.com/");
        assert_eq!(t.title, "B");
        assert_eq!(t.scroll_y, 640.0);
        assert!(!t.awake);
        assert_eq!(l.iter().position(|t| t.id == r), Some(2));
        assert!(l.reopen_closed(t0).is_none());
    }

    #[test]
    fn closed_stack_keeps_at_most_20() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        for i in 0..25 {
            let id = l.open(Some(u(&format!("https://t{i}.com/"))), t0, false);
            l.close(id, t0);
        }
        let mut n = 0;
        while l.reopen_closed(t0).is_some() { n += 1; }
        assert_eq!(n, 20);
    }

    #[test]
    fn activate_index_8_selects_last() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        l.open(None, t0, false);
        let last = l.open(None, t0, false);
        let first = l.iter().next().unwrap().id;
        l.activate(first, t0);
        l.activate_index(8, t0);
        assert_eq!(l.active(), l.iter().last().unwrap().id);
        let _ = last;
    }

    #[test]
    fn activate_next_and_previous_wrap() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        let first = l.active();
        let _b = l.open(None, t0, true);
        let c = l.open(None, t0, true);
        assert_eq!(l.active(), c);
        l.activate_next(t0);
        assert_eq!(l.active(), first);
        l.activate_previous(t0);
        assert_eq!(l.active(), c);
    }

    #[test]
    fn deactivated_tab_gets_last_viewed_now() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(42);
        let mut l = TabList::new(t0);
        let a = l.active();
        let b = l.open(None, t0, false);
        l.activate(b, t1);
        assert_eq!(l.get(a).unwrap().last_viewed, t1);
    }
}
