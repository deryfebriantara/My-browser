//! Entri password dan aturan pencocokan situs.
//!
//! Situs dicocokkan dengan origin persis (skema, host, port). `github.com`
//! tidak cocok dengan `github.com.evil.example`, `evil-github.com`, maupun
//! `http://github.com`, jadi password tidak ditawarkan di situs tiruan.

use serde::{Deserialize, Serialize};
use url::Url;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Origin `https://host[:port]` dari sebuah URL. Hanya http dan https.
pub fn origin_of(url: &str) -> Option<String> {
    let url = Url::parse(url.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Entry {
    pub id: String,
    /// Origin persis tempat password dipakai, mis. `https://github.com`.
    pub origin: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub note: String,
    /// Detik sejak epoch Unix.
    pub created: u64,
    pub updated: u64,
    #[serde(default)]
    pub last_used: Option<u64>,
}

impl std::fmt::Debug for Entry {
    /// Password tidak pernah ikut tercetak (log, panic, debug).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("id", &self.id)
            .field("origin", &self.origin)
            .field("username", &self.username)
            .field("password", &"<disembunyikan>")
            .finish()
    }
}

impl Entry {
    /// Host untuk ditampilkan, mis. `github.com`.
    pub fn host(&self) -> &str {
        self.origin.split_once("://").map_or(self.origin.as_str(), |(_, rest)| rest)
    }
}

#[derive(Clone, PartialEq, Zeroize, ZeroizeOnDrop)]
pub struct NewEntry {
    pub origin: String,
    pub username: String,
    pub password: String,
    pub note: String,
}

impl std::fmt::Debug for NewEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewEntry").field("origin", &self.origin).field("username", &self.username).finish_non_exhaustive()
    }
}

/// Apa yang ditawarkan setelah pengguna login dengan sebuah password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// Belum ada akun ini untuk situs ini.
    Save,
    /// Akun sudah ada dengan password lain (id entrinya).
    Update(String),
    /// Sudah tersimpan persis sama; tidak perlu bertanya.
    Nothing,
}

/// Id entri acak (128 bit, heksadesimal).
pub(crate) fn new_id() -> String {
    crate::hex::encode(&crate::crypto::random::<16>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_normalized_and_limited_to_web() {
        assert_eq!(origin_of("https://GitHub.com/login?next=/"), Some("https://github.com".into()));
        assert_eq!(origin_of("https://github.com:443/x"), Some("https://github.com".into()));
        assert_eq!(origin_of("http://localhost:3000/a"), Some("http://localhost:3000".into()));
        assert_eq!(origin_of("  https://a.example  "), Some("https://a.example".into()));
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "data:text/html,x", "bukan url", ""] {
            assert_eq!(origin_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn debug_output_never_contains_the_password() {
        let e = Entry {
            id: "1".into(),
            origin: "https://a.example".into(),
            username: "dery".into(),
            password: "sangat-rahasia".into(),
            note: String::new(),
            created: 0,
            updated: 0,
            last_used: None,
        };
        assert!(!format!("{e:?}").contains("sangat-rahasia"));
        assert_eq!(e.host(), "a.example");
        let n = NewEntry { origin: "o".into(), username: "u".into(), password: "sangat-rahasia".into(), note: String::new() };
        assert!(!format!("{n:?}").contains("sangat-rahasia"));
    }

    #[test]
    fn ids_are_random() {
        assert_ne!(new_id(), new_id());
        assert_eq!(new_id().len(), 32);
    }
}
