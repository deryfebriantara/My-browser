//! Latar jendela: gradasi tema Space di belakang sidebar dan kartu.
//! Area kosongnya bisa diseret untuk memindahkan jendela.

use browser_core::theme::{Rgb, Theme};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSColor, NSGradient, NSRectFill, NSView, NSWorkspace};
use objc2_foundation::{NSArray, NSRect};
use std::cell::Cell;

fn color(c: Rgb) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        f64::from(c[0]) / 255.0,
        f64::from(c[1]) / 255.0,
        f64::from(c[2]) / 255.0,
        1.0,
    )
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
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let theme = self.ivars().theme.get();
            let bounds = self.bounds();
            // Increase Contrast: warna gelap pertama tanpa gradasi.
            if NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast() {
                color(theme.stops[0]).setFill();
                NSRectFill(bounds);
                return;
            }
            let colors: Vec<Retained<NSColor>> = theme.stops.iter().map(|c| color(*c)).collect();
            let array = NSArray::from_retained_slice(&colors);
            if let Some(gradient) = NSGradient::initWithColors(NSGradient::alloc(), &array) {
                // Sudut CSS (0 = ke atas, searah jarum jam) ke sudut AppKit
                // (0 = ke kanan, berlawanan jarum jam).
                gradient.drawInRect_angle(bounds, 90.0 - theme.angle_deg);
            }
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
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    #[cfg(debug_assertions)]
    pub fn theme(&self) -> &'static Theme {
        self.ivars().theme.get()
    }
}
