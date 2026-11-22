//! Jendela Riwayat (⌘Y): daftar halaman per hari, cari, buka, hapus satu
//! entri, dan Hapus Riwayat. Tampilannya HTML lokal (PanelView); proses
//! WebContent-nya hanya ada selama jendela terbuka.

use crate::dialogs;
use crate::local_time;
use crate::persistence;
use crate::state::AppState;
use block2::RcBlock;
use browser_core::history::{HistoryEntry, display_url, start_of_local_day};
use engine_webkit::PanelView;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::MainThreadOnly;
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSRunLoop, NSSize, NSString};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;
use url::Url;

const HTML: &str = include_str!("history_window.html");
/// Jumlah entri per halaman yang dikirim ke tampilan.
const PAGE: usize = 200;

#[derive(Default)]
pub struct HistoryWindow {
    window: RefCell<Option<Retained<NSWindow>>>,
    panel: RefCell<Option<PanelView>>,
    /// Pencarian yang sedang tampil, untuk dikirim ulang saat riwayat berubah.
    query: RefCell<String>,
}

/// Entri untuk tampilan. Judul berasal dari situs: tampilan hanya boleh
/// menyisipkannya sebagai teks.
fn entries_json(entries: &[&HistoryEntry]) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|e| {
                json!({
                    "url": e.url,
                    "title": e.title,
                    "display": display_url(&e.url),
                    "last": e.last_visit,
                    "visits": e.visit_count,
                })
            })
            .collect(),
    )
}

impl HistoryWindow {
    pub fn is_open(&self) -> bool {
        self.window.borrow().is_some()
    }

    pub fn open(&self, state: &Rc<AppState>) {
        if let Some(window) = self.window.borrow().as_ref() {
            window.makeKeyAndOrderFront(None);
            return;
        }
        let mtm = state.mtm();
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(760.0, 560.0)),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str("Riwayat"));
        window.setContentMinSize(NSSize::new(520.0, 360.0));
        window.setDelegate(Some(ProtocolObject::from_ref(&*state.ui.target)));
        let weak = Rc::downgrade(state);
        let panel = PanelView::new(
            mtm,
            HTML,
            Box::new(move |text| {
                if let Some(state) = weak.upgrade() {
                    state.history_window.handle(&state, &text);
                }
            }),
        );
        window.setContentView(Some(&panel.view()));
        window.center();
        window.makeKeyAndOrderFront(None);
        *self.panel.borrow_mut() = Some(panel);
        *self.window.borrow_mut() = Some(window);
        self.query.borrow_mut().clear();
    }

    /// Jendela akan ditutup: panel dan jendela dilepas di putaran run loop
    /// berikutnya (bukan di tengah proses menutup jendela itu sendiri).
    pub fn window_will_close(&self, state: &Rc<AppState>, window: *const AnyObject) {
        let ours = self.window.borrow().as_ref().is_some_and(|w| std::ptr::eq(Retained::as_ptr(w).cast(), window));
        if !ours {
            return;
        }
        let weak = Rc::downgrade(state);
        let block = RcBlock::new(move || {
            if let Some(state) = weak.upgrade() {
                drop(state.history_window.panel.borrow_mut().take());
                drop(state.history_window.window.borrow_mut().take());
            }
        });
        // SAFETY: dari main thread ke run loop main thread.
        unsafe { NSRunLoop::mainRunLoop().performBlock(&block) };
    }

    /// Riwayat berubah: tampilan yang terbuka diperbarui.
    pub fn changed(&self, state: &AppState) {
        if self.is_open() {
            let query = self.query.borrow().clone();
            self.send_entries(state, &query, 0);
        }
    }

    fn post(&self, message: Value) {
        if let Some(panel) = self.panel.borrow().as_ref() {
            panel.send(&message.to_string());
        }
    }

    fn send_entries(&self, state: &AppState, query: &str, offset: usize) {
        let message = {
            let browser = state.browser.borrow();
            let (entries, total) = browser.search_history(query, None, offset, PAGE);
            json!({"t": "entries", "q": query, "offset": offset, "total": total, "entries": entries_json(&entries)})
        };
        self.post(message);
    }

    pub fn handle(&self, state: &Rc<AppState>, text: &str) {
        let Ok(message) = serde_json::from_str::<Value>(text) else { return };
        match message["t"].as_str() {
            Some("ready") => self.send_entries(state, "", 0),
            Some("query") => {
                let query: String = message["q"].as_str().unwrap_or_default().chars().take(200).collect();
                let offset = message["offset"].as_u64().unwrap_or(0) as usize;
                *self.query.borrow_mut() = query.clone();
                self.send_entries(state, &query, offset);
            }
            Some("open") => {
                // Hanya alamat web dari riwayat; tampilan tidak bisa membuka skema lain.
                let Some(url) = message["url"].as_str().and_then(|u| Url::parse(u).ok()) else { return };
                if !matches!(url.scheme(), "http" | "https") {
                    return;
                }
                let new_tab = message["new_tab"].as_bool().unwrap_or(false);
                {
                    let mut browser = state.browser.borrow_mut();
                    if new_tab {
                        browser.open_tab(url.as_str(), true, Instant::now());
                    } else {
                        let active = browser.tabs().active();
                        browser.navigate_url(active, url, false, Instant::now());
                    }
                }
                state.refresh();
                state.ui.window.makeKeyAndOrderFront(None);
            }
            Some("delete") => {
                let Some(url) = message["url"].as_str() else { return };
                let deleted = state.browser.borrow_mut().delete_history(url);
                if deleted {
                    state.save_history_now();
                    state.refresh();
                    self.post(json!({"t": "toast", "text": "Dihapus dari riwayat"}));
                }
            }
            Some("clear") => clear_dialog(state),
            _ => {}
        }
    }
}

/// Dialog Hapus Riwayat (dari jendela Riwayat dan menu Riwayat).
pub fn clear_dialog(state: &Rc<AppState>) {
    let options: Vec<String> =
        ["Satu jam terakhir", "Hari ini", "Hari ini dan kemarin", "Semua riwayat"].map(String::from).to_vec();
    let info = "Halaman yang terakhir dibuka dalam rentang ini dihapus dari riwayat dan saran address bar. \
                Tab yang terbuka, cookie, login, dan cache tidak ikut dihapus.";
    let weak = Rc::downgrade(state);
    let shown = dialogs::choose(&state.dialog_window(), "Hapus Riwayat", info, &options, "Hapus Riwayat", move |choice| {
        let (Some(state), Some(choice)) = (weak.upgrade(), choice) else { return };
        let now = state.browser.borrow().unix_now();
        let today = start_of_local_day(now, local_time::offset_at(now));
        let since = match choice {
            0 => Some(now.saturating_sub(3600)),
            1 => Some(today),
            2 => {
                let yesterday = today.saturating_sub(1);
                Some(start_of_local_day(yesterday, local_time::offset_at(yesterday)))
            }
            _ => None,
        };
        let removed = state.browser.borrow_mut().clear_history_since(since);
        if since.is_none() {
            persistence::remove_history_leftovers();
        }
        state.save_history_now();
        state.refresh();
        state.ui.nav.show_notice(&format!("Riwayat dihapus ({removed} halaman)"));
    });
    if !shown {
        state.ui.nav.show_notice("Tutup dialog lain dulu, lalu coba Hapus Riwayat lagi");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use browser_core::history::History;

    #[test]
    fn entries_json_has_display_url_and_no_extra_fields() {
        let mut h = History::new();
        h.record("https://www.youtube.com/watch?v=1", "<b>Video</b>", false, 5);
        let (entries, _) = h.search("", None, 0, 10);
        let v = entries_json(&entries);
        assert_eq!(v[0]["display"], "youtube.com/watch?v=1");
        assert_eq!(v[0]["title"], "<b>Video</b>", "judul dikirim apa adanya; tampilan memakai textContent");
        assert_eq!(v[0].as_object().unwrap().len(), 5);
    }
}
