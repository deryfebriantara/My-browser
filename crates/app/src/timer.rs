//! Tick berkala untuk kebijakan tidur dan penyimpanan sesi.

use crate::state::AppState;
use block2::RcBlock;
use browser_core::sleep::Pressure;
use objc2::rc::Retained;
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};
use std::ptr::NonNull;
use std::rc::Rc;

pub const TICK_SECS: f64 = 30.0;

/// Timer berulang di common modes, supaya tetap berjalan saat menu terbuka
/// atau window di-resize. Kembalian harus disimpan selama aplikasi hidup.
pub fn start(state: &Rc<AppState>) -> Retained<NSTimer> {
    let weak = Rc::downgrade(state);
    let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
        if let Some(state) = weak.upgrade() {
            state.tick(Pressure::Normal);
        }
    });
    let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(TICK_SECS, true, &block) };
    unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
    timer
}
