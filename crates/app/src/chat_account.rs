//! Akun Claude untuk panel Tanya Claude: status, masuk, keluar, dan ganti
//! profil Claude Code.
//!
//! Masuk menjalankan `claude auth login` untuk profil terpilih. CLI membuka
//! halaman login dengan perintah `open`; khusus untuk proses login itu,
//! `open` diganti skrip kecil (lewat PATH) yang menyerahkan URL-nya ke
//! my-browser, jadi login berlangsung di tab my-browser. Setelah pengguna
//! menyetujui, halaman dialihkan ke localhost tempat CLI menunggu, dan login
//! selesai. Kalau diminta, kode bisa ditempel di panel.

use crate::background;
use crate::dialogs;
use crate::persistence;
use crate::state::AppState;
use browser_core::claude_chat::{self as cc, Account};
use browser_core::tabs::TabId;
use dispatch2::DispatchQueue;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Pengganti `open` untuk proses login: mencatat URL ke berkas di
/// `MB_LOGIN_URLS`, tidak membuka browser lain.
const OPEN_SHIM: &str = "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in http*) printf '%s\\n' \"$a\" >> \"$MB_LOGIN_URLS\";; esac; done\nexit 0\n";
/// Lama menunggu pengguna menyelesaikan login.
const LOGIN_WAIT: Duration = Duration::from_secs(600);

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// Profil Claude Code terpilih, dalam bentuk `~/.claude`.
pub(crate) fn config_dir(state: &AppState) -> String {
    state.browser.borrow().settings().claude_config_dir.clone().unwrap_or_else(|| cc::DEFAULT_CONFIG_DIR.into())
}

/// Lokasi `claude` yang pertama ada.
pub(crate) fn find_claude(home: &Path, configured: Option<&str>) -> Option<PathBuf> {
    let versions: Vec<String> = std::fs::read_dir(home.join(".nvm/versions/node"))
        .map(|dir| dir.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    cc::claude_candidates(home, configured, &versions).into_iter().find(|p| p.is_file())
}

/// Perintah `claude` untuk profil terpilih: `CLAUDE_CONFIG_DIR` selalu
/// diset (tanpanya CLI membaca login bawaan yang bisa berbeda), dan variabel
/// sesi Claude Code dari lingkungan browser tidak ikut terbawa.
pub(crate) fn claude_command(state: &AppState) -> Result<Command, String> {
    let settings = state.browser.borrow().settings().clone();
    let home = home();
    let claude = find_claude(&home, settings.claude_path.as_deref()).ok_or_else(|| {
        "Claude Code wasn't found. Install it from https://claude.com/claude-code, or set claude_path in \
         settings.json to the location of the claude binary."
            .to_string()
    })?;
    let mut command = Command::new(&claude);
    command.env("CLAUDE_CONFIG_DIR", cc::expand_home(&config_dir(state), &home));
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy();
        if key == "CLAUDECODE" || key.starts_with("CLAUDE_CODE_") {
            command.env_remove(key.as_ref());
        }
    }
    Ok(command)
}

/// Profil Claude Code yang ada di folder home.
fn profiles() -> Vec<String> {
    let names: Vec<String> = std::fs::read_dir(home())
        .map(|dir| {
            dir.filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    cc::profile_dirs(&names)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Checking,
    SignedIn,
    SignedOut,
    SigningIn,
}

struct Login {
    child: Child,
    stdin: ChildStdin,
}

impl Drop for Login {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct ChatAccount {
    phase: Cell<Phase>,
    account: RefCell<Account>,
    login: RefCell<Option<Login>>,
    /// Naik setiap login dimulai atau dibatalkan; event login lama dibuang.
    generation: Cell<u64>,
    login_tab: Cell<Option<TabId>>,
    /// Pesan error untuk panel (merah).
    message: RefCell<Option<String>>,
    /// Keterangan untuk panel (abu-abu).
    note: RefCell<Option<String>>,
}

impl Default for ChatAccount {
    fn default() -> Self {
        Self {
            phase: Cell::new(Phase::Checking),
            account: RefCell::default(),
            login: RefCell::default(),
            generation: Cell::new(0),
            login_tab: Cell::new(None),
            message: RefCell::default(),
            note: RefCell::default(),
        }
    }
}

impl ChatAccount {
    /// `Some(false)` kalau sudah pasti belum masuk.
    pub fn signed_in(&self) -> Option<bool> {
        match self.phase.get() {
            Phase::SignedIn => Some(true),
            Phase::SignedOut => Some(false),
            _ => None,
        }
    }

    fn post(&self, state: &AppState) {
        let current = config_dir(state);
        let mut list = profiles();
        if !list.contains(&current) {
            list.insert(0, current.clone());
        }
        let profiles: Vec<Value> = list.iter().map(|d| json!({"dir": d, "label": cc::profile_label(d)})).collect();
        let phase = match self.phase.get() {
            Phase::Checking => "checking",
            Phase::SignedIn => "signed_in",
            Phase::SignedOut => "signed_out",
            Phase::SigningIn => "signing_in",
        };
        let account = self.account.borrow();
        state.chat.post(json!({
            "t": "account",
            "state": phase,
            "email": account.email,
            "plan": account.plan,
            "profile": current,
            "profile_label": cc::profile_label(&current),
            "profiles": profiles,
            "message": *self.message.borrow(),
            "note": *self.note.borrow(),
        }));
    }

    /// Membaca `claude auth status --json` di thread latar.
    pub fn refresh(&self, state: &Rc<AppState>) {
        if self.phase.get() != Phase::SigningIn {
            self.phase.set(Phase::Checking);
        }
        self.post(state);
        let command = claude_command(state);
        let weak = Rc::downgrade(state);
        background::run(
            move || {
                let mut command = command?;
                let out = command
                    .args(["auth", "status", "--json"])
                    .stdin(Stdio::null())
                    .output()
                    .map_err(|e| e.to_string())?;
                cc::parse_auth_status(&String::from_utf8_lossy(&out.stdout))
                    .ok_or_else(|| "Couldn't read the account status.".to_string())
            },
            move |result: Result<Account, String>| {
                let Some(state) = weak.upgrade() else { return };
                let me = &state.chat.account;
                if me.phase.get() == Phase::SigningIn {
                    return;
                }
                match result {
                    Ok(account) => {
                        me.phase.set(if account.logged_in { Phase::SignedIn } else { Phase::SignedOut });
                        *me.account.borrow_mut() = account;
                        *me.message.borrow_mut() = None;
                    }
                    Err(e) => {
                        me.phase.set(Phase::SignedOut);
                        *me.message.borrow_mut() = Some(e);
                    }
                }
                me.post(&state);
            },
        );
    }

    /// Mulai `claude auth login` (langganan Claude, atau API Console).
    pub fn login(&self, state: &Rc<AppState>, console: bool) {
        if self.login.borrow().is_some() {
            return;
        }
        match self.start_login(state, console) {
            Ok(()) => {
                self.phase.set(Phase::SigningIn);
                *self.message.borrow_mut() = None;
                *self.note.borrow_mut() = None;
            }
            Err(e) => *self.message.borrow_mut() = Some(e),
        }
        self.post(state);
    }

    fn start_login(&self, state: &Rc<AppState>, console: bool) -> Result<(), String> {
        let dir = persistence::data_dir().join("claude-login");
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).map_err(|e| format!("Couldn't create the sign-in folder: {e}"))?;
        let shim = bin.join("open");
        std::fs::write(&shim, OPEN_SHIM)
            .and_then(|_| std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o700)))
            .map_err(|e| e.to_string())?;
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let urls = dir.join(format!("urls-{generation}.txt"));
        let _ = std::fs::remove_file(&urls);

        let mut command = claude_command(state)?;
        command
            .args(["auth", "login", if console { "--console" } else { "--claudeai" }])
            .current_dir(&dir)
            .env("PATH", format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin.display()))
            .env("MB_LOGIN_URLS", &urls)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|e| format!("Couldn't run claude auth login: {e}"))?;
        let stdin = child.stdin.take().expect("stdin di-pipe");
        let stdout = child.stdout.take().expect("stdout di-pipe");
        let stderr = child.stderr.take().expect("stderr di-pipe");
        *self.login.borrow_mut() = Some(Login { child, stdin });
        self.login_tab.set(None);

        // URL dari pengganti `open`.
        let file = urls.clone();
        std::thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < LOGIN_WAIT {
                if let Some(url) = std::fs::read_to_string(&file).ok().and_then(|t| t.lines().next().map(String::from))
                {
                    DispatchQueue::main()
                        .exec_async(move || with_chat(|s| s.chat.account.open_login_tab(s, generation, url)));
                    return;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        });
        // Keluaran CLI: URL cadangan, lalu selesai saat proses berakhir.
        std::thread::spawn(move || {
            let mut output = String::new();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(url) = cc::find_login_url(&line) {
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(1500));
                        DispatchQueue::main()
                            .exec_async(move || with_chat(|s| s.chat.account.open_login_tab(s, generation, url)));
                    });
                }
                output.push_str(&line);
                output.push('\n');
            }
            let mut errors = String::new();
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                errors.push_str(&line);
                errors.push('\n');
            }
            let tail = format!("{output}{errors}");
            DispatchQueue::main().exec_async(move || with_chat(|s| s.chat.account.login_finished(s, generation, tail)));
        });
        Ok(())
    }

    fn open_login_tab(&self, state: &Rc<AppState>, generation: u64, url: String) {
        if generation != self.generation.get() || self.login_tab.get().is_some() || self.login.borrow().is_none() {
            return;
        }
        let tab = state.browser.borrow_mut().open_tab(&url, true, Instant::now());
        state.refresh();
        self.login_tab.set(tab);
        *self.note.borrow_mut() = Some(
            "The sign-in page opened in a new tab. Sign in and approve access there; this panel continues on its own afterward.".into(),
        );
        self.post(state);
    }

    fn login_finished(&self, state: &Rc<AppState>, generation: u64, output: String) {
        if generation != self.generation.get() {
            return;
        }
        let Some(mut login) = self.login.borrow_mut().take() else { return };
        let success = login.child.wait().is_ok_and(|s| s.success());
        drop(login);
        let _ =
            std::fs::remove_file(persistence::data_dir().join("claude-login").join(format!("urls-{generation}.txt")));
        self.phase.set(Phase::Checking);
        *self.note.borrow_mut() = None;
        if success {
            self.close_login_tab(state);
            *self.message.borrow_mut() = None;
            state.ui.nav.show_notice("Signed in to Claude");
        } else {
            let detail = output
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty() && !l.contains("Paste code"))
                .unwrap_or("")
                .to_string();
            *self.message.borrow_mut() = Some(format!("Sign-in didn't finish. {detail}").trim().to_string());
        }
        self.refresh(state);
    }

    /// Menutup tab login kalau masih di halaman login atau callback.
    fn close_login_tab(&self, state: &Rc<AppState>) {
        let Some(tab) = self.login_tab.take() else { return };
        let host = state
            .browser
            .borrow()
            .tabs()
            .get(tab)
            .and_then(|t| t.url.as_ref().and_then(|u| u.host_str().map(String::from)));
        if host.is_some_and(|h| {
            matches!(
                h.as_str(),
                "localhost"
                    | "127.0.0.1"
                    | "claude.ai"
                    | "claude.com"
                    | "platform.claude.com"
                    | "console.anthropic.com"
            )
        }) {
            state.close_tab(tab.0);
        }
    }

    /// Kode dari halaman login, kalau CLI memintanya.
    pub fn submit_code(&self, code: &str) {
        if let Some(login) = self.login.borrow_mut().as_mut() {
            let _ = writeln!(login.stdin, "{}", code.trim()).and_then(|_| login.stdin.flush());
        }
    }

    pub fn cancel_login(&self, state: &Rc<AppState>) {
        self.generation.set(self.generation.get() + 1);
        self.login.borrow_mut().take();
        self.phase.set(Phase::Checking);
        *self.message.borrow_mut() = None;
        *self.note.borrow_mut() = Some("Sign-in canceled.".into());
        self.refresh(state);
    }

    /// Keluar dari profil ini. Claude Code di Terminal yang memakai profil
    /// yang sama juga ikut keluar, jadi ditanyakan dulu.
    pub fn logout(&self, state: &Rc<AppState>) {
        let profile = config_dir(state);
        let info = format!(
            "Signing out applies to the {profile} profile: Claude Code in Terminal that uses this profile is signed out too."
        );
        let weak = Rc::downgrade(state);
        dialogs::confirm(
            &state.dialog_window(),
            "Sign out of your Claude account?",
            &info,
            &["Sign Out", "Cancel"],
            move |choice| {
                let Some(state) = weak.upgrade() else { return };
                if choice != 0 {
                    return;
                }
                state.chat.reset_conversation();
                let command = claude_command(&state);
                let weak = Rc::downgrade(&state);
                background::run(
                    move || {
                        command
                            .and_then(|mut c| {
                                c.args(["auth", "logout"]).stdin(Stdio::null()).output().map_err(|e| e.to_string())
                            })
                            .map(|_| ())
                    },
                    move |_result: Result<(), String>| {
                        if let Some(state) = weak.upgrade() {
                            state.chat.account.refresh(&state);
                        }
                    },
                );
            },
        );
    }

    /// Ganti profil Claude Code. Percakapan dimulai baru, karena sesi
    /// Claude Code tersimpan per profil.
    pub fn switch_profile(&self, state: &Rc<AppState>, dir: &str) {
        if !profiles().iter().any(|p| p == dir) || dir == config_dir(state) {
            return;
        }
        self.cancel_login_silently();
        let value = dir.to_string();
        state.browser.borrow_mut().update_settings(|s| s.claude_config_dir = Some(value));
        state.refresh();
        state.chat.reset_conversation();
        *self.message.borrow_mut() = None;
        self.refresh(state);
    }

    fn cancel_login_silently(&self) {
        self.generation.set(self.generation.get() + 1);
        self.login.borrow_mut().take();
        self.phase.set(Phase::Checking);
    }

    /// Jawaban Claude gagal karena login: periksa ulang status supaya panel
    /// menampilkan tombol Masuk.
    pub fn auth_failed(&self, state: &Rc<AppState>) {
        self.refresh(state);
    }
}

fn with_chat(f: impl FnOnce(&Rc<AppState>)) {
    crate::chat::with_state(f);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_shim_records_only_web_urls() {
        let dir = std::env::temp_dir().join(format!("mb-shim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("open");
        std::fs::write(&shim, OPEN_SHIM).unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o700)).unwrap();
        let urls = dir.join("urls.txt");
        let status = Command::new(&shim)
            .args(["-a", "Safari", "https://claude.com/cai/oauth/authorize?x=1&y=2"])
            .env("MB_LOGIN_URLS", &urls)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(std::fs::read_to_string(&urls).unwrap(), "https://claude.com/cai/oauth/authorize?x=1&y=2\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
