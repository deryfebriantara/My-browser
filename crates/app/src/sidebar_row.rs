//! Satu baris tab di sidebar. Seluruh baris adalah tombol (`selectTab:`,
//! tag = id tab), jadi klik mouse, VoiceOver, dan pengujian memakai jalur
//! yang sama. Tombol tutup muncul saat kursor di atas baris.

use crate::actions::ActionTarget;
use browser_core::sidebar::RowInfo;
use browser_core::tabs::TabKind;
use browser_core::theme::SECONDARY_ALPHA;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSBezierPath, NSButton, NSColor, NSEvent, NSFont, NSImage, NSImageView, NSLineBreakMode,
    NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};

pub const ROW_HEIGHT: f64 = 28.0;

#[derive(Default)]
pub struct RowIvars {
    active: Cell<bool>,
    hover: Cell<bool>,
    close: RefCell<Option<Retained<NSButton>>>,
    glyph: RefCell<Option<Retained<NSImageView>>>,
    title: RefCell<Option<Retained<NSTextField>>>,
}

pub fn symbol(name: &str, description: &str) -> Retained<NSImage> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        Some(&NSString::from_str(description)),
    )
    .expect("SF Symbol tersedia di macOS 11+")
}

define_class!(
    #[unsafe(super(NSButton, objc2_app_kit::NSControl, NSView, objc2_app_kit::NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBTabRowView"]
    #[ivars = RowIvars]
    pub struct TabRowView;

    impl TabRowView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let alpha = if self.ivars().active.get() {
                0.24
            } else if self.ivars().hover.get() {
                0.10
            } else {
                0.0
            };
            if alpha > 0.0 {
                NSColor::colorWithWhite_alpha(1.0, alpha).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(self.bounds(), 8.0, 8.0).fill();
            }
        }

        /// Klik di mana pun di baris mengenai baris itu, kecuali tombol tutup.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            let hit: Option<Retained<NSView>> = unsafe { msg_send![super(self), hitTest: point] };
            let on_close = hit.as_ref().is_some_and(|hit| {
                self.ivars().close.borrow().as_ref().is_some_and(|close| {
                    !close.isHidden() && std::ptr::eq(Retained::as_ptr(hit).cast::<u8>(), Retained::as_ptr(close).cast())
                })
            });
            match hit {
                Some(hit) if on_close => Some(hit),
                Some(_) => Some(Retained::into_super(Retained::into_super(Retained::into_super(self.retain())))),
                None => None,
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.set_hover(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.set_hover(false);
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> Bool {
            Bool::YES
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn mouse_down_can_move_window(&self) -> Bool {
            Bool::NO
        }
    }
);

fn small_button(
    mtm: MainThreadMarker,
    image: &NSImage,
    target: &ActionTarget,
    action: objc2::runtime::Sel,
    tag: isize,
) -> Retained<NSButton> {
    let button =
        unsafe { NSButton::buttonWithImage_target_action(image, Some(target as &AnyObject), Some(action), mtm) };
    button.setBordered(false);
    button.setTag(tag);
    button
}

impl TabRowView {
    pub fn new(
        mtm: MainThreadMarker,
        row: &RowInfo,
        icon: &NSImage,
        width: f64,
        target: &ActionTarget,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RowIvars::default());
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, ROW_HEIGHT));
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.ivars().active.set(row.active);
        this.setBordered(false);
        this.setTitle(&NSString::new());
        this.setTag(row.id.0 as isize);
        unsafe {
            this.setTarget(Some(target as &AnyObject));
            this.setAction(Some(sel!(selectTab:)));
        }
        this.setAccessibilityLabel(Some(&NSString::from_str(&row.accessibility_label())));
        this.setAccessibilitySelected(row.active);
        this.setToolTip(Some(&NSString::from_str(&row.title)));
        let text_alpha = if row.asleep { SECONDARY_ALPHA } else { 1.0 };

        let image = NSImageView::imageViewWithImage(icon, mtm);
        image.setFrame(NSRect::new(NSPoint::new(8.0, 6.0), NSSize::new(16.0, 16.0)));
        image.setAlphaValue(text_alpha);
        this.addSubview(&image);

        let title = NSTextField::labelWithString(&NSString::from_str(&row.title), mtm);
        title.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        title.setTextColor(Some(&NSColor::colorWithWhite_alpha(1.0, text_alpha)));
        title.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        title.setUsesSingleLineMode(true);
        title.setFrame(NSRect::new(NSPoint::new(32.0, 5.0), NSSize::new((width - 32.0 - 28.0).max(0.0), 17.0)));
        this.addSubview(&title);
        *this.ivars().title.borrow_mut() = Some(title);

        let trailing = if row.playing {
            Some(symbol("speaker.wave.2.fill", "Playing audio"))
        } else if row.asleep {
            Some(symbol("moon.fill", "Asleep"))
        } else {
            None
        };
        if let Some(glyph) = trailing {
            let view = NSImageView::imageViewWithImage(&glyph, mtm);
            view.setContentTintColor(Some(&NSColor::colorWithWhite_alpha(1.0, SECONDARY_ALPHA)));
            view.setFrame(NSRect::new(NSPoint::new(width - 24.0, 7.0), NSSize::new(14.0, 14.0)));
            this.addSubview(&view);
            *this.ivars().glyph.borrow_mut() = Some(view);
        }

        let (name, label) = if row.kind == TabKind::Today { ("xmark", "Close Tab") } else { ("minus", "Unload Tab") };
        let close = small_button(mtm, &symbol(name, label), target, sel!(closeTabButton:), row.id.0 as isize);
        close.setToolTip(Some(&NSString::from_str(label)));
        close.setContentTintColor(Some(&NSColor::whiteColor()));
        close.setFrame(NSRect::new(NSPoint::new(width - 25.0, 6.0), NSSize::new(16.0, 16.0)));
        close.setHidden(true);
        this.addSubview(&close);
        *this.ivars().close.borrow_mut() = Some(close);

        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::ActiveAlways
            | NSTrackingAreaOptions::InVisibleRect;
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                NSRect::ZERO,
                options,
                Some(&this as &AnyObject),
                None,
            )
        };
        this.addTrackingArea(&area);
        this
    }

    /// Sama dengan kursor masuk atau keluar dari baris.
    pub fn set_hover(&self, on: bool) {
        self.ivars().hover.set(on);
        if let Some(close) = self.ivars().close.borrow().as_ref() {
            close.setHidden(!on);
        }
        if let Some(glyph) = self.ivars().glyph.borrow().as_ref() {
            glyph.setHidden(on);
        }
        let view: &NSView = self;
        view.setNeedsDisplay(true);
    }

    pub fn close_button(&self) -> Option<Retained<NSButton>> {
        self.ivars().close.borrow().clone()
    }

    /// Jumlah baris yang dipakai judul (harus selalu 1).
    pub fn title_lines(&self) -> usize {
        let Some(title) = self.ivars().title.borrow().clone() else { return 0 };
        let Some(font) = title.font() else { return 1 };
        let line = font.ascender() - font.descender() + font.leading();
        let Some(cell) = title.cell() else { return 1 };
        let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(title.frame().size.width, 1000.0));
        let height = cell.cellSizeForBounds(bounds).height;
        ((height / line).round() as usize).max(1)
    }
}
