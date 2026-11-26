//! Window utama: gradasi tema di belakang, sidebar di kiri, kartu halaman
//! di sebelahnya, dan kartu panel Tanya Claude di kanan saat terbuka.
//! Title bar transparan; tombol jendela berada di atas sidebar.

use crate::actions::ActionTarget;
use crate::backdrop::Backdrop;
use crate::card::Card;
use crate::layout::{self, Frames, SidebarMode};
use crate::paint::{DragHandle, DragHandler, HoverHandler, PaintView};
use crate::sidebar::Sidebar;
use block2::RcBlock;
use browser_core::theme::Theme;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAutoresizingMaskOptions, NSBackingStoreType, NSView, NSWindow,
    NSWindowButton, NSWindowStyleMask, NSWindowTitleVisibility, NSWorkspace,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

pub struct Ui {
    pub window: Retained<NSWindow>,
    pub sidebar: Sidebar,
    pub card: Card,
    pub target: Retained<ActionTarget>,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    backdrop: Retained<Backdrop>,
    panel_card: Card,
    shown: RefCell<Option<Retained<NSView>>>,
    /// Panel di kanan kartu halaman (Tanya Claude).
    panel: RefCell<Option<Retained<NSView>>>,
    mode: Cell<SidebarMode>,
    sidebar_width: Cell<f64>,
    /// Viewport emulasi tab yang tampil (None: memenuhi kartu).
    shown_viewport: Cell<Option<(f64, f64)>>,
    /// Pita tipis di tepi kiri saat sidebar tersembunyi (kursor di sini memunculkan overlay).
    edge: Retained<PaintView>,
    edge_hovered: Cell<bool>,
    overlay_shadow: Retained<PaintView>,
    /// Pegangan ubah lebar di tepi kanan sidebar.
    handle: Retained<DragHandle>,
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
        // Area kosong sidebar dan tepi di sekitar kartu ikut menyeret jendela
        // (view yang mengembalikan mouseDownCanMoveWindow = YES).
        window.setMovableByWindowBackground(true);
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
        let edge = PaintView::container(mtm, false, true);
        root.addSubview(&edge);
        let overlay_shadow = PaintView::shadow(mtm, layout::CARD_RADIUS);
        root.addSubview(&overlay_shadow);
        let sidebar = Sidebar::build(mtm, &target, theme);
        root.addSubview(&sidebar.view);
        let handle = DragHandle::new(mtm, None);
        root.addSubview(&handle);

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
            edge,
            edge_hovered: Cell::new(false),
            overlay_shadow,
            handle,
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
    /// None kalau tombol jendela tidak berada di jendela ini (layar penuh
    /// memindahkannya ke jendela toolbar tersendiri).
    pub(crate) fn traffic_light_center(&self) -> Option<f64> {
        if self.window.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return None;
        }
        let root = self.window.contentView()?;
        let close = self.window.standardWindowButton(NSWindowButton::CloseButton)?;
        let same_window =
            close.window().is_some_and(|w| std::ptr::eq(Retained::as_ptr(&w), Retained::as_ptr(&self.window)));
        if !same_window {
            return None;
        }
        let parent = unsafe { close.superview() }?;
        let frame = close.frame();
        let center = NSPoint::new(frame.origin.x + frame.size.width / 2.0, frame.origin.y + frame.size.height / 2.0);
        let in_root = parent.convertPoint_toView(center, Some(&root));
        Some(root.bounds().size.height - in_root.y)
    }

    /// Menata ulang sidebar, kartu, dan panel sesuai ukuran jendela.
    pub fn relayout(&self) {
        let frames = self.frames();
        // Pusat tombol jendela diukur dari tepi atas sidebar (bukan jendela).
        let window_h = self.window.contentView().map_or(0.0, |v| v.bounds().size.height);
        let top_gap = window_h - (frames.sidebar.y + frames.sidebar.h);
        let top = layout::sidebar_top_center(self.traffic_light_center(), top_gap);
        let mode = self.mode.get();
        self.sidebar.set_frame(frames.sidebar, top);
        self.sidebar.set_overlay(mode == SidebarMode::Overlay);
        self.sidebar.view.setHidden(mode == SidebarMode::Hidden);
        let height = frames.sidebar.h + if mode == SidebarMode::Docked { 0.0 } else { 2.0 * layout::INSET };
        self.edge.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(layout::INSET, height)));
        self.edge.setHidden(mode != SidebarMode::Hidden);
        let s = frames.sidebar;
        self.overlay_shadow.setFrame(NSRect::new(NSPoint::new(s.x, s.y), NSSize::new(s.w, s.h)));
        self.overlay_shadow.setHidden(mode != SidebarMode::Overlay);
        self.handle.setFrame(NSRect::new(NSPoint::new(s.w - 2.0, 0.0), NSSize::new(4.0, s.h)));
        self.handle.setHidden(mode != SidebarMode::Docked);
        if let Some(window) = self.window.contentView().and_then(|v| v.window()) {
            window.invalidateCursorRectsForView(&self.handle);
        }
        for button in [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton] {
            if let Some(button) = self.window.standardWindowButton(button) {
                button.setHidden(mode == SidebarMode::Hidden);
            }
        }
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

    pub fn sidebar_mode(&self) -> SidebarMode {
        self.mode.get()
    }

    pub fn sidebar_width(&self) -> f64 {
        self.sidebar_width.get()
    }

    /// Menempel (`true`) atau sembunyi (⌘S).
    pub fn set_sidebar_visible(&self, visible: bool) {
        self.mode.set(if visible { SidebarMode::Docked } else { SidebarMode::Hidden });
        self.relayout();
    }

    /// Sidebar tersembunyi muncul melayang dari kiri (meluncur 180 ms,
    /// atau langsung kalau Reduce Motion menyala).
    pub fn show_overlay(&self) {
        if self.mode.get() != SidebarMode::Hidden {
            return;
        }
        self.mode.set(SidebarMode::Overlay);
        self.relayout();
        if NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion() {
            return;
        }
        let views: [Retained<NSView>; 2] =
            [Retained::into_super(self.sidebar.view.clone()), Retained::into_super(self.overlay_shadow.clone())];
        let finals: Vec<NSRect> = views.iter().map(|v| v.frame()).collect();
        for (view, end) in views.iter().zip(&finals) {
            let mut start = *end;
            start.origin.x -= end.size.width + layout::INSET;
            view.setFrame(start);
        }
        let block = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            // SAFETY: AppKit memberi konteks animasi yang berlaku selama blok berjalan.
            unsafe { context.as_ref() }.setDuration(0.18);
            for (view, end) in views.iter().zip(&finals) {
                view.animator().setFrame(*end);
            }
        });
        NSAnimationContext::runAnimationGroup(&block);
    }

    pub fn hide_overlay(&self) {
        if self.mode.get() == SidebarMode::Overlay {
            self.mode.set(SidebarMode::Hidden);
            self.relayout();
        }
    }

    /// Lebar sidebar baru (dibatasi 200–360).
    pub fn set_sidebar_width(&self, width: f64) {
        self.sidebar_width.set(width.clamp(layout::SIDEBAR_MIN, layout::SIDEBAR_MAX));
        self.relayout();
    }

    /// Kursor sedang di atas sidebar (koordinat jendela).
    pub fn pointer_in_sidebar(&self) -> bool {
        let point = self.window.mouseLocationOutsideOfEventStream();
        let frame = self.sidebar.view.convertRect_toView(self.sidebar.view.bounds(), None);
        objc2_foundation::NSPointInRect(point, frame)
    }

    pub fn set_edge_hovered(&self, inside: bool) {
        self.edge_hovered.set(inside);
    }

    pub fn edge_hovered(&self) -> bool {
        self.edge_hovered.get()
    }

    /// Pita tepi kiri, sidebar (untuk keluar dari overlay), dan pegangan ubah lebar.
    pub fn install_hooks(&self, edge: HoverHandler, sidebar: HoverHandler, resize: DragHandler) {
        self.edge.set_hover_handler(edge);
        self.sidebar.view.set_hover_handler(sidebar);
        self.handle.set_handler(resize);
    }

    #[cfg(debug_assertions)]
    pub fn backdrop_info(&self) -> String {
        self.backdrop.layer_info()
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
