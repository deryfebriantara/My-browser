//! Kebijakan menidurkan tab: keputusan murni, tanpa efek samping.

use crate::tabs::TabId;
use std::time::{Duration, Instant};

/// Keadaan satu tab yang relevan untuk keputusan tidur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabSnapshot {
    pub id: TabId,
    pub awake: bool,
    pub active: bool,
    pub last_viewed: Instant,
    pub media_active: bool,
    /// Ada teks yang sudah diketik pengguna tapi belum dikirim.
    pub draft: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepPolicy {
    /// Tab latar ditidurkan setelah tidak dilihat selama ini.
    pub idle_after: Duration,
    /// Jumlah maksimum tab bangun, termasuk tab aktif.
    pub max_awake: usize,
}

impl Default for SleepPolicy {
    fn default() -> Self {
        Self { idle_after: Duration::from_secs(30 * 60), max_awake: 5 }
    }
}

/// Sinyal memory pressure dari macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressure {
    Normal,
    Warning,
    Critical,
}

/// Tab yang harus ditidurkan sekarang, paling lama tidak dilihat lebih dulu.
/// Tab aktif dan tab yang sedang memakai media tidak pernah dipilih. Tab
/// dengan teks yang belum dikirim baru dipilih saat memory kritis.
pub fn tabs_to_sleep(tabs: &[TabSnapshot], now: Instant, policy: &SleepPolicy, pressure: Pressure) -> Vec<TabId> {
    let mut candidates: Vec<&TabSnapshot> = tabs
        .iter()
        .filter(|t| t.awake && !t.active && !t.media_active && (!t.draft || pressure == Pressure::Critical))
        .collect();
    candidates.sort_by_key(|t| t.last_viewed);

    if pressure != Pressure::Normal {
        return candidates.iter().map(|t| t.id).collect();
    }

    let (mut chosen, rest): (Vec<&TabSnapshot>, Vec<&TabSnapshot>) =
        candidates.into_iter().partition(|t| now.saturating_duration_since(t.last_viewed) >= policy.idle_after);

    let mut awake = tabs.iter().filter(|t| t.awake).count() - chosen.len();
    for t in rest {
        if awake <= policy.max_awake {
            break;
        }
        chosen.push(t);
        awake -= 1;
    }
    chosen.sort_by_key(|t| t.last_viewed);
    chosen.into_iter().map(|t| t.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::TabId;
    use std::time::{Duration, Instant};

    fn snap(id: u64, awake: bool, active: bool, idle_s: u64, media: bool, now: Instant) -> TabSnapshot {
        TabSnapshot { id: TabId(id), awake, active, last_viewed: now - Duration::from_secs(idle_s), media_active: media, draft: false }
    }
    fn drafted(t: TabSnapshot) -> TabSnapshot {
        TabSnapshot { draft: true, ..t }
    }
    /// Kebijakan kecil supaya angka di test mudah dibaca.
    const POLICY: SleepPolicy = SleepPolicy { idle_after: Duration::from_secs(300), max_awake: 3 };
    fn run(tabs: &[TabSnapshot], now: Instant, p: Pressure) -> Vec<TabId> {
        tabs_to_sleep(tabs, now, &POLICY, p)
    }

    #[test]
    fn default_policy_is_30_minutes_and_5_tabs() {
        let p = SleepPolicy::default();
        assert_eq!((p.idle_after, p.max_awake), (Duration::from_secs(1800), 5));
    }

    #[test]
    fn draft_tab_kept_until_memory_is_critical() {
        let n = Instant::now() + Duration::from_secs(10_000);
        let t = [snap(1, true, true, 0, false, n), drafted(snap(2, true, false, 9_000, false, n))];
        assert!(run(&t, n, Pressure::Normal).is_empty());
        assert!(run(&t, n, Pressure::Warning).is_empty());
        assert_eq!(run(&t, n, Pressure::Critical), vec![TabId(2)]);
    }

    #[test]
    fn draft_tabs_count_toward_cap_but_are_skipped() {
        let n = Instant::now() + Duration::from_secs(10_000);
        let t = [
            snap(1, true, true, 0, false, n),
            drafted(snap(2, true, false, 100, false, n)),
            snap(3, true, false, 50, false, n),
            snap(4, true, false, 10, false, n),
        ];
        assert_eq!(run(&t, n, Pressure::Normal), vec![TabId(3)]);
    }

    #[test]
    fn sleeps_background_tab_after_idle_timeout() {
        let n = Instant::now() + Duration::from_secs(10_000);
        assert_eq!(run(&[snap(1, true, true, 0, false, n), snap(2, true, false, 301, false, n)], n, Pressure::Normal), vec![TabId(2)]);
    }

    #[test]
    fn keeps_tab_idle_less_than_timeout() {
        let n = Instant::now() + Duration::from_secs(10_000);
        assert!(run(&[snap(1, true, true, 0, false, n), snap(2, true, false, 299, false, n)], n, Pressure::Normal).is_empty());
    }

    #[test]
    fn never_sleeps_active_or_media_tab() {
        let n = Instant::now() + Duration::from_secs(10_000);
        assert!(run(&[snap(1, true, true, 9_000, false, n), snap(2, true, false, 9_000, true, n)], n, Pressure::Critical).is_empty());
    }

    #[test]
    fn caps_awake_tabs_sleeping_least_recently_viewed() {
        let n = Instant::now() + Duration::from_secs(10_000);
        let t = [snap(1, true, true, 0, false, n), snap(2, true, false, 10, false, n), snap(3, true, false, 20, false, n), snap(4, true, false, 30, false, n)];
        assert_eq!(run(&t, n, Pressure::Normal), vec![TabId(4)]);
    }

    #[test]
    fn media_tabs_count_toward_cap_but_are_skipped() {
        let n = Instant::now() + Duration::from_secs(10_000);
        let t = [snap(1, true, true, 0, false, n), snap(2, true, false, 100, true, n), snap(3, true, false, 50, false, n), snap(4, true, false, 10, false, n)];
        assert_eq!(run(&t, n, Pressure::Normal), vec![TabId(3)]);
    }

    #[test]
    fn pressure_sleeps_all_eligible_background_tabs() {
        let n = Instant::now() + Duration::from_secs(10_000);
        let t = [snap(1, true, true, 0, false, n), snap(2, true, false, 5, false, n), snap(3, false, false, 5, false, n)];
        assert_eq!(run(&t, n, Pressure::Warning), vec![TabId(2)]);
    }
}
