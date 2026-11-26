//! Favicon: memilih kandidat dari halaman, kunci cache per host, monogram
//! pengganti, dan cache PNG 32 px di disk.

use crate::persist::write_private;
use crate::theme::Rgb;
use serde::Deserialize;
use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
pub use url::Url;

/// Script (dunia script terpisah) yang mengembalikan JSON
/// `[{"href", "rel", "sizes"}]` untuk tag `<link>` ikon di halaman.
pub const ICON_CANDIDATES_JS: &str = r#"JSON.stringify(Array.from(document.querySelectorAll('link[rel]'))
  .filter(l => /(^|\s)(icon|apple-touch-icon|apple-touch-icon-precomposed)(\s|$)/i.test(l.rel))
  .slice(0, 20)
  .map(l => ({href: l.href, rel: l.rel.toLowerCase(), sizes: l.getAttribute('sizes') || ''})))"#;

pub const MAX_FILES: usize = 2000;
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;
pub const REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);
pub const MAX_ICON_BYTES: usize = 512 * 1024;

#[derive(Deserialize)]
struct Candidate {
    href: String,
    #[serde(default)]
    rel: String,
    #[serde(default)]
    sizes: String,
}

/// Sisi terbesar dari atribut `sizes` ("16x16 32x32"); None untuk kosong atau "any".
fn largest_side(sizes: &str) -> Option<u32> {
    sizes
        .split_whitespace()
        .filter_map(|s| {
            let (w, h) = s.to_ascii_lowercase().split_once('x').map(|(w, h)| (w.to_string(), h.to_string()))?;
            Some(w.parse::<u32>().ok()?.max(h.parse::<u32>().ok()?))
        })
        .max()
}

/// Urutan coba ambil: ikon berukuran minimal 32 px (terkecil dulu), ikon
/// tanpa ukuran, ikon di bawah 32 px, apple-touch-icon, lalu
/// `<origin>/favicon.ico`. Hanya https (pengambil favicon di app tidak boleh
/// memuat http biasa), tanpa duplikat.
pub fn pick_candidates(json: &str, page: &Url) -> Vec<Url> {
    let parsed: Vec<Candidate> = serde_json::from_str(json).unwrap_or_default();
    let mut big = Vec::new();
    let mut unsized_icons = Vec::new();
    let mut small = Vec::new();
    let mut touch = Vec::new();
    for c in parsed {
        let Ok(url) = page.join(&c.href) else { continue };
        if url.scheme() != "https" {
            continue;
        }
        let size = largest_side(&c.sizes);
        if c.rel.split_whitespace().any(|r| r.starts_with("apple-touch-icon")) {
            touch.push((size.unwrap_or(u32::MAX), url));
        } else {
            match size {
                Some(s) if s >= 32 => big.push((s, url)),
                Some(s) => small.push((s, url)),
                None => unsized_icons.push(url),
            }
        }
    }
    big.sort_by_key(|(s, _)| *s);
    small.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    touch.sort_by_key(|(s, _)| *s);
    let mut out: Vec<Url> = Vec::new();
    let all = big
        .into_iter()
        .map(|(_, u)| u)
        .chain(unsized_icons)
        .chain(small.into_iter().map(|(_, u)| u))
        .chain(touch.into_iter().map(|(_, u)| u));
    for url in all {
        if !out.contains(&url) {
            out.push(url);
        }
    }
    if page.scheme() == "https"
        && let Ok(ico) = page.join("/favicon.ico")
        && !out.contains(&ico)
    {
        out.push(ico);
    }
    out
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Nama berkas cache untuk host: FNV-1a 64 dari host huruf kecil, 16 digit hex.
pub fn host_key(host: &str) -> String {
    format!("{:016x}", fnv1a(host.to_lowercase().as_bytes()))
}

/// Warna ubin monogram; semuanya cukup gelap untuk huruf putih.
const MONOGRAM_COLORS: [Rgb; 8] = [
    [0x5b, 0x4b, 0xc4],
    [0x1f, 0x6f, 0x8b],
    [0x2e, 0x7d, 0x32],
    [0xad, 0x45, 0x2c],
    [0x8e, 0x3b, 0x8f],
    [0x37, 0x47, 0x4f],
    [0x9a, 0x5b, 0x13],
    [0xb0, 0x3a, 0x5b],
];

/// Huruf dan warna ubin pengganti favicon.
pub fn monogram(host: &str) -> (char, Rgb) {
    let bare = host.strip_prefix("www.").unwrap_or(host);
    let letter = bare.chars().find(|c| c.is_alphanumeric()).and_then(|c| c.to_uppercase().next()).unwrap_or('?');
    let color = MONOGRAM_COLORS[(fnv1a(host.to_lowercase().as_bytes()) % MONOGRAM_COLORS.len() as u64) as usize];
    (letter, color)
}

/// Cache PNG favicon di satu folder, satu berkas per host.
pub struct FaviconCache {
    dir: PathBuf,
}

impl FaviconCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Jalur berkas cache untuk host.
    pub fn path_for(&self, host: &str) -> PathBuf {
        self.dir.join(format!("{}.png", host_key(host)))
    }

    pub fn load(&self, host: &str) -> Option<Vec<u8>> {
        std::fs::read(self.path_for(host)).ok()
    }

    /// Belum ada, atau lebih tua dari `REFRESH_AFTER`.
    pub fn needs_refresh(&self, host: &str, now: SystemTime) -> bool {
        let Ok(modified) = std::fs::metadata(self.path_for(host)).and_then(|m| m.modified()) else { return true };
        now.duration_since(modified).is_ok_and(|age| age > REFRESH_AFTER)
    }

    pub fn store(&self, host: &str, png: &[u8]) -> io::Result<()> {
        write_private(&self.path_for(host), png)?;
        self.prune(MAX_FILES, MAX_BYTES)
    }

    fn entries(&self) -> io::Result<Vec<(SystemTime, u64, PathBuf)>> {
        let mut out = Vec::new();
        let dir = match std::fs::read_dir(&self.dir) {
            Ok(d) => d,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e),
        };
        for entry in dir {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != "png") {
                continue;
            }
            let meta = std::fs::metadata(&path)?;
            out.push((meta.modified()?, meta.len(), path));
        }
        Ok(out)
    }

    /// Membuang berkas tertua sampai jumlah dan ukuran total di dalam batas.
    pub fn prune(&self, max_files: usize, max_bytes: u64) -> io::Result<()> {
        let mut entries = self.entries()?;
        entries.sort_by_key(|e| std::cmp::Reverse(e.0));
        let mut total = 0u64;
        for (i, (_, size, path)) in entries.into_iter().enumerate() {
            total += size;
            if i >= max_files || total > max_bytes {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(())
    }

    /// Membuang ikon host yang tidak ada di `hosts`. Mengembalikan jumlah yang dibuang.
    pub fn retain_hosts(&self, hosts: &HashSet<String>) -> io::Result<usize> {
        let keep: HashSet<String> = hosts.iter().map(|h| host_key(h)).collect();
        let mut removed = 0;
        for (_, _, path) in self.entries()? {
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if !keep.contains(&stem) && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::time::{Duration, SystemTime};

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn strs(v: Vec<Url>) -> Vec<String> {
        v.into_iter().map(|u| u.to_string()).collect()
    }

    #[test]
    fn pick_prefers_smallest_icon_of_at_least_32px() {
        let json = r#"[
            {"href":"https://e.example/16.png","rel":"icon","sizes":"16x16"},
            {"href":"https://e.example/192.png","rel":"icon","sizes":"192x192"},
            {"href":"https://e.example/32.png","rel":"icon","sizes":"32x32"}
        ]"#;
        let got = strs(pick_candidates(json, &u("https://e.example/page")));
        assert_eq!(got[0], "https://e.example/32.png");
        assert_eq!(got[1], "https://e.example/192.png");
        assert_eq!(got.last().unwrap(), "https://e.example/favicon.ico");
        assert!(got.contains(&"https://e.example/16.png".to_string()));
    }

    #[test]
    fn pick_falls_back_to_apple_touch_then_favicon_ico() {
        let json = r#"[{"href":"https://e.example/touch.png","rel":"apple-touch-icon","sizes":""}]"#;
        let got = strs(pick_candidates(json, &u("https://e.example/")));
        assert_eq!(got, vec!["https://e.example/touch.png", "https://e.example/favicon.ico"]);
    }

    #[test]
    fn pick_keeps_https_only_and_resolves_relative_hrefs() {
        let json = r#"[
            {"href":"/rel.png","rel":"shortcut icon","sizes":""},
            {"href":"http://cdn.example/plain.png","rel":"icon","sizes":"32x32"},
            {"href":"https://e.example/rel.png","rel":"icon","sizes":""}
        ]"#;
        let got = strs(pick_candidates(json, &u("https://e.example/a/b")));
        assert_eq!(got, vec!["https://e.example/rel.png", "https://e.example/favicon.ico"]);
    }

    #[test]
    fn pick_survives_garbage_json() {
        assert_eq!(strs(pick_candidates("nope", &u("https://e.example/x"))), vec!["https://e.example/favicon.ico"]);
        assert!(pick_candidates("nope", &u("http://e.example/x")).is_empty());
    }

    #[test]
    fn host_key_is_16_hex_and_case_insensitive() {
        let k = host_key("Example.com");
        assert_eq!(k.len(), 16);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(k, host_key("example.com"));
        assert_ne!(k, host_key("example.org"));
    }

    #[test]
    fn monogram_strips_www_and_uppercases() {
        let (c, color) = monogram("www.example.com");
        assert_eq!(c, 'E');
        assert_eq!(monogram("www.example.com").1, color);
        assert_eq!(monogram("ápple.example").0, 'Á');
        assert_eq!(monogram("---").0, '?');
    }

    #[test]
    fn cache_store_then_load_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let cache = FaviconCache::new(d.path().join("favicons"));
        assert!(cache.load("example.com").is_none());
        cache.store("example.com", b"png-bytes").unwrap();
        assert_eq!(cache.load("example.com").unwrap(), b"png-bytes");
        assert_eq!(cache.load("EXAMPLE.com").unwrap(), b"png-bytes");
    }

    #[test]
    fn needs_refresh_after_seven_days() {
        let d = tempfile::tempdir().unwrap();
        let cache = FaviconCache::new(d.path().to_path_buf());
        let now = SystemTime::now();
        assert!(cache.needs_refresh("example.com", now));
        cache.store("example.com", b"x").unwrap();
        assert!(!cache.needs_refresh("example.com", now));
        let file = std::fs::File::options()
            .write(true)
            .open(d.path().join(format!("{}.png", host_key("example.com"))))
            .unwrap();
        file.set_modified(now - Duration::from_secs(8 * 24 * 3600)).unwrap();
        assert!(cache.needs_refresh("example.com", now));
    }

    #[test]
    fn prune_removes_oldest_beyond_limits() {
        let d = tempfile::tempdir().unwrap();
        let cache = FaviconCache::new(d.path().to_path_buf());
        let now = SystemTime::now();
        for (i, host) in ["a.example", "b.example", "c.example", "d.example", "e.example"].iter().enumerate() {
            std::fs::write(d.path().join(format!("{}.png", host_key(host))), [0u8; 10]).unwrap();
            let f =
                std::fs::File::options().write(true).open(d.path().join(format!("{}.png", host_key(host)))).unwrap();
            f.set_modified(now - Duration::from_secs(100 - i as u64 * 10)).unwrap();
        }
        cache.prune(3, 1_000).unwrap();
        let alive = |h: &str| cache.load(h).is_some();
        assert!(!alive("a.example") && !alive("b.example"));
        assert!(alive("c.example") && alive("d.example") && alive("e.example"));
        cache.prune(10, 20).unwrap();
        assert!(!alive("c.example") && alive("d.example") && alive("e.example"));
    }

    #[test]
    fn retain_hosts_deletes_icons_of_other_hosts() {
        let d = tempfile::tempdir().unwrap();
        let cache = FaviconCache::new(d.path().to_path_buf());
        for host in ["keep.example", "drop.example", "also-drop.example"] {
            cache.store(host, b"x").unwrap();
        }
        let keep: HashSet<String> = ["keep.example".to_string()].into();
        assert_eq!(cache.retain_hosts(&keep).unwrap(), 2);
        assert!(cache.load("keep.example").is_some());
        assert!(cache.load("drop.example").is_none());
    }
}
