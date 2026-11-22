//! Mengubah daftar filter EasyList menjadi daftar aturan content blocker WebKit.

use adblock::lists::{FilterSet, ParseOptions};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Batas aturan per daftar. WebKit membatasi jumlah aturan per content rule
/// list; 50.000 adalah batas lama yang aman untuk semua versi.
pub const MAX_RULES_PER_LIST: usize = 50_000;

/// Mengonversi teks filter dengan crate adblock apa adanya. Mengembalikan
/// aturan WebKit dan jumlah baris filter yang dilewati.
pub fn convert(filter_text: &str) -> (Vec<Value>, usize) {
    let mut set = FilterSet::new(true);
    set.add_filter_list(filter_text.to_string(), ParseOptions::default());
    let candidates = filter_text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('!') && !l.starts_with('['))
        .count();
    let (rules, used) = set.into_content_blocking().expect("FilterSet dibuat dalam mode debug");
    let rules = rules.into_iter().map(|r| serde_json::to_value(r).expect("CbRule bisa diserialisasi")).collect();
    (rules, candidates.saturating_sub(used.len()))
}

/// Pengecualian kosmetik (`domain#@#selector`) per selector.
/// `None` berarti selector itu dikecualikan di semua situs.
type Unhide = HashMap<String, Option<Vec<String>>>;

/// Baris filter yang ditangani sendiri karena crate adblock salah
/// mengonversinya atau tidak mendukungnya:
/// - `domain#@#selector` dijadikan aturan sembunyi dengan `unless-domain`,
///   padahal aturan umum `##selector` tetap berlaku di mana-mana, jadi
///   pengecualiannya tidak pernah berlaku.
/// - `@@...$generichide` dibuang, sehingga aturan sembunyi umum tetap berlaku
///   di situs yang seharusnya bebas darinya (mis. YouTube, GitHub, Gmail).
struct Input {
    main: String,
    /// Domain situs dari baris generichide.
    generichide: Vec<String>,
    /// Baris generichide yang polanya bukan domain (mis. `://192.168.`).
    unsupported: usize,
    unhide: Unhide,
}

fn split_input(text: &str) -> Input {
    let mut input = Input { main: String::new(), generichide: Vec::new(), unsupported: 0, unhide: HashMap::new() };
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('!') {
            if let Some((domains, selector)) = trimmed.split_once("#@#") {
                let domains: Vec<String> =
                    domains.split(',').map(str::trim).filter(|d| !d.is_empty()).map(str::to_lowercase).collect();
                let entry = input.unhide.entry(selector.to_string()).or_insert_with(|| Some(Vec::new()));
                match (domains.is_empty(), entry) {
                    (true, entry) => *entry = None,
                    (false, Some(list)) => list.extend(domains),
                    (false, None) => {}
                }
                continue;
            }
            if let Some(domains) = generichide_domains(trimmed) {
                if domains.is_empty() {
                    input.unsupported += 1;
                }
                input.generichide.extend(domains);
                continue;
            }
        }
        input.main.push_str(line);
        input.main.push('\n');
    }
    input
}

fn is_hostname(host: &str) -> bool {
    !host.is_empty()
        && !host.ends_with('.')
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && host.chars().any(|c| c.is_ascii_alphabetic())
}

/// Domain situs dari baris `@@...$generichide`. None kalau baris itu bukan
/// generichide; daftar kosong kalau polanya tidak bisa dinyatakan sebagai
/// domain (mis. rentang IP `://192.168.`).
fn generichide_domains(line: &str) -> Option<Vec<String>> {
    let (pattern, options) = line.strip_prefix("@@")?.rsplit_once('$')?;
    let options: Vec<&str> = options.split(',').map(str::trim).collect();
    if !options.iter().any(|o| matches!(*o, "generichide" | "ghide")) {
        return None;
    }
    let host = pattern
        .strip_prefix("||")
        .or_else(|| pattern.strip_prefix("://"))
        .map(|h| h.trim_end_matches(['^', '/', ':']))
        .filter(|h| is_hostname(h));
    if let Some(host) = host {
        return Some(vec![host.to_lowercase()]);
    }
    if !pattern.is_empty() {
        return Some(Vec::new());
    }
    let domains: Vec<String> = options
        .iter()
        .find_map(|o| o.strip_prefix("domain="))
        .map(|d| d.split('|').map(str::to_lowercase).collect())
        .unwrap_or_default();
    Some(if domains.iter().all(|d| is_hostname(d)) { domains } else { Vec::new() })
}

/// Nama domain tanpa awalan `*` dari `if-domain`/`unless-domain`.
fn take_domains(trigger: &mut Map<String, Value>, key: &str) -> Vec<String> {
    match trigger.remove(key) {
        Some(Value::Array(items)) => {
            items.iter().filter_map(Value::as_str).map(|d| d.trim_start_matches('*').to_string()).collect()
        }
        _ => Vec::new(),
    }
}

/// Menulis domain dengan awalan `*`: di WebKit, domain tanpa `*` hanya cocok
/// dengan host yang persis sama (youtube.com tidak mencakup www.youtube.com).
fn put_domains(trigger: &mut Map<String, Value>, key: &str, domains: Vec<String>) {
    let mut seen = Vec::new();
    for d in domains {
        if !seen.contains(&d) {
            seen.push(d);
        }
    }
    if !seen.is_empty() {
        trigger.insert(key.to_string(), seen.into_iter().map(|d| Value::String(format!("*{d}"))).collect());
    }
}

fn covers(parent: &str, host: &str) -> bool {
    host == parent || host.strip_suffix(parent).is_some_and(|rest| rest.ends_with('.'))
}

/// Merapikan satu aturan `css-display-none`: domain mencakup subdomain, dan
/// pengecualian kosmetik diterapkan. None berarti aturan dibuang.
fn fix_cosmetic(mut rule: Value, unhide: &Unhide) -> Option<Value> {
    let selector = rule["action"]["selector"].as_str().unwrap_or_default().to_string();
    let trigger = rule.get_mut("trigger")?.as_object_mut()?;
    let mut if_domain = take_domains(trigger, "if-domain");
    let mut unless_domain = take_domains(trigger, "unless-domain");
    match unhide.get(&selector) {
        Some(None) => return None,
        Some(Some(excepted)) if if_domain.is_empty() => unless_domain.extend(excepted.iter().cloned()),
        Some(Some(excepted)) => {
            if_domain.retain(|d| !excepted.iter().any(|e| covers(e, d)));
            if if_domain.is_empty() {
                return None;
            }
        }
        None => {}
    }
    put_domains(trigger, "if-domain", if_domain);
    put_domains(trigger, "unless-domain", unless_domain);
    Some(rule)
}

fn is_exception(rule: &Value) -> bool {
    rule["action"]["type"] == "ignore-previous-rules"
}

/// Satu aturan `ignore-previous-rules` untuk semua situs generichide.
/// Pemicunya harus cocok dengan SEMUA muatan di halaman situs itu
/// (`url-filter` `.*` dan `if-domain`), bukan hanya dokumennya: WebKit
/// menambahkan lagi stylesheet aturan umum untuk setiap subresource yang
/// tidak terkena `ignore-previous-rules` (diamati: YouTube memuat CSS dari
/// ytimg.com, GitHub dari githubassets.com).
fn generichide_rule(domains: Vec<String>) -> Option<Value> {
    let mut trigger = Map::new();
    trigger.insert("url-filter".into(), Value::String(".*".into()));
    put_domains(&mut trigger, "if-domain", domains);
    trigger
        .contains_key("if-domain")
        .then(|| serde_json::json!({ "trigger": trigger, "action": { "type": "ignore-previous-rules" } }))
}

/// Mengonversi teks filter menjadi daftar-daftar aturan WebKit siap tulis,
/// masing-masing paling banyak `max` aturan. Mengembalikan juga jumlah baris
/// filter yang dilewati.
///
/// Susunan daftar: aturan sembunyi umum (tanpa `if-domain`) ditaruh di daftar
/// tersendiri, diikuti aturan generichide (`ignore-previous-rules` yang hanya
/// membatalkan aturan sebelumnya di daftar yang sama), lalu pengecualian.
/// Aturan jaringan dan aturan sembunyi khusus situs berada di daftar lain,
/// jadi generichide tidak menyentuhnya.
pub fn build_lists(filter_text: &str, max: usize) -> (Vec<Vec<Value>>, usize) {
    let input = split_input(filter_text);
    let (rules, skipped_main) = convert(&input.main);
    let generichide = generichide_rule(input.generichide);

    let mut generic = Vec::new();
    let mut others = Vec::new();
    let mut exceptions = Vec::new();
    for rule in rules {
        if rule["action"]["type"] == "css-display-none" {
            let Some(rule) = fix_cosmetic(rule, &input.unhide) else { continue };
            if rule["trigger"].get("if-domain").is_some() { others.push(rule) } else { generic.push(rule) }
        } else if is_exception(&rule) {
            exceptions.push(rule);
        } else {
            others.push(rule);
        }
    }

    let tail: Vec<Value> = generichide.into_iter().chain(exceptions.iter().cloned()).collect();
    let room = max.saturating_sub(tail.len()).max(1);
    let mut lists: Vec<Vec<Value>> =
        generic.chunks(room).map(|c| c.iter().chain(tail.iter()).cloned().collect()).collect();
    lists.extend(chunk_with(others, exceptions, max));
    (lists, skipped_main + input.unsupported)
}

/// Memecah aturan menjadi beberapa daftar berukuran paling banyak `max`.
/// `ignore-previous-rules` di WebKit hanya berlaku di dalam satu daftar, jadi
/// seluruh aturan pengecualian ditambahkan di akhir setiap potongan.
pub fn chunk(rules: Vec<Value>, max: usize) -> Vec<Vec<Value>> {
    let (exceptions, others): (Vec<Value>, Vec<Value>) = rules.into_iter().partition(is_exception);
    chunk_with(others, exceptions, max)
}

fn chunk_with(others: Vec<Value>, exceptions: Vec<Value>, max: usize) -> Vec<Vec<Value>> {
    let room = max.saturating_sub(exceptions.len()).max(1);
    let mut out: Vec<Vec<Value>> =
        others.chunks(room).map(|c| c.iter().chain(exceptions.iter()).cloned().collect()).collect();
    if out.is_empty() && !exceptions.is_empty() {
        out.push(exceptions);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_network_cosmetic_and_exception_rules() {
        let (rules, _) = convert("||ads.example.com^\n##.ad-banner\n@@||good.example.com^\n");
        assert!(rules.iter().any(|r| r["action"]["type"] == "block"
            && r["trigger"]["url-filter"].as_str().unwrap().contains("ads\\.example\\.com")));
        assert!(
            rules.iter().any(|r| r["action"]["type"] == "css-display-none" && r["action"]["selector"] == ".ad-banner")
        );
        assert!(rules.iter().any(|r| r["action"]["type"] == "ignore-previous-rules"));
    }

    #[test]
    fn chunk_appends_all_exceptions_to_every_chunk() {
        let block =
            |i: usize| serde_json::json!({"trigger": {"url-filter": format!("a{i}")}, "action": {"type": "block"}});
        let ignore = |i: usize| serde_json::json!({"trigger": {"url-filter": format!("b{i}")}, "action": {"type": "ignore-previous-rules"}});
        let rules: Vec<_> = (0..120_000).map(block).chain((0..1_000).map(ignore)).collect();
        let chunks = chunk(rules, 50_000);
        assert!(chunks.iter().all(|c| c.len() <= 50_000));
        assert!(
            chunks.iter().all(|c| c[c.len() - 1_000..].iter().all(|r| r["action"]["type"] == "ignore-previous-rules"))
        );
        let blocks: usize = chunks.iter().map(|c| c.len() - 1_000).sum();
        assert_eq!(blocks, 120_000);
    }

    fn css<'a>(rules: &'a [Value], selector: &str) -> Vec<&'a Value> {
        rules
            .iter()
            .filter(|r| r["action"]["type"] == "css-display-none" && r["action"]["selector"] == selector)
            .collect()
    }

    fn all_rules(text: &str) -> Vec<Value> {
        build_lists(text, MAX_RULES_PER_LIST).0.concat()
    }

    /// YouTube: `.video-ads` membungkus tombol "Lewati". EasyList
    /// menyembunyikannya secara umum, lalu mengecualikan youtube.com.
    #[test]
    fn cosmetic_exception_unhides_generic_selector_on_its_domains() {
        let all = all_rules("##.video-ads\nads.google.com,youtube.com#@#.video-ads\n");
        let rules = css(&all, ".video-ads");
        assert_eq!(rules.len(), 1, "{rules:?}");
        assert_eq!(rules[0]["trigger"]["unless-domain"], serde_json::json!(["*ads.google.com", "*youtube.com"]));
        assert!(rules[0]["trigger"].get("if-domain").is_none());
    }

    #[test]
    fn site_cosmetic_rules_cover_subdomains() {
        let all = all_rules("youtube.com##.feed-ad\n~example.com##.banner\n");
        assert_eq!(css(&all, ".feed-ad")[0]["trigger"]["if-domain"], serde_json::json!(["*youtube.com"]));
        assert_eq!(css(&all, ".banner")[0]["trigger"]["unless-domain"], serde_json::json!(["*example.com"]));
    }

    #[test]
    fn cosmetic_exception_removes_its_domains_from_site_rules() {
        let all = all_rules("a.com,b.com##.x\nb.com##.y\nm.c.com##.z\nb.com#@#.x\nb.com#@#.y\nc.com#@#.z\n");
        assert_eq!(css(&all, ".x")[0]["trigger"]["if-domain"], serde_json::json!(["*a.com"]));
        assert!(css(&all, ".y").is_empty());
        assert!(css(&all, ".z").is_empty(), "pengecualian c.com juga berlaku untuk m.c.com");
    }

    #[test]
    fn generic_cosmetic_exception_drops_the_rule() {
        assert!(css(&all_rules("##.x\n#@#.x\n"), ".x").is_empty());
    }

    /// WebKit menambahkan stylesheet aturan umum lagi untuk setiap subresource
    /// yang tidak terkena `ignore-previous-rules` (mis. CSS dari ytimg.com di
    /// halaman YouTube), jadi generichide harus cocok dengan semua muatan di
    /// halaman itu: `url-filter` `.*` dan `if-domain` situsnya.
    fn is_generichide(r: &Value, domain: &str) -> bool {
        r["action"]["type"] == "ignore-previous-rules"
            && r["trigger"]["url-filter"] == ".*"
            && r["trigger"]["if-domain"].as_array().is_some_and(|d| d.iter().any(|x| x == domain))
    }

    /// `$generichide` mematikan aturan sembunyi umum di situs itu, tetapi
    /// aturan khusus situs dan aturan jaringan tetap berlaku.
    #[test]
    fn generichide_cancels_only_generic_cosmetic_rules() {
        let text = "##.ad\n@@||www.youtube.com^$generichide\nyoutube.com##.feed-ad\n||ads.example.com^\n";
        let (lists, skipped) = build_lists(text, MAX_RULES_PER_LIST);
        assert_eq!(skipped, 0);
        let needle = "*www.youtube.com";
        let list = lists.iter().find(|l| !css(l, ".ad").is_empty()).expect("ada daftar berisi .ad");
        let ad = list.iter().position(|r| r["action"]["selector"] == ".ad").unwrap();
        let gh =
            list.iter().position(|r| is_generichide(r, needle)).expect("generichide di daftar yang sama dengan .ad");
        assert!(gh > ad);
        for list in &lists {
            if let Some(gh) = list.iter().position(|r| is_generichide(r, needle)) {
                assert!(
                    list[..gh]
                        .iter()
                        .all(|r| r["action"]["type"] == "css-display-none" && r["trigger"].get("if-domain").is_none()),
                    "hanya aturan sembunyi umum yang boleh berada sebelum generichide: {list:?}"
                );
            }
        }
        let all = lists.concat();
        assert!(!css(&all, ".feed-ad").is_empty());
        assert!(all.iter().any(|r| r["action"]["type"] == "block"));
    }

    #[test]
    fn generichide_with_domain_option() {
        let lists = build_lists("##.ad\n@@$generichide,domain=a.com|b.com\n", MAX_RULES_PER_LIST).0;
        let list = lists.iter().find(|l| !css(l, ".ad").is_empty()).unwrap();
        let gh = list
            .iter()
            .position(|r| is_generichide(r, "*a.com") && is_generichide(r, "*b.com"))
            .expect("generichide per domain");
        assert!(gh > list.iter().position(|r| r["action"]["selector"] == ".ad").unwrap());
    }

    #[test]
    fn generic_cosmetic_lists_respect_max_and_carry_exceptions() {
        let mut text: String = (0..10).map(|i| format!("##.ad{i}\n")).collect();
        text.push_str("@@||www.youtube.com^$generichide\n@@||good.example.com^\n||ads.example.com^\n");
        let lists = build_lists(&text, 6).0;
        assert!(lists.iter().all(|l| l.len() <= 6), "{:?}", lists.iter().map(Vec::len).collect::<Vec<_>>());
        let generic: usize =
            lists.iter().map(|l| l.iter().filter(|r| r["action"]["type"] == "css-display-none").count()).sum();
        assert_eq!(generic, 10);
        for list in lists.iter().filter(|l| l.iter().any(|r| r["action"]["type"] == "css-display-none")) {
            let last_css = list.iter().rposition(|r| r["action"]["type"] == "css-display-none").unwrap();
            let gh = list.iter().position(|r| is_generichide(r, "*www.youtube.com")).unwrap();
            assert!(gh > last_css);
            assert!(list.iter().any(|r| r["action"]["type"] == "ignore-previous-rules"
                && r["trigger"]["url-filter"].as_str().unwrap().contains("good\\.example\\.com")));
        }
    }

    #[test]
    fn chunk_without_exceptions_splits_by_max() {
        let rules: Vec<_> = (0..120_001)
            .map(|i| serde_json::json!({"trigger": {"url-filter": format!("a{i}")}, "action": {"type": "block"}}))
            .collect();
        let sizes: Vec<usize> = chunk(rules, 50_000).iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![50_000, 50_000, 20_001]);
    }
}
