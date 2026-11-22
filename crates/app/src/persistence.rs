//! Lokasi dan penyimpanan `settings.json`, `session.json`, dan `history.json`.
//! `MYBROWSER_DATA_DIR` mengganti lokasi bawaan (dipakai saat pengujian).

use browser_core::history::History;
use browser_core::persist::{app_support_dir, write_private};
use browser_core::session::SessionFile;
use browser_core::settings::Settings;
use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MYBROWSER_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    app_support_dir(Path::new(&home))
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

fn session_path() -> PathBuf {
    data_dir().join("session.json")
}

fn history_path() -> PathBuf {
    data_dir().join("history.json")
}

pub fn load() -> (Settings, Option<SessionFile>, History) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    (Settings::load(&settings_path()), SessionFile::load(&session_path()), History::load(&history_path(), now))
}

/// Menyimpan riwayat dengan izin 0600 (hanya pengguna ini).
pub fn save_history(bytes: &[u8]) {
    if let Err(e) = write_private(&history_path(), bytes) {
        eprintln!("my-browser: gagal menyimpan riwayat: {e}");
    }
}

/// Setelah seluruh riwayat dihapus: salinan lama yang disisihkan karena
/// rusak juga dibuang.
pub fn remove_history_leftovers() {
    for suffix in [".bad", ".tmp"] {
        let mut path = history_path().into_os_string();
        path.push(suffix);
        let _ = std::fs::remove_file(path);
    }
}

pub fn save_session(session: &SessionFile) {
    if let Err(e) = session.save(&session_path()) {
        eprintln!("my-browser: gagal menyimpan sesi: {e}");
    }
}

pub fn save_settings(settings: &Settings) {
    if let Err(e) = settings.save(&settings_path()) {
        eprintln!("my-browser: gagal menyimpan pengaturan: {e}");
    }
}
