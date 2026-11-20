//! Pengirim event yang selalu menunda ke putaran run loop berikutnya, dan
//! membuang event dari page yang sudah dilepas.

use block2::RcBlock;
use browser_core::engine::PageEvent;
use objc2_foundation::NSRunLoop;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone)]
pub(crate) struct Emitter {
    sink: Rc<dyn Fn(PageEvent)>,
    alive: Rc<Cell<bool>>,
}

impl Emitter {
    pub(crate) fn new(sink: Box<dyn Fn(PageEvent)>) -> Self {
        Self { sink: Rc::from(sink), alive: Rc::new(Cell::new(true)) }
    }

    /// Dipanggil saat page dilepas; event yang masih antre tidak dikirim.
    pub(crate) fn kill(&self) {
        self.alive.set(false);
    }

    pub(crate) fn emit(&self, event: PageEvent) {
        let sink = self.sink.clone();
        let alive = self.alive.clone();
        let slot = Cell::new(Some(event));
        let block = RcBlock::new(move || {
            if let Some(event) = slot.take()
                && alive.get()
            {
                sink(event);
            }
        });
        // SAFETY: emit hanya dipanggil dari main thread (semua objek AppKit/WebKit
        // di crate ini MainThreadOnly), dan blok dijalankan di run loop main thread
        // yang sama, jadi isi blok yang tidak Send tidak pernah berpindah thread.
        unsafe { NSRunLoop::mainRunLoop().performBlock(&block) };
    }
}
