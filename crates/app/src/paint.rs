//! View kecil berbasis layer: isi bersudut, garis tepi, bayangan kartu, dan
//! wadah yang menjungkir koordinat (atas ke bawah). Semua tampilan disetel
//! sebagai properti Core Animation, jadi tidak ada bitmap per view.

use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSColor, NSCursor, NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSEvent, NSEventMask, NSEventType,
    NSTrackingArea, NSTrackingAreaOptions, NSView, NSViewLayerContentsRedrawPolicy,
};
use objc2_core_graphics::CGPath;
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

/// Ruang di sekitar kartu untuk bayangannya (wadah kartu sebesar ini lebih lebar).
pub const SHADOW_MARGIN: f64 = 20.0;

#[derive(Default)]
pub struct PaintIvars {
    fill: RefCell<Option<Retained<NSColor>>>,
    stroke: RefCell<Option<(Retained<NSColor>, f64)>>,
    radius: Cell<f64>,
    /// Bayangan kartu: persegi bersudut seukuran view, berisi warna latar jendela.
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
        /// Isi, garis tepi, dan bayangan disetel sebagai properti layer, bukan
        /// digambar: tidak ada bitmap seukuran view (IOSurface) per view.
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            let Some(layer) = self.layer() else { return };
            let ivars = self.ivars();
            let radius = ivars.radius.get();
            layer.setCornerRadius(radius);
            let fill = ivars.fill.borrow().clone().or_else(|| ivars.shadow.get().then(NSColor::windowBackgroundColor));
            layer.setBackgroundColor(fill.map(|c| c.CGColor()).as_deref());
            match ivars.stroke.borrow().as_ref() {
                Some((color, width)) => {
                    layer.setBorderColor(Some(&color.CGColor()));
                    layer.setBorderWidth(*width);
                }
                None => layer.setBorderWidth(0.0),
            }
            if ivars.shadow.get() {
                layer.setShadowColor(Some(&NSColor::blackColor().CGColor()));
                layer.setShadowOpacity(0.35);
                layer.setShadowRadius(8.0);
                layer.setShadowOffset(NSSize::new(0.0, -4.0));
                // Dengan shadowPath, Core Animation tidak menghitung bayangan dari isi layer.
                // SAFETY: tanpa transformasi (null).
                let path = unsafe { CGPath::with_rounded_rect(self.bounds(), radius, radius, std::ptr::null()) };
                layer.setShadowPath(Some(&path));
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
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setWantsLayer(true);
        // Bentuk bayangan mengikuti ukuran view saat diubah.
        this.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::DuringViewResize);
        this
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
