//! Penanda nama berkas download yang sedang dipakai, lintas semua page, supaya
//! dua download bersamaan dengan nama sama tidak berebut berkas yang sama.

use browser_core::downloads::unique_path;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

thread_local! {
    static RESERVED: RefCell<HashSet<PathBuf>> = RefCell::default();
}

/// Memilih nama yang belum ada di disk maupun di daftar `reserved`, lalu
/// menandainya terpakai.
pub(crate) fn reserve(
    reserved: &mut HashSet<PathBuf>,
    dir: &Path,
    suggested: &str,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    let path = unique_path(dir, suggested, |p| exists(p) || reserved.contains(p));
    reserved.insert(path.clone());
    path
}

/// `reserve` dengan daftar bersama milik main thread dan pengecekan disk.
pub(crate) fn reserve_on_disk(dir: &Path, suggested: &str) -> PathBuf {
    RESERVED.with(|r| reserve(&mut r.borrow_mut(), dir, suggested, |p| p.exists()))
}

/// Dipanggil saat download selesai atau gagal.
pub(crate) fn release(path: &Path) {
    RESERVED.with(|r| r.borrow_mut().remove(path));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn concurrent_downloads_with_same_name_get_distinct_paths() {
        let mut reserved = HashSet::new();
        let a = reserve(&mut reserved, Path::new("/d"), "a.zip", |_| false);
        let b = reserve(&mut reserved, Path::new("/d"), "a.zip", |_| false);
        assert_eq!(a, Path::new("/d/a.zip"));
        assert_eq!(b, Path::new("/d/a (1).zip"));
    }

    #[test]
    fn released_name_can_be_reused() {
        let mut reserved = HashSet::new();
        let a = reserve(&mut reserved, Path::new("/d"), "a.zip", |_| false);
        reserved.remove(&a);
        assert_eq!(reserve(&mut reserved, Path::new("/d"), "a.zip", |_| false), Path::new("/d/a.zip"));
    }
}
