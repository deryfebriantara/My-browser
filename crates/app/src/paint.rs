//! View kecil yang menggambar dirinya sendiri: isi bersudut, garis tepi,
//! bayangan kartu, dan wadah yang menjungkir koordinat (atas ke bawah).
//! Digambar sekali ke backing store view, bukan per frame isi halaman.

use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSCursor, NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSEvent, NSEventMask,
    NSEventType, NSGraphicsContext, NSShadow, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Tipe pasteboard untuk tab yang diseret di sidebar (isinya id tab).
pub const TAB_DRAG_TYPE: &str = "dev.my-browser.tab";

/// Tahap seret-lepas yang diteruskan ke penangan lepasan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPhase {
    Update,
    Exit,
    Perform(u64),
}

/// Penangan lepasan: tahap dan titik (koordinat view). `true` menerima.
pub type DropHandler = Rc<dyn Fn(DropPhase, NSPoint) -> bool>;

/// Penangan kursor masuk (`true`) atau keluar (`false`) dari view.
pub type HoverHandler = Rc<dyn Fn(bool)>;

/// Jarak bayangan kartu dari tepi kartu (ruang untuk blur).
pub const SHADOW_MARGIN: f64 = 20.0;

#[derive(Default)]
pub struct PaintIvars {
    fill: RefCell<Option<Retained<NSColor>>>,
    stroke: RefCell<Option<(Retained<NSColor>, f64)>>,
    radius: Cell<f64>,
    /// Bayangan kartu: persegi bersudut seukuran view dikurangi `SHADOW_MARGIN`.
    shadow: Cell<bool>,
    /// Klik di area kosong view ini diteruskan ke view di bawahnya.
    passthrough: Cell<bool>,
    flipped: Cell<bool>,
    drop: RefCell<Option<DropHandler>>,
    hover: RefCell<Option<HoverHandler>>,
}

define_class!(
    #[unsafe(super(NSView, objc2_app_kit::NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBPaintView"]
    #[ivars = PaintIvars]
    pub struct PaintView;

    impl PaintView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            let bounds = self.bounds();
            let radius = ivars.radius.get();
            if ivars.shadow.get() {
                let m = SHADOW_MARGIN;
                let rect = NSRect::new(
                    NSPoint::new(m, m),
                    NSSize::new((bounds.size.width - 2.0 * m).max(0.0), (bounds.size.height - 2.0 * m).max(0.0)),
                );
                let shadow = NSShadow::new();
                shadow.setShadowColor(Some(&NSColor::colorWithWhite_alpha(0.0, 0.35)));
                shadow.setShadowBlurRadius(12.0);
                shadow.setShadowOffset(NSSize::new(0.0, -4.0));
                NSGraphicsContext::saveGraphicsState_class();
                shadow.set();
                NSColor::windowBackgroundColor().setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius).fill();
                NSGraphicsContext::restoreGraphicsState_class();
            }
            if let Some(fill) = ivars.fill.borrow().as_ref() {
                fill.setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, radius, radius).fill();
            }
            if let Some((color, width)) = ivars.stroke.borrow().as_ref() {
                let inset = width / 2.0;
                let rect = NSRect::new(
                    NSPoint::new(inset, inset),
                    NSSize::new(bounds.size.width - *width, bounds.size.height - *width),
                );
                color.setStroke();
                let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, radius, radius);
                path.setLineWidth(*width);
                path.stroke();
            }
        }

        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            let hit: Option<Retained<NSView>> = unsafe { msg_send![super(self), hitTest: point] };
            let is_self = hit.as_ref().is_some_and(|h| std::ptr::eq(Retained::as_ptr(h).cast::<u8>(), (self as *const Self).cast()));
            if is_self && self.ivars().passthrough.get() { None } else { hit }
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> Bool {
            Bool::new(self.ivars().flipped.get())
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn mouse_down_can_move_window(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            let handler = self.ivars().hover.borrow().clone();
            if let Some(handler) = handler {
                handler(true);
            }
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let handler = self.ivars().hover.borrow().clone();
            if let Some(handler) = handler {
                handler(false);
            }
        }
    }

    unsafe impl NSObjectProtocol for PaintView {}

    unsafe impl NSDraggingDestination for PaintView {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.drag_update(sender)
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.drag_update(sender)
        }

        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.call_drop(DropPhase::Exit, NSPoint::new(0.0, 0.0));
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> Bool {
            let id = sender
                .draggingPasteboard()
                .stringForType(&NSString::from_str(TAB_DRAG_TYPE))
                .and_then(|s| s.to_string().parse::<u64>().ok());
            let point = self.convertPoint_fromView(sender.draggingLocation(), None);
            Bool::new(id.is_some_and(|id| self.call_drop(DropPhase::Perform(id), point)))
        }
    }
);

impl PaintView {
    fn make(mtm: MainThreadMarker, ivars: PaintIvars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    /// Persegi bersudut berisi `color`.
    pub fn fill(mtm: MainThreadMarker, color: &NSColor, radius: f64) -> Retained<Self> {
        let ivars = PaintIvars { fill: RefCell::new(Some(color.retain())), ..PaintIvars::default() };
        ivars.radius.set(radius);
        Self::make(mtm, ivars)
    }

    /// Garis tepi bersudut yang tidak menerima klik.
    pub fn outline(mtm: MainThreadMarker, color: &NSColor, width: f64, radius: f64) -> Retained<Self> {
        let ivars = PaintIvars { stroke: RefCell::new(Some((color.retain(), width))), ..PaintIvars::default() };
        ivars.radius.set(radius);
        ivars.passthrough.set(true);
        Self::make(mtm, ivars)
    }

    /// Bayangan kartu (lihat `SHADOW_MARGIN`); tidak menerima klik.
    pub fn shadow(mtm: MainThreadMarker, radius: f64) -> Retained<Self> {
        let ivars = PaintIvars::default();
        ivars.radius.set(radius);
        ivars.shadow.set(true);
        ivars.passthrough.set(true);
        Self::make(mtm, ivars)
    }

    /// Wadah transparan; `flipped` menaruh titik nol di kiri atas.
    pub fn container(mtm: MainThreadMarker, flipped: bool, passthrough: bool) -> Retained<Self> {
        let ivars = PaintIvars::default();
        ivars.flipped.set(flipped);
        ivars.passthrough.set(passthrough);
        Self::make(mtm, ivars)
    }

    /// Menjadikan view ini tujuan lepasan tab yang diseret di sidebar.
    pub fn set_drop_handler(&self, handler: DropHandler) {
        *self.ivars().drop.borrow_mut() = Some(handler);
        self.registerForDraggedTypes(&NSArray::from_retained_slice(&[NSString::from_str(TAB_DRAG_TYPE)]));
    }

    fn call_drop(&self, phase: DropPhase, point: NSPoint) -> bool {
        let handler = self.ivars().drop.borrow().clone();
        handler.is_some_and(|h| h(phase, point))
    }

    fn drag_update(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        let point = self.convertPoint_fromView(sender.draggingLocation(), None);
        if self.call_drop(DropPhase::Update, point) { NSDragOperation::Move } else { NSDragOperation::None }
    }

    /// Memanggil `handler(true)` saat kursor masuk ke view dan `handler(false)` saat keluar.
    pub fn set_hover_handler(&self, handler: HoverHandler) {
        *self.ivars().hover.borrow_mut() = Some(handler);
        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::ActiveAlways
            | NSTrackingAreaOptions::InVisibleRect;
        // SAFETY: owner (view ini) menangani mouseEntered:/mouseExited:.
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                NSRect::ZERO,
                options,
                Some(self as &AnyObject),
                None,
            )
        };
        self.addTrackingArea(&area);
    }
}

/// Penangan pegangan seret: geser horizontal total sejak mouse ditekan, dan
/// apakah mouse sudah dilepas.
pub type DragHandler = Rc<dyn Fn(f64, bool)>;

pub struct HandleIvars {
    handler: RefCell<Option<DragHandler>>,
}

define_class!(
    /// Pegangan tipis untuk mengubah lebar (kursor panah kiri-kanan).
    #[unsafe(super(NSView, objc2_app_kit::NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBDragHandle"]
    #[ivars = HandleIvars]
    pub struct DragHandle;

    impl DragHandle {
        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            // Pengganti (columnResizeCursorInDirections:) baru ada di macOS 15;
            // my-browser masih mendukung macOS 14.
            #[allow(deprecated)]
            let cursor = NSCursor::resizeLeftRightCursor();
            self.addCursorRect_cursor(self.bounds(), &cursor);
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn mouse_down_can_move_window(&self) -> Bool {
            Bool::NO
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let Some(window) = self.window() else { return };
            let Some(handler) = self.ivars().handler.borrow().clone() else { return };
            let start = event.locationInWindow().x;
            while let Some(next) = window.nextEventMatchingMask(NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp) {
                let done = next.r#type() == NSEventType::LeftMouseUp;
                handler(next.locationInWindow().x - start, done);
                if done {
                    break;
                }
            }
        }
    }
);

impl DragHandle {
    pub fn new(mtm: MainThreadMarker, handler: Option<DragHandler>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(HandleIvars { handler: RefCell::new(handler) });
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    pub fn set_handler(&self, handler: DragHandler) {
        *self.ivars().handler.borrow_mut() = Some(handler);
    }
}
