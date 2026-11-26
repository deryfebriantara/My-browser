//! Status Space dan tab di `spaces.json` (menggantikan `session.json`).
//!
//! Tahap ini hanya punya satu Space (Personal). Formatnya sudah memuat daftar
//! Space supaya tahap berikutnya tidak perlu mengganti format.

use crate::persist::{read_json, write_private};
use crate::session::SessionFile;
use crate::tabs::{TabKind, TabList, Url};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;
use std::time::Instant;

pub const SPACES_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpacesFile {
    pub version: u32,
    pub active_space: usize,
    pub spaces: Vec<SavedSpace>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedSpace {
    pub id: u64,
    pub name: String,
    pub theme: String,
    pub store: StoreRef,
    pub active_index: Option<usize>,
    pub tabs: Vec<SpaceTab>,
}

/// Penyimpanan data situs (cookie, login) milik Space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreRef {
    /// `WKWebsiteDataStore.defaultDataStore` (login yang sudah ada).
    Default,
    /// `dataStoreForIdentifier` dengan UUID ini (dipakai mulai tahap 2).
    Identified(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceTab {
    pub kind: TabKind,
    pub url: String,
    pub title: String,
    pub scroll_y: f64,
    #[serde(default)]
    pub saved_url: Option<String>,
    #[serde(default)]
    pub saved_title: String,
}

/// Identitas Space yang sedang dipakai (tanpa tab).
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceMeta {
    pub id: u64,
    pub name: String,
    pub theme: String,
    pub store: StoreRef,
}

pub enum LoadedSpaces {
    Loaded(SpacesFile),
    /// Tidak ada, atau rusak (berkas rusak disisihkan sebagai `.bad`).
    Missing,
    /// Ditulis versi my-browser yang lebih baru: berkas dibiarkan apa adanya.
    Newer(u32),
}

#[derive(Deserialize)]
struct VersionOnly {
    version: u32,
}

impl SavedSpace {
    pub fn meta(&self) -> SpaceMeta {
        SpaceMeta { id: self.id, name: self.name.clone(), theme: self.theme.clone(), store: self.store.clone() }
    }
}

impl SpacesFile {
    pub fn load(path: &Path) -> LoadedSpaces {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return LoadedSpaces::Missing,
            Err(e) => {
                eprintln!("my-browser: gagal membaca {}: {e}", path.display());
                return LoadedSpaces::Missing;
            }
        };
        // Versi dibaca dulu: berkas dari versi lebih baru tidak boleh
        // disisihkan sebagai rusak hanya karena ada kolom yang belum dikenal.
        if let Ok(v) = serde_json::from_slice::<VersionOnly>(&bytes)
            && v.version > SPACES_VERSION
        {
            return LoadedSpaces::Newer(v.version);
        }
        match read_json::<SpacesFile>(path) {
            Some(file) => LoadedSpaces::Loaded(file),
            None => LoadedSpaces::Missing,
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        write_private(path, &serde_json::to_vec_pretty(self).map_err(io::Error::other)?)
    }

    /// Impor `session.json` lama: satu Space Personal, semua tab di Today.
    pub fn from_session(s: &SessionFile) -> SpacesFile {
        let tabs = s
            .tabs
            .iter()
            .map(|t| SpaceTab {
                kind: TabKind::Today,
                url: t.url.clone(),
                title: t.title.clone(),
                scroll_y: t.scroll_y,
                saved_url: None,
                saved_title: String::new(),
            })
            .collect();
        let mut file = Self::personal_default();
        file.spaces[0].active_index = s.active_index;
        file.spaces[0].tabs = tabs;
        file
    }

    /// Satu Space Personal tanpa tab.
    pub fn personal_default() -> SpacesFile {
        SpacesFile {
            version: SPACES_VERSION,
            active_space: 0,
            spaces: vec![SavedSpace {
                id: 1,
                name: "Personal".into(),
                theme: "personal".into(),
                store: StoreRef::Default,
                active_index: None,
                tabs: Vec::new(),
            }],
        }
    }
}

impl TabList {
    /// Semua tab dipulihkan dalam keadaan tidur. URL yang tidak valid
    /// dilewati; Space tanpa tab menghasilkan satu tab kosong.
    pub fn from_space(s: &SavedSpace, now: Instant) -> TabList {
        let mut list = TabList::new(now);
        let mut active = None;
        let mut first = None;
        // Tab baru masuk di paling atas bagiannya, jadi dibuka dari belakang.
        for (i, saved) in s.tabs.iter().enumerate().rev() {
            let Ok(url) = Url::parse(&saved.url) else { continue };
            let id = list.open(Some(url), now, true);
            if saved.kind != TabKind::Today {
                list.place(id, saved.kind, 0);
            }
            let tab = list.get_mut(id).expect("tab baru ada");
            tab.title = saved.title.clone();
            tab.scroll_y = saved.scroll_y;
            tab.restore_scroll = saved.scroll_y > 0.0;
            if saved.kind != TabKind::Today {
                tab.saved_url = saved.saved_url.as_deref().and_then(|u| Url::parse(u).ok()).or(tab.url.clone());
                tab.saved_title = saved.saved_title.clone();
            }
            if s.active_index == Some(i) {
                active = Some(id);
            }
            first = Some(id);
        }
        if let Some(target) = active.or(first) {
            list.activate(target, now);
        }
        list
    }

    /// Tab kosong tidak disimpan.
    pub fn to_space(&self, meta: &SpaceMeta) -> SavedSpace {
        let mut active_index = None;
        let mut tabs = Vec::new();
        for tab in self.iter() {
            let Some(url) = &tab.url else { continue };
            if tab.id == self.active() {
                active_index = Some(tabs.len());
            }
            tabs.push(SpaceTab {
                kind: tab.kind,
                url: url.to_string(),
                title: tab.title.clone(),
                scroll_y: tab.scroll_y,
                saved_url: tab.saved_url.as_ref().map(|u| u.to_string()),
                saved_title: tab.saved_title.clone(),
            });
        }
        SavedSpace {
            id: meta.id,
            name: meta.name.clone(),
            theme: meta.theme.clone(),
            store: meta.store.clone(),
            active_index,
            tabs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{SESSION_VERSION, SavedTab, SessionFile};
    use crate::tabs::{TabKind, TabList, Url};
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn meta() -> SpaceMeta {
        SpaceMeta { id: 1, name: "Personal".into(), theme: "personal".into(), store: StoreRef::Default }
    }

    /// [Pinned p, Today a, Today b], b aktif.
    fn sample(t0: Instant) -> TabList {
        let mut l = TabList::new(t0);
        let b = l.open(Some(u("https://b.example/")), t0, true);
        let a = l.open(Some(u("https://a.example/")), t0, true);
        let p = l.open(Some(u("https://p.example/")), t0, true);
        l.get_mut(p).unwrap().title = "P".into();
        l.place(p, TabKind::Pinned, 0);
        l.get_mut(p).unwrap().url = Some(u("https://p.example/now"));
        l.get_mut(a).unwrap().scroll_y = 120.0;
        l.activate(b, t0);
        l
    }

    fn file_with(tabs: &TabList) -> SpacesFile {
        SpacesFile { version: SPACES_VERSION, active_space: 0, spaces: vec![tabs.to_space(&meta())] }
    }

    #[test]
    fn round_trip_keeps_kinds_saved_urls_and_active_index() {
        let t0 = Instant::now();
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("spaces.json");
        file_with(&sample(t0)).save(&path).unwrap();
        let LoadedSpaces::Loaded(back) = SpacesFile::load(&path) else { panic!("harus Loaded") };
        let space = &back.spaces[0];
        assert_eq!(space.active_index, Some(2));
        let kinds: Vec<TabKind> = space.tabs.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TabKind::Pinned, TabKind::Today, TabKind::Today]);
        assert_eq!(space.tabs[0].url, "https://p.example/now");
        assert_eq!(space.tabs[0].saved_url.as_deref(), Some("https://p.example/"));
        assert_eq!(space.tabs[0].saved_title, "P");
        let list = TabList::from_space(space, t0);
        let p = list.iter().next().unwrap();
        assert_eq!(p.kind, TabKind::Pinned);
        assert_eq!(p.saved_url.as_ref().unwrap().as_str(), "https://p.example/");
        assert_eq!(list.get(list.active()).unwrap().url.as_ref().unwrap().as_str(), "https://b.example/");
        assert!(list.iter().all(|t| !t.awake));
        let a = list.iter().nth(1).unwrap();
        assert_eq!(a.scroll_y, 120.0);
        assert!(a.restore_scroll);
    }

    #[test]
    fn saved_file_is_private_0600() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("spaces.json");
        SpacesFile::personal_default().save(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn missing_file_is_missing() {
        let d = tempfile::tempdir().unwrap();
        assert!(matches!(SpacesFile::load(&d.path().join("spaces.json")), LoadedSpaces::Missing));
    }

    #[test]
    fn corrupt_file_is_set_aside_and_missing() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("spaces.json");
        std::fs::write(&path, b"{\"version\":1,\"spa").unwrap();
        assert!(matches!(SpacesFile::load(&path), LoadedSpaces::Missing));
        assert!(d.path().join("spaces.json.bad").exists());
    }

    #[test]
    fn newer_version_is_reported_and_left_untouched() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("spaces.json");
        let bytes = br#"{"version":2,"active_space":0,"spaces":[],"future":true}"#;
        std::fs::write(&path, bytes).unwrap();
        assert!(matches!(SpacesFile::load(&path), LoadedSpaces::Newer(2)));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!d.path().join("spaces.json.bad").exists());
    }

    #[test]
    fn import_from_session_puts_tabs_in_personal_today() {
        let saved = |url: &str, title: &str| SavedTab { url: url.into(), title: title.into(), scroll_y: 0.0 };
        let s = SessionFile {
            version: SESSION_VERSION,
            active_index: Some(1),
            tabs: vec![saved("https://a.example/", "A"), saved("https://b.example/", "B")],
        };
        let f = SpacesFile::from_session(&s);
        assert_eq!(f.version, SPACES_VERSION);
        assert_eq!(f.active_space, 0);
        let space = &f.spaces[0];
        assert_eq!((space.id, space.name.as_str(), space.theme.as_str()), (1, "Personal", "personal"));
        assert_eq!(space.store, StoreRef::Default);
        assert_eq!(space.active_index, Some(1));
        assert!(space.tabs.iter().all(|t| t.kind == TabKind::Today && t.saved_url.is_none()));
        assert_eq!(space.tabs[1].title, "B");
    }

    #[test]
    fn import_skips_invalid_urls_and_bad_active_index() {
        let saved = |url: &str| SavedTab { url: url.into(), title: String::new(), scroll_y: 0.0 };
        let s = SessionFile {
            version: SESSION_VERSION,
            active_index: Some(7),
            tabs: vec![saved("not a url"), saved("https://a.example/")],
        };
        let t0 = Instant::now();
        let list = TabList::from_space(&SpacesFile::from_session(&s).spaces[0], t0);
        assert_eq!(list.visible().count(), 1);
        assert_eq!(list.get(list.active()).unwrap().url.as_ref().unwrap().as_str(), "https://a.example/");
    }

    #[test]
    fn empty_space_restores_one_blank_tab() {
        let t0 = Instant::now();
        let list = TabList::from_space(&SpacesFile::personal_default().spaces[0], t0);
        assert_eq!(list.iter().count(), 1);
        assert!(list.get(list.active()).unwrap().is_blank());
    }

    #[test]
    fn blank_tabs_are_not_saved() {
        let t0 = Instant::now();
        let mut l = sample(t0);
        l.open(None, t0, true);
        let space = l.to_space(&meta());
        assert_eq!(space.tabs.len(), 3);
        assert_eq!(space.active_index, None);
    }
}
