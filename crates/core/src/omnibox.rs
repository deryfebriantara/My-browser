//! Mengubah teks address bar menjadi URL tujuan.

use std::net::Ipv4Addr;
use url::Url;

/// Mengurai input address bar. Aturan diterapkan berurutan; aturan pertama yang
/// cocok menentukan hasil. Teks yang tidak terlihat seperti alamat dikirim ke
/// mesin pencari lewat `search_template` (`{}` diganti kata kunci).
pub fn parse_input(input: &str, search_template: &str) -> Option<Url> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }

    let lower = text.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Url::parse(text).ok().or_else(|| search(text, search_template));
    }
    if lower.starts_with("about:") {
        return Url::parse(text).ok();
    }
    if text.chars().any(char::is_whitespace) {
        return search(text, search_template);
    }

    let host = host_part(text);
    if host.eq_ignore_ascii_case("localhost") || host.parse::<Ipv4Addr>().is_ok() {
        return Url::parse(&format!("http://{text}")).ok();
    }
    if looks_like_domain(host) {
        return Url::parse(&format!("https://{text}")).ok();
    }
    search(text, search_template)
}

/// Teks sebelum `/` pertama, tanpa akhiran `:<angka>`.
fn host_part(text: &str) -> &str {
    let authority = text.split('/').next().unwrap_or(text);
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    }
}

/// Mengandung titik dan label terakhirnya minimal dua huruf ASCII.
fn looks_like_domain(host: &str) -> bool {
    match host.rsplit_once('.') {
        Some((name, tld)) => !name.is_empty() && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()),
        None => false,
    }
}

fn search(text: &str, template: &str) -> Option<Url> {
    let query: String = url::form_urlencoded::byte_serialize(text.as_bytes()).collect();
    Url::parse(&template.replace("{}", &query)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str = "https://www.google.com/search?q={}";
    fn parse(s: &str) -> Option<String> {
        parse_input(s, G).map(|u| u.to_string())
    }

    #[test] fn domain_gets_https() { assert_eq!(parse("youtube.com").unwrap(), "https://youtube.com/"); }
    #[test] fn explicit_http_kept() { assert_eq!(parse("http://example.com").unwrap(), "http://example.com/"); }
    #[test] fn domain_with_path_kept() { assert_eq!(parse("github.com/servo/servo").unwrap(), "https://github.com/servo/servo"); }
    #[test] fn words_become_search() { assert_eq!(parse("resep nasi goreng").unwrap(), "https://www.google.com/search?q=resep+nasi+goreng"); }
    #[test] fn trims_whitespace() { assert_eq!(parse("  youtube.com  ").unwrap(), "https://youtube.com/"); }
    #[test] fn empty_is_none() { assert!(parse("   ").is_none()); }
    #[test] fn localhost_uses_http() { assert_eq!(parse("localhost:3000").unwrap(), "http://localhost:3000/"); }
    #[test] fn ipv4_uses_http() { assert_eq!(parse("192.168.1.1").unwrap(), "http://192.168.1.1/"); }
    #[test] fn numeric_tld_is_search() { assert_eq!(parse("v1.2").unwrap(), "https://www.google.com/search?q=v1.2"); }
    #[test] fn javascript_scheme_is_search() { assert!(parse("javascript:alert(1)").unwrap().starts_with("https://www.google.com/search?q=javascript")); }
    #[test] fn non_ascii_query_encoded() { assert_eq!(parse("café").unwrap(), "https://www.google.com/search?q=caf%C3%A9"); }
    #[test] fn about_blank_allowed() { assert_eq!(parse("about:blank").unwrap(), "about:blank"); }
}
