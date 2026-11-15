//! Nama berkas tujuan download yang aman dan tidak menimpa berkas lain.

use std::path::{Path, PathBuf};

/// Hanya komponen terakhir nama dari server yang dipakai (tanpa path).
/// Nama kosong, `.`, atau `..` menjadi `download`. Kalau sudah ada, ditambah
/// penghitung sebelum ekstensi: `a.pdf`, `a (1).pdf`, `a (2).pdf`.
pub fn unique_path(dir: &Path, suggested: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let name = suggested.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let name = if name.is_empty() || name == "." || name == ".." { "download" } else { name };
    let first = dir.join(name);
    if !exists(&first) {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !exists(p))
        .expect("penghitung tidak terbatas")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn ex<'a>(names: &'a [&'a str]) -> impl Fn(&Path) -> bool + 'a {
        move |p| names.iter().any(|n| p.file_name().unwrap() == *n)
    }

    #[test]
    fn keeps_name_when_free() {
        assert_eq!(unique_path(Path::new("/d"), "a.pdf", ex(&[])), Path::new("/d/a.pdf"));
    }

    #[test]
    fn adds_counter_before_extension() {
        assert_eq!(unique_path(Path::new("/d"), "a.pdf", ex(&["a.pdf", "a (1).pdf"])), Path::new("/d/a (2).pdf"));
    }

    #[test]
    fn counter_without_extension() {
        assert_eq!(unique_path(Path::new("/d"), "README", ex(&["README"])), Path::new("/d/README (1)"));
    }

    #[test]
    fn strips_path_components() {
        assert_eq!(unique_path(Path::new("/d"), "../../etc/passwd", ex(&[])), Path::new("/d/passwd"));
    }

    #[test]
    fn empty_name_becomes_download() {
        assert_eq!(unique_path(Path::new("/d"), "", ex(&[])), Path::new("/d/download"));
        assert_eq!(unique_path(Path::new("/d"), "..", ex(&[])), Path::new("/d/download"));
    }
}
