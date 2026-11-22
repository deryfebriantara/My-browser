//! Rencana screenshot halaman penuh dan screenshot elemen. Dokumen dipotong
//! menjadi beberapa gambar supaya setiap gambar tetap tajam saat dilihat
//! Claude; satu gambar yang sangat tinggi akan diperkecil sampai tak terbaca.

use crate::engine::Rect;

/// Lebar gambar paling besar, dalam piksel.
pub const MAX_WIDTH: f64 = 1280.0;
/// Tinggi satu potongan gambar paling besar, dalam piksel.
pub const TILE_HEIGHT: f64 = 1600.0;
/// Jumlah potongan paling banyak; bagian dokumen sesudahnya tidak difoto.
pub const MAX_TILES: usize = 6;
/// Elemen kecil diperbesar sampai skala ini supaya detailnya terlihat.
pub const MAX_ELEMENT_SCALE: f64 = 2.0;
/// Ruang di sekitar elemen, dalam poin CSS.
pub const ELEMENT_PADDING: f64 = 8.0;

/// Bagian dokumen yang difoto (koordinat dokumen, poin CSS) dan skalanya.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Piksel per poin CSS.
    pub scale: f64,
    pub tiles: Vec<Rect>,
    /// Dokumen lebih panjang dari `MAX_TILES` potongan.
    pub truncated: bool,
}

/// Seluruh dokumen, diperkecil kalau lebih lebar dari `MAX_WIDTH`.
pub fn full_page(width: f64, height: f64) -> Plan {
    let scale = (MAX_WIDTH / width).min(1.0);
    split(Rect { x: 0.0, y: 0.0, width, height }, scale)
}

/// Satu elemen plus sedikit ruang di sekelilingnya. Elemen kecil diperbesar.
pub fn element(area: Rect) -> Plan {
    let x = (area.x - ELEMENT_PADDING).max(0.0);
    let y = (area.y - ELEMENT_PADDING).max(0.0);
    let padded = Rect {
        x,
        y,
        width: area.x + area.width + ELEMENT_PADDING - x,
        height: area.y + area.height + ELEMENT_PADDING - y,
    };
    let scale = (MAX_WIDTH / padded.width).min(MAX_ELEMENT_SCALE);
    split(padded, scale)
}

fn split(area: Rect, scale: f64) -> Plan {
    let step = TILE_HEIGHT / scale;
    let end = area.y + area.height;
    let mut tiles = Vec::new();
    let mut y = area.y;
    while y < end && tiles.len() < MAX_TILES {
        tiles.push(Rect { x: area.x, y, width: area.width, height: step.min(end - y) });
        y += step;
    }
    Plan { scale, tiles, truncated: y < end }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect { x, y, width, height }
    }

    #[test]
    fn short_page_is_one_tile_at_full_size() {
        let plan = full_page(1000.0, 900.0);
        assert_eq!(plan, Plan { scale: 1.0, tiles: vec![r(0.0, 0.0, 1000.0, 900.0)], truncated: false });
    }

    #[test]
    fn long_page_is_split_into_tiles() {
        let plan = full_page(800.0, 4000.0);
        assert_eq!(
            plan.tiles,
            vec![r(0.0, 0.0, 800.0, 1600.0), r(0.0, 1600.0, 800.0, 1600.0), r(0.0, 3200.0, 800.0, 800.0)]
        );
        assert!(!plan.truncated);
    }

    #[test]
    fn wide_page_is_scaled_down_and_tiles_cover_more_document() {
        let plan = full_page(2560.0, 4000.0);
        assert_eq!(plan.scale, 0.5);
        assert_eq!(plan.tiles, vec![r(0.0, 0.0, 2560.0, 3200.0), r(0.0, 3200.0, 2560.0, 800.0)]);
    }

    #[test]
    fn very_long_page_is_truncated_after_max_tiles() {
        let plan = full_page(800.0, 100_000.0);
        assert_eq!(plan.tiles.len(), MAX_TILES);
        assert!(plan.truncated);
        assert_eq!(plan.tiles.last().unwrap().y, 1600.0 * (MAX_TILES - 1) as f64);
    }

    #[test]
    fn small_element_is_padded_and_enlarged() {
        let plan = element(r(100.0, 2000.0, 120.0, 40.0));
        assert_eq!(plan.scale, 2.0);
        assert_eq!(plan.tiles, vec![r(92.0, 1992.0, 136.0, 56.0)]);
    }

    #[test]
    fn element_padding_stays_inside_the_document() {
        let plan = element(r(0.0, 3.0, 50.0, 10.0));
        assert_eq!(plan.tiles, vec![r(0.0, 0.0, 58.0, 21.0)]);
    }

    #[test]
    fn wide_element_is_not_enlarged_past_max_width() {
        let plan = element(r(8.0, 8.0, 2544.0, 100.0));
        assert_eq!(plan.scale, 0.5);
    }
}
