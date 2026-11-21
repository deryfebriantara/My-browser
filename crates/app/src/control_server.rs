//! Socket kontrol untuk server MCP. Thread latar menerima koneksi; setiap
//! permintaan dijalankan di main thread lewat antrean utama GCD, lalu
//! balasannya ditulis kembali ke koneksi.
//!
//! Setiap koneksi dikenali dari kernel (pid peer dan lokasi program) dan
//! setiap permintaan dicek izinnya (`mcp_access`): program yang belum
//! diizinkan memunculkan dialog, panel Tanya Claude hanya boleh membaca.

use crate::control;
use crate::procinfo::{self, Client};
use crate::state::AppState;
use browser_core::control::{ControlRequest, ControlResponse};
use browser_core::mcp_access::ClientKind;
use dispatch2::DispatchQueue;
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// Batas waktu satu permintaan (script halaman yang macet, dialog terbuka).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(40);
/// Lama menunggu pengguna menjawab dialog izin, sedikit di bawah batas
/// baca server MCP (45 detik) supaya balasannya masih sampai.
const APPROVAL_WAIT: Duration = Duration::from_secs(35);

static NEXT_CONNECTION: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Hanya diisi dan dibaca di main thread.
    static STATE: RefCell<Weak<AppState>> = const { RefCell::new(Weak::new()) };
}

/// Membuat socket di `path`. Folder data dibuat dulu kalau belum ada
/// (peluncuran pertama), dan socket lama (sisa browser yang tertutup tidak
/// wajar) dihapus.
fn bind(path: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Mulai mendengarkan di `path`.
pub fn start(state: &Rc<AppState>, path: PathBuf) {
    STATE.with(|s| *s.borrow_mut() = Rc::downgrade(state));
    let listener = match bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("my-browser: socket kontrol {} tidak bisa dibuat: {e}", path.display());
            return;
        }
    };
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
    let id = NEXT_CONNECTION.fetch_add(1, Ordering::Relaxed) + 1;
    let me = std::process::id() as libc::pid_t;
    // Program yang tidak bisa dikenali tidak dilayani.
    let Some(client) = procinfo::peer_pid(&conn).and_then(|pid| procinfo::identify(pid, me, id)) else { return };
    let Ok(reader) = conn.try_clone() else { return };
    let mut writer = conn;
    for line in BufReader::new(reader).lines() {
        let Ok(line) = line else { break };
        let response = match serde_json::from_str::<ControlRequest>(&line) {
            Ok(request) => match authorize(&client) {
                Ok(()) => dispatch(request, client.kind.clone()),
                Err(e) => ControlResponse::Error(e),
            },
            Err(e) => ControlResponse::Error(format!("Permintaan tidak valid: {e}")),
        };
        let Ok(text) = serde_json::to_string(&response) else { break };
        if writeln!(writer, "{text}").and_then(|_| writer.flush()).is_err() {
            break;
        }
    }
    DispatchQueue::main().exec_async(move || {
        if let Some(state) = STATE.with(|s| s.borrow().upgrade()) {
            state.mcp_access.disconnected(id);
        }
    });
}

/// Izin untuk satu permintaan. Kalau pengguna perlu ditanya, menunggu
/// jawabannya paling lama `APPROVAL_WAIT`; jawaban yang datang belakangan
/// tetap berlaku untuk permintaan berikutnya.
fn authorize(client: &Client) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let client = client.clone();
    DispatchQueue::main().exec_async(move || {
        let state = STATE.with(|s| s.borrow().upgrade());
        match state {
            Some(state) => state.mcp_access.request(&state, client, tx),
            None => {
                let _ = tx.send(Err("my-browser sedang ditutup.".into()));
            }
        }
    });
    rx.recv_timeout(APPROVAL_WAIT).unwrap_or_else(|_| {
        Err("Menunggu izin di my-browser: setujui dialog yang muncul di browser, lalu coba lagi.".into())
    })
}

fn dispatch(request: ControlRequest, kind: ClientKind) -> ControlResponse {
    let (tx, rx) = mpsc::channel();
    DispatchQueue::main().exec_async(move || {
        let state = STATE.with(|s| s.borrow().upgrade());
        let Some(state) = state else {
            let _ = tx.send(ControlResponse::Error("my-browser sedang ditutup.".into()));
            return;
        };
        objc2::rc::autoreleasepool(|_| {
            control::handle(&state, request, &kind, Box::new(move |response| {
                let _ = tx.send(response);
            }));
        });
    });
    rx.recv_timeout(REQUEST_TIMEOUT)
        .unwrap_or_else(|_| ControlResponse::Error("my-browser tidak menjawab dalam 40 detik.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_creates_missing_data_dir_and_replaces_stale_socket() {
        let base = std::env::temp_dir().join(format!("mb-sock-{}", std::process::id()));
        let path = base.join("data").join("control.sock");
        let _ = std::fs::remove_dir_all(&base);
        let first = bind(&path).expect("folder data yang belum ada dibuat");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        drop(first);
        let _second = bind(&path).expect("socket sisa dihapus lalu dibuat ulang");
        assert!(UnixStream::connect(&path).is_ok());
        let _ = std::fs::remove_dir_all(&base);
    }
}
