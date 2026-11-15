//! Tab strip: satu tombol judul dan satu tombol tutup per tab, lalu tombol plus.

use crate::actions::ActionTarget;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezelStyle, NSButton, NSButtonType, NSControlStateValueOff, NSControlStateValueOn,
    NSImage, NSLineBreakMode, NSScrollView, NSView,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};

pub const HEIGHT: f64 = 28.0;
const MIN_W: f64 = 80.0;
const MAX_W: f64 = 200.0;
const PLUS_W: f64 = 28.0;

pub struct TabInfo {
    pub id: u64,
    pub title: String,
    pub active: bool,
    /// Tab ber-URL yang page-nya sedang dilepas.
    pub sleeping: bool,
}

pub struct TabStrip {
    scroll: Retained<NSScrollView>,
    doc: Retained<NSView>,
}

fn symbol(name: &str, description: &str) -> Retained<NSImage> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        Some(&NSString::from_str(description)),
    )
    .expect("SF Symbol tersedia di macOS 11+")
}

impl TabStrip {
    pub fn build(mtm: MainThreadMarker, width: f64, y: f64) -> TabStrip {
        let frame = NSRect::new(NSPoint::new(0.0, y), NSSize::new(width, HEIGHT));
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), frame);
        scroll.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin);
        scroll.setHasHorizontalScroller(true);
        scroll.setHasVerticalScroller(false);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        let doc = NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, HEIGHT)));
        scroll.setDocumentView(Some(&doc));
        TabStrip { scroll, doc }
    }

    pub fn view(&self) -> &NSScrollView {
        &self.scroll
    }

    /// Lebar tiap tab, dibagi rata dari lebar strip, dibatasi 80 sampai 200pt.
    pub fn tab_width(visible: f64, count: usize) -> f64 {
        ((visible - PLUS_W) / count.max(1) as f64).clamp(MIN_W, MAX_W)
    }

    pub fn render(&self, mtm: MainThreadMarker, tabs: &[TabInfo], target: &ActionTarget) {
        let visible = self.scroll.contentSize().width;
        let w = Self::tab_width(visible, tabs.len());
        self.doc.setSubviews(&NSArray::new());
        self.doc.setFrameSize(NSSize::new((w * tabs.len() as f64 + PLUS_W).max(visible), HEIGHT));

        for (i, tab) in tabs.iter().enumerate() {
            let x = w * i as f64;
            let title = NSString::from_str(&tab.title);
            let button = unsafe { NSButton::buttonWithTitle_target_action(&title, Some(target), Some(sel!(selectTab:)), mtm) };
            button.setButtonType(NSButtonType::PushOnPushOff);
            button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
            button.setState(if tab.active { NSControlStateValueOn } else { NSControlStateValueOff });
            button.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            button.setTag(tab.id as isize);
            if tab.sleeping {
                button.setAlphaValue(0.5);
                button.setToolTip(Some(&NSString::from_str("Tidur, dimuat ulang saat dibuka")));
            } else {
                button.setToolTip(Some(&title));
            }
            button.setFrame(NSRect::new(NSPoint::new(x + 2.0, 2.0), NSSize::new(w - 4.0, 24.0)));
            self.doc.addSubview(&button);

            let close = unsafe {
                NSButton::buttonWithImage_target_action(&symbol("xmark", "Tutup tab"), Some(target), Some(sel!(closeTabButton:)), mtm)
            };
            close.setBordered(false);
            close.setTag(tab.id as isize);
            close.setFrame(NSRect::new(NSPoint::new(x + w - 22.0, 6.0), NSSize::new(16.0, 16.0)));
            self.doc.addSubview(&close);
        }

        let plus = unsafe {
            NSButton::buttonWithImage_target_action(&symbol("plus", "Tab baru"), Some(target), Some(sel!(newTab:)), mtm)
        };
        plus.setBordered(false);
        plus.setFrame(NSRect::new(NSPoint::new(w * tabs.len() as f64 + 2.0, 2.0), NSSize::new(24.0, 24.0)));
        self.doc.addSubview(&plus);
    }
}
