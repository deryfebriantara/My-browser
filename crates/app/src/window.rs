//! Window utama: nav bar di atas, tab strip, lalu area konten.

use crate::actions::ActionTarget;
use crate::nav_bar::{self, NavBar};
use crate::tab_strip::{self, TabStrip};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;

pub struct Ui {
    pub window: Retained<NSWindow>,
    pub nav: NavBar,
    pub tabs: TabStrip,
    pub content: Retained<NSView>,
    pub target: Retained<ActionTarget>,
    shown: RefCell<Option<Retained<NSView>>>,
}

impl Ui {
    pub fn build(mtm: MainThreadMarker, target: Retained<ActionTarget>) -> Ui {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 800.0));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str("my-browser"));
        window.setContentMinSize(NSSize::new(800.0, 500.0));
        window.setDelegate(Some(ProtocolObject::from_ref(&*target)));
        window.center();

        let root = window.contentView().expect("window punya content view");
        let size = root.bounds().size;
        let (w, h) = (size.width, size.height);

        let (bar, nav) = NavBar::build(mtm, w, h - nav_bar::HEIGHT, &target);
        root.addSubview(&bar);

        let tabs = TabStrip::build(mtm, w, h - nav_bar::HEIGHT - tab_strip::HEIGHT);
        root.addSubview(tabs.view());

        let content_h = h - nav_bar::HEIGHT - tab_strip::HEIGHT;
        let content =
            NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, content_h)));
        content.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        root.addSubview(&content);

        Ui { window, nav, tabs, content, target, shown: RefCell::new(None) }
    }

    /// Mengganti view di area konten. `None` untuk tab kosong. `viewport`
    /// adalah ukuran emulasi perangkat tab itu (None: memenuhi area konten).
    pub fn show_content(&self, view: Option<Retained<NSView>>, viewport: Option<(f64, f64)>) {
        if let Some(old) = self.shown.borrow_mut().take() {
            old.removeFromSuperview();
        }
        if let Some(view) = view {
            self.content.addSubview(&view);
            *self.shown.borrow_mut() = Some(view);
        }
        self.layout_content(viewport);
    }

    /// Menata view konten: memenuhi area konten, atau seukuran viewport
    /// emulasi di tengah atas. Viewport yang lebih besar dari area konten
    /// terpotong di layar, tetapi halaman tetap di-layout seukuran itu.
    pub fn layout_content(&self, viewport: Option<(f64, f64)>) {
        let Some(view) = self.shown.borrow().clone() else { return };
        match viewport {
            None => {
                view.setFrame(self.content.bounds());
                view.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
            }
            Some((width, height)) => {
                let area = self.content.bounds().size;
                let x = ((area.width - width) / 2.0).max(0.0).round();
                // Area konten tidak flipped (titik nol di kiri bawah).
                let y = area.height - height;
                view.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)));
                view.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewMinXMargin
                        | NSAutoresizingMaskOptions::ViewMaxXMargin
                        | NSAutoresizingMaskOptions::ViewMinYMargin,
                );
            }
        }
    }

    /// Ukuran area konten (poin).
    pub fn content_size(&self) -> (f64, f64) {
        let size = self.content.bounds().size;
        (size.width, size.height)
    }

    pub fn shown_view(&self) -> Option<Retained<NSView>> {
        self.shown.borrow().clone()
    }
}
