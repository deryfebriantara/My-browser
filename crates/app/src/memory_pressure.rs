//! Sinyal memory pressure macOS lewat dispatch source.

use crate::state::AppState;
use block2::RcBlock;
use browser_core::sleep::Pressure;
use dispatch2::{
    _dispatch_source_type_memorypressure, DispatchObject, DispatchQueue, DispatchRetained, DispatchSource,
    dispatch_source_memorypressure_flags_t as Flags,
};
use std::rc::Rc;

/// Harus disimpan selama aplikasi hidup; melepasnya menghentikan pemantauan.
pub struct PressureWatch {
    _source: DispatchRetained<DispatchSource>,
}

pub fn start(state: &Rc<AppState>) -> PressureWatch {
    let warn = Flags::DISPATCH_MEMORYPRESSURE_WARN.0 as usize;
    let critical = Flags::DISPATCH_MEMORYPRESSURE_CRITICAL.0 as usize;
    let source = unsafe {
        DispatchSource::new(
            (&raw const _dispatch_source_type_memorypressure).cast_mut(),
            0,
            warn | critical,
            Some(DispatchQueue::main()),
        )
    };
    let weak = Rc::downgrade(state);
    let reader = source.clone();
    let handler = RcBlock::new(move || {
        let flags = reader.data();
        let pressure = if flags & critical != 0 {
            Pressure::Critical
        } else if flags & warn != 0 {
            Pressure::Warning
        } else {
            return;
        };
        if let Some(state) = weak.upgrade() {
            state.tick(pressure);
        }
    });
    // SAFETY: handler dikirim ke main queue, jadi isi blok yang tidak Send
    // (Weak<AppState>) hanya pernah disentuh dari main thread.
    unsafe { source.set_event_handler_with_block(RcBlock::as_ptr(&handler)) };
    source.resume();
    PressureWatch { _source: source }
}
