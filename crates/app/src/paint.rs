//! View kecil yang menggambar dirinya sendiri: isi bersudut, garis tepi,
//! bayangan kartu, dan wadah yang menjungkir koordinat (atas ke bawah).
//! Digambar sekali ke backing store view, bukan per frame isi halaman.

use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{NSBezierPath, NSColor, NSGraphicsContext, NSShadow, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::{Cell, RefCell};

/// Jarak bayangan kartu dari tepi kartu (ruang untuk blur).
pub const SHADOW_MARGIN: f64 = 20.0;

#[derive(Default)]
pub struct PaintIvars {
    fill: RefCell<Option<Retained<NSColor>>>,
    stroke: RefCell<Option<(Retained<NSColor>, f64)>>,
    radius: Cell<f64>,
    /// Bayangan kartu: persegi bersudut seukuran view dikurangi `SHADOW_MARGIN`.
    shadow: Cell<bool>,
    /// Klik di area kosong view ini diteruskan ke view di bawahnya.
    passthrough: Cell<bool>,
    flipped: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSView, objc2_app_kit::NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBPaintView"]
    #[ivars = PaintIvars]
    pub struct PaintView;

    impl PaintView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            let bounds = self.bounds();
            let radius = ivars.radius.get();
            if ivars.shadow.get() {
                let m = SHADOW_MARGIN;
                let rect = NSRect::new(
                    NSPoint::new(m, m),
                    NSSize::new((bounds.size.width - 2.0 * m).max(0.0), (bounds.size.height - 2.0 * m).max(0.0)),
                );
                let shadow = NSShadow::new();
                shadow.setShadowColor(Some(&NSColor::colorWithWhite_alpha(0.0, 0.35)));
                shadow.setShadowBlurRadius(12.0);
                shadow.setShadowOffset(NSSize::new(0.0, -4.0));
                NSGraphicsContext::saveGraphicsState_class();
                shadow.set();
                NSColor::windowBackgroundColor().setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius).fill();
                NSGraphicsContext::restoreGraphicsState_class();
            }
            if let Some(fill) = ivars.fill.borrow().as_ref() {
                fill.setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, radius, radius).fill();
            }
            if let Some((color, width)) = ivars.stroke.borrow().as_ref() {
                let inset = width / 2.0;
                let rect = NSRect::new(
                    NSPoint::new(inset, inset),
                    NSSize::new(bounds.size.width - *width, bounds.size.height - *width),
                );
                color.setStroke();
                let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius);
                path.setLineWidth(*width);
                path.stroke();
            }
        }

        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            let hit: Option<Retained<NSView>> = unsafe { msg_send![super(self), hitTest: point] };
            let is_self = hit.as_ref().is_some_and(|h| std::ptr::eq(Retained::as_ptr(h).cast::<u8>(), (self as *const Self).cast()));
            if is_self && self.ivars().passthrough.get() { None } else { hit }
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> Bool {
            Bool::new(self.ivars().flipped.get())
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn mouse_down_can_move_window(&self) -> Bool {
            Bool::YES
        }
    }
);

impl PaintView {
    fn make(mtm: MainThreadMarker, ivars: PaintIvars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    /// Persegi bersudut berisi `color`.
    pub fn fill(mtm: MainThreadMarker, color: &NSColor, radius: f64) -> Retained<Self> {
        let ivars = PaintIvars { fill: RefCell::new(Some(color.retain())), ..PaintIvars::default() };
        ivars.radius.set(radius);
        Self::make(mtm, ivars)
    }

    /// Garis tepi bersudut yang tidak menerima klik.
    pub fn outline(mtm: MainThreadMarker, color: &NSColor, width: f64, radius: f64) -> Retained<Self> {
        let ivars = PaintIvars { stroke: RefCell::new(Some((color.retain(), width))), ..PaintIvars::default() };
        ivars.radius.set(radius);
        ivars.passthrough.set(true);
        Self::make(mtm, ivars)
    }

    /// Bayangan kartu (lihat `SHADOW_MARGIN`); tidak menerima klik.
    pub fn shadow(mtm: MainThreadMarker, radius: f64) -> Retained<Self> {
        let ivars = PaintIvars::default();
        ivars.radius.set(radius);
        ivars.shadow.set(true);
        ivars.passthrough.set(true);
        Self::make(mtm, ivars)
    }

    /// Wadah transparan; `flipped` menaruh titik nol di kiri atas.
    pub fn container(mtm: MainThreadMarker, flipped: bool, passthrough: bool) -> Retained<Self> {
        let ivars = PaintIvars::default();
        ivars.flipped.set(flipped);
        ivars.passthrough.set(passthrough);
        Self::make(mtm, ivars)
    }
}
