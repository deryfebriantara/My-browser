//! Pencocokan host untuk pengecualian pemblokir iklan per situs.

/// Huruf kecil, tanpa awalan `www.`.
pub fn normalize_host(host: &str) -> String {
    let lower = host.to_ascii_lowercase();
    lower.strip_prefix("www.").map(str::to_string).unwrap_or(lower)
}

/// Cocok kalau host sama persis dengan salah satu pengecualian atau subdomainnya.
pub fn host_matches(host: &str, exceptions: &[String]) -> bool {
    let host = normalize_host(host);
    exceptions.iter().any(|e| host == *e || host.ends_with(&format!(".{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_www_and_lowercases() {
        assert_eq!(normalize_host("WWW.Detik.com"), "detik.com");
    }

    #[test]
    fn matches_exact_and_subdomains_only() {
        let ex = vec!["detik.com".to_string()];
        assert!(host_matches("detik.com", &ex));
        assert!(host_matches("news.detik.com", &ex));
        assert!(host_matches("WWW.DETIK.COM", &ex));
        assert!(!host_matches("notdetik.com", &ex));
    }
}
