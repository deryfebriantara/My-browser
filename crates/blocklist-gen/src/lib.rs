//! Mengubah daftar filter EasyList menjadi daftar aturan content blocker WebKit.

use adblock::lists::{FilterSet, ParseOptions};
use serde_json::Value;

/// Batas aturan per daftar. WebKit membatasi jumlah aturan per content rule
/// list; 50.000 adalah batas lama yang aman untuk semua versi.
pub const MAX_RULES_PER_LIST: usize = 50_000;

/// Mengonversi teks filter. Mengembalikan aturan WebKit (sudah diurutkan supaya
/// `ignore-previous-rules` benar) dan jumlah baris filter yang dilewati.
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

fn is_exception(rule: &Value) -> bool {
    rule["action"]["type"] == "ignore-previous-rules"
}

/// Memecah aturan menjadi beberapa daftar berukuran paling banyak `max`.
/// `ignore-previous-rules` di WebKit hanya berlaku di dalam satu daftar, jadi
/// seluruh aturan pengecualian ditambahkan di akhir setiap potongan.
pub fn chunk(rules: Vec<Value>, max: usize) -> Vec<Vec<Value>> {
    let (exceptions, others): (Vec<Value>, Vec<Value>) = rules.into_iter().partition(is_exception);
    let room = max.saturating_sub(exceptions.len()).max(1);
    let mut out: Vec<Vec<Value>> = others
        .chunks(room)
        .map(|c| c.iter().cloned().chain(exceptions.iter().cloned()).collect())
        .collect();
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
        assert!(rules.iter().any(|r| r["action"]["type"] == "css-display-none" && r["action"]["selector"] == ".ad-banner"));
        assert!(rules.iter().any(|r| r["action"]["type"] == "ignore-previous-rules"));
    }

    #[test]
    fn chunk_appends_all_exceptions_to_every_chunk() {
        let block = |i: usize| serde_json::json!({"trigger": {"url-filter": format!("a{i}")}, "action": {"type": "block"}});
        let ignore = |i: usize| serde_json::json!({"trigger": {"url-filter": format!("b{i}")}, "action": {"type": "ignore-previous-rules"}});
        let rules: Vec<_> = (0..120_000).map(block).chain((0..1_000).map(ignore)).collect();
        let chunks = chunk(rules, 50_000);
        assert!(chunks.iter().all(|c| c.len() <= 50_000));
        assert!(chunks.iter().all(|c| c[c.len() - 1_000..].iter().all(|r| r["action"]["type"] == "ignore-previous-rules")));
        let blocks: usize = chunks.iter().map(|c| c.len() - 1_000).sum();
        assert_eq!(blocks, 120_000);
    }

    #[test]
    fn chunk_without_exceptions_splits_by_max() {
        let rules: Vec<_> = (0..120_001).map(|i| serde_json::json!({"trigger": {"url-filter": format!("a{i}")}, "action": {"type": "block"}})).collect();
        let sizes: Vec<usize> = chunk(rules, 50_000).iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![50_000, 50_000, 20_001]);
    }
}
