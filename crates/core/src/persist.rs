//! Lokasi data pengguna dan penulisan berkas yang aman dari crash.

use std::io;
use std::path::{Path, PathBuf};

/// `<home>/Library/Application Support/my-browser`
pub fn app_support_dir(home: &Path) -> PathBuf {
    home.join("Library").join("Application Support").join("my-browser")
}

/// Menulis ke `<path>.tmp` lalu `rename`, supaya berkas lama tetap utuh kalau
/// aplikasi mati di tengah penulisan. Direktori induk dibuat kalau belum ada.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Membaca dan mengurai JSON. Berkas yang tidak ada menghasilkan `None` tanpa
/// peringatan; berkas rusak menghasilkan `None` dengan satu baris di stderr.
pub(crate) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!("my-browser: gagal membaca {}: {e}", path.display());
            return None;
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("my-browser: {} rusak, memakai nilai bawaan: {e}", path.display());
            None
        }
    }
}
