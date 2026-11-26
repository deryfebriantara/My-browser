//! Satu baris tab (atau ubin favorit) di sidebar. Seluruh baris adalah tombol
//! (`selectTab:`, tag = id tab), jadi klik mouse, VoiceOver, dan pengujian
//! memakai jalur yang sama. Tombol tutup muncul saat kursor di atas baris;
//! klik kanan membuka menu konteks tab.

use crate::actions::ActionTarget;
use crate::layout::TILE_HEIGHT;
use browser_core::sidebar::RowInfo;
use browser_core::tabs::TabKind;
use browser_core::theme::SECONDARY_ALPHA;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSBezierPath, NSButton, NSColor, NSDragOperation, NSDraggingContext,
    NSDraggingItem, NSDraggingSession, NSDraggingSource, NSEvent, NSEventMask, NSEventType, NSFont, NSImage,
    NSImageView, NSLineBreakMode, NSMenu, NSMenuItem, NSPasteboardItem, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};

pub const ROW_HEIGHT: f64 = 28.0;

/// Baris di daftar Pinned/Today, atau ubin di grid favorit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStyle {
    Row,
    Tile,
}

#[derive(Default)]
pub struct RowIvars {
    tile: Cell<bool>,
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
            let ivars = self.ivars();
            let (active, hover, base, radius) =
                if ivars.tile.get() { (0.28, 0.18, 0.12, 10.0) } else { (0.24, 0.10, 0.0, 8.0) };
            let alpha = if ivars.active.get() {
                active
            } else if ivars.hover.get() {
                hover
            } else {
                base
            };
            if alpha > 0.0 {
                NSColor::colorWithWhite_alpha(1.0, alpha).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(self.bounds(), radius, radius).fill();
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

        /// Klik memilih tab; menyeret lebih dari 3 pt memulai seret-lepas.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.track_press(event);
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn mouse_down_can_move_window(&self) -> Bool {
            Bool::NO
        }
    }

    unsafe impl NSObjectProtocol for TabRowView {}

    unsafe impl NSDraggingSource for TabRowView {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn source_operation_mask(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
            NSDragOperation::Move
        }

        #[unsafe(method(draggingSession:willBeginAtPoint:))]
        fn will_begin(&self, _session: &NSDraggingSession, _point: NSPoint) {
            self.notify_drag(objc2::sel!(sidebarDragBegan:));
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(&self, _session: &NSDraggingSession, _point: NSPoint, _operation: NSDragOperation) {
            self.notify_drag(objc2::sel!(sidebarDragEnded:));
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

/// Menu klik kanan untuk satu tab; setiap item membawa tag = id tab.
fn context_menu(mtm: MainThreadMarker, row: &RowInfo, target: &ActionTarget) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let add = |title: &str, action: objc2::runtime::Sel, enabled: bool| {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(action),
                &NSString::new(),
            )
        };
        unsafe { item.setTarget(Some(target as &AnyObject)) };
        item.setTag(row.id.0 as isize);
        item.setEnabled(enabled);
        menu.addItem(&item);
    };
    match row.kind {
        TabKind::Today => {
            add("Pin Tab", sel!(togglePin:), true);
            add("Add to Favorites", sel!(toggleFavorite:), true);
        }
        TabKind::Pinned => {
            add("Unpin Tab", sel!(togglePin:), true);
            add("Add to Favorites", sel!(toggleFavorite:), true);
        }
        TabKind::Favorite => add("Remove from Favorites", sel!(toggleFavorite:), true),
    }
    if row.kind != TabKind::Today {
        add("Set Current Page as Pinned URL", sel!(setPinnedURL:), row.saved_differs);
    }
    add("Copy Link", sel!(copyTabLink:), row.url.is_some());
    if !row.asleep {
        add("Sleep Tab", sel!(sleepTab:), !row.active);
    }
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let close = if row.kind == TabKind::Today { "Close Tab" } else { "Unload Tab" };
    add(close, sel!(closeTabButton:), true);
    menu
}

impl TabRowView {
    pub fn new(
        mtm: MainThreadMarker,
        row: &RowInfo,
        style: RowStyle,
        icon: &NSImage,
        width: f64,
        target: &ActionTarget,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RowIvars::default());
        let height = if style == RowStyle::Tile { TILE_HEIGHT } else { ROW_HEIGHT };
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.ivars().tile.set(style == RowStyle::Tile);
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
        // SAFETY: menu dibuat di main thread dan dipegang oleh view.
        unsafe { this.setMenu(Some(&context_menu(mtm, row, target))) };
        let text_alpha = if row.asleep { SECONDARY_ALPHA } else { 1.0 };

        if style == RowStyle::Tile {
            let image = NSImageView::imageViewWithImage(icon, mtm);
            image.setFrame(NSRect::new(NSPoint::new(((width - 20.0) / 2.0).round(), 10.0), NSSize::new(20.0, 20.0)));
            // Favicon di cache berukuran 16 pt (PNG 32 px); di ubin diperbesar ke 20 pt.
            image.setImageScaling(objc2_app_kit::NSImageScaling::ScaleProportionallyUpOrDown);
            image.setAlphaValue(text_alpha);
            this.addSubview(&image);
            if row.asleep {
                let moon = NSImageView::imageViewWithImage(&symbol("moon.fill", "Asleep"), mtm);
                moon.setContentTintColor(Some(&NSColor::colorWithWhite_alpha(1.0, SECONDARY_ALPHA)));
                moon.setFrame(NSRect::new(NSPoint::new(width - 13.0, 4.0), NSSize::new(9.0, 9.0)));
                this.addSubview(&moon);
            }
        } else {
            this.add_row_parts(mtm, row, icon, width, text_alpha, target);
        }

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

    /// Ikon, judul, tanda tidur/audio, dan tombol tutup untuk gaya baris.
    fn add_row_parts(
        &self,
        mtm: MainThreadMarker,
        row: &RowInfo,
        icon: &NSImage,
        width: f64,
        text_alpha: f64,
        target: &ActionTarget,
    ) {
        let image = NSImageView::imageViewWithImage(icon, mtm);
        image.setFrame(NSRect::new(NSPoint::new(8.0, 6.0), NSSize::new(16.0, 16.0)));
        image.setAlphaValue(text_alpha);
        self.addSubview(&image);

        let title = NSTextField::labelWithString(&NSString::from_str(&row.title), mtm);
        title.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        title.setTextColor(Some(&NSColor::colorWithWhite_alpha(1.0, text_alpha)));
        title.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        title.setUsesSingleLineMode(true);
        title.setFrame(NSRect::new(NSPoint::new(32.0, 5.0), NSSize::new((width - 32.0 - 28.0).max(0.0), 17.0)));
        self.addSubview(&title);
        *self.ivars().title.borrow_mut() = Some(title);

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
            self.addSubview(&view);
            *self.ivars().glyph.borrow_mut() = Some(view);
        }

        let (name, label) = if row.kind == TabKind::Today { ("xmark", "Close Tab") } else { ("minus", "Unload Tab") };
        let close = small_button(mtm, &symbol(name, label), target, sel!(closeTabButton:), row.id.0 as isize);
        close.setToolTip(Some(&NSString::from_str(label)));
        close.setContentTintColor(Some(&NSColor::whiteColor()));
        close.setFrame(NSRect::new(NSPoint::new(width - 25.0, 6.0), NSSize::new(16.0, 16.0)));
        close.setHidden(true);
        self.addSubview(&close);
        *self.ivars().close.borrow_mut() = Some(close);
    }

    /// Menunggu mouse dilepas (klik) atau digeser lebih dari 3 pt (seret).
    fn track_press(&self, down: &NSEvent) {
        let Some(window) = self.window() else { return };
        let start = down.locationInWindow();
        loop {
            let Some(event) = window.nextEventMatchingMask(NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp)
            else {
                return;
            };
            if event.r#type() == NSEventType::LeftMouseUp {
                let point = self.convertPoint_fromView(event.locationInWindow(), None);
                if self.isEnabled() && objc2_foundation::NSPointInRect(point, self.bounds()) {
                    unsafe { self.sendAction_to(self.action(), self.target().as_deref()) };
                }
                return;
            }
            let now = event.locationInWindow();
            if (now.x - start.x).hypot(now.y - start.y) > 3.0 {
                self.begin_drag(down);
                return;
            }
        }
    }

    fn begin_drag(&self, event: &NSEvent) {
        let item = NSPasteboardItem::new();
        item.setString_forType(
            &NSString::from_str(&self.tag().to_string()),
            &NSString::from_str(crate::paint::TAB_DRAG_TYPE),
        );
        let dragging =
            NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*item));
        let bounds = self.bounds();
        if let Some(rep) = self.bitmapImageRepForCachingDisplayInRect(bounds) {
            self.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
            let image = NSImage::initWithSize(NSImage::alloc(), bounds.size);
            image.addRepresentation(&rep);
            // SAFETY: isi gambar seret berupa NSImage.
            unsafe { dragging.setDraggingFrame_contents(bounds, Some(&image)) };
        }
        let items = NSArray::from_retained_slice(&[dragging]);
        self.beginDraggingSessionWithItems_event_source(&items, event, ProtocolObject::from_ref(self));
    }

    /// Memberi tahu sidebar (lewat responder chain sampai delegate jendela)
    /// bahwa seret dimulai atau selesai.
    fn notify_drag(&self, action: objc2::runtime::Sel) {
        let app = NSApplication::sharedApplication(MainThreadMarker::from(self));
        unsafe { app.sendAction_to_from(action, None, Some(self)) };
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
