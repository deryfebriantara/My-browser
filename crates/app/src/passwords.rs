//! Pengelola password: menawarkan simpan setelah login, mengisi login
//! (Cmd+\), menyalin password acak, dan mengunci vault otomatis.
//!
//! Vault hanya terbuka di memory setelah master password diketik, lalu
//! dikunci lagi setelah tidak dipakai (bawaan 10 menit), saat layar Mac
//! tidur atau dikunci, saat Mac tidur, atau lewat menu. Vault tidak punya
//! jalur dari MCP maupun panel Tanya Claude.

use crate::background;
use crate::dialogs;
use crate::persistence;
use crate::state::AppState;
use block2::RcBlock;
use browser_core::engine::{Page, Secret};
use browser_core::settings::Settings;
use browser_core::tabs::TabId;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardTypeString, NSWorkspace, NSWorkspaceScreensDidSleepNotification,
    NSWorkspaceSessionDidResignActiveNotification, NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSObjectProtocol, NSOperationQueue, NSString, NSTimer,
};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vault::{KdfParams, NewEntry, Offer, Vault, VaultError, VaultFile, generator, origin_of};
use zeroize::Zeroizing;

/// Panjang minimum master password.
pub const MIN_MASTER: usize = 10;
/// Clipboard berisi password dibersihkan setelah sekian detik.
const CLIPBOARD_SECS: f64 = 30.0;

type Then = Box<dyn FnOnce(&Rc<AppState>, &mut Vault)>;

pub(crate) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn vault_path() -> PathBuf {
    persistence::data_dir().join("vault.json")
}

pub(crate) fn host(origin: &str) -> &str {
    origin.split_once("://").map_or(origin, |(_, host)| host)
}

fn notice(state: &AppState, message: &str) {
    state.ui.show_notice(message);
}

/// Setelah pengguna login: tawarkan simpan atau perbarui, atau diam.
/// Saat vault terkunci, akun yang diketahui tersimpan (dari pembukaan
/// terakhir) tidak ditanyakan lagi.
pub(crate) fn decide(
    vault: Option<&Vault>,
    known: &HashSet<(String, String)>,
    settings: &Settings,
    origin: &str,
    username: &str,
    password: &str,
) -> Option<Offer> {
    if !settings.password_offer_save || settings.password_never_save.iter().any(|h| h == host(origin)) {
        return None;
    }
    match vault {
        Some(vault) => match vault.offer(origin, username, password) {
            Offer::Nothing => None,
            offer => Some(offer),
        },
        None if known.contains(&(origin.to_string(), username.to_string())) => None,
        None => Some(Offer::Save),
    }
}

/// Menyimpan vault ke disk.
pub(crate) fn persist(vault: &Vault) -> Result<(), String> {
    vault::save(&vault_path(), &vault.to_file()).map_err(|e| format!("Couldn't save passwords: {e}"))
}

#[derive(Default)]
pub struct Passwords {
    vault: RefCell<Option<Vault>>,
    last_used: Cell<Option<Instant>>,
    /// Akun tersimpan (origin, username) dari pembukaan terakhir. Hanya di
    /// memory; tidak ada daftar situs yang ditulis ke disk tanpa enkripsi.
    known: RefCell<HashSet<(String, String)>>,
    /// Argon2id sedang berjalan di thread latar.
    unlocking: Cell<bool>,
    observers: RefCell<Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
}

impl Passwords {
    pub fn is_unlocked(&self) -> bool {
        self.vault.borrow().is_some()
    }

    fn touch(&self) {
        self.last_used.set(Some(Instant::now()));
    }

    pub(crate) fn refresh_known(&self, vault: &Vault) {
        *self.known.borrow_mut() = vault.entries().iter().map(|e| (e.origin.clone(), e.username.clone())).collect();
    }

    /// Menutup vault: kunci dan entri dihapus dari memory.
    pub fn lock(&self, state: &AppState, announce: bool) {
        let was_open = self.vault.borrow_mut().take().is_some();
        self.last_used.set(None);
        if was_open && announce {
            notice(state, "Passwords locked");
        }
        state.password_manager.locked(state);
    }

    /// Dipanggil timer aplikasi: kunci setelah lama tidak dipakai.
    pub fn tick(&self, state: &AppState) {
        let minutes = state.browser.borrow().settings().password_lock_minutes.max(1);
        if self.last_used.get().is_some_and(|t| t.elapsed() >= Duration::from_secs(minutes * 60)) {
            self.lock(state, true);
        }
    }

    /// Kunci saat layar tidur, sesi berganti, Mac tidur, atau layar dikunci.
    pub fn install_lock_observers(&self, state: &Rc<AppState>) {
        let make = |state: &Rc<AppState>| {
            let weak = Rc::downgrade(state);
            RcBlock::new(move |_n: NonNull<NSNotification>| {
                if let Some(state) = weak.upgrade() {
                    state.passwords.lock(&state, false);
                }
            })
        };
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        let main = NSOperationQueue::mainQueue();
        let mut observers = self.observers.borrow_mut();
        for name in unsafe {
            [
                NSWorkspaceScreensDidSleepNotification,
                NSWorkspaceSessionDidResignActiveNotification,
                NSWorkspaceWillSleepNotification,
            ]
        } {
            let block = make(state);
            observers.push(unsafe {
                workspace.addObserverForName_object_queue_usingBlock(Some(name), None, Some(&main), &block)
            });
        }
        let distributed = NSDistributedNotificationCenter::defaultCenter();
        let block = make(state);
        let name = NSString::from_str("com.apple.screenIsLocked");
        observers.push(unsafe {
            distributed.addObserverForName_object_queue_usingBlock(Some(&name), None, Some(&main), &block)
        });
    }

    /// Menjalankan `then` dengan vault terbuka. Kalau perlu, membuat vault
    /// baru atau meminta master password dulu; `reason` tampil di dialog.
    pub fn with_vault(
        &self,
        state: &Rc<AppState>,
        reason: &str,
        then: impl FnOnce(&Rc<AppState>, &mut Vault) + 'static,
    ) {
        let open = self.vault.borrow_mut().take();
        if let Some(mut vault) = open {
            // Vault dikeluarkan selama `then` berjalan, supaya `then` boleh
            // memanggil pengelola password lagi tanpa pinjaman ganda.
            self.touch();
            then(state, &mut vault);
            *self.vault.borrow_mut() = Some(vault);
            return;
        }
        if self.unlocking.get() || dialogs::is_open() {
            notice(state, "Finish the open dialog first");
            return;
        }
        match vault::load(&vault_path()) {
            Err(e) => {
                let info = format!("{e}\n\nFile: {}", vault_path().display());
                dialogs::confirm(&state.dialog_window(), "Couldn't Open Password Vault", &info, &["OK"], |_| {});
            }
            Ok(None) => self.create_flow(state, reason, Box::new(then), None),
            Ok(Some(file)) => self.unlock_flow(state, reason, file, Box::new(then), None),
        }
    }

    fn unlock_flow(&self, state: &Rc<AppState>, reason: &str, file: VaultFile, then: Then, error: Option<&str>) {
        let mut info = format!("Enter your master password {reason}.");
        if let Some(error) = error {
            info = format!("{error}\n\n{info}");
        }
        let weak = Rc::downgrade(state);
        let reason = reason.to_string();
        dialogs::ask_secrets(
            &state.dialog_window(),
            "Unlock Passwords",
            &info,
            &["Master password"],
            "Unlock",
            move |values| {
                let (Some(state), Some(mut values)) = (weak.upgrade(), values) else { return };
                let master = values.remove(0);
                if master.is_empty() {
                    state.passwords.unlock_flow(&state, &reason, file, then, Some("Master password can't be empty."));
                    return;
                }
                state.passwords.unlocking.set(true);
                let copy = file.clone();
                background::run(
                    move || Vault::unlock(&copy, &master),
                    move |result| {
                        state.passwords.unlocking.set(false);
                        match result {
                            Ok(vault) => state.passwords.opened(&state, vault, then),
                            Err(VaultError::WrongPassword) => state.passwords.unlock_flow(
                                &state,
                                &reason,
                                file,
                                then,
                                Some("Incorrect master password."),
                            ),
                            Err(e) => {
                                dialogs::confirm(
                                    &state.dialog_window(),
                                    "Couldn't Open Password Vault",
                                    &e.to_string(),
                                    &["OK"],
                                    |_| {},
                                );
                            }
                        }
                    },
                );
            },
        );
    }

    fn create_flow(&self, state: &Rc<AppState>, reason: &str, then: Then, error: Option<&str>) {
        let mut info = format!(
            "my-browser will create an encrypted password vault {reason}. Your master password locks all your \
             passwords and can't be recovered if you forget it, so keep it somewhere safe. Use at least {MIN_MASTER} \
             characters."
        );
        if let Some(error) = error {
            info = format!("{error}\n\n{info}");
        }
        let weak = Rc::downgrade(state);
        let reason = reason.to_string();
        let placeholders = ["Master password", "Confirm master password"];
        dialogs::ask_secrets(
            &state.dialog_window(),
            "Create Master Password",
            &info,
            &placeholders,
            "Create",
            move |values| {
                let (Some(state), Some(values)) = (weak.upgrade(), values) else { return };
                let (first, second): (&Zeroizing<String>, &Zeroizing<String>) = (&values[0], &values[1]);
                let problem = if first.chars().count() < MIN_MASTER {
                    Some("Master password is too short.")
                } else if first != second {
                    Some("The master passwords don't match.")
                } else {
                    None
                };
                if let Some(problem) = problem {
                    state.passwords.create_flow(&state, &reason, then, Some(problem));
                    return;
                }
                let master = first.clone();
                state.passwords.unlocking.set(true);
                background::run(
                    move || Vault::create(&master, KdfParams::DEFAULT),
                    move |result| {
                        state.passwords.unlocking.set(false);
                        // Jangan pernah menimpa vault yang sudah ada.
                        let saved = match result {
                            _ if vault_path().exists() => Err("A password vault already exists.".to_string()),
                            Ok(vault) => persist(&vault).map(|_| vault),
                            Err(e) => Err(e.to_string()),
                        };
                        match saved {
                            Ok(vault) => state.passwords.opened(&state, vault, then),
                            Err(e) => {
                                dialogs::confirm(
                                    &state.dialog_window(),
                                    "Couldn't Create Password Vault",
                                    &e,
                                    &["OK"],
                                    |_| {},
                                );
                            }
                        }
                    },
                );
            },
        );
    }

    fn opened(&self, state: &Rc<AppState>, mut vault: Vault, then: Then) {
        self.refresh_known(&vault);
        self.touch();
        then(state, &mut vault);
        *self.vault.borrow_mut() = Some(vault);
        state.password_manager.unlocked(state);
    }

    /// Penangkap login di halaman melaporkan login: tawarkan simpan.
    pub fn on_credential(&self, state: &Rc<AppState>, origin: String, username: String, password: Secret) {
        let Some(origin) = origin_of(&origin) else { return };
        let settings = state.browser.borrow().settings().clone();
        let offer = decide(
            self.vault.borrow().as_ref(),
            &self.known.borrow(),
            &settings,
            &origin,
            &username,
            password.expose(),
        );
        let Some(offer) = offer else { return };
        if dialogs::is_open() || self.unlocking.get() {
            return;
        }
        let site = host(&origin).to_string();
        let (title, button) = match offer {
            Offer::Update(_) => (format!("Update Password for {site}?"), "Update"),
            _ => (format!("Save Password for {site}?"), "Save"),
        };
        let mut info = if username.is_empty() {
            "Account without a username.".to_string()
        } else {
            format!("Account: {username}")
        };
        if !vault_path().exists() {
            info.push_str("\n\nPasswords are stored encrypted and locked with a master password you'll create.");
        }
        let weak = Rc::downgrade(state);
        let buttons = [button, "Not Now", "Never for This Site"];
        dialogs::confirm(&state.dialog_window(), &title, &info, &buttons, move |choice| {
            let Some(state) = weak.upgrade() else { return };
            match choice {
                0 => {
                    let reason = format!("to save the password for {site}");
                    state.passwords.with_vault(&state, &reason, move |state, vault| {
                        let message = match vault.offer(&origin, &username, password.expose()) {
                            Offer::Nothing => "This password is already saved".to_string(),
                            Offer::Update(id) => {
                                vault.update(&id, None, Some(password.expose()), None, now());
                                format!("Password updated for {site}")
                            }
                            Offer::Save => {
                                let entry = NewEntry {
                                    origin: origin.clone(),
                                    username: username.clone(),
                                    password: password.expose().to_string(),
                                    note: String::new(),
                                };
                                vault.add(entry, now());
                                format!("Password saved for {site}")
                            }
                        };
                        match persist(vault) {
                            Ok(()) => {
                                state.passwords.refresh_known(vault);
                                notice(state, &message);
                            }
                            Err(e) => notice(state, &e),
                        }
                        state.password_manager.changed(state, vault);
                    });
                }
                2 => {
                    state.browser.borrow_mut().update_settings(|s| s.password_never_save.push(site.clone()));
                    state.refresh();
                    notice(&state, &format!("Won't offer to save passwords for {site} again"));
                }
                _ => {}
            }
        });
    }

    /// Cmd+\: isi login di tab aktif dengan akun tersimpan untuk situs ini.
    pub fn fill_active(&self, state: &Rc<AppState>) {
        let (tab, url) = {
            let b = state.browser.borrow();
            let id = b.tabs().active();
            (id, b.tabs().get(id).and_then(|t| t.url.as_ref().map(|u| u.to_string())))
        };
        let Some(origin) = url.as_deref().and_then(origin_of) else {
            notice(state, "Open a login page first to fill a password");
            return;
        };
        if state.browser.borrow().page(tab).is_none() {
            return;
        }
        let site = host(&origin).to_string();
        self.with_vault(state, &format!("to fill the password on {site}"), move |state, vault| {
            let accounts: Vec<(String, String, Secret)> = vault
                .for_origin(&origin)
                .iter()
                .map(|e| (e.id.clone(), e.username.clone(), Secret::new(e.password.clone())))
                .collect();
            match accounts.len() {
                0 => notice(
                    state,
                    &format!("No saved passwords for {site}. Log in as usual to save one, or add it in Passwords > Manage Passwords…"),
                ),
                1 => {
                    let (id, user, password) = accounts.into_iter().next().expect("satu akun");
                    fill(state, tab, origin, id, user, password);
                }
                _ => {
                    let names: Vec<String> =
                        accounts.iter().map(|(_, u, _)| if u.is_empty() { "(no username)".into() } else { u.clone() }).collect();
                    let weak = Rc::downgrade(state);
                    let title = format!("Choose an Account for {site}");
                    dialogs::choose(&state.dialog_window(), &title, "The login will be filled in on this page.", &names, "Fill", move |choice| {
                        let (Some(state), Some(i)) = (weak.upgrade(), choice) else { return };
                        if let Some((id, user, password)) = accounts.into_iter().nth(i) {
                            fill(&state, tab, origin, id, user, password);
                        }
                    });
                }
            }
        });
    }

    fn mark_used(&self, state: &AppState, id: &str) {
        let mut guard = self.vault.borrow_mut();
        if let Some(vault) = guard.as_mut() {
            vault.mark_used(id, now());
            if let Err(e) = persist(vault) {
                drop(guard);
                notice(state, &e);
            }
        }
    }

    /// Menu Copy Random Password: password baru ke clipboard (dibersihkan sendiri).
    pub fn copy_generated(&self, state: &AppState) {
        let password = Zeroizing::new(generator::generate(generator::DEFAULT_LENGTH, true));
        copy_secret(&password);
        notice(state, "Random password copied; clipboard clears in 30 seconds");
    }

    pub(crate) fn take_vault(&self) -> Option<Vault> {
        self.vault.borrow_mut().take()
    }

    pub(crate) fn put_vault(&self, vault: Vault) {
        self.touch();
        *self.vault.borrow_mut() = Some(vault);
    }
}

fn fill(state: &Rc<AppState>, tab: TabId, origin: String, id: String, username: String, password: Secret) {
    let b = state.browser.borrow();
    let Some(page) = b.page(tab) else {
        drop(b);
        notice(state, "The tab is closed or sleeping");
        return;
    };
    let weak = Rc::downgrade(state);
    let user = username.clone();
    page.fill_login(
        &origin,
        &user,
        &password,
        Box::new(move |result| {
            let Some(state) = weak.upgrade() else { return };
            let value: serde_json::Value = result.ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            if let Some(error) = value["error"].as_str() {
                notice(&state, error);
            } else if value["filled_password"] == true || value["filled_username"] == true {
                let message = if username.is_empty() {
                    "Password filled".to_string()
                } else {
                    format!("Login filled for {username}")
                };
                notice(&state, &message);
                state.passwords.mark_used(&state, &id);
            } else {
                notice(&state, "No login fields found");
            }
        }),
    );
}

/// Menyalin rahasia ke clipboard, ditandai "concealed" supaya pengelola
/// clipboard tidak mencatatnya, lalu dibersihkan setelah 30 detik kalau
/// clipboard belum diganti.
pub(crate) fn copy_secret(text: &str) {
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    board.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
    board.setString_forType(&NSString::from_str(""), &NSString::from_str("org.nspasteboard.ConcealedType"));
    let count = board.changeCount();
    let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
        let board = NSPasteboard::generalPasteboard();
        if board.changeCount() == count {
            board.clearContents();
        }
    });
    let _ = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(CLIPBOARD_SECS, false, &block) };
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: KdfParams = KdfParams { m_kib: 64, t: 1, p: 1 };

    fn vault_with(origin: &str, user: &str, pass: &str) -> Vault {
        let mut v = Vault::create("master-panjang", FAST).unwrap();
        v.add(NewEntry { origin: origin.into(), username: user.into(), password: pass.into(), note: String::new() }, 1);
        v
    }

    #[test]
    fn offers_follow_vault_state_and_settings() {
        let settings = Settings::default();
        let none = HashSet::new();
        let v = vault_with("https://github.com", "dery", "pw");
        assert_eq!(decide(Some(&v), &none, &settings, "https://github.com", "dery", "pw"), None, "sudah tersimpan");
        assert!(matches!(
            decide(Some(&v), &none, &settings, "https://github.com", "dery", "baru"),
            Some(Offer::Update(_))
        ));
        assert_eq!(decide(Some(&v), &none, &settings, "https://github.com", "lain", "x"), Some(Offer::Save));
        assert_eq!(
            decide(None, &none, &settings, "https://github.com", "dery", "pw"),
            Some(Offer::Save),
            "terkunci, belum diketahui"
        );
        let known: HashSet<_> = [("https://github.com".to_string(), "dery".to_string())].into();
        assert_eq!(
            decide(None, &known, &settings, "https://github.com", "dery", "pw"),
            None,
            "terkunci, akun diketahui"
        );
    }

    #[test]
    fn settings_can_silence_offers() {
        let none = HashSet::new();
        let off = Settings { password_offer_save: false, ..Settings::default() };
        assert_eq!(decide(None, &none, &off, "https://a.example", "u", "p"), None);
        let never = Settings { password_never_save: vec!["a.example".into()], ..Settings::default() };
        assert_eq!(decide(None, &none, &never, "https://a.example", "u", "p"), None);
        assert_eq!(decide(None, &none, &never, "https://b.example", "u", "p"), Some(Offer::Save));
    }

    #[test]
    fn host_strips_scheme() {
        assert_eq!(host("https://github.com"), "github.com");
        assert_eq!(host("http://localhost:3000"), "localhost:3000");
    }
}
