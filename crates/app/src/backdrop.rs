//! Latar jendela: gradasi tema Space di belakang sidebar dan kartu, sebagai
//! `CAGradientLayer` (digambar GPU, tanpa bitmap seukuran jendela). Area
//! kosongnya bisa diseret untuk memindahkan jendela.

use browser_core::theme::{Rgb, Theme};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSColor, NSView, NSWorkspace};
use objc2_foundation::{NSArray, NSPoint, NSRect};
use objc2_quartz_core::{CAGradientLayer, CALayer};
use std::cell::Cell;

fn color(c: Rgb) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        f64::from(c[0]) / 255.0,
        f64::from(c[1]) / 255.0,
        f64::from(c[2]) / 255.0,
        1.0,
    )
}

/// Titik awal dan akhir gradasi (satuan 0–1, titik nol kiri bawah) untuk
/// sudut CSS (0 = ke atas, searah jarum jam).
fn endpoints(css_deg: f64) -> (NSPoint, NSPoint) {
    let rad = (90.0 - css_deg).to_radians();
    let (dx, dy) = (rad.cos() / 2.0, rad.sin() / 2.0);
    (NSPoint::new(0.5 - dx, 0.5 - dy), NSPoint::new(0.5 + dx, 0.5 + dy))
}

pub struct BackdropIvars {
    theme: Cell<&'static Theme>,
}

define_class!(
    #[unsafe(super(NSView, objc2_app_kit::NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBBackdrop"]
    #[ivars = BackdropIvars]
    pub struct Backdrop;

    impl Backdrop {
        #[unsafe(method_id(makeBackingLayer))]
        fn make_backing_layer(&self) -> Retained<CALayer> {
            Retained::into_super(CAGradientLayer::new())
        }

        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            self.apply_theme();
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn mouse_down_can_move_window(&self) -> Bool {
            Bool::YES
        }
    }
);

impl Backdrop {
    pub fn new(mtm: MainThreadMarker, theme: &'static Theme) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(BackdropIvars { theme: Cell::new(theme) });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setWantsLayer(true);
        // Warna disetel langsung: AppKit tidak selalu memanggil updateLayer
        // untuk view dengan backing layer sendiri.
        this.apply_theme();
        this
    }

    /// Menerapkan gradasi tema ke layer (atau warna polos saat Increase Contrast).
    fn apply_theme(&self) {
        let Some(layer) = self.layer() else { return };
        let Ok(gradient) = layer.downcast::<CAGradientLayer>() else { return };
        let theme = self.ivars().theme.get();
        if NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast() {
            unsafe { gradient.setColors(None) };
            gradient.setBackgroundColor(Some(&color(theme.stops[0]).CGColor()));
            return;
        }
        let cg: Vec<_> = theme.stops.iter().map(|c| color(*c).CGColor()).collect();
        // SAFETY: CGColor dijembatani gratis (toll-free) ke objek Objective-C.
        let objects: Vec<&AnyObject> =
            cg.iter().map(|c| unsafe { &*(Retained::as_ptr(c).cast::<AnyObject>()) }).collect();
        let array = NSArray::from_slice(&objects);
        unsafe { gradient.setColors(Some(&array)) };
        let (start, end) = endpoints(theme.angle_deg);
        gradient.setStartPoint(start);
        gradient.setEndPoint(end);
    }

    /// Kelas layer dan jumlah warna gradasinya (untuk pengujian).
    #[cfg(debug_assertions)]
    pub fn layer_info(&self) -> String {
        let Some(layer) = self.layer() else { return "tanpa layer".into() };
        let class = layer.class().name().to_string_lossy().to_string();
        let colors = layer.downcast::<CAGradientLayer>().ok().and_then(|g| g.colors()).map_or(0, |c| c.count());
        format!("{class} colors={colors} frame={:?}", self.frame())
    }

    #[cfg(debug_assertions)]
    pub fn theme(&self) -> &'static Theme {
        self.ivars().theme.get()
    }
}
