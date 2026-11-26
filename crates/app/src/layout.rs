//! Perhitungan tata letak jendela (murni, tanpa AppKit). Koordinat AppKit:
//! titik nol di kiri bawah.

/// Jarak kartu dari tepi jendela, dan jarak antarkartu.
pub const INSET: f64 = 8.0;
pub const SIDEBAR_MIN: f64 = 200.0;
pub const SIDEBAR_MAX: f64 = 360.0;
/// Lebar panel Tanya Claude.
pub const PANEL_WIDTH: f64 = 380.0;
/// Lebar kartu halaman paling kecil sebelum sidebar dipersempit.
pub const CARD_MIN: f64 = 320.0;
pub const CARD_RADIUS: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Frame {
    fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w: w.max(0.0), h: h.max(0.0) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    /// Sidebar di kiri, kartu di sebelahnya.
    Docked,
    /// Sidebar disembunyikan (⌘S): kartu memenuhi jendela.
    Hidden,
    /// Sidebar tersembunyi yang sedang muncul melayang karena kursor di tepi kiri.
    #[cfg_attr(not(test), expect(dead_code, reason = "overlay sidebar dibuat di Task 10"))]
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frames {
    pub sidebar: Frame,
    pub card: Frame,
    pub panel: Option<Frame>,
}

/// Frame sidebar, kartu halaman, dan panel (kalau terbuka) untuk jendela
/// `window_w`×`window_h`. Lebar sidebar dibatasi 200–360 dan dipersempit ke
/// 200 dulu kalau kartu akan lebih sempit dari `CARD_MIN`.
pub fn layout(window_w: f64, window_h: f64, mode: SidebarMode, sidebar_w: f64, panel: bool) -> Frames {
    let panel_space = if panel { PANEL_WIDTH + INSET } else { 0.0 };
    let inner_h = window_h - 2.0 * INSET;
    let mut sidebar = sidebar_w.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
    if mode == SidebarMode::Docked {
        let card = window_w - sidebar - INSET - panel_space;
        if card < CARD_MIN {
            sidebar = (sidebar - (CARD_MIN - card)).max(SIDEBAR_MIN);
        }
    }
    let (sidebar_frame, card_x) = match mode {
        SidebarMode::Docked => (Frame::new(0.0, 0.0, sidebar, window_h), sidebar),
        SidebarMode::Hidden | SidebarMode::Overlay => (Frame::new(INSET, INSET, sidebar, inner_h), INSET),
    };
    let card = Frame::new(card_x, INSET, window_w - card_x - INSET - panel_space, inner_h);
    let panel = panel.then(|| Frame::new((window_w - INSET - PANEL_WIDTH).max(0.0), INSET, PANEL_WIDTH, inner_h));
    Frames { sidebar: sidebar_frame, card, panel }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(x: f64, y: f64, w: f64, h: f64) -> Frame {
        Frame { x, y, w, h }
    }

    #[test]
    fn docked_default_frames() {
        let l = layout(1280.0, 800.0, SidebarMode::Docked, 240.0, false);
        assert_eq!(l.sidebar, f(0.0, 0.0, 240.0, 800.0));
        assert_eq!(l.card, f(240.0, 8.0, 1032.0, 784.0));
        assert_eq!(l.panel, None);
    }

    #[test]
    fn hidden_card_fills_window_with_inset() {
        let l = layout(1280.0, 800.0, SidebarMode::Hidden, 240.0, false);
        assert_eq!(l.card, f(8.0, 8.0, 1264.0, 784.0));
    }

    #[test]
    fn overlay_sidebar_floats_over_card() {
        let l = layout(1280.0, 800.0, SidebarMode::Overlay, 240.0, false);
        assert_eq!(l.sidebar, f(8.0, 8.0, 240.0, 784.0));
        assert_eq!(l.card, f(8.0, 8.0, 1264.0, 784.0));
    }

    #[test]
    fn panel_takes_right_side_with_gap() {
        let l = layout(1280.0, 800.0, SidebarMode::Docked, 240.0, true);
        assert_eq!(l.panel, Some(f(892.0, 8.0, 380.0, 784.0)));
        assert_eq!(l.card, f(240.0, 8.0, 1280.0 - 240.0 - 8.0 - 388.0, 784.0));
    }

    #[test]
    fn narrow_window_with_panel_shrinks_sidebar_first_and_never_negative() {
        let l = layout(800.0, 500.0, SidebarMode::Docked, 360.0, true);
        assert_eq!(l.sidebar.w, 200.0);
        for frame in [l.sidebar, l.card, l.panel.unwrap()] {
            assert!(frame.w >= 0.0 && frame.h >= 0.0, "{frame:?}");
        }
        let tiny = layout(100.0, 10.0, SidebarMode::Hidden, 360.0, true);
        for frame in [tiny.sidebar, tiny.card, tiny.panel.unwrap()] {
            assert!(frame.w >= 0.0 && frame.h >= 0.0, "{frame:?}");
        }
    }

    #[test]
    fn sidebar_width_is_clamped() {
        assert_eq!(layout(1280.0, 800.0, SidebarMode::Docked, 100.0, false).sidebar.w, 200.0);
        assert_eq!(layout(1280.0, 800.0, SidebarMode::Docked, 500.0, false).sidebar.w, 360.0);
    }
}
