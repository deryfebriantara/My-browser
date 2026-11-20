//! Panel "Tanya Claude": chat di samping halaman yang dijawab Claude Code.
//!
//! Proses `claude` baru dijalankan saat pesan pertama dikirim, dan dimatikan
//! saat panel ditutup. Percakapan dilanjutkan dengan `--resume` saat panel
//! dibuka lagi, jadi tidak ada proses Claude yang memakan memory selama
//! panel tertutup. Event dari proses dibaca di thread latar lalu diteruskan
//! ke main thread lewat antrean utama GCD.

use crate::persistence;
use crate::state::{AppState, display_title};
use browser_core::claude_chat::{self as cc, ChatEvent, Launch, TabContext};
use dispatch2::DispatchQueue;
use engine_webkit::PanelView;
use objc2::MainThreadMarker;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const PANEL_HTML: &str = include_str!("chat_panel.html");
/// Baris stderr terakhir yang disimpan untuk pesan error.
const STDERR_LINES: usize = 12;

thread_local! {
    /// Hanya diisi dan dibaca di main thread.
    static STATE: RefCell<Weak<AppState>> = const { RefCell::new(Weak::new()) };
}

fn with_state(f: impl FnOnce(&Rc<AppState>)) {
    if let Some(state) = STATE.with(|s| s.borrow().upgrade()) {
        f(&state);
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Item {
    User(String),
    Assistant { text: String, tools: Vec<String> },
    Error(String),
}

impl Item {
    fn to_json(&self) -> Value {
        match self {
            Item::User(text) => json!({"role": "user", "text": text}),
            Item::Assistant { text, tools } => json!({"role": "assistant", "text": text, "tools": tools}),
            Item::Error(text) => json!({"role": "error", "text": text}),
        }
    }
}

struct Process {
    child: Child,
    stdin: ChildStdin,
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
pub struct Chat {
    panel: RefCell<Option<PanelView>>,
    items: RefCell<Vec<Item>>,
    process: RefCell<Option<Process>>,
    /// Sesi Claude Code untuk `--resume`.
    session: RefCell<Option<String>>,
    busy: Cell<bool>,
    /// Naik setiap proses diganti atau dihentikan, supaya event dari proses
    /// lama yang masih antre dibuang.
    generation: Cell<u64>,
}

impl Chat {
    pub fn is_open(&self) -> bool {
        self.panel.borrow().is_some()
    }

    pub fn toggle(&self, state: &Rc<AppState>) {
        if self.is_open() { self.close(state) } else { self.open(state) }
    }

    fn open(&self, state: &Rc<AppState>) {
        STATE.with(|s| *s.borrow_mut() = Rc::downgrade(state));
        let Some(mtm) = MainThreadMarker::new() else { return };
        let weak = Rc::downgrade(state);
        let panel = PanelView::new(
            mtm,
            PANEL_HTML,
            Box::new(move |text| {
                if let Some(state) = weak.upgrade() {
                    state.chat.handle_message(&state, &text);
                }
            }),
        );
        let view = panel.view();
        *self.panel.borrow_mut() = Some(panel);
        state.ui.set_panel(Some(view.clone()));
        state.ui.window.makeFirstResponder(Some(&view));
    }

    fn close(&self, state: &Rc<AppState>) {
        self.stop();
        let panel = self.panel.borrow_mut().take();
        state.ui.set_panel(None);
        drop(panel);
        // Fokus kembali ke halaman.
        if let Some(view) = state.ui.shown_view() {
            state.ui.window.makeFirstResponder(Some(&view));
        }
    }

    /// Dipanggil saat aplikasi keluar.
    pub fn shutdown(&self) {
        self.generation.set(self.generation.get() + 1);
        self.process.borrow_mut().take();
    }

    fn post(&self, message: Value) {
        if let Some(panel) = self.panel.borrow().as_ref() {
            panel.send(&message.to_string());
        }
    }

    fn handle_message(&self, state: &Rc<AppState>, text: &str) {
        let Ok(message) = serde_json::from_str::<Value>(text) else { return };
        match message["t"].as_str() {
            Some("ready") => {
                let items: Vec<Value> = self.items.borrow().iter().map(Item::to_json).collect();
                self.post(json!({"t": "reset", "items": items, "busy": self.busy.get()}));
                #[cfg(debug_assertions)]
                self.selftest(state, true);
            }
            Some("send") => self.send(state, message["text"].as_str().unwrap_or_default()),
            Some("stop") => self.stop(),
            Some("new") => {
                self.stop();
                self.items.borrow_mut().clear();
                *self.session.borrow_mut() = None;
                self.post(json!({"t": "reset", "items": [], "busy": false}));
            }
            Some("close") => self.close(state),
            Some("open") => {
                let url = message["url"].as_str().unwrap_or_default();
                if url.starts_with("https://") || url.starts_with("http://") {
                    state.browser.borrow_mut().open_tab(url, true, Instant::now());
                    state.refresh();
                }
            }
            _ => {}
        }
    }

    fn send(&self, state: &Rc<AppState>, text: &str) {
        let text = text.trim();
        if text.is_empty() || self.busy.get() {
            return;
        }
        self.items.borrow_mut().push(Item::User(text.to_string()));
        self.post(json!({"t": "user", "text": text}));
        let starting = self.process.borrow().is_none();
        self.items.borrow_mut().push(Item::Assistant { text: String::new(), tools: Vec::new() });
        self.busy.set(true);
        self.post(json!({"t": "start", "status": if starting { "Menyalakan Claude…" } else { "" }}));

        let tab = active_tab(state);
        let context = tab.as_ref().map(ActiveTab::as_context);
        let line = cc::user_line(&cc::with_context(text, context.as_ref()));
        if let Err(e) = self.write_line(state, &line) {
            self.fail(e);
        }
    }

    /// Mengirim satu baris ke proses Claude, menjalankannya dulu kalau belum
    /// ada. Proses yang ternyata sudah mati dijalankan ulang sekali.
    fn write_line(&self, state: &Rc<AppState>, line: &str) -> Result<(), String> {
        for _ in 0..2 {
            if self.process.borrow().is_none() {
                let process = self.spawn(state)?;
                *self.process.borrow_mut() = Some(process);
            }
            let written = {
                let mut process = self.process.borrow_mut();
                let stdin = &mut process.as_mut().expect("proses baru saja dipasang").stdin;
                writeln!(stdin, "{line}").and_then(|_| stdin.flush())
            };
            if written.is_ok() {
                return Ok(());
            }
            self.generation.set(self.generation.get() + 1);
            self.process.borrow_mut().take();
        }
        Err("Pesan tidak bisa dikirim ke Claude Code.".into())
    }

    fn spawn(&self, state: &Rc<AppState>) -> Result<Process, String> {
        let settings = state.browser.borrow().settings().clone();
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let claude = find_claude(&home, settings.claude_path.as_deref()).ok_or_else(|| {
            "Claude Code tidak ditemukan. Pasang dari https://claude.com/claude-code, atau isi claude_path \
             di settings.json dengan lokasi binary claude."
                .to_string()
        })?;
        let config_dir = settings.claude_config_dir.clone().unwrap_or_else(|| cc::DEFAULT_CONFIG_DIR.into());
        let data_dir = persistence::data_dir();
        let workdir = data_dir.join("claude-chat");
        std::fs::create_dir_all(&workdir).map_err(|e| format!("Folder kerja Claude tidak bisa dibuat: {e}"))?;
        let exe = std::env::current_exe().map_err(|e| format!("Lokasi my-browser tidak terbaca: {e}"))?;
        let session = self.session.borrow().clone();
        let args = cc::launch_args(&Launch {
            browser_exe: &exe,
            data_dir: &data_dir,
            resume: session.as_deref(),
            model: settings.claude_model.as_deref(),
        });

        let mut command = Command::new(&claude);
        command
            .args(&args)
            .current_dir(&workdir)
            .env("CLAUDE_CONFIG_DIR", cc::expand_home(&config_dir, &home))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Kalau browser dibuka dari dalam sesi Claude Code, variabel sesi itu
        // tidak boleh terbawa ke proses panel.
        for (key, _) in std::env::vars_os() {
            let key = key.to_string_lossy();
            if key == "CLAUDECODE" || key.starts_with("CLAUDE_CODE_") {
                command.env_remove(key.as_ref());
            }
        }
        let mut child = command.spawn().map_err(|e| format!("Claude Code ({}) tidak bisa dijalankan: {e}", claude.display()))?;
        let stdin = child.stdin.take().expect("stdin di-pipe");
        let stdout = child.stdout.take().expect("stdout di-pipe");
        let stderr = child.stderr.take().expect("stderr di-pipe");

        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
        let tail_writer = tail.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let mut tail = tail_writer.lock().unwrap_or_else(|e| e.into_inner());
                tail.push_back(line);
                if tail.len() > STDERR_LINES {
                    tail.pop_front();
                }
            }
        });
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let events = cc::parse_line(&line);
                if !events.is_empty() {
                    DispatchQueue::main().exec_async(move || with_state(|s| s.chat.deliver(generation, events)));
                }
            }
            // Beri thread stderr kesempatan menyelesaikan baris terakhirnya.
            std::thread::sleep(std::time::Duration::from_millis(100));
            let tail = tail.lock().map(|t| Vec::from(t.clone()).join("\n")).unwrap_or_default();
            DispatchQueue::main().exec_async(move || with_state(|s| s.chat.exited(generation, tail)));
        });
        Ok(Process { child, stdin })
    }

    fn deliver(&self, generation: u64, events: Vec<ChatEvent>) {
        if generation != self.generation.get() {
            return;
        }
        for event in events {
            match event {
                ChatEvent::Session(id) => *self.session.borrow_mut() = Some(id),
                ChatEvent::TextStart => {
                    if self.assistant_text_len() > 0 {
                        self.append_text("\n\n");
                    }
                }
                ChatEvent::Text(text) => self.append_text(&text),
                ChatEvent::Tool(label) => {
                    if let Some(Item::Assistant { tools, .. }) = self.items.borrow_mut().last_mut() {
                        tools.push(label.clone());
                    }
                    self.post(json!({"t": "tool", "label": label}));
                }
                ChatEvent::Done { error } => {
                    if !self.busy.replace(false) {
                        continue;
                    }
                    let error = error.map(|e| self.with_hint(e));
                    self.drop_empty_answer();
                    if let Some(e) = &error {
                        self.items.borrow_mut().push(Item::Error(e.clone()));
                    }
                    self.post(json!({"t": "done", "error": error}));
                    #[cfg(debug_assertions)]
                    with_state(|s| self.selftest(s, false));
                }
            }
        }
    }

    /// Proses Claude selesai atau mati.
    fn exited(&self, generation: u64, stderr: String) {
        if generation != self.generation.get() {
            return;
        }
        self.process.borrow_mut().take();
        if self.busy.get() {
            let detail = if stderr.trim().is_empty() { String::new() } else { format!("\n\n{}", stderr.trim()) };
            self.fail(format!("Claude Code berhenti sebelum selesai menjawab.{detail}"));
        }
    }

    /// Menghentikan jawaban yang sedang berjalan dengan mematikan prosesnya;
    /// pesan berikutnya melanjutkan sesi yang sama lewat `--resume`.
    fn stop(&self) {
        self.generation.set(self.generation.get() + 1);
        self.process.borrow_mut().take();
        if self.busy.replace(false) {
            if let Some(Item::Assistant { text, .. }) = self.items.borrow_mut().last_mut() {
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str("*(dihentikan)*");
            }
            self.post(json!({"t": "done", "stopped": true}));
        }
    }

    fn fail(&self, error: String) {
        self.busy.set(false);
        self.drop_empty_answer();
        let error = self.with_hint(error);
        self.items.borrow_mut().push(Item::Error(error.clone()));
        self.post(json!({"t": "done", "error": error}));
    }

    /// Uji otomatis, hanya di build debug: `MYBROWSER_CHAT_SELFTEST` menunjuk
    /// berkas berisi pertanyaan satu per baris. Setiap kali panel dibuka, satu
    /// pertanyaan berikutnya dikirim; transkrip ditulis ke `<berkas>.out`
    /// setiap panel siap dan setiap giliran selesai.
    #[cfg(debug_assertions)]
    fn selftest(&self, state: &Rc<AppState>, ready: bool) {
        let Some(path) = std::env::var_os("MYBROWSER_CHAT_SELFTEST").map(PathBuf::from) else { return };
        let items: Vec<Value> = self.items.borrow().iter().map(Item::to_json).collect();
        let _ = std::fs::write(path.with_extension("out"), Value::Array(items).to_string());
        if !ready || self.busy.get() {
            return;
        }
        let asked = self.items.borrow().iter().filter(|i| matches!(i, Item::User(_))).count();
        let questions = std::fs::read_to_string(&path).unwrap_or_default();
        if let Some(question) = questions.lines().filter(|l| !l.trim().is_empty()).nth(asked) {
            self.send(state, question);
        }
    }

    fn with_hint(&self, error: String) -> String {
        let config_dir = STATE
            .with(|s| s.borrow().upgrade())
            .and_then(|s| s.browser.borrow().settings().claude_config_dir.clone())
            .unwrap_or_else(|| cc::DEFAULT_CONFIG_DIR.into());
        match cc::error_hint(&error, &config_dir) {
            Some(hint) => format!("{error}\n\n{hint}"),
            None => error,
        }
    }

    fn assistant_text_len(&self) -> usize {
        match self.items.borrow().last() {
            Some(Item::Assistant { text, .. }) => text.len(),
            _ => 0,
        }
    }

    fn append_text(&self, chunk: &str) {
        if let Some(Item::Assistant { text, .. }) = self.items.borrow_mut().last_mut() {
            text.push_str(chunk);
        }
        self.post(json!({"t": "delta", "text": chunk}));
    }

    /// Jawaban kosong (gagal sebelum ada teks atau tool) tidak disimpan.
    fn drop_empty_answer(&self) {
        let mut items = self.items.borrow_mut();
        if matches!(items.last(), Some(Item::Assistant { text, tools }) if text.is_empty() && tools.is_empty()) {
            items.pop();
        }
    }
}

/// Tab aktif saat pertanyaan dikirim.
struct ActiveTab {
    id: u64,
    title: String,
    url: Option<String>,
    picked: bool,
}

impl ActiveTab {
    fn as_context(&self) -> TabContext<'_> {
        TabContext { id: self.id, title: &self.title, url: self.url.as_deref(), picked: self.picked }
    }
}

fn active_tab(state: &AppState) -> Option<ActiveTab> {
    let b = state.browser.borrow();
    let tab = b.tabs().get(b.tabs().active())?;
    Some(ActiveTab {
        id: tab.id.0,
        title: display_title(tab),
        url: tab.url.as_ref().map(|u| u.to_string()),
        picked: state.picked_tab.get() == Some(tab.id),
    })
}

/// Lokasi `claude` yang pertama ada.
fn find_claude(home: &Path, configured: Option<&str>) -> Option<PathBuf> {
    let versions: Vec<String> = std::fs::read_dir(home.join(".nvm/versions/node"))
        .map(|dir| dir.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    cc::claude_candidates(home, configured, &versions).into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_items_serialize_for_the_panel() {
        let items = [
            Item::User("halo".into()),
            Item::Assistant { text: "hai".into(), tools: vec!["Membaca halaman".into()] },
            Item::Error("gagal".into()),
        ];
        let json: Vec<Value> = items.iter().map(Item::to_json).collect();
        assert_eq!(json[0], json!({"role": "user", "text": "halo"}));
        assert_eq!(json[1]["tools"][0], "Membaca halaman");
        assert_eq!(json[2]["role"], "error");
    }

    #[test]
    fn find_claude_uses_configured_file_when_it_exists() {
        let dir = std::env::temp_dir().join(format!("mb-claude-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("claude");
        std::fs::write(&fake, "").unwrap();
        assert_eq!(find_claude(Path::new("/tidak/ada"), Some(fake.to_str().unwrap())), Some(fake.clone()));
        assert_ne!(find_claude(Path::new("/tidak/ada"), Some("/tidak/ada/claude")), Some(PathBuf::from("/tidak/ada/claude")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
