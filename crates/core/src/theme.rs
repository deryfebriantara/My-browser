//! Tema gradasi Space dan aturan kontras teks sidebar (putih).

pub type Rgb = [u8; 3];

/// Gradasi latar jendela satu Space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub id: &'static str,
    /// Tiga warna gradasi, dari pojok kiri atas ke kanan bawah.
    pub stops: [Rgb; 3],
    pub angle_deg: f64,
    /// Warna titik Space di pemilih Space.
    pub dot: Rgb,
}

/// Ungu ke merah muda. Ujung terang mockup (#c45a8c, 4,05:1) digelapkan
/// menjadi #a34b74 (5,5:1) supaya teks putih dan teks sekunder lolos 4,5:1.
pub const PERSONAL: Theme = Theme {
    id: "personal",
    stops: [[0x2b, 0x24, 0x70], [0x6a, 0x3f, 0xa0], [0xa3, 0x4b, 0x74]],
    angle_deg: 165.0,
    dot: [0xc4, 0x5a, 0x8c],
};

/// Biru tua ke hijau toska. Ujung terang mockup (#3aa39a, 3,05:1) digelapkan
/// menjadi #29746d (5,5:1).
pub const WORK: Theme = Theme {
    id: "work",
    stops: [[0x0f, 0x3a, 0x5f], [0x1f, 0x6f, 0x8b], [0x29, 0x74, 0x6d]],
    angle_deg: 165.0,
    dot: [0x3a, 0xa3, 0x9a],
};

pub const PRESETS: &[Theme] = &[PERSONAL, WORK];

/// Tema menurut id; id yang tidak dikenal memakai Personal.
pub fn theme(id: &str) -> &'static Theme {
    PRESETS.iter().find(|t| t.id == id).unwrap_or(&PRESETS[0])
}

/// Alpha teks putih untuk label bagian dan baris tab yang tidur: alpha
/// terkecil (kelipatan 0,05) yang masih lolos 4,5:1 di semua warna preset.
pub const SECONDARY_ALPHA: f64 = 0.9;

fn channel(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn luminance(c: Rgb) -> f64 {
    0.2126 * channel(c[0]) + 0.7152 * channel(c[1]) + 0.0722 * channel(c[2])
}

/// Rasio kontras WCAG 2.x antara dua warna.
pub fn contrast_ratio(fg: Rgb, bg: Rgb) -> f64 {
    let (a, b) = (luminance(fg), luminance(bg));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Warna `fg` dengan alpha `alpha` di atas `bg`.
pub fn blend(fg: Rgb, bg: Rgb, alpha: f64) -> Rgb {
    let mix = |f: u8, b: u8| (f64::from(f) * alpha + f64::from(b) * (1.0 - alpha)).round() as u8;
    [mix(fg[0], bg[0]), mix(fg[1], bg[1]), mix(fg[2], bg[2])]
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgb = [255, 255, 255];

    #[test]
    fn contrast_ratio_matches_known_values() {
        assert!((contrast_ratio(WHITE, [0, 0, 0]) - 21.0).abs() < 0.01);
        // Merah muda mockup tidak lolos 4.5:1; karena itu ujung gradasinya digelapkan.
        assert!((contrast_ratio(WHITE, [0xc4, 0x5a, 0x8c]) - 4.05).abs() < 0.05);
    }

    #[test]
    fn every_preset_stop_passes_for_text_and_secondary_text() {
        for t in PRESETS {
            for s in t.stops {
                assert!(contrast_ratio(WHITE, s) >= 4.5, "{} {:?}", t.id, s);
                assert!(contrast_ratio(blend(WHITE, s, SECONDARY_ALPHA), s) >= 4.5, "{} {:?}", t.id, s);
            }
        }
    }

    #[test]
    fn unknown_theme_falls_back_to_personal() {
        assert_eq!(theme("nope").id, "personal");
        assert_eq!(theme("work").id, "work");
    }
}
