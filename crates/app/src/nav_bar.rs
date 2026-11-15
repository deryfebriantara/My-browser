//! Nav bar: mundur, maju, muat ulang/berhenti, address field.

use crate::actions::ActionTarget;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSButton, NSImage, NSTextField, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

pub const HEIGHT: f64 = 36.0;

pub struct NavBar {
    pub back: Retained<NSButton>,
    pub forward: Retained<NSButton>,
    pub reload: Retained<NSButton>,
    pub address: Retained<NSTextField>,
    reload_icon: Retained<NSImage>,
    stop_icon: Retained<NSImage>,
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
        address.setFrame(NSRect::new(NSPoint::new(108.0, 7.0), NSSize::new(width - 116.0, 22.0)));
        address.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        address.setPlaceholderString(Some(&NSString::from_str("Cari atau ketik alamat")));
        unsafe {
            address.setTarget(Some(target));
            address.setAction(Some(sel!(addressEntered:)));
        }
        unsafe { address.setDelegate(Some(ProtocolObject::from_ref(target))) };

        for view in [&*back, &*forward, &*reload] {
            bar.addSubview(view);
        }
        bar.addSubview(&address);
        (bar, NavBar { back, forward, reload, address, reload_icon, stop_icon })
    }

    pub fn update(&self, can_back: bool, can_forward: bool, loading: bool, url: &str) {
        self.back.setEnabled(can_back);
        self.forward.setEnabled(can_forward);
        self.reload.setImage(Some(if loading { &self.stop_icon } else { &self.reload_icon }));
        if !self.is_editing() {
            self.address.setStringValue(&NSString::from_str(url));
        }
    }

    pub fn is_editing(&self) -> bool {
        self.address.currentEditor().is_some()
    }
}
