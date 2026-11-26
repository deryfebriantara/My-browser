//! Window utama: gradasi tema di belakang, sidebar di kiri, kartu halaman
//! di sebelahnya, dan kartu panel Tanya Claude di kanan saat terbuka.
//! Title bar transparan; tombol jendela berada di atas sidebar.

use crate::actions::ActionTarget;
use crate::backdrop::Backdrop;
use crate::card::Card;
use crate::layout::{self, Frames, SidebarMode};
use crate::sidebar::Sidebar;
use browser_core::theme::Theme;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSView, NSWindow, NSWindowButton, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};

pub struct Ui {
    pub window: Retained<NSWindow>,
    pub sidebar: Sidebar,
    pub card: Card,
    pub target: Retained<ActionTarget>,
    backdrop: Retained<Backdrop>,
    panel_card: Card,
    shown: RefCell<Option<Retained<NSView>>>,
    /// Panel di kanan kartu halaman (Tanya Claude).
    panel: RefCell<Option<Retained<NSView>>>,
    mode: Cell<SidebarMode>,
    sidebar_width: Cell<f64>,
    /// Viewport emulasi tab yang tampil (None: memenuhi kartu).
    shown_viewport: Cell<Option<(f64, f64)>>,
}

impl Ui {
    pub fn build(
        mtm: MainThreadMarker,
        target: Retained<ActionTarget>,
        theme: &'static Theme,
        sidebar_width: f64,
        sidebar_visible: bool,
    ) -> Ui {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 800.0));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable
            | NSWindowStyleMask::FullSizeContentView;
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
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setContentMinSize(NSSize::new(800.0, 500.0));
        window.setDelegate(Some(ProtocolObject::from_ref(&*target)));
        window.center();

        let root = window.contentView().expect("window punya content view");
        let backdrop = Backdrop::new(mtm, theme);
        backdrop.setFrame(root.bounds());
        backdrop.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        root.addSubview(&backdrop);

        // Urutan: kartu di bawah sidebar, supaya bayangan kartu tidak
        // menangkap klik di tepi kanan sidebar.
        let card = Card::build(mtm);
        root.addSubview(&card.view);
        let panel_card = Card::build(mtm);
        panel_card.view.setHidden(true);
        root.addSubview(&panel_card.view);
        let sidebar = Sidebar::build(mtm, &target);
        root.addSubview(&sidebar.view);

        let mode = if sidebar_visible { SidebarMode::Docked } else { SidebarMode::Hidden };
        let ui = Ui {
            window,
            sidebar,
            card,
            target,
            backdrop,
            panel_card,
            shown: RefCell::new(None),
            panel: RefCell::new(None),
            mode: Cell::new(mode),
            sidebar_width: Cell::new(sidebar_width),
            shown_viewport: Cell::new(None),
        };
        ui.relayout();
        ui
    }

    fn frames(&self) -> Frames {
        let size = self.window.contentView().map(|v| v.bounds().size).unwrap_or(NSSize::new(0.0, 0.0));
        layout::layout(
            size.width,
            size.height,
            self.mode.get(),
            self.sidebar_width.get(),
            self.panel.borrow().is_some(),
        )
    }

    /// Jarak pusat tombol jendela (merah/kuning/hijau) dari tepi atas jendela.
    fn traffic_light_center(&self) -> f64 {
        let fallback = 14.0;
        let (Some(root), Some(close)) =
            (self.window.contentView(), self.window.standardWindowButton(NSWindowButton::CloseButton))
        else {
            return fallback;
        };
        let Some(parent) = (unsafe { close.superview() }) else { return fallback };
        let frame = close.frame();
        let center = NSPoint::new(frame.origin.x + frame.size.width / 2.0, frame.origin.y + frame.size.height / 2.0);
        let in_root = parent.convertPoint_toView(center, Some(&root));
        root.bounds().size.height - in_root.y
    }

    /// Menata ulang sidebar, kartu, dan panel sesuai ukuran jendela.
    pub fn relayout(&self) {
        let frames = self.frames();
        let top = match self.mode.get() {
            SidebarMode::Docked => self.traffic_light_center(),
            SidebarMode::Hidden | SidebarMode::Overlay => self.traffic_light_center() - layout::INSET,
        };
        self.sidebar.set_frame(frames.sidebar, top);
        self.sidebar.view.setHidden(self.mode.get() == SidebarMode::Hidden);
        self.card.set_frame(frames.card);
        match frames.panel {
            Some(panel) => {
                self.panel_card.set_frame(panel);
                self.panel_card.view.setHidden(false);
            }
            None => self.panel_card.view.setHidden(true),
        }
        let viewport = self.shown_viewport.get();
        self.layout_content(viewport);
    }

    /// Mengganti view di kartu halaman. `None` untuk tab kosong. `viewport`
    /// adalah ukuran emulasi perangkat tab itu (None: memenuhi kartu).
    pub fn show_content(&self, view: Option<Retained<NSView>>, viewport: Option<(f64, f64)>) {
        if let Some(old) = self.shown.borrow_mut().take() {
            old.removeFromSuperview();
        }
        if let Some(view) = view {
            self.card.content.addSubview(&view);
            *self.shown.borrow_mut() = Some(view);
        }
        self.shown_viewport.set(viewport);
        self.layout_content(viewport);
    }

    /// Menata view halaman: memenuhi kartu, atau seukuran viewport emulasi di
    /// tengah atas. Viewport yang lebih besar dari kartu terpotong di layar,
    /// tetapi halaman tetap di-layout seukuran itu.
    pub fn layout_content(&self, viewport: Option<(f64, f64)>) {
        self.shown_viewport.set(viewport);
        let Some(view) = self.shown.borrow().clone() else { return };
        match viewport {
            None => {
                view.setFrame(self.card.content.bounds());
                view.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
            }
            Some((width, height)) => {
                let area = self.card.content.bounds().size;
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

    /// Memasang (atau melepas, `None`) panel di kanan. Kartu halaman menyempit;
    /// halaman dan viewport emulasi ikut menyesuaikan.
    pub fn set_panel(&self, view: Option<Retained<NSView>>) {
        if let Some(old) = self.panel.borrow_mut().take() {
            old.removeFromSuperview();
        }
        if let Some(view) = view {
            view.setFrame(self.panel_card.content.bounds());
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            self.panel_card.content.addSubview(&view);
            *self.panel.borrow_mut() = Some(view);
        }
        self.relayout();
        if let Some(view) = self.panel.borrow().as_ref() {
            view.setFrame(self.panel_card.content.bounds());
        }
    }

    /// Ukuran kartu halaman (poin).
    pub fn content_size(&self) -> (f64, f64) {
        let size = self.card.content.bounds().size;
        (size.width, size.height)
    }

    pub fn shown_view(&self) -> Option<Retained<NSView>> {
        self.shown.borrow().clone()
    }

    pub fn show_notice(&self, text: &str) {
        self.card.show_toast(text);
    }

    #[cfg(debug_assertions)]
    pub fn sidebar_mode(&self) -> SidebarMode {
        self.mode.get()
    }

    #[cfg(debug_assertions)]
    pub fn theme_id(&self) -> &'static str {
        self.backdrop.theme().id
    }

    /// Frame sidebar, kartu, dan panel dalam koordinat jendela (untuk pengujian).
    #[cfg(debug_assertions)]
    pub fn frames_in_window(&self) -> (NSRect, NSRect, Option<NSRect>) {
        let sidebar = self.sidebar.view.convertRect_toView(self.sidebar.view.bounds(), None);
        let panel = (!self.panel_card.view.isHidden()).then(|| self.panel_card.frame_in_window());
        (sidebar, self.card.frame_in_window(), panel)
    }

    #[cfg(debug_assertions)]
    pub fn traffic_lights_hidden(&self) -> bool {
        self.window.standardWindowButton(NSWindowButton::CloseButton).is_none_or(|b| b.isHidden())
    }
}
