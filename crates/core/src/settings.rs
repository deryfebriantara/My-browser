//! Pengaturan pengguna di `settings.json`.

use crate::persist::{read_json, write_atomic};
use crate::sleep::SleepPolicy;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Template mesin pencari; `{}` diganti kata kunci.
    pub search_template: String,
    /// Host yang pemblokir iklannya dimatikan (tanpa awalan `www.`).
    pub blocker_exceptions: Vec<String>,
    pub sleep_idle_secs: u64,
    pub max_awake: usize,
}

impl Default for Settings {
    fn default() -> Self {
        let policy = SleepPolicy::default();
        Self {
            search_template: "https://www.google.com/search?q={}".into(),
            blocker_exceptions: Vec::new(),
            sleep_idle_secs: policy.idle_after.as_secs(),
            max_awake: policy.max_awake,
        }
    }
}

impl Settings {
    /// Berkas tidak ada atau rusak menghasilkan nilai bawaan.
    pub fn load(path: &Path) -> Settings {
        read_json(path).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self).map_err(io::Error::other)?)
    }

    pub fn sleep_policy(&self) -> SleepPolicy {
        SleepPolicy { idle_after: Duration::from_secs(self.sleep_idle_secs), max_awake: self.max_awake.max(1) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults() {
        let s = Settings::default();
        assert_eq!(s.search_template, "https://www.google.com/search?q={}");
        assert_eq!((s.sleep_idle_secs, s.max_awake), (300, 3));
        assert!(s.blocker_exceptions.is_empty());
    }

    #[test]
    fn settings_missing_fields_use_defaults() {
        let s: Settings = serde_json::from_str(r#"{"max_awake":5}"#).unwrap();
        assert_eq!((s.max_awake, s.sleep_idle_secs), (5, 300));
    }

    #[test]
    fn corrupt_settings_file_gives_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, "{bukan json").unwrap();
        assert_eq!(Settings::load(&p), Settings::default());
    }

    #[test]
    fn corrupt_settings_file_is_kept_aside_not_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, "{bukan json").unwrap();
        Settings::load(&p);
        assert!(!p.exists());
        assert_eq!(std::fs::read_to_string(d.path().join("settings.json.bad")).unwrap(), "{bukan json");
    }
}
