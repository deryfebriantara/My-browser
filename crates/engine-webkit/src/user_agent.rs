//! User agent yang menyamai Safari, supaya login Google dan situs lain tidak
//! memperlakukan browser ini sebagai webview tertanam.

use objc2_foundation::{NSBundle, NSString};

/// Versi Safari yang dipakai kalau versi terpasang tidak bisa dibaca.
pub const FALLBACK_SAFARI_VERSION: &str = "18.6";

/// Nilai untuk `applicationNameForUserAgent`. WebKit menambahkannya di akhir
/// user agent bawaan, sehingga hasilnya sama dengan user agent Safari.
pub fn safari_application_name(safari_version: &str) -> String {
    format!("Version/{safari_version} Safari/605.1.15")
}

/// User agent Safari iPhone dengan versi yang sama, untuk emulasi HP.
pub fn iphone_user_agent(safari_version: &str) -> String {
    let os = safari_version.replace('.', "_");
    format!(
        "Mozilla/5.0 (iPhone; CPU iPhone OS {os} like Mac OS X) AppleWebKit/605.1.15 \
         (KHTML, like Gecko) Version/{safari_version} Mobile/15E148 Safari/604.1"
    )
}

/// Membaca `CFBundleShortVersionString` dari `/Applications/Safari.app`.
pub fn installed_safari_version() -> Option<String> {
    let bundle = NSBundle::bundleWithPath(&NSString::from_str("/Applications/Safari.app"))?;
    let value = bundle.objectForInfoDictionaryKey(&NSString::from_str("CFBundleShortVersionString"))?;
    let version = value.downcast::<NSString>().ok()?;
    Some(version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_name_matches_safari_format() {
        assert_eq!(safari_application_name("18.6"), "Version/18.6 Safari/605.1.15");
    }

    #[test]
    fn iphone_user_agent_matches_mobile_safari_format() {
        assert_eq!(
            iphone_user_agent("18.6"),
            "Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Mobile/15E148 Safari/604.1"
        );
    }

    #[test]
    fn installed_safari_version_is_numeric() {
        let v = installed_safari_version().expect("Safari terpasang di /Applications");
        assert!(v.split('.').next().unwrap().parse::<u32>().is_ok());
    }
}
