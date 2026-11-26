//! Format `session.json` lama. Hanya dibaca sekali untuk diimpor ke
//! `spaces.json`; berkasnya tidak pernah ditulis lagi, supaya build lama masih
//! bisa membukanya.

use crate::persist::read_json;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SESSION_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionFile {
    pub version: u32,
    pub active_index: Option<usize>,
    pub tabs: Vec<SavedTab>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub url: String,
    pub title: String,
    pub scroll_y: f64,
}

impl SessionFile {
    /// Berkas tidak ada, rusak, atau versinya lain menghasilkan `None`.
    pub fn load(path: &Path) -> Option<SessionFile> {
        let s: SessionFile = read_json(path)?;
        if s.version != SESSION_VERSION {
            eprintln!("my-browser: versi sesi {} tidak dikenal, sesi diabaikan", s.version);
            return None;
        }
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_or_old_session_is_none() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("session.json");
        std::fs::write(&p, r#"{"version":1,"tabs":[{"url":"https://a.com""#).unwrap();
        assert!(SessionFile::load(&p).is_none());
        std::fs::write(&p, r#"{"version":99,"active_index":null,"tabs":[]}"#).unwrap();
        assert!(SessionFile::load(&p).is_none());
    }

    #[test]
    fn missing_session_is_none() {
        let d = tempfile::tempdir().unwrap();
        assert!(SessionFile::load(&d.path().join("session.json")).is_none());
    }
}
