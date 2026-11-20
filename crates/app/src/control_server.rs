//! Socket kontrol untuk server MCP. Thread latar menerima koneksi; setiap
//! permintaan dijalankan di main thread lewat antrean utama GCD, lalu
//! balasannya ditulis kembali ke koneksi.

use crate::control;
use crate::state::AppState;
use browser_core::control::{ControlRequest, ControlResponse};
use dispatch2::DispatchQueue;
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::mpsc;
use std::time::Duration;

/// Batas waktu satu permintaan (script halaman yang macet, dialog terbuka).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(40);

thread_local! {
    /// Hanya diisi dan dibaca di main thread.
    static STATE: RefCell<Weak<AppState>> = const { RefCell::new(Weak::new()) };
}

/// Mulai mendengarkan di `path`. Socket lama (sisa browser yang tertutup
/// tidak wajar) dihapus dulu.
pub fn start(state: &Rc<AppState>, path: PathBuf) {
    STATE.with(|s| *s.borrow_mut() = Rc::downgrade(state));
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("my-browser: socket kontrol {} tidak bisa dibuat: {e}", path.display());
            return;
        }
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            if same_user(&conn) {
                std::thread::spawn(move || serve(conn));
            }
        }
    });
}

/// Menghapus file socket saat aplikasi keluar.
pub fn stop(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Hanya proses milik pengguna yang sama yang boleh mengontrol browser.
fn same_user(conn: &UnixStream) -> bool {
    let (mut uid, mut gid) = (0, 0);
    let rc = unsafe { libc::getpeereid(conn.as_raw_fd(), &mut uid, &mut gid) };
    rc == 0 && uid == unsafe { libc::getuid() }
}

fn serve(conn: UnixStream) {
    let Ok(reader) = conn.try_clone() else { return };
    let mut writer = conn;
    for line in BufReader::new(reader).lines() {
        let Ok(line) = line else { break };
        let response = match serde_json::from_str::<ControlRequest>(&line) {
            Ok(request) => dispatch(request),
            Err(e) => ControlResponse::Error(format!("Permintaan tidak valid: {e}")),
        };
        let Ok(text) = serde_json::to_string(&response) else { break };
        if writeln!(writer, "{text}").and_then(|_| writer.flush()).is_err() {
            break;
        }
    }
}

fn dispatch(request: ControlRequest) -> ControlResponse {
    let (tx, rx) = mpsc::channel();
    DispatchQueue::main().exec_async(move || {
        let state = STATE.with(|s| s.borrow().upgrade());
        let Some(state) = state else {
            let _ = tx.send(ControlResponse::Error("my-browser sedang ditutup.".into()));
            return;
        };
        objc2::rc::autoreleasepool(|_| {
            control::handle(&state, request, Box::new(move |response| {
                let _ = tx.send(response);
            }));
        });
    });
    rx.recv_timeout(REQUEST_TIMEOUT)
        .unwrap_or_else(|_| ControlResponse::Error("my-browser tidak menjawab dalam 40 detik.".into()))
}
