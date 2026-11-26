//! Model sidebar: tab terlihat per bagian, siap digambar oleh UI.

use crate::tabs::{TabId, TabKind, TabList, Url};

/// Host untuk tampilan dan kunci favicon: huruf kecil, tanpa awalan
/// `www.`. `None` untuk alamat yang bukan http(s).
pub fn site_host(url: &Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    Some(host.strip_prefix("www.").map(str::to_string).unwrap_or(host))
}

/// Satu baris (atau ubin favorit) di sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct RowInfo {
    pub id: TabId,
    pub kind: TabKind,
    pub title: String,
    pub host: Option<String>,
    pub url: Option<String>,
    pub active: bool,
    pub asleep: bool,
    pub playing: bool,
}

impl RowInfo {
    /// Label VoiceOver: "<judul>, <host>[, asleep][, playing audio]".
    pub fn accessibility_label(&self) -> String {
        let mut parts = vec![self.title.clone()];
        if let Some(host) = &self.host {
            parts.push(host.clone());
        }
        if self.asleep {
            parts.push("asleep".into());
        }
        if self.playing {
            parts.push("playing audio".into());
        }
        parts.join(", ")
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SidebarModel {
    pub favorites: Vec<RowInfo>,
    pub pinned: Vec<RowInfo>,
    pub today: Vec<RowInfo>,
}

/// Tab terlihat dikelompokkan per bagian dalam urutan sidebar. Tab kosong tidak ikut.
pub fn sidebar_model(tabs: &TabList) -> SidebarModel {
    let active = tabs.active();
    let mut model = SidebarModel::default();
    for tab in tabs.visible() {
        let host = tab.url.as_ref().and_then(site_host);
        let title = match (&host, tab.title.is_empty()) {
            (Some(host), true) => host.clone(),
            _ => tab.display_title(),
        };
        let row = RowInfo {
            id: tab.id,
            kind: tab.kind,
            title,
            host,
            url: tab.url.as_ref().map(|u| u.to_string()),
            active: tab.id == active,
            asleep: !tab.awake && tab.url.is_some(),
            playing: tab.media_active,
        };
        match tab.kind {
            TabKind::Favorite => model.favorites.push(row),
            TabKind::Pinned => model.pinned.push(row),
            TabKind::Today => model.today.push(row),
        }
    }
    model
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn model_groups_sections_in_order_and_skips_blank() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        let t2 = l.open(Some(u("https://t2.example/")), t0, true);
        let t1 = l.open(Some(u("https://t1.example/")), t0, true);
        let p = l.open(Some(u("https://p.example/")), t0, true);
        let f = l.open(Some(u("https://f.example/")), t0, true);
        l.place(p, TabKind::Pinned, 0);
        l.place(f, TabKind::Favorite, 0);
        l.activate(t1, t0);
        l.open(None, t0, false);
        let m = sidebar_model(&l);
        let ids = |rows: &[RowInfo]| rows.iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(&m.favorites), vec![f]);
        assert_eq!(ids(&m.pinned), vec![p]);
        assert_eq!(ids(&m.today), vec![t1, t2]);
        assert!(m.today[0].active);
        assert!(!m.today[1].active);
        assert!(m.today[1].asleep);
    }

    #[test]
    fn model_keeps_long_emoji_and_rtl_titles_intact() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        let a = l.open(Some(u("https://a.example/")), t0, true);
        let b = l.open(Some(u("https://b.example/")), t0, true);
        let crabs = "🦀".repeat(300);
        l.get_mut(a).unwrap().title = crabs.clone();
        l.get_mut(b).unwrap().title = "مرحبا بالعالم".into();
        let m = sidebar_model(&l);
        assert_eq!(m.today[1].title, crabs);
        assert_eq!(m.today[0].title, "مرحبا بالعالم");
        assert!(m.today[1].accessibility_label().starts_with(&crabs));
    }

    #[test]
    fn empty_title_shows_host() {
        let t0 = Instant::now();
        let mut l = TabList::new(t0);
        l.open(Some(u("https://www.example.com/x")), t0, true);
        let m = sidebar_model(&l);
        assert_eq!(m.today[0].title, "example.com");
        assert_eq!(m.today[0].host.as_deref(), Some("example.com"));
    }

    #[test]
    fn accessibility_label_mentions_asleep_and_audio() {
        let row = RowInfo {
            id: TabId(1),
            kind: TabKind::Today,
            title: "Docs".into(),
            host: Some("example.com".into()),
            url: Some("https://example.com/".into()),
            active: false,
            asleep: true,
            playing: true,
        };
        assert_eq!(row.accessibility_label(), "Docs, example.com, asleep, playing audio");
        let plain = RowInfo { host: None, asleep: false, playing: false, ..row };
        assert_eq!(plain.accessibility_label(), "Docs");
    }

    #[test]
    fn site_host_strips_www_and_rejects_non_http() {
        assert_eq!(site_host(&u("https://WWW.Example.com/")).as_deref(), Some("example.com"));
        assert_eq!(site_host(&u("http://sub.example.org/a")).as_deref(), Some("sub.example.org"));
        assert_eq!(site_host(&u("about:blank")), None);
        assert_eq!(site_host(&u("file:///tmp/x")), None);
    }
}
