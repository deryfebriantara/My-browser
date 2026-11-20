//! Bantuan khusus YouTube: melanjutkan video dari posisi terakhir dan
//! melewati iklan video.

use url::Url;

/// Script yang mengembalikan posisi video pertama di halaman (detik), atau null.
pub const VIDEO_TIME_JS: &str =
    "(function(){var v=document.querySelector('video');return v?v.currentTime:null;})()";

/// Script pemblokir bawaan yang melewati iklan video YouTube. Dipasang di
/// setiap halaman; script sendiri yang membatasi diri ke youtube.com.
pub const AD_SKIP_JS: &str = include_str!("youtube_ad_skip.js");

const HOSTS: [&str; 3] = ["youtube.com", "www.youtube.com", "m.youtube.com"];

/// Halaman tonton YouTube (`/watch?v=...`).
pub fn is_youtube_watch(url: &Url) -> bool {
    url.host_str().is_some_and(|h| HOSTS.contains(&h))
        && url.path() == "/watch"
        && url.query_pairs().any(|(k, _)| k == "v")
}

/// Menyetel parameter `t=<detik>s` (dibulatkan ke bawah), menggantikan `t` lama.
/// Posisi di bawah satu detik tidak mengubah URL.
pub fn with_resume_time(url: &Url, seconds: f64) -> Url {
    if !(seconds >= 1.0) {
        return url.clone();
    }
    let kept: Vec<(String, String)> =
        url.query_pairs().filter(|(k, _)| k != "t").map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    let mut out = url.clone();
    out.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .append_pair("t", &format!("{}s", seconds.floor() as u64));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url { Url::parse(s).unwrap() }

    #[test]
    fn adds_t_param() {
        assert_eq!(with_resume_time(&u("https://www.youtube.com/watch?v=abc"), 125.7).as_str(), "https://www.youtube.com/watch?v=abc&t=125s");
    }

    #[test]
    fn replaces_existing_t_keeps_others() {
        assert_eq!(
            with_resume_time(&u("https://www.youtube.com/watch?v=abc&t=10s&list=PL1"), 60.0).as_str(),
            "https://www.youtube.com/watch?v=abc&list=PL1&t=60s"
        );
    }

    #[test]
    fn under_one_second_unchanged() {
        assert_eq!(with_resume_time(&u("https://www.youtube.com/watch?v=abc"), 0.4).as_str(), "https://www.youtube.com/watch?v=abc");
    }

    #[test]
    fn detects_watch_pages_only() {
        assert!(is_youtube_watch(&u("https://m.youtube.com/watch?v=x")));
        assert!(!is_youtube_watch(&u("https://www.youtube.com/")));
        assert!(!is_youtube_watch(&u("https://youtube.com.evil.com/watch?v=x")));
    }
}
