//! Zona waktu lokal (untuk batas "hari ini" dan waktu di riwayat).

/// Selisih waktu lokal dari UTC (detik) pada saat `unix`, mengikuti
/// pengaturan zona waktu macOS termasuk musim panas.
pub fn offset_at(unix: u64) -> i32 {
    let time = unix as libc::time_t;
    // SAFETY: tm berisi bilangan bulat dan pointer; nol adalah nilai yang sah.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r hanya menulis ke `tm` milik kita (aman lintas thread).
    let ok = unsafe { !libc::localtime_r(&time, &mut tm).is_null() };
    if ok { tm.tm_gmtoff as i32 } else { 0 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn offset_is_within_a_day() {
        let offset = super::offset_at(1_790_000_000);
        assert!(offset.abs() < 86_400);
    }
}
