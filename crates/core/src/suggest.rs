//! Saran address bar dari riwayat lokal dan tab yang terbuka. Tidak ada yang
//! dikirim ke luar: tidak ada saran dari mesin pencari.

use crate::history::{History, HistoryEntry, display_url, split_host, visit_key};
use crate::omnibox::{Parsed, classify_input, search_url};
use crate::tabs::TabId;
use std::collections::HashMap;
use url::Url;

/// Jumlah baris paling banyak di dropdown.
pub const MAX_ROWS: usize = 8;
/// Jumlah baris "Pindah ke tab" paling banyak.
pub const MAX_TAB_ROWS: usize = 3;
const DAY: u64 = 86_400;

/// Tab yang bisa disarankan (bukan tab aktif, punya URL).
pub struct TabCandidate<'a> {
    pub id: TabId,
    pub title: &'a str,
    pub url: &'a Url,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SuggestionKind {
    /// Cari teks yang diketik di mesin pencari.
    Search,
    /// Buka teks yang diketik sebagai alamat.
    Url,
    /// Halaman dari riwayat.
    History,
    /// Pindah ke tab yang sudah terbuka.
    SwitchTab(TabId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub kind: SuggestionKind,
    pub title: String,
    pub detail: String,
    /// Tujuan saat baris ini dipilih.
    pub url: Url,
    /// Isi address bar saat baris ini disorot dengan tombol panah.
    pub fill: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Suggestions {
    /// Baris pertama adalah tindakan bawaan saat Enter ditekan.
    pub rows: Vec<Suggestion>,
    /// Lanjutan teks untuk dilengkapi inline. `Some("")` kalau teks yang
    /// diketik sudah lengkap.
    pub inline_tail: Option<String>,
}

/// Kandidat pelengkapan inline.
struct Inline {
    url: Url,
    title: String,
    fill: String,
    tail: String,
}

/// Saran untuk `input`. `allow_inline` mematikan pelengkapan inline (mis.
/// setelah pengguna menghapus huruf).
pub fn suggest(
    input: &str,
    history: &History,
    tabs: &[TabCandidate],
    search_template: &str,
    allow_inline: bool,
    now: u64,
) -> Suggestions {
    let query = input.trim();
    let Some(parsed) = classify_input(query, search_template) else { return Suggestions::default() };
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let prefix = normalized_prefix(query);
    let mut rows: Vec<Suggestion> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    let mut inline_tail = None;

    // Baris bawaan.
    let inline = if allow_inline { inline_candidate(query, history, now) } else { None };
    let first = match inline {
        Some(c) => {
            inline_tail = Some(c.tail);
            Suggestion {
                kind: SuggestionKind::History,
                detail: display_url(c.url.as_str()),
                title: c.title,
                fill: c.fill,
                url: c.url,
            }
        }
        None => match &parsed {
            Parsed::Url(url) => Suggestion {
                kind: SuggestionKind::Url,
                title: display_url(url.as_str()),
                detail: "Open address".into(),
                url: url.clone(),
                fill: query.to_string(),
            },
            Parsed::Search(url) => search_row(query, url, search_template),
        },
    };
    let first_is_search = first.kind == SuggestionKind::Search;
    if let Some(key) = visit_key(&first.url) {
        keys.push(key);
    }
    rows.push(first);
    let history_slots = MAX_ROWS - usize::from(!first_is_search);

    // Tab yang sudah terbuka.
    let mut tab_matches: Vec<(u8, usize, &TabCandidate)> = tabs
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let tier = match_tier(&words, &prefix, t.url.as_str(), t.title);
            (tier > 0).then_some((tier, i, t))
        })
        .collect();
    tab_matches.sort_by_key(|(tier, i, _)| (std::cmp::Reverse(*tier), *i));
    for (_, _, tab) in tab_matches.into_iter().take(MAX_TAB_ROWS) {
        let display = display_url(tab.url.as_str());
        rows.push(Suggestion {
            kind: SuggestionKind::SwitchTab(tab.id),
            title: if tab.title.trim().is_empty() { display.clone() } else { tab.title.to_string() },
            detail: format!("Switch to tab · {display}"),
            url: tab.url.clone(),
            fill: tab.url.to_string(),
        });
        if let Some(key) = visit_key(tab.url) {
            keys.push(key);
        }
    }

    // Riwayat, berperingkat.
    let mut scored: Vec<(f64, &HistoryEntry)> = history
        .entries()
        .iter()
        .filter_map(|e| {
            let tier = match_tier(&words, &prefix, &e.url, &e.title);
            (tier > 0).then(|| (f64::from(tier) * (1.0 + (1.0 + frecency(e, now)).ln()), e))
        })
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| {
        sb.total_cmp(sa).then(b.last_visit.cmp(&a.last_visit)).then(a.url.len().cmp(&b.url.len()))
    });
    for (_, entry) in scored {
        if rows.len() >= history_slots {
            break;
        }
        if keys.contains(&entry.url) {
            continue;
        }
        let Ok(url) = Url::parse(&entry.url) else { continue };
        let display = display_url(&entry.url);
        rows.push(Suggestion {
            kind: SuggestionKind::History,
            title: if entry.title.is_empty() { display.clone() } else { entry.title.clone() },
            detail: display,
            url,
            fill: entry.url.clone(),
        });
        keys.push(entry.url.clone());
    }

    // Pencarian selalu tersedia sebagai baris terakhir.
    if !first_is_search && let Some(url) = search_url(query, search_template) {
        rows.push(search_row(query, &url, search_template));
    }
    Suggestions { rows, inline_tail }
}

fn search_row(query: &str, url: &Url, template: &str) -> Suggestion {
    let engine = Url::parse(&template.replace("{}", ""))
        .ok()
        .and_then(|u| u.host_str().map(|h| h.strip_prefix("www.").unwrap_or(h).to_string()))
        .unwrap_or_default();
    Suggestion {
        kind: SuggestionKind::Search,
        title: format!("Search “{query}”"),
        detail: engine,
        url: url.clone(),
        fill: query.to_string(),
    }
}

/// Query tanpa skema dan `www.`, huruf kecil.
fn normalized_prefix(query: &str) -> String {
    let lower = query.to_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")).unwrap_or(&lower);
    rest.strip_prefix("www.").unwrap_or(rest).to_string()
}

/// Di mana `word` (huruf kecil) muncul di `hay` tanpa membedakan huruf besar:
/// `Some(true)` kalau ada yang di awal kata, `Some(false)` kalau hanya di
/// tengah kata, `None` kalau tidak ada. Kata ASCII dicocokkan tanpa alokasi,
/// karena fungsi ini dipanggil untuk setiap entri riwayat di setiap ketikan.
fn word_match(hay: &str, word: &str) -> Option<bool> {
    let at_word_start = |hay: &str, i: usize| hay[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
    if !word.is_ascii() {
        let lower = hay.to_lowercase();
        let mut found = None;
        for (i, _) in lower.match_indices(word) {
            if at_word_start(&lower, i) {
                return Some(true);
            }
            found = Some(false);
        }
        return found;
    }
    let (h, w) = (hay.as_bytes(), word.as_bytes());
    if w.is_empty() || w.len() > h.len() {
        return None;
    }
    let mut found = None;
    for i in 0..=h.len() - w.len() {
        // Byte pertama yang cocok dengan huruf ASCII selalu awal karakter.
        if h[i..i + w.len()].eq_ignore_ascii_case(w) {
            if at_word_start(hay, i) {
                return Some(true);
            }
            found = Some(false);
        }
    }
    found
}

/// `text` diawali `prefix` (huruf kecil ASCII) tanpa membedakan huruf besar.
fn starts_ci(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len() && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// Seberapa cocok satu halaman: 4 awal host, 3 awal URL, 2 semua kata di awal
/// kata, 1 semua kata muncul (kata pendek tetap harus di awal kata), 0 tidak cocok.
fn match_tier(words: &[String], prefix: &str, url: &str, title: &str) -> u8 {
    if words.is_empty() {
        return 0;
    }
    if prefix.is_ascii() && !prefix.contains(char::is_whitespace) {
        let (host, rest) = split_host(url);
        if starts_ci(host, prefix) {
            return 4;
        }
        // Awal alamat tanpa skema: host lengkap lalu awal path.
        if prefix.len() > host.len() && starts_ci(prefix, host) && starts_ci(rest, &prefix[host.len()..]) {
            return 3;
        }
    }
    let mut tier = 2;
    for word in words {
        match (word_match(title, word), word_match(url, word)) {
            (Some(true), _) | (_, Some(true)) => {}
            (Some(false), _) | (_, Some(false)) if word.chars().count() >= 3 => tier = 1,
            _ => return 0,
        }
    }
    tier
}

/// Gabungan seberapa sering dan seberapa baru halaman dibuka. Alamat yang
/// diketik bernilai lebih.
fn frecency(entry: &HistoryEntry, now: u64) -> f64 {
    let age = now.saturating_sub(entry.last_visit);
    let decay = match age {
        a if a <= 4 * DAY => 1.0,
        a if a <= 14 * DAY => 0.7,
        a if a <= 31 * DAY => 0.5,
        _ => 0.3,
    };
    (f64::from(entry.visit_count) + 3.0 * f64::from(entry.typed_count)) * decay
}

/// Kandidat pelengkapan inline: hanya awal host (atau awal alamat lengkap
/// kalau query berisi `/`), dari halaman yang pernah diketik atau sering
/// dikunjungi. Ekornya ditempel ke teks asli pengguna.
fn inline_candidate(query: &str, history: &History, now: u64) -> Option<Inline> {
    if !query.is_ascii() || query.contains(char::is_whitespace) {
        return None;
    }
    let lower = query.to_ascii_lowercase();
    let (scheme, rest) = if let Some(rest) = lower.strip_prefix("https://") {
        (Some("https"), rest)
    } else if let Some(rest) = lower.strip_prefix("http://") {
        (Some("http"), rest)
    } else {
        (None, lower.as_str())
    };
    let nq = rest.strip_prefix("www.").unwrap_or(rest);
    if nq.is_empty() {
        return None;
    }
    let scheme_ok =
        |e: &HistoryEntry| scheme.is_none_or(|s| e.url.strip_prefix(s).is_some_and(|r| r.starts_with("://")));

    if nq.contains('/') {
        // Tahap alamat: hanya halaman yang pernah diketik.
        let best = history
            .entries()
            .iter()
            .filter(|e| e.typed_count >= 1 && scheme_ok(e))
            .filter(|e| display_url(&e.url).to_ascii_lowercase().starts_with(nq))
            .max_by(|a, b| frecency(a, now).total_cmp(&frecency(b, now)))?;
        let fill = display_url(&best.url);
        let tail = fill.get(nq.len()..)?.to_string();
        let title = if best.title.is_empty() { fill.clone() } else { best.title.clone() };
        return Some(Inline { url: Url::parse(&best.url).ok()?, title, fill, tail });
    }

    // Tahap host: kelompokkan per host.
    struct HostScore<'a> {
        host: String,
        typed: u32,
        visits: u32,
        score: f64,
        root: Option<&'a HistoryEntry>,
        sample: &'a HistoryEntry,
    }
    let mut hosts: Vec<HostScore> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for entry in history.entries().iter().filter(|e| scheme_ok(e)) {
        let (host, rest) = split_host(&entry.url);
        if !starts_ci(host, nq) {
            continue;
        }
        let i = *index.entry(host).or_insert_with(|| {
            let host = host.to_ascii_lowercase();
            hosts.push(HostScore { host, typed: 0, visits: 0, score: 0.0, root: None, sample: entry });
            hosts.len() - 1
        });
        let h = &mut hosts[i];
        h.typed += entry.typed_count;
        h.visits += entry.visit_count;
        h.score += frecency(entry, now);
        let is_root = rest == "/" || rest.is_empty();
        if is_root && h.root.is_none_or(|r| !r.url.starts_with("https://") && entry.url.starts_with("https://")) {
            h.root = Some(entry);
        }
    }
    let best = hosts
        .into_iter()
        .filter(|h| h.typed >= 1 || h.visits >= 3)
        .max_by(|a, b| a.score.total_cmp(&b.score).then(b.host.len().cmp(&a.host.len())))?;
    let (url, title) = match best.root {
        Some(root) => (Url::parse(&root.url).ok()?, root.title.clone()),
        None => {
            // Halaman depan belum pernah dibuka: bangun dari halaman lain di host itu.
            let sample = Url::parse(&best.sample.url).ok()?;
            let mut root = sample.clone();
            root.set_path("/");
            root.set_query(None);
            (root, String::new())
        }
    };
    let tail = best.host.get(nq.len()..)?.to_string();
    let title = if title.is_empty() { best.host.clone() } else { title };
    Some(Inline { url, title, fill: best.host, tail })
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str = "https://www.google.com/search?q={}";
    const NOW: u64 = 1_790_000_000;

    fn history(entries: &[(&str, &str, u32, u32, u64)]) -> History {
        let mut h = History::new();
        // Dicatat dari yang paling lama supaya urutan terbaru dulu terjaga.
        let mut list = entries.to_vec();
        list.sort_by_key(|e| std::cmp::Reverse(e.4));
        for &(url, title, visits, typed, age_days) in list.iter() {
            for i in 0..visits {
                h.record(url, title, i < typed, NOW - age_days * DAY);
            }
        }
        h
    }

    fn run(input: &str, h: &History) -> Suggestions {
        suggest(input, h, &[], G, true, NOW)
    }

    fn titles(s: &Suggestions) -> Vec<&str> {
        s.rows.iter().map(|r| r.title.as_str()).collect()
    }

    #[test]
    fn blank_input_gives_nothing() {
        assert_eq!(run("   ", &History::new()), Suggestions::default());
    }

    #[test]
    fn words_give_search_first() {
        let s = run("resep nasi", &History::new());
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.rows[0].kind, SuggestionKind::Search);
        assert_eq!(s.rows[0].title, "Search “resep nasi”");
        assert_eq!(s.rows[0].detail, "google.com");
        assert_eq!(s.rows[0].url.as_str(), "https://www.google.com/search?q=resep+nasi");
        assert_eq!(s.inline_tail, None);
    }

    #[test]
    fn address_without_history_gives_url_then_search() {
        let s = run("example.com/foo", &History::new());
        assert_eq!(s.rows[0].kind, SuggestionKind::Url);
        assert_eq!(s.rows[0].url.as_str(), "https://example.com/foo");
        assert_eq!(s.rows.last().unwrap().kind, SuggestionKind::Search);
    }

    #[test]
    fn inline_completes_typed_host_keeping_user_text() {
        let h = history(&[("https://www.youtube.com/", "YouTube", 2, 2, 0)]);
        for input in ["you", "YOU", "https://www.you", "www.you"] {
            let s = run(input, &h);
            assert_eq!(s.inline_tail.as_deref(), Some("tube.com"), "{input}");
            assert_eq!(s.rows[0].kind, SuggestionKind::History);
            assert_eq!(s.rows[0].url.as_str(), "https://www.youtube.com/");
            assert_eq!(s.rows[0].title, "YouTube");
        }
    }

    #[test]
    fn no_inline_when_disallowed_but_row_still_shown() {
        let h = history(&[("https://www.youtube.com/", "YouTube", 2, 2, 0)]);
        let s = suggest("you", &h, &[], G, false, NOW);
        assert_eq!(s.inline_tail, None);
        assert_eq!(s.rows[0].kind, SuggestionKind::Search);
        assert!(titles(&s).contains(&"YouTube"));
    }

    #[test]
    fn inline_builds_root_from_other_pages_of_host() {
        let h = history(&[("https://github.com/a/b", "repo", 1, 1, 0)]);
        let s = run("git", &h);
        assert_eq!(s.inline_tail.as_deref(), Some("hub.com"));
        assert_eq!(s.rows[0].url.as_str(), "https://github.com/");
    }

    #[test]
    fn host_visited_once_by_link_is_not_inlined() {
        let h = history(&[("https://ubuntu.com/download", "Download Ubuntu", 1, 0, 0)]);
        let s = run("ubu", &h);
        assert_eq!(s.inline_tail, None);
        assert!(titles(&s).contains(&"Download Ubuntu"));
        let h = history(&[("https://ubuntu.com/download", "Download Ubuntu", 3, 0, 0)]);
        assert_eq!(run("ubu", &h).inline_tail.as_deref(), Some("ntu.com"), "sering dikunjungi");
    }

    #[test]
    fn path_stage_completes_typed_address() {
        let h = history(&[("https://github.com/servo/servo", "Servo", 1, 1, 0)]);
        assert_eq!(run("github.com/serv", &h).inline_tail.as_deref(), Some("o/servo"));
        let h = history(&[("https://github.com/servo/servo", "Servo", 5, 0, 0)]);
        assert_eq!(run("github.com/serv", &h).inline_tail, None, "tahap alamat hanya yang pernah diketik");
    }

    #[test]
    fn non_ascii_query_is_not_inlined() {
        let h = history(&[("https://café.example/", "Kafe", 3, 3, 0)]);
        assert_eq!(run("caf\u{e9}", &h).inline_tail, None);
    }

    #[test]
    fn title_words_match() {
        let h = history(&[("https://resep.com/123", "Resep Nasi Goreng Spesial", 1, 0, 0)]);
        assert!(titles(&run("nasi goreng", &h)).contains(&"Resep Nasi Goreng Spesial"));
    }

    #[test]
    fn short_words_must_start_a_word() {
        let h =
            history(&[("https://www.youtube.com/", "YouTube", 1, 0, 0), ("https://ubuntu.com/", "Ubuntu", 1, 0, 0)]);
        let t = run("ub", &h);
        assert!(titles(&t).contains(&"Ubuntu"));
        assert!(!titles(&t).contains(&"YouTube"));
    }

    #[test]
    fn typed_and_recent_rank_higher() {
        let h = history(&[
            ("https://berita.com/lama", "Berita lama", 3, 0, 60),
            ("https://berita.com/baru", "Berita baru", 3, 0, 1),
            ("https://berita.com/ketik", "Berita ketik", 1, 1, 1),
        ]);
        let s = suggest("berita", &h, &[], G, false, NOW);
        let t = titles(&s);
        let pos = |x: &str| t.iter().position(|y| *y == x).unwrap();
        assert!(pos("Berita ketik") < pos("Berita lama"));
        assert!(pos("Berita baru") < pos("Berita lama"));
    }

    #[test]
    fn open_tab_suggested_before_history_without_duplicate() {
        let h = history(&[("https://docs.rs/url", "url - Rust", 2, 0, 0)]);
        let url = Url::parse("https://docs.rs/url").unwrap();
        let tabs = [TabCandidate { id: TabId(7), title: "url - Rust", url: &url }];
        let s = suggest("url rust", &h, &tabs, G, true, NOW);
        assert_eq!(s.rows[0].kind, SuggestionKind::Search);
        assert_eq!(s.rows[1].kind, SuggestionKind::SwitchTab(TabId(7)));
        assert_eq!(s.rows[1].detail, "Switch to tab · docs.rs/url");
        assert_eq!(s.rows.iter().filter(|r| r.url == url).count(), 1);
    }

    #[test]
    fn at_most_eight_rows_with_search_last() {
        let entries: Vec<(String, String)> =
            (0..20).map(|i| (format!("https://belajar{i}.com/"), format!("Belajar {i}"))).collect();
        let list: Vec<(&str, &str, u32, u32, u64)> =
            entries.iter().map(|(u, t)| (u.as_str(), t.as_str(), 1, 0, 0)).collect();
        let s = suggest("belajar", &history(&list), &[], G, false, NOW);
        assert_eq!(s.rows.len(), MAX_ROWS);
        assert_eq!(s.rows[0].kind, SuggestionKind::Search);
        let pages: Vec<(String, String)> =
            (0..20).map(|i| (format!("https://belajar.com/bab{i}"), format!("Bab {i}"))).collect();
        let list: Vec<(&str, &str, u32, u32, u64)> =
            pages.iter().map(|(u, t)| (u.as_str(), t.as_str(), 1, 0, 0)).collect();
        let s = suggest("belajar.com", &history(&list), &[], G, false, NOW);
        assert_eq!(s.rows.len(), MAX_ROWS);
        assert_eq!(s.rows[0].kind, SuggestionKind::Url);
        assert_eq!(s.rows.last().unwrap().kind, SuggestionKind::Search);
    }

    #[test]
    fn complete_host_gives_empty_tail() {
        let h = history(&[("https://www.youtube.com/", "YouTube", 2, 2, 0)]);
        let s = run("youtube.com", &h);
        assert_eq!(s.inline_tail.as_deref(), Some(""));
        assert_eq!(s.rows[0].kind, SuggestionKind::History);
    }

    #[test]
    fn localhost_port_completes() {
        let h = history(&[("http://localhost:5173/", "Vite", 1, 1, 0)]);
        assert_eq!(run("localhost:51", &h).inline_tail.as_deref(), Some("73"));
    }

    #[test]
    fn fill_is_full_url_for_pages_and_query_for_search() {
        let h = history(&[("https://docs.rs/url", "url - Rust", 1, 0, 0)]);
        let s = suggest("url", &h, &[], G, false, NOW);
        assert_eq!(s.rows[0].fill, "url");
        assert_eq!(s.rows[1].fill, "https://docs.rs/url");
    }

    #[test]
    #[ignore = "ukur dengan: cargo test --release -p my-browser-core suggest_10k -- --ignored --nocapture"]
    fn suggest_10k_is_fast() {
        let mut h = History::new();
        for i in 0..10_000u64 {
            h.record(
                &format!("https://situs{i}.example.com/halaman/{i}?q=x"),
                &format!("Judul halaman nomor {i}"),
                i % 7 == 0,
                NOW - i * 60,
            );
        }
        let start = std::time::Instant::now();
        let mut rows = 0;
        for q in ["s", "si", "sit", "situ", "situs", "situs1", "situs12", "judul halaman", "nomor 99", "example"] {
            rows += suggest(q, &h, &[], G, true, NOW).rows.len();
        }
        let per = start.elapsed() / 10;
        println!("rata-rata per ketikan: {per:?} ({rows} baris)");
        assert!(per < std::time::Duration::from_millis(5), "{per:?}");
    }
}
