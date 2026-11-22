//! Pengait uji khusus build debug (`debug_ui` lewat socket kontrol): mengetik
//! dan menekan tombol di address bar lewat field editor sungguhan, mengirim
//! aksi menu, dan membaca keadaan jendela. Tidak dikompilasi di build release.

use crate::state::AppState;
use browser_core::control::ControlResponse;
use objc2::msg_send;
use objc2::runtime::Sel;
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;
use serde_json::{Value, json};
use std::ffi::CString;
use std::rc::Rc;

fn ok(data: Value) -> ControlResponse {
    ControlResponse::Ok(data)
}

fn err(message: &str) -> ControlResponse {
    ControlResponse::Error(message.into())
}

fn selector(name: Option<&str>) -> Option<Sel> {
    CString::new(name?).ok().map(|name| Sel::register(&name))
}

pub fn handle(state: &Rc<AppState>, action: &str, text: Option<&str>) -> ControlResponse {
    match action {
        "focus_address" => {
            state.ui.window.makeKeyAndOrderFront(None);
            state.focus_address();
            ok(address_state(state))
        }
        // Satu insertText per huruf, seperti mengetik di keyboard.
        "type_address" => {
            let Some(editor) = state.ui.nav.address.currentEditor() else {
                return err("address bar tidak sedang diedit");
            };
            for ch in text.unwrap_or_default().chars() {
                let ch = NSString::from_str(&ch.to_string());
                let _: () = unsafe { msg_send![&*editor, insertText: &*ch] };
            }
            ok(address_state(state))
        }
        // Selector field editor, mis. moveDown:, insertNewline:, cancelOperation:.
        "key_address" => {
            let Some(editor) = state.ui.nav.address.currentEditor() else {
                return err("address bar tidak sedang diedit");
            };
            let Some(sel) = selector(text) else { return err("selector tidak valid") };
            let _: () = unsafe { msg_send![&*editor, doCommandBySelector: sel] };
            ok(address_state(state))
        }
        "address_state" => ok(address_state(state)),
        // Aksi menu milik my-browser, mis. showHistory:.
        "menu" => {
            let Some(sel) = selector(text) else { return err("selector tidak valid") };
            let app = NSApplication::sharedApplication(state.mtm());
            let sent = unsafe { app.sendAction_to_from(sel, Some(&state.ui.target), None) };
            ok(json!({"sent": sent}))
        }
        "windows" => {
            let app = NSApplication::sharedApplication(state.mtm());
            let windows: Vec<Value> = app
                .windows()
                .iter()
                .map(|w| json!({"title": w.title().to_string(), "visible": w.isVisible(), "key": w.isKeyWindow()}))
                .collect();
            ok(json!({"windows": windows}))
        }
        // Pesan seolah-olah dari tampilan jendela Riwayat.
        "history_message" => {
            state.history_window.handle(state, text.unwrap_or_default());
            ok(json!({}))
        }
        // Keluar lewat jalur normal (applicationWillTerminate menyimpan sesi dan riwayat).
        "quit" => {
            let app = NSApplication::sharedApplication(state.mtm());
            let block = block2::RcBlock::new(move || app.terminate(None));
            // SAFETY: dari main thread ke run loop main thread; balasan dikirim dulu.
            unsafe { objc2_foundation::NSRunLoop::mainRunLoop().performBlock(&block) };
            ok(json!({}))
        }
        _ => err("aksi debug_ui tidak dikenal"),
    }
}

fn address_state(state: &AppState) -> Value {
    let field = &state.ui.nav.address;
    let (text, selection, editing) = match field.currentEditor() {
        Some(editor) => {
            let range = editor.selectedRange();
            (editor.string().to_string(), [range.location, range.length], true)
        }
        None => (field.stringValue().to_string(), [0, 0], false),
    };
    let screen = state.ui.window.convertRectToScreen(field.convertRect_toView(field.bounds(), None));
    let (rows, selected) = state.address.rows();
    let rows: Vec<Value> = rows
        .iter()
        .map(|r| json!({"kind": format!("{:?}", r.kind), "title": r.title, "detail": r.detail, "url": r.url.as_str(), "fill": r.fill}))
        .collect();
    let rect = |r: objc2_foundation::NSRect| json!([r.origin.x, r.origin.y, r.size.width, r.size.height]);
    json!({
        "text": text,
        "selection": selection,
        "editing": editing,
        "popup_visible": state.address.popup_visible(),
        "popup_frame": state.address.popup_frame().map(rect),
        "field_frame": rect(screen),
        "rows": rows,
        "selected": selected,
    })
}
