//! Izin program untuk socket kontrol (main thread). Kebijakannya ada di
//! `browser_core::mcp_access`; di sini dialog dan antrean permintaan yang
//! menunggu jawaban pengguna.

use crate::dialogs;
use crate::procinfo::Client;
use crate::state::AppState;
use browser_core::mcp_access::{Access, decide, display_name};
use objc2_app_kit::NSApplication;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::mpsc;

pub const DENIED: &str = "Pengguna menolak program ini mengontrol my-browser. Izin bisa diatur ulang lewat menu \
Claude > Revoke MCP Client Access di my-browser.";

const OTHER_DIALOG: &str = "Ada dialog lain yang terbuka di my-browser. Selesaikan dulu, lalu coba lagi.";

pub type Waiter = mpsc::Sender<Result<(), String>>;

#[derive(Default)]
pub struct McpAccess {
    /// Lokasi program yang ditolak sampai browser ditutup.
    denied: RefCell<Vec<String>>,
    /// Koneksi yang diizinkan sekali (berlaku sampai koneksinya putus).
    once: RefCell<HashSet<u64>>,
    /// Permintaan yang menunggu jawaban dialog, per lokasi program.
    pending: RefCell<HashMap<String, Vec<(u64, Waiter)>>>,
}

impl McpAccess {
    /// Memutuskan permintaan dari `client`; jawabannya dikirim lewat `waiter`
    /// (langsung, atau setelah pengguna menjawab dialog).
    pub fn request(&self, state: &Rc<AppState>, client: Client, waiter: Waiter) {
        if self.once.borrow().contains(&client.id) {
            let _ = waiter.send(Ok(()));
            return;
        }
        let always = state.browser.borrow().settings().mcp_allowed_clients.clone();
        let access = decide(&client.kind, &always, &self.denied.borrow());
        match access {
            Access::Allowed => {
                let _ = waiter.send(Ok(()));
            }
            Access::Denied => {
                let _ = waiter.send(Err(DENIED.into()));
            }
            Access::Ask => {
                let first = {
                    let mut pending = self.pending.borrow_mut();
                    let list = pending.entry(client.path.clone()).or_default();
                    list.push((client.id, waiter));
                    list.len() == 1
                };
                if first {
                    self.prompt(state, &client);
                }
            }
        }
    }

    fn prompt(&self, state: &Rc<AppState>, client: &Client) {
        // Program sedang menunggu jawaban: tampilkan browser ke depan.
        #[allow(deprecated)]
        NSApplication::sharedApplication(state.mtm()).activateIgnoringOtherApps(true);
        let name = display_name(&client.path);
        let title = format!("Allow {name} to control my-browser?");
        let mut info = format!("Program: {}\nPID: {}", client.path, client.pid);
        if let Some(cwd) = &client.cwd {
            info.push_str(&format!("\nWorking folder: {cwd}"));
        }
        info.push_str(
            "\n\nThis program will be able to read and screenshot tabs, read your browsing history, open pages, and \
             manage tabs. Clicking, typing, and JavaScript still require Claude > Allow Claude to Interact. Deny if \
             you don't recognize this program.",
        );
        let weak = Rc::downgrade(state);
        let path = client.path.clone();
        let buttons = ["Always Allow", "Allow Once", "Deny"];
        let shown = dialogs::confirm(&state.ui.window, &title, &info, &buttons, move |choice| {
            if let Some(state) = weak.upgrade() {
                state.mcp_access.resolve(&state, &path, choice);
            }
        });
        if !shown {
            let waiters = self.pending.borrow_mut().remove(&client.path).unwrap_or_default();
            for (_, waiter) in waiters {
                let _ = waiter.send(Err(OTHER_DIALOG.into()));
            }
        }
    }

    fn resolve(&self, state: &Rc<AppState>, path: &str, choice: usize) {
        let waiters = self.pending.borrow_mut().remove(path).unwrap_or_default();
        let result = match choice {
            0 => {
                state.browser.borrow_mut().update_settings(|s| {
                    if !s.mcp_allowed_clients.iter().any(|p| p == path) {
                        s.mcp_allowed_clients.push(path.to_string());
                    }
                });
                state.refresh();
                state.ui.nav.show_notice(&format!("{} can now control the browser", display_name(path)));
                Ok(())
            }
            1 => {
                self.once.borrow_mut().extend(waiters.iter().map(|(id, _)| *id));
                Ok(())
            }
            _ => {
                self.denied.borrow_mut().push(path.to_string());
                Err(DENIED.to_string())
            }
        };
        for (_, waiter) in waiters {
            let _ = waiter.send(result.clone());
        }
    }

    /// Koneksi putus: izin sekali untuk koneksi itu berakhir.
    pub fn disconnected(&self, id: u64) {
        self.once.borrow_mut().remove(&id);
        for list in self.pending.borrow_mut().values_mut() {
            list.retain(|(waiting, _)| *waiting != id);
        }
    }

    /// Menu Revoke MCP Client Access: semua program ditanya lagi.
    pub fn revoke_all(&self, state: &AppState) {
        state.browser.borrow_mut().update_settings(|s| s.mcp_allowed_clients.clear());
        self.denied.borrow_mut().clear();
        self.once.borrow_mut().clear();
        state.refresh();
        state.ui.nav.show_notice("MCP client access revoked; programs will be asked again when they connect");
    }
}
