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
        // Sheet (dialog) yang menempel dan yang masih terlihat.
        "sheets" => ok(sheets_state(state)),
        // Mengisi semua kolom yang bisa diedit di sheet (mis. master password).
        "sheet_fill" => {
            let app = NSApplication::sharedApplication(state.mtm());
            let mut filled = 0;
            for window in app.windows().iter() {
                let Some(sheet) = window.attachedSheet() else { continue };
                let Some(content) = sheet.contentView() else { continue };
                for field in views(&content).into_iter().filter_map(|v| v.downcast::<objc2_app_kit::NSTextField>().ok())
                {
                    if field.isEditable() {
                        field.setStringValue(&NSString::from_str(text.unwrap_or_default()));
                        filled += 1;
                    }
                }
            }
            ok(json!({"filled": filled}))
        }
        // Menekan tombol sheet lewat performClick (jalur yang sama dengan klik).
        "sheet_button" => {
            let title = text.unwrap_or_default();
            let app = NSApplication::sharedApplication(state.mtm());
            for window in app.windows().iter() {
                let Some(sheet) = window.attachedSheet() else { continue };
                let Some(content) = sheet.contentView() else { continue };
                if let Some(button) = views(&content).into_iter().find_map(|v| {
                    v.downcast::<objc2_app_kit::NSButton>().ok().filter(|b| b.title().to_string() == title)
                }) {
                    // SAFETY: main thread; sender boleh nil.
                    unsafe { button.performClick(None) };
                    return ok(json!({"clicked": title}));
                }
            }
            err("tombol sheet tidak ditemukan")
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

fn views(root: &objc2_app_kit::NSView) -> Vec<objc2::rc::Retained<objc2_app_kit::NSView>> {
    let mut out = Vec::new();
    for sub in root.subviews().iter() {
        out.extend(views(&sub));
        out.push(sub);
    }
    out
}

fn sheet_json(sheet: &objc2_app_kit::NSWindow) -> Value {
    let all = sheet.contentView().map(|c| views(&c)).unwrap_or_default();
    let texts: Vec<String> = all
        .iter()
        .filter_map(|v| v.downcast_ref::<objc2_app_kit::NSTextField>())
        .filter(|f| !f.isEditable())
        .map(|f| f.stringValue().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    let buttons: Vec<String> = all
        .iter()
        .filter_map(|v| v.downcast_ref::<objc2_app_kit::NSButton>())
        .map(|b| b.title().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    json!({"visible": sheet.isVisible(), "texts": texts, "buttons": buttons})
}

fn sheets_state(state: &AppState) -> Value {
    let app = NSApplication::sharedApplication(state.mtm());
    let windows = app.windows();
    let attached: Vec<Value> = windows.iter().filter_map(|w| w.attachedSheet()).map(|s| sheet_json(&s)).collect();
    let visible_sheets: Vec<Value> =
        windows.iter().filter(|w| w.isSheet() && w.isVisible()).map(|s| sheet_json(&s)).collect();
    json!({"attached": attached, "visible_sheet_windows": visible_sheets})
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
