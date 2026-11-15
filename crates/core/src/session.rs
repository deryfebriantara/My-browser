//! Sesi tab di `session.json`, dipulihkan saat aplikasi dibuka lagi.

use crate::persist::{read_json, write_atomic};
use crate::tabs::{TabList, Url};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;
use std::time::Instant;

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

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self).map_err(io::Error::other)?)
    }
}

impl TabList {
    /// Tab kosong (tanpa URL) tidak disimpan.
    pub fn to_session(&self) -> SessionFile {
        let mut active_index = None;
        let mut tabs = Vec::new();
        for tab in self.iter() {
            let Some(url) = &tab.url else { continue };
            if tab.id == self.active() {
                active_index = Some(tabs.len());
            }
            tabs.push(SavedTab { url: url.to_string(), title: tab.title.clone(), scroll_y: tab.scroll_y });
        }
        SessionFile { version: SESSION_VERSION, active_index, tabs }
    }

    /// Semua tab dipulihkan dalam keadaan tidur. Sesi kosong menghasilkan satu
    /// tab kosong. URL yang tidak valid dilewati.
    pub fn from_session(s: &SessionFile, now: Instant) -> TabList {
        let mut list = TabList::new(now);
        let blank = list.active();
        let mut active = None;
        for (i, saved) in s.tabs.iter().enumerate() {
            let Ok(url) = Url::parse(&saved.url) else { continue };
            let id = list.open(Some(url), now, true);
            let tab = list.get_mut(id).expect("tab baru ada");
            tab.title = saved.title.clone();
            tab.scroll_y = saved.scroll_y;
            tab.restore_scroll = saved.scroll_y > 0.0;
            if s.active_index == Some(i) {
                active = Some(id);
            }
        }
        if list.iter().count() > 1 {
            list.close(blank, now);
            let target = active.unwrap_or_else(|| list.iter().next().expect("tidak kosong").id);
            list.activate(target, now);
        }
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::{TabList, Url};
    use std::time::Instant;

    fn u(s: &str) -> Url { Url::parse(s).unwrap() }

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

    #[test]
    fn session_roundtrip_restores_all_tabs_asleep() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        let a = l.active();
        l.get_mut(a).unwrap().url = Some(u("https://a.com/"));
        let b = l.open(Some(u("https://b.com/")), t0, true);
        let c = l.open(Some(u("https://c.com/")), t0, false);
        for (id, title, y) in [(a, "A", 0.0), (b, "B", 120.0), (c, "C", 0.0)] {
            let t = l.get_mut(id).unwrap();
            t.title = title.into();
            t.scroll_y = y;
            t.awake = true;
        }
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("session.json");
        l.to_session().save(&p).unwrap();
        let back = TabList::from_session(&SessionFile::load(&p).unwrap(), t0);
        let got: Vec<(String, String, f64, bool)> = back
            .iter()
            .map(|t| (t.url.as_ref().unwrap().to_string(), t.title.clone(), t.scroll_y, t.awake))
            .collect();
        assert_eq!(
            got,
            vec![
                ("https://a.com/".into(), "A".into(), 0.0, false),
                ("https://b.com/".into(), "B".into(), 120.0, false),
                ("https://c.com/".into(), "C".into(), 0.0, false),
            ]
        );
        assert_eq!(back.get(back.active()).unwrap().title, "B");
    }

    #[test]
    fn session_skips_blank_tabs() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        l.open(Some(u("https://a.com/")), t0, false);
        assert_eq!(l.to_session().tabs.len(), 1);
    }

    #[test]
    fn empty_session_gives_one_blank_tab() {
        let s = SessionFile { version: SESSION_VERSION, active_index: None, tabs: vec![] };
        let l = TabList::from_session(&s, Instant::now());
        assert_eq!(l.iter().count(), 1);
        assert!(l.get(l.active()).unwrap().url.is_none());
    }

    #[test]
    fn save_leaves_no_tmp_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("session.json");
        SessionFile { version: SESSION_VERSION, active_index: None, tabs: vec![] }.save(&p).unwrap();
        let names: Vec<String> = std::fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names, vec!["session.json".to_string()]);
    }
}
