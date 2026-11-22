//! Nav bar: mundur, maju, muat ulang/berhenti, address field.

use crate::actions::ActionTarget;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSButton, NSImage, NSTextField, NSView};
use block2::RcBlock;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSTimer};
use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

pub const HEIGHT: f64 = 36.0;
const NOTICE_W: f64 = 260.0;
/// Ruang tombol Tanya Claude di ujung kanan.
const CLAUDE_W: f64 = 36.0;
const NOTICE_SECS: f64 = 6.0;

pub struct NavBar {
    pub back: Retained<NSButton>,
    pub forward: Retained<NSButton>,
    pub reload: Retained<NSButton>,
    pub address: Retained<NSTextField>,
    reload_icon: Retained<NSImage>,
    stop_icon: Retained<NSImage>,
    notice: Retained<NSTextField>,
    /// Naik setiap pemberitahuan baru, supaya timer lama tidak menyembunyikan
    /// pemberitahuan yang lebih baru.
    notice_gen: Rc<Cell<u64>>,
}

fn symbol(name: &str, description: &str) -> Retained<NSImage> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        Some(&NSString::from_str(description)),
    )
    .expect("SF Symbol tersedia di macOS 11+")
}

fn icon_button(mtm: MainThreadMarker, image: &NSImage, target: &AnyObject, action: Sel, x: f64) -> Retained<NSButton> {
    let button = unsafe { NSButton::buttonWithImage_target_action(image, Some(target), Some(action), mtm) };
    button.setBordered(false);
    button.setFrame(NSRect::new(NSPoint::new(x, 4.0), NSSize::new(28.0, 28.0)));
    button
}

impl NavBar {
    /// Membangun nav bar selebar `width`, menempel di tepi atas.
    pub fn build(mtm: MainThreadMarker, width: f64, y: f64, target: &ActionTarget) -> (Retained<NSView>, NavBar) {
        let bar = NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::new(0.0, y), NSSize::new(width, HEIGHT)));
        bar.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin);

        let reload_icon = symbol("arrow.clockwise", "Muat ulang");
        let stop_icon = symbol("xmark", "Berhenti");
        let back = icon_button(mtm, &symbol("chevron.left", "Mundur"), target, sel!(goBack:), 8.0);
        let forward = icon_button(mtm, &symbol("chevron.right", "Maju"), target, sel!(goForward:), 40.0);
        let reload = icon_button(mtm, &reload_icon, target, sel!(reloadOrStop:), 72.0);

        let address = NSTextField::new(mtm);
        address.setFrame(NSRect::new(NSPoint::new(108.0, 7.0), NSSize::new(width - 116.0 - CLAUDE_W, 22.0)));
        address.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        address.setPlaceholderString(Some(&NSString::from_str("Cari atau ketik alamat")));
        unsafe {
            address.setTarget(Some(target));
            address.setAction(Some(sel!(addressEntered:)));
        }
        unsafe { address.setDelegate(Some(ProtocolObject::from_ref(target))) };

        let notice = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        notice.setFrame(NSRect::new(NSPoint::new(width - NOTICE_W - 8.0 - CLAUDE_W, 9.0), NSSize::new(NOTICE_W, 18.0)));
        notice.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        notice.setHidden(true);

        let claude = icon_button(mtm, &symbol("sparkles", "Ask Claude"), target, sel!(toggleChat:), width - CLAUDE_W + 2.0);
        claude.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
        claude.setToolTip(Some(&NSString::from_str("Ask Claude (⌘J)")));

        for view in [&*back, &*forward, &*reload, &*claude] {
            bar.addSubview(view);
        }
        bar.addSubview(&address);
        bar.addSubview(&notice);
        let notice_gen = Rc::new(Cell::new(0));
        (bar, NavBar { back, forward, reload, address, reload_icon, stop_icon, notice, notice_gen })
    }

    pub fn update(&self, can_back: bool, can_forward: bool, loading: bool, url: &str) {
        self.back.setEnabled(can_back);
        self.forward.setEnabled(can_forward);
        self.reload.setImage(Some(if loading { &self.stop_icon } else { &self.reload_icon }));
        if !self.is_editing() {
            self.address.setStringValue(&NSString::from_str(url));
        }
    }

    /// Menampilkan pesan di kanan nav bar selama 6 detik; address field
    /// menyempit selama pesan tampil.
    pub fn show_notice(&self, text: &str) {
        let generation = self.notice_gen.get() + 1;
        self.notice_gen.set(generation);
        self.notice.setStringValue(&NSString::from_str(text));
        self.notice.setToolTip(Some(&NSString::from_str(text)));
        if self.notice.isHidden() {
            let size = self.address.frame().size;
            self.address.setFrameSize(NSSize::new(size.width - NOTICE_W - 8.0, size.height));
            self.notice.setHidden(false);
        }
        let notice = self.notice.clone();
        let address = self.address.clone();
        let current = self.notice_gen.clone();
        let hide = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            if current.get() != generation || notice.isHidden() {
                return;
            }
            notice.setHidden(true);
            let size = address.frame().size;
            address.setFrameSize(NSSize::new(size.width + NOTICE_W + 8.0, size.height));
        });
        let _ = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(NOTICE_SECS, false, &hide) };
    }

    pub fn is_editing(&self) -> bool {
        self.address.currentEditor().is_some()
    }
}
