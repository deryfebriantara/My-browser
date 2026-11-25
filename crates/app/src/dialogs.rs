//! Sheet di jendela utama untuk pengelola password: kolom rahasia (master
//! password), konfirmasi, dan pilihan akun. Hanya satu sheet sekaligus;
//! balasan lewat closure di main thread.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSModalResponse, NSPopUpButton, NSSecureTextField, NSView, NSWindow,
};
use objc2_foundation::{NSPoint, NSRect, NSRunLoop, NSSize, NSString};
use std::cell::{Cell, RefCell};
use zeroize::Zeroizing;

const FIELD_W: f64 = 280.0;
const ROW_H: f64 = 30.0;

thread_local! {
    static OPEN: Cell<bool> = const { Cell::new(false) };
}

/// Sebuah sheet sedang tampil; sheet baru tidak bisa dibuka.
pub fn is_open() -> bool {
    OPEN.with(|o| o.get())
}

fn alert(mtm: MainThreadMarker, title: &str, info: &str, buttons: &[&str]) -> Retained<NSAlert> {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(info));
    for title in buttons {
        alert.addButtonWithTitle(&NSString::from_str(title));
    }
    alert
}

/// Uji otomatis, hanya di build debug: `MYBROWSER_DIALOG_SELFTEST` berisi
/// teks untuk kolom rahasia (mis. master password). Setiap dialog dijawab
/// otomatis dengan tombol ke-`MYBROWSER_DIALOG_CHOICE` (bawaan 0), dan
/// judulnya dicatat ke stderr.
#[cfg(debug_assertions)]
fn selftest(alert: &NSAlert) -> Option<String> {
    let master = std::env::var("MYBROWSER_DIALOG_SELFTEST").ok()?;
    eprintln!(
        "[selftest] dialog: {} | {}",
        alert.messageText(),
        alert.informativeText().to_string().replace('\n', " ")
    );
    Some(master)
}

/// Menampilkan sheet; `done` menerima indeks tombol (0 = pertama).
fn present(window: &NSWindow, alert: &NSAlert, done: impl FnOnce(usize) + 'static) -> bool {
    if is_open() {
        return false;
    }
    #[cfg(debug_assertions)]
    if selftest(alert).is_some() {
        let choice = std::env::var("MYBROWSER_DIALOG_CHOICE").ok().and_then(|c| c.parse().ok()).unwrap_or(0);
        let done = std::cell::Cell::new(Some(done));
        let block = RcBlock::new(move || {
            if let Some(done) = done.take() {
                done(choice);
            }
        });
        unsafe { objc2_foundation::NSRunLoop::mainRunLoop().performBlock(&block) };
        return true;
    }
    OPEN.with(|o| o.set(true));
    let done = RefCell::new(Some(done));
    let sheet = alert.window();
    let block = RcBlock::new(move |response: NSModalResponse| {
        // Sheet ini ditutup dulu, dan langkah berikutnya dijalankan di putaran
        // run loop berikutnya. Kalau langkah berikutnya membuka sheet lain
        // langsung dari dalam handler ini (mis. Save lalu master password),
        // sheet baru menempel di jendela tetapi tidak terlihat, sehingga
        // jendela terkunci tanpa dialog yang bisa ditutup.
        sheet.orderOut(None);
        OPEN.with(|o| o.set(false));
        let Some(done) = done.borrow_mut().take() else { return };
        let index = (response - NSAlertFirstButtonReturn).max(0) as usize;
        let next = Cell::new(Some(done));
        let later = RcBlock::new(move || {
            if let Some(done) = next.take() {
                done(index);
            }
        });
        // SAFETY: dari main thread ke run loop main thread.
        unsafe { NSRunLoop::mainRunLoop().performBlock(&later) };
    });
    alert.beginSheetModalForWindow_completionHandler(window, Some(&block));
    true
}

/// Sheet dengan tombol-tombol. `false` kalau sheet lain masih terbuka.
pub fn confirm(
    window: &NSWindow,
    title: &str,
    info: &str,
    buttons: &[&str],
    done: impl FnOnce(usize) + 'static,
) -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    present(window, &alert(mtm, title, info, buttons), done)
}

/// Sheet dengan kolom rahasia (NSSecureTextField: selama kolom aktif,
/// ketikan tidak bisa dibaca aplikasi lain). `done` menerima isi kolom kalau
/// tombol pertama dipilih, `None` kalau batal.
pub fn ask_secrets(
    window: &NSWindow,
    title: &str,
    info: &str,
    placeholders: &[&str],
    confirm: &str,
    done: impl FnOnce(Option<Vec<Zeroizing<String>>>) + 'static,
) -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let alert = alert(mtm, title, info, &[confirm, "Cancel"]);
    let height = ROW_H * placeholders.len() as f64;
    let accessory =
        NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(FIELD_W, height)));
    let fields: Vec<Retained<NSSecureTextField>> = placeholders
        .iter()
        .enumerate()
        .map(|(i, placeholder)| {
            let y = height - ROW_H * (i as f64 + 1.0) + 4.0;
            let field = NSSecureTextField::initWithFrame(
                NSSecureTextField::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, y), NSSize::new(FIELD_W, 24.0)),
            );
            field.setPlaceholderString(Some(&NSString::from_str(placeholder)));
            accessory.addSubview(&field);
            field
        })
        .collect();
    alert.setAccessoryView(Some(&accessory));
    alert.layout();
    if let Some(first) = fields.first() {
        alert.window().setInitialFirstResponder(Some(first));
    }
    #[cfg(debug_assertions)]
    if let Ok(master) = std::env::var("MYBROWSER_DIALOG_SELFTEST") {
        for field in &fields {
            field.setStringValue(&NSString::from_str(&master));
        }
    }
    present(window, &alert, move |button| {
        let values =
            (button == 0).then(|| fields.iter().map(|f| Zeroizing::new(f.stringValue().to_string())).collect());
        for field in &fields {
            field.setStringValue(&NSString::from_str(""));
        }
        done(values);
    })
}

/// Sheet dengan daftar pilihan. `done` menerima indeks pilihan, atau `None`.
pub fn choose(
    window: &NSWindow,
    title: &str,
    info: &str,
    options: &[String],
    confirm: &str,
    done: impl FnOnce(Option<usize>) + 'static,
) -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let alert = alert(mtm, title, info, &[confirm, "Cancel"]);
    let popup = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(FIELD_W, 26.0)),
        false,
    );
    for option in options {
        popup.addItemWithTitle(&NSString::from_str(option));
    }
    alert.setAccessoryView(Some(&popup));
    present(window, &alert, move |button| {
        let index = popup.indexOfSelectedItem();
        done((button == 0 && index >= 0).then_some(index as usize));
    })
}
