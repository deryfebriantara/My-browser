//! Riwayat dan saran address bar milik `Browser`: akses riwayat untuk UI,
//! MCP, dan jendela Riwayat, serta membuka saran.

use super::{Browser, UiChange};
use crate::engine::Engine;
use crate::history::{History, HistoryEntry};
use crate::suggest::{Suggestion, SuggestionKind, Suggestions, TabCandidate, suggest};
use crate::tabs::TabId;
use std::time::Instant;

impl<E: Engine> Browser<E> {
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
    pub fn search_history(
        &self,
        query: &str,
        since: Option<u64>,
        offset: usize,
        limit: usize,
    ) -> (Vec<&HistoryEntry>, usize) {
        self.history.search(query, since, offset, limit)
    }

    /// Detik unix sekarang menurut jam riwayat.
    pub fn unix_now(&self) -> u64 {
        (self.clock)()
    }

    /// Saran address bar untuk `input` dari riwayat dan tab lain yang terbuka.
    pub fn suggest(&self, input: &str, allow_inline: bool) -> Suggestions {
        let active = self.tabs.active();
        let tabs: Vec<TabCandidate> = self
            .tabs
            .iter()
            .filter(|t| t.id != active)
            .filter_map(|t| t.url.as_ref().map(|url| TabCandidate { id: t.id, title: &t.title, url }))
            .collect();
        suggest(input, &self.history, &tabs, &self.settings.search_template, allow_inline, (self.clock)())
    }

    /// Membuka saran address bar. "Pindah ke tab" menutup tab asal kalau tab
    /// itu kosong, seperti Safari.
    pub fn open_suggestion(&mut self, suggestion: &Suggestion, now: Instant) -> bool {
        let active = self.tabs.active();
        match suggestion.kind {
            SuggestionKind::SwitchTab(id) => {
                if id == active || self.tabs.get(id).is_none() {
                    return false;
                }
                let blank = self.tabs.get(active).is_some_and(|t| t.url.is_none());
                self.activate(id, now);
                if blank {
                    self.close_tab(active, now);
                }
                true
            }
            SuggestionKind::Search => self.navigate_url(active, suggestion.url.clone(), false, now),
            SuggestionKind::Url | SuggestionKind::History => {
                self.navigate_url(active, suggestion.url.clone(), true, now)
            }
        }
    }
}
