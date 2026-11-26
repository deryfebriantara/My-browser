//! Lokasi dan penyimpanan `settings.json`, `spaces.json`, dan `history.json`.
//! `session.json` lama hanya dibaca untuk diimpor dan tidak pernah ditulis.
//! `MYBROWSER_DATA_DIR` mengganti lokasi bawaan (dipakai saat pengujian).

use browser_core::history::History;
use browser_core::persist::{app_support_dir, write_private};
use browser_core::session::SessionFile;
use browser_core::settings::Settings;
use browser_core::spaces_file::{LoadedSpaces, SpacesFile};
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

fn spaces_path() -> PathBuf {
    data_dir().join("spaces.json")
}

fn history_path() -> PathBuf {
    data_dir().join("history.json")
}

/// Semua yang dimuat saat aplikasi dibuka.
pub struct Loaded {
    pub settings: Settings,
    pub spaces: SpacesFile,
    /// `spaces.json` ditulis versi my-browser yang lebih baru (berisi versi
    /// formatnya). Selama ada, status tab tidak disimpan.
    pub newer_spaces: Option<u32>,
    pub history: History,
}

pub fn load() -> Loaded {
    load_from(&data_dir())
}

/// `spaces.json` dipakai kalau ada; kalau tidak, `session.json` diimpor
/// (tanpa diubah), atau mulai dengan satu Space kosong.
pub fn load_from(dir: &Path) -> Loaded {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let import = || match SessionFile::load(&dir.join("session.json")) {
        Some(session) => SpacesFile::from_session(&session),
        None => SpacesFile::personal_default(),
    };
    let (spaces, newer_spaces) = match SpacesFile::load(&dir.join("spaces.json")) {
        LoadedSpaces::Loaded(file) => (file, None),
        LoadedSpaces::Missing => (import(), None),
        LoadedSpaces::Newer(version) => (import(), Some(version)),
    };
    Loaded {
        settings: Settings::load(&dir.join("settings.json")),
        spaces,
        newer_spaces,
        history: History::load(&dir.join("history.json"), now),
    }
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

/// Menyimpan status Space dan tab dengan izin 0600.
pub fn save_spaces(file: &SpacesFile) {
    if let Err(e) = file.save(&spaces_path()) {
        eprintln!("my-browser: gagal menyimpan spaces.json: {e}");
    }
}

pub fn save_settings(settings: &Settings) {
    if let Err(e) = settings.save(&settings_path()) {
        eprintln!("my-browser: gagal menyimpan pengaturan: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &[u8] =
        br#"{"version":1,"active_index":0,"tabs":[{"url":"https://a.example/","title":"A","scroll_y":0.0}]}"#;

    #[test]
    fn session_json_is_imported_and_left_untouched() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("session.json"), SESSION).unwrap();
        let loaded = load_from(d.path());
        assert_eq!(loaded.spaces.spaces[0].tabs[0].url, "https://a.example/");
        assert_eq!(loaded.newer_spaces, None);
        assert_eq!(std::fs::read(d.path().join("session.json")).unwrap(), SESSION);
        assert!(!d.path().join("spaces.json").exists());
    }

    #[test]
    fn newer_spaces_file_sets_newer_spaces() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("session.json"), SESSION).unwrap();
        std::fs::write(d.path().join("spaces.json"), br#"{"version":2,"active_space":0,"spaces":[]}"#).unwrap();
        let loaded = load_from(d.path());
        assert_eq!(loaded.newer_spaces, Some(2));
        assert_eq!(loaded.spaces.spaces[0].tabs[0].url, "https://a.example/");
    }

    #[test]
    fn existing_spaces_json_wins_over_session_json() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("session.json"), SESSION).unwrap();
        SpacesFile::personal_default().save(&d.path().join("spaces.json")).unwrap();
        let loaded = load_from(d.path());
        assert!(loaded.spaces.spaces[0].tabs.is_empty());
    }
}
