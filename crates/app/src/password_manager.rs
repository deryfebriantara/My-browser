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
        window.setTitle(&NSString::from_str("Password"));
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
                if state.passwords.is_unlocked() { self.unlocked(state) } else { self.locked(state) }
            }
            Some("unlock") => state.passwords.with_vault(state, "untuk mengelola password", |s, v| s.password_manager.changed(s, v)),
            Some("reveal") => state.passwords.with_vault(state, "untuk melihat password", move |s, v| {
                if let Some(e) = v.get(&id) {
                    s.password_manager.post(json!({"t": "secret", "id": id, "password": e.password}));
                }
            }),
            Some("copy") => state.passwords.with_vault(state, "untuk menyalin password", move |s, v| {
                if let Some(e) = v.get(&id) {
                    copy_secret(&e.password);
                    s.password_manager.toast("Password disalin; clipboard dibersihkan dalam 30 detik");
                }
            }),
            Some("copy_user") => state.passwords.with_vault(state, "untuk menyalin username", move |s, v| {
                if let Some(e) = v.get(&id) {
                    copy_plain(&e.username);
                    s.password_manager.toast("Username disalin");
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
        let (id, url, username, password, note) =
            (message["id"].as_str().map(String::from), field("url"), field("username"), field("password"), field("note"));
        state.passwords.with_vault(state, "untuk menyimpan password", move |s, v| {
            let manager = &s.password_manager;
            let form_error = |text: &str| manager.post(json!({"t": "form_error", "text": text}));
            let text = match &id {
                Some(id) => {
                    let password = (!password.is_empty()).then_some(password.as_str());
                    if !v.update(id, Some(&username), password, Some(&note), now()) {
                        return form_error("Entri tidak ditemukan.");
                    }
                    "Perubahan disimpan".to_string()
                }
                None => {
                    let Some(origin) = origin_from_input(&url) else {
                        return form_error("Alamat situs tidak valid. Contoh: https://github.com");
                    };
                    if password.is_empty() {
                        return form_error("Password tidak boleh kosong.");
                    }
                    if v.entries().iter().any(|e| e.origin == origin && e.username == *username) {
                        return form_error("Akun ini sudah ada untuk situs itu; edit entri yang ada.");
                    }
                    let entry = NewEntry {
                        origin: origin.clone(),
                        username: username.to_string(),
                        password: password.to_string(),
                        note: note.to_string(),
                    };
                    v.add(entry, now());
                    format!("Password untuk {} disimpan", host(&origin))
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
        state.passwords.with_vault(state, "untuk menghapus password", move |s, v| {
            let Some(entry) = v.get(&id) else { return };
            let title = format!("Hapus password untuk {}?", entry.host());
            let info = format!("Akun: {}. Penghapusan tidak bisa dibatalkan.", if entry.username.is_empty() { "(tanpa username)" } else { &entry.username });
            let weak = Rc::downgrade(s);
            dialogs::confirm(&s.dialog_window(), &title, &info, &["Hapus", "Batal"], move |choice| {
                let Some(state) = weak.upgrade() else { return };
                if choice != 0 {
                    return;
                }
                state.passwords.with_vault(&state, "untuk menghapus password", move |s, v| {
                    if v.delete(&id) {
                        match persist(v) {
                            Ok(()) => {
                                s.passwords.refresh_known(v);
                                s.password_manager.changed(s, v);
                                s.password_manager.toast("Password dihapus");
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
        panel.setMessage(Some(&NSString::from_str("Pilih berkas CSV ekspor password (Chrome, Safari/Passwords, Firefox, Bitwarden)")));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        let Some(path) = panel.URLs().firstObject().and_then(|u| u.path()).map(|p| p.to_string()) else { return };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => Zeroizing::new(text),
            Err(e) => return self.toast(&format!("Berkas tidak bisa dibaca: {e}")),
        };
        let imported = match csv::import(&text) {
            Ok(imported) => imported,
            Err(e) => return self.toast(&e),
        };
        let skipped = imported.skipped;
        state.passwords.with_vault(state, "untuk mengimpor password", move |s, v| {
            let (added, updated) = v.import(imported.entries, now());
            match persist(v) {
                Ok(()) => {
                    s.passwords.refresh_known(v);
                    s.password_manager.changed(s, v);
                    let info = format!(
                        "{added} password ditambahkan, {updated} diperbarui, {skipped} baris dilewati.\n\nBerkas CSV itu berisi \
                         password tanpa enkripsi. Sebaiknya hapus sekarang (juga dari Sampah):\n{path}"
                    );
                    dialogs::confirm(&s.dialog_window(), "Impor selesai", &info, &["OK"], |_| {});
                }
                Err(e) => s.password_manager.toast(&e),
            }
        });
    }

    /// Ekspor meminta master password lagi (diverifikasi ke berkas di disk),
    /// karena hasilnya berisi semua password tanpa enkripsi.
    fn export(&self, state: &Rc<AppState>) {
        let info = "Berkas CSV hasil ekspor berisi semua password tanpa enkripsi. Siapa pun yang mendapat berkas itu bisa \
                    membaca semuanya. Masukkan master password untuk melanjutkan.";
        let weak = Rc::downgrade(state);
        dialogs::ask_secrets(&state.dialog_window(), "Ekspor Password", info, &["Master password"], "Lanjut", move |values| {
            let (Some(state), Some(mut values)) = (weak.upgrade(), values) else { return };
            let master = values.remove(0);
            let file = match vault::load(&vault_path()) {
                Ok(Some(file)) => file,
                _ => return state.password_manager.toast("Belum ada vault password."),
            };
            background::run(
                move || Vault::unlock(&file, &master),
                move |result| match result {
                    Ok(verified) => state.password_manager.write_export(&state, &verified),
                    Err(VaultError::WrongPassword) => state.password_manager.toast("Master password salah; ekspor dibatalkan."),
                    Err(e) => state.password_manager.toast(&e.to_string()),
                },
            );
        });
    }

    fn write_export(&self, _state: &Rc<AppState>, vault: &Vault) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let panel = NSSavePanel::savePanel(mtm);
        panel.setNameFieldStringValue(&NSString::from_str("password-my-browser.csv"));
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
            Ok(()) => self.toast(&format!("{} password diekspor. Hapus berkasnya setelah dipakai.", vault.entries().len())),
            Err(e) => self.toast(&format!("Ekspor gagal: {e}")),
        }
    }

    /// Ganti master password: master password sekarang diverifikasi ke berkas
    /// di disk, lalu kunci vault dibungkus ulang dengan yang baru.
    fn change_master(&self, state: &Rc<AppState>, error: Option<&str>) {
        if !state.passwords.is_unlocked() {
            state.passwords.with_vault(state, "untuk mengganti master password", |s, _| {
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
        let mut info = format!("Master password baru minimal {MIN_MASTER} karakter dan tidak bisa dipulihkan kalau lupa.");
        if let Some(error) = error {
            info = format!("{error}\n\n{info}");
        }
        let weak = Rc::downgrade(state);
        let fields = ["Master password sekarang", "Master password baru", "Ulangi master password baru"];
        dialogs::ask_secrets(&state.dialog_window(), "Ganti Master Password", &info, &fields, "Ganti", move |values| {
            let (Some(state), Some(values)) = (weak.upgrade(), values) else { return };
            let (current, new, again) = (values[0].clone(), values[1].clone(), values[2].clone());
            if new.chars().count() < MIN_MASTER {
                return state.password_manager.change_master(&state, Some("Master password baru terlalu pendek."));
            }
            if new != again {
                return state.password_manager.change_master(&state, Some("Kedua master password baru tidak sama."));
            }
            let (Ok(Some(file)), Some(mut vault)) = (vault::load(&vault_path()), state.passwords.take_vault()) else {
                return state.password_manager.toast("Vault tidak terbuka.");
            };
            background::run(
                move || {
                    let outcome = match Vault::unlock(&file, &current).and_then(|_| vault.change_master(&new, KdfParams::DEFAULT)) {
                        Err(VaultError::WrongPassword) => Change::WrongCurrent,
                        Err(e) => Change::Failed(e.to_string()),
                        Ok(()) => match vault::save(&vault_path(), &vault.to_file()) {
                            Ok(()) => Change::Done,
                            Err(e) => {
                                // Disk masih memakai master password lama: kembalikan
                                // bungkusan di memory supaya simpanan berikutnya tidak
                                // diam-diam mengganti master password.
                                let _ = vault.change_master(&current, KdfParams::DEFAULT);
                                Change::Failed(format!("Master password tidak diganti: {e}"))
                            }
                        },
                    };
                    (vault, outcome)
                },
                move |(vault, outcome)| {
                    state.passwords.put_vault(vault);
                    match outcome {
                        Change::Done => state.password_manager.toast("Master password diganti"),
                        Change::WrongCurrent => state.password_manager.change_master(&state, Some("Master password sekarang salah.")),
                        Change::Failed(e) => state.password_manager.toast(&e),
                    }
                },
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_for_the_window_never_contains_passwords() {
        let mut v = Vault::create("master-panjang", KdfParams { m_kib: 64, t: 1, p: 1 }).unwrap();
        v.add(NewEntry { origin: "https://b.example".into(), username: "u2".into(), password: "rahasia-b".into(), note: String::new() }, 1);
        v.add(NewEntry { origin: "https://a.example".into(), username: "u1".into(), password: "rahasia-a".into(), note: "n".into() }, 1);
        let list = entries_json(&v);
        let text = list.to_string();
        assert!(!text.contains("rahasia"), "{text}");
        assert_eq!(list[0]["host"], "a.example", "urut per situs");
        assert_eq!(list[1]["username"], "u2");
    }

    #[test]
    fn site_input_accepts_bare_hosts() {
        assert_eq!(origin_from_input("github.com/login"), Some("https://github.com".into()));
        assert_eq!(origin_from_input("http://localhost:3000"), Some("http://localhost:3000".into()));
        assert_eq!(origin_from_input("ftp://x.example"), None);
        assert_eq!(origin_from_input(""), None);
    }
}
