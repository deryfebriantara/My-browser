//! Kartu halaman: wadah bersudut membulat dengan bayangan, berisi view
//! halaman, tampilan kosong, toast pemberitahuan, dan garis tepi oranye
//! selama Claude boleh berinteraksi.

use crate::layout::{CARD_RADIUS, Frame};
use crate::paint::{PaintView, SHADOW_MARGIN};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSColor, NSFont, NSLineBreakMode, NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSTimer};
use objc2_quartz_core::kCACornerCurveContinuous;
use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

const TOAST_SECS: f64 = 6.0;
const TOAST_HEIGHT: f64 = 28.0;

pub struct Card {
    /// Wadah terluar (lebih besar dari kartu sebesar `SHADOW_MARGIN`); klik di
    /// luar kartu diteruskan ke view di bawahnya.
    pub view: Retained<PaintView>,
    /// Induk view halaman.
    pub content: Retained<NSView>,
    clip: Retained<NSView>,
    shadow: Retained<PaintView>,
    empty: Retained<PaintView>,
    empty_title: Retained<NSTextField>,
    empty_hint: Retained<NSTextField>,
    toast: Retained<PaintView>,
    toast_label: Retained<NSTextField>,
    toast_gen: Rc<Cell<u64>>,
    outline: Retained<PaintView>,
}

fn label(mtm: MainThreadMarker, text: &str, font: &NSFont, color: &NSColor) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    field.setFont(Some(font));
    field.setTextColor(Some(color));
    field.setAlignment(NSTextAlignment::Center);
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    field
}

impl Card {
    pub fn build(mtm: MainThreadMarker) -> Card {
        let view = PaintView::container(mtm, false, true);
        // Layer bayangan seukuran kartu, di bawah kartu.
        let shadow = PaintView::shadow(mtm, CARD_RADIUS);
        view.addSubview(&shadow);

        let clip = NSView::initWithFrame(NSView::alloc(mtm), NSRect::ZERO);
        clip.setWantsLayer(true);
        if let Some(layer) = clip.layer() {
            layer.setCornerRadius(CARD_RADIUS);
            // SAFETY: konstanta statis Core Animation.
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
        }
        clip.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        view.addSubview(&clip);

        let content = NSView::initWithFrame(NSView::alloc(mtm), NSRect::ZERO);
        content.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        clip.addSubview(&content);

        // Tampilan kosong: latar jendela biasa, nama Space, dan petunjuk.
        let empty = PaintView::fill(mtm, &NSColor::windowBackgroundColor(), 0.0);
        empty.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let empty_title = label(mtm, "", &NSFont::boldSystemFontOfSize(22.0), &NSColor::labelColor());
        let hint =
            label(mtm, "Press ⌘L to type an address", &NSFont::systemFontOfSize(13.0), &NSColor::secondaryLabelColor());
        empty.addSubview(&empty_title);
        empty.addSubview(&hint);
        empty.setHidden(true);
        clip.addSubview(&empty);

        let toast = PaintView::fill(mtm, &NSColor::colorWithWhite_alpha(0.08, 0.86), TOAST_HEIGHT / 2.0);
        let toast_label = label(mtm, "", &NSFont::systemFontOfSize(12.0), &NSColor::whiteColor());
        toast.addSubview(&toast_label);
        toast.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMinXMargin
                | NSAutoresizingMaskOptions::ViewMaxXMargin
                | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        toast.setHidden(true);
        clip.addSubview(&toast);

        let outline = PaintView::outline(mtm, &NSColor::systemOrangeColor(), 2.0, CARD_RADIUS);
        outline.setHidden(true);
        view.addSubview(&outline);

        Card {
            view,
            content,
            clip,
            shadow,
            empty,
            empty_title,
            empty_hint: hint,
            toast,
            toast_label,
            toast_gen: Rc::new(Cell::new(0)),
            outline,
        }
    }

    /// `f` adalah frame kartu (tanpa bayangan) dalam koordinat induk.
    pub fn set_frame(&self, f: Frame) {
        let m = SHADOW_MARGIN;
        self.view.setFrame(NSRect::new(NSPoint::new(f.x - m, f.y - m), NSSize::new(f.w + 2.0 * m, f.h + 2.0 * m)));
        let inner = NSRect::new(NSPoint::new(m, m), NSSize::new(f.w, f.h));
        self.clip.setFrame(inner);
        self.outline.setFrame(inner);
        self.shadow.setFrame(inner);
        self.content.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), inner.size));
        self.empty.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), inner.size));
        // Nama Space dan petunjuk di tengah kartu.
        let (cx, cy) = (f.w / 2.0, f.h / 2.0);
        let width = (f.w - 40.0).max(0.0);
        self.empty_title.setFrame(NSRect::new(NSPoint::new(cx - width / 2.0, cy + 4.0), NSSize::new(width, 30.0)));
        self.empty_hint.setFrame(NSRect::new(NSPoint::new(cx - width / 2.0, cy - 24.0), NSSize::new(width, 20.0)));
        self.place_toast();
    }

    /// Frame kartu (tanpa bayangan) dalam koordinat jendela.
    #[cfg(debug_assertions)]
    pub fn frame_in_window(&self) -> NSRect {
        self.clip.convertRect_toView(self.clip.bounds(), None)
    }

    pub fn show_empty(&self, space_name: Option<&str>) {
        match space_name {
            Some(name) => {
                self.empty_title.setStringValue(&NSString::from_str(name));
                self.empty.setHidden(false);
            }
            None => self.empty.setHidden(true),
        }
    }

    #[cfg(debug_assertions)]
    pub fn empty_visible(&self) -> bool {
        !self.empty.isHidden()
    }

    pub fn show_toast(&self, text: &str) {
        let generation = self.toast_gen.get() + 1;
        self.toast_gen.set(generation);
        self.toast_label.setStringValue(&NSString::from_str(text));
        self.toast.setToolTip(Some(&NSString::from_str(text)));
        self.toast.setHidden(false);
        self.place_toast();
        let toast = self.toast.clone();
        let current = self.toast_gen.clone();
        let hide = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            if current.get() == generation {
                toast.setHidden(true);
            }
        });
        let _ = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(TOAST_SECS, false, &hide) };
    }

    /// Teks toast yang sedang tampil.
    #[cfg(debug_assertions)]
    pub fn toast_text(&self) -> Option<String> {
        (!self.toast.isHidden()).then(|| self.toast_label.stringValue().to_string())
    }

    fn place_toast(&self) {
        let area = self.clip.bounds().size;
        let text = self.toast_label.fittingSize().width;
        let width = (text + 28.0).min((area.width - 24.0).max(0.0));
        self.toast.setFrame(NSRect::new(
            NSPoint::new(((area.width - width) / 2.0).round(), area.height - 12.0 - TOAST_HEIGHT),
            NSSize::new(width, TOAST_HEIGHT),
        ));
        self.toast_label.setFrame(NSRect::new(NSPoint::new(14.0, 6.0), NSSize::new((width - 28.0).max(0.0), 16.0)));
    }

    pub fn set_outline(&self, on: bool) {
        self.outline.setHidden(!on);
    }

    #[cfg(debug_assertions)]
    pub fn outline_visible(&self) -> bool {
        !self.outline.isHidden()
    }
}
