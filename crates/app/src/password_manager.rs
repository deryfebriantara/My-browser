//! Jendela Kelola Password: daftar akun, cari, tambah, edit, hapus, salin,
//! impor/ekspor CSV, kunci, dan ganti master password.
//!
//! Tampilannya HTML lokal (PanelView). Daftar akun dikirim tanpa password;
//! password hanya dikirim saat pengguna menekan Lihat, dan disembunyikan lagi
//! setelah 20 detik. Ekspor dan ganti master password meminta master
//! password lagi walaupun vault sedang terbuka.

use crate::background;
use crate::dialogs;
use crate::passwords::{MIN_MASTER, copy_secret, host, now, persist, vault_path};
use crate::state::AppState;
use block2::RcBlock;
use engine_webkit::PanelView;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSModalResponseOK, NSOpenPanel, NSPasteboard, NSPasteboardTypeString, NSSavePanel, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSRunLoop, NSSize, NSString};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::rc::Rc;
use vault::{KdfParams, NewEntry, Vault, VaultError, csv, generator, origin_of};
use zeroize::Zeroizing;

const HTML: &str = include_str!("password_manager.html");

/// Hasil ganti master password.
enum Change {
    Done,
    WrongCurrent,
    Failed(String),
}

#[derive(Default)]
pub struct PasswordManager {
    window: RefCell<Option<Retained<NSWindow>>>,
    panel: RefCell<Option<PanelView>>,
}

/// Daftar akun untuk tampilan, tanpa password.
fn entries_json(vault: &Vault) -> Value {
    let mut list: Vec<&vault::Entry> = vault.entries().iter().collect();
    list.sort_by(|a, b| (a.host(), &a.username).cmp(&(b.host(), &b.username)));
    Value::Array(
        list.iter()
            .map(|e| json!({"id": e.id, "origin": e.origin, "host": e.host(), "username": e.username, "note": e.note}))
            .collect(),
    )
}

/// Origin dari isian alamat; `github.com` dianggap `https://github.com`.
fn origin_from_input(input: &str) -> Option<String> {
    let input = input.trim();
    if input.contains("://") { origin_of(input) } else { origin_of(&format!("https://{input}")) }
}

/// Jumlah dengan kata benda tunggal atau jamak: `1 password`, `3 passwords`.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn copy_plain(text: &str) {
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    board.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
}

impl PasswordManager {
    pub fn open(&self, state: &Rc<AppState>) {
        if let Some(window) = self.window.borrow().as_ref() {
            window.makeKeyAndOrderFront(None);
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else { return };
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(720.0, 520.0)),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str("Passwords"));
        window.setContentMinSize(NSSize::new(560.0, 360.0));
        window.setDelegate(Some(ProtocolObject::from_ref(&*state.ui.target)));
        let weak = Rc::downgrade(state);
        let panel = PanelView::new(
            mtm,
            HTML,
            Box::new(move |text| {
                if let Some(state) = weak.upgrade() {
                    state.password_manager.handle(&state, &text);
                }
            }),
        );
        window.setContentView(Some(&panel.view()));
        window.center();
        window.makeKeyAndOrderFront(None);
        *self.panel.borrow_mut() = Some(panel);
        *self.window.borrow_mut() = Some(window);
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
                drop(state.password_manager.panel.borrow_mut().take());
                drop(state.password_manager.window.borrow_mut().take());
            }
        });
        // SAFETY: dari main thread ke run loop main thread.
        unsafe { NSRunLoop::mainRunLoop().performBlock(&block) };
    }

    fn post(&self, message: Value) {
        if let Some(panel) = self.panel.borrow().as_ref() {
            panel.send(&message.to_string());
        }
    }

    fn toast(&self, text: &str) {
        self.post(json!({"t": "toast", "text": text}));
    }

    pub fn locked(&self, _state: &AppState) {
        self.post(json!({"t": "locked"}));
    }

    pub fn unlocked(&self, state: &AppState) {
        if self.panel.borrow().is_none() {
            return;
        }
        if let Some(vault) = state.passwords.take_vault() {
            self.changed(state, &vault);
            state.passwords.put_vault(vault);
        }
    }

    pub fn changed(&self, _state: &AppState, vault: &Vault) {
        self.post(json!({"t": "entries", "entries": entries_json(vault)}));
    }

    fn handle(&self, state: &Rc<AppState>, text: &str) {
        let Ok(message) = serde_json::from_str::<Value>(text) else { return };
        let id = message["id"].as_str().unwrap_or_default().to_string();
        match message["t"].as_str() {
            Some("ready") => {
                if state.passwords.is_unlocked() {
                    self.unlocked(state)
                } else {
                    self.locked(state)
                }
            }
            Some("unlock") => {
                state.passwords.with_vault(state, "to manage passwords", |s, v| s.password_manager.changed(s, v))
            }
            Some("reveal") => state.passwords.with_vault(state, "to show the password", move |s, v| {
                if let Some(e) = v.get(&id) {
                    s.password_manager.post(json!({"t": "secret", "id": id, "password": e.password}));
                }
            }),
            Some("copy") => state.passwords.with_vault(state, "to copy the password", move |s, v| {
                if let Some(e) = v.get(&id) {
                    copy_secret(&e.password);
                    s.password_manager.toast("Password copied; clipboard clears in 30 seconds");
                }
            }),
            Some("copy_user") => state.passwords.with_vault(state, "to copy the username", move |s, v| {
                if let Some(e) = v.get(&id) {
                    copy_plain(&e.username);
                    s.password_manager.toast("Username copied");
                }
            }),
            Some("save") => self.save(state, &message),
            Some("delete") => self.delete(state, id),
            Some("generate") => {
                let password = Zeroizing::new(generator::generate(generator::DEFAULT_LENGTH, true));
                self.post(json!({"t": "generated", "password": password.as_str()}));
            }
            Some("import") => self.import(state),
            Some("export") => self.export(state),
            Some("change_master") => self.change_master(state, None),
            Some("lock") => state.passwords.lock(state, true),
            _ => {}
        }
    }

    fn save(&self, state: &Rc<AppState>, message: &Value) {
        let field = |k: &str| Zeroizing::new(message[k].as_str().unwrap_or_default().to_string());
        let (id, url, username, password, note) = (
            message["id"].as_str().map(String::from),
            field("url"),
            field("username"),
            field("password"),
            field("note"),
        );
        state.passwords.with_vault(state, "to save the password", move |s, v| {
            let manager = &s.password_manager;
            let form_error = |text: &str| manager.post(json!({"t": "form_error", "text": text}));
            let text = match &id {
                Some(id) => {
                    let password = (!password.is_empty()).then_some(password.as_str());
                    if !v.update(id, Some(&username), password, Some(&note), now()) {
                        return form_error("Entry not found.");
                    }
                    "Changes saved".to_string()
                }
                None => {
                    let Some(origin) = origin_from_input(&url) else {
                        return form_error("Invalid website address. Example: https://github.com");
                    };
                    if password.is_empty() {
                        return form_error("Password can't be empty.");
                    }
                    if v.entries().iter().any(|e| e.origin == origin && e.username == *username) {
                        return form_error("This account already exists for that site; edit the existing entry.");
                    }
                    let entry = NewEntry {
                        origin: origin.clone(),
                        username: username.to_string(),
                        password: password.to_string(),
                        note: note.to_string(),
                    };
                    v.add(entry, now());
                    format!("Password for {} saved", host(&origin))
                }
            };
            match persist(v) {
                Ok(()) => {
                    s.passwords.refresh_known(v);
                    manager.changed(s, v);
                    manager.post(json!({"t": "saved", "text": text}));
                }
                Err(e) => form_error(&e),
            }
        });
    }

    fn delete(&self, state: &Rc<AppState>, id: String) {
        state.passwords.with_vault(state, "to delete the password", move |s, v| {
            let Some(entry) = v.get(&id) else { return };
            let title = format!("Delete Password for {}?", entry.host());
            let info = format!(
                "Account: {}. This can't be undone.",
                if entry.username.is_empty() { "(no username)" } else { &entry.username }
            );
            let weak = Rc::downgrade(s);
            dialogs::confirm(&s.dialog_window(), &title, &info, &["Delete", "Cancel"], move |choice| {
                let Some(state) = weak.upgrade() else { return };
                if choice != 0 {
                    return;
                }
                state.passwords.with_vault(&state, "to delete the password", move |s, v| {
                    if v.delete(&id) {
                        match persist(v) {
                            Ok(()) => {
                                s.passwords.refresh_known(v);
                                s.password_manager.changed(s, v);
                                s.password_manager.toast("Password deleted");
                            }
                            Err(e) => s.password_manager.toast(&e),
                        }
                    }
                });
            });
        });
    }

    fn import(&self, state: &Rc<AppState>) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setMessage(Some(&NSString::from_str(
            "Choose a CSV file of exported passwords (Chrome, Safari/Passwords, Firefox, Bitwarden)",
        )));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        let Some(path) = panel.URLs().firstObject().and_then(|u| u.path()).map(|p| p.to_string()) else { return };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => Zeroizing::new(text),
            Err(e) => return self.toast(&format!("Couldn't read the file: {e}")),
        };
        let imported = match csv::import(&text) {
            Ok(imported) => imported,
            Err(e) => return self.toast(&e),
        };
        let skipped = imported.skipped;
        state.passwords.with_vault(state, "to import passwords", move |s, v| {
            let (added, updated) = v.import(imported.entries, now());
            match persist(v) {
                Ok(()) => {
                    s.passwords.refresh_known(v);
                    s.password_manager.changed(s, v);
                    let info = format!(
                        "{} added, {updated} updated, {} skipped.\n\nThat CSV file contains unencrypted passwords. You \
                         should delete it now (and empty it from the Trash too):\n{path}",
                        count(added, "password", "passwords"),
                        count(skipped, "row", "rows"),
                    );
                    dialogs::confirm(&s.dialog_window(), "Import Complete", &info, &["OK"], |_| {});
                }
                Err(e) => s.password_manager.toast(&e),
            }
        });
    }

    /// Ekspor meminta master password lagi (diverifikasi ke berkas di disk),
    /// karena hasilnya berisi semua password tanpa enkripsi.
    fn export(&self, state: &Rc<AppState>) {
        let info = "The exported CSV file contains all your passwords unencrypted. Anyone who gets the file can read \
                    all of them. Enter your master password to continue.";
        let weak = Rc::downgrade(state);
        dialogs::ask_secrets(
            &state.dialog_window(),
            "Export Passwords",
            info,
            &["Master password"],
            "Continue",
            move |values| {
                let (Some(state), Some(mut values)) = (weak.upgrade(), values) else { return };
                let master = values.remove(0);
                let file = match vault::load(&vault_path()) {
                    Ok(Some(file)) => file,
                    _ => return state.password_manager.toast("There's no password vault yet."),
                };
                background::run(
                    move || Vault::unlock(&file, &master),
                    move |result| match result {
                        Ok(verified) => state.password_manager.write_export(&state, &verified),
                        Err(VaultError::WrongPassword) => {
                            state.password_manager.toast("Incorrect master password; export canceled.")
                        }
                        Err(e) => state.password_manager.toast(&e.to_string()),
                    },
                );
            },
        );
    }

    fn write_export(&self, _state: &Rc<AppState>, vault: &Vault) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let panel = NSSavePanel::savePanel(mtm);
        panel.setNameFieldStringValue(&NSString::from_str("my-browser-passwords.csv"));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        let Some(path) = panel.URL().and_then(|u| u.path()).map(|p| p.to_string()) else { return };
        let text = Zeroizing::new(csv::export(vault.entries()));
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .and_then(|mut f| f.write_all(text.as_bytes()));
        match written {
            Ok(()) => {
                let exported = count(vault.entries().len(), "password", "passwords");
                self.toast(&format!("Exported {exported}. Delete the file when you're done with it."))
            }
            Err(e) => self.toast(&format!("Export failed: {e}")),
        }
    }

    /// Ganti master password: master password sekarang diverifikasi ke berkas
    /// di disk, lalu kunci vault dibungkus ulang dengan yang baru.
    fn change_master(&self, state: &Rc<AppState>, error: Option<&str>) {
        if !state.passwords.is_unlocked() {
            state.passwords.with_vault(state, "to change the master password", |s, _| {
                // Dijalankan setelah vault terbuka dan dialog buka kunci tertutup.
                let weak = Rc::downgrade(s);
                let block = RcBlock::new(move || {
                    if let Some(s) = weak.upgrade() {
                        s.password_manager.change_master(&s, None);
                    }
                });
                unsafe { NSRunLoop::mainRunLoop().performBlock(&block) };
            });
            return;
        }
        let mut info = format!(
            "The new master password must be at least {MIN_MASTER} characters and can't be recovered if you forget it."
        );
        if let Some(error) = error {
            info = format!("{error}\n\n{info}");
        }
        let weak = Rc::downgrade(state);
        let fields = ["Current master password", "New master password", "Confirm new master password"];
        dialogs::ask_secrets(
            &state.dialog_window(),
            "Change Master Password",
            &info,
            &fields,
            "Change",
            move |values| {
                let (Some(state), Some(values)) = (weak.upgrade(), values) else { return };
                let (current, new, again) = (values[0].clone(), values[1].clone(), values[2].clone());
                if new.chars().count() < MIN_MASTER {
                    return state.password_manager.change_master(&state, Some("The new master password is too short."));
                }
                if new != again {
                    return state.password_manager.change_master(&state, Some("The new master passwords don't match."));
                }
                let (Ok(Some(file)), Some(mut vault)) = (vault::load(&vault_path()), state.passwords.take_vault())
                else {
                    return state.password_manager.toast("The password vault isn't open.");
                };
                background::run(
                    move || {
                        let outcome = match Vault::unlock(&file, &current)
                            .and_then(|_| vault.change_master(&new, KdfParams::DEFAULT))
                        {
                            Err(VaultError::WrongPassword) => Change::WrongCurrent,
                            Err(e) => Change::Failed(e.to_string()),
                            Ok(()) => match vault::save(&vault_path(), &vault.to_file()) {
                                Ok(()) => Change::Done,
                                Err(e) => {
                                    // Disk masih memakai master password lama: kembalikan
                                    // bungkusan di memory supaya simpanan berikutnya tidak
                                    // diam-diam mengganti master password.
                                    let _ = vault.change_master(&current, KdfParams::DEFAULT);
                                    Change::Failed(format!("Master password not changed: {e}"))
                                }
                            },
                        };
                        (vault, outcome)
                    },
                    move |(vault, outcome)| {
                        state.passwords.put_vault(vault);
                        match outcome {
                            Change::Done => state.password_manager.toast("Master password changed"),
                            Change::WrongCurrent => {
                                state.password_manager.change_master(&state, Some("Incorrect current master password."))
                            }
                            Change::Failed(e) => state.password_manager.toast(&e),
                        }
                    },
                );
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_for_the_window_never_contains_passwords() {
        let mut v = Vault::create("master-panjang", KdfParams { m_kib: 64, t: 1, p: 1 }).unwrap();
        v.add(
            NewEntry {
                origin: "https://b.example".into(),
                username: "u2".into(),
                password: "rahasia-b".into(),
                note: String::new(),
            },
            1,
        );
        v.add(
            NewEntry {
                origin: "https://a.example".into(),
                username: "u1".into(),
                password: "rahasia-a".into(),
                note: "n".into(),
            },
            1,
        );
        let list = entries_json(&v);
        let text = list.to_string();
        assert!(!text.contains("rahasia"), "{text}");
        assert_eq!(list[0]["host"], "a.example", "urut per situs");
        assert_eq!(list[1]["username"], "u2");
    }

    #[test]
    fn counts_use_singular_and_plural() {
        assert_eq!(count(1, "password", "passwords"), "1 password");
        assert_eq!(count(0, "password", "passwords"), "0 passwords");
        assert_eq!(count(3, "row", "rows"), "3 rows");
    }

    #[test]
    fn site_input_accepts_bare_hosts() {
        assert_eq!(origin_from_input("github.com/login"), Some("https://github.com".into()));
        assert_eq!(origin_from_input("http://localhost:3000"), Some("http://localhost:3000".into()));
        assert_eq!(origin_from_input("ftp://x.example"), None);
        assert_eq!(origin_from_input(""), None);
    }
}
