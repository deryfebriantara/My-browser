//! Lokasi dan penyimpanan `settings.json` dan `session.json`.
//! `MYBROWSER_DATA_DIR` mengganti lokasi bawaan (dipakai saat pengujian).

use browser_core::persist::app_support_dir;
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

pub fn load() -> (Settings, Option<SessionFile>) {
    (Settings::load(&settings_path()), SessionFile::load(&session_path()))
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
