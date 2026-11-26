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
            let Some(editor) = state.ui.sidebar.address.currentEditor() else {
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
            let Some(editor) = state.ui.sidebar.address.currentEditor() else {
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
                .map(|w| {
                    json!({"title": w.title().to_string(), "visible": w.isVisible(), "key": w.isKeyWindow(), "number": w.windowNumber()})
                })
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
        // Host (dari tab dan riwayat) yang favicon-nya sudah ada di cache.
        "favicons" => {
            let cached: Vec<Value> = state
                .favicons
                .cached(state)
                .into_iter()
                .map(|(host, path)| json!({"host": host, "path": path.display().to_string()}))
                .collect();
            ok(json!({"cached": cached}))
        }
        "sidebar_state" => ok(sidebar_state(state)),
        // "<id>" mengklik baris; "<id>:close" mengklik tombol tutupnya (seperti
        // kursor di atas baris lalu klik), lewat performClick tombol sungguhan.
        "sidebar_click" => {
            let text = text.unwrap_or_default();
            let (id, part) = text.split_once(':').unwrap_or((text, ""));
            if id == "toggle" {
                unsafe { state.ui.sidebar.toggle_button().performClick(None) };
                return ok(json!({"clicked": text}));
            }
            if id == "interact_off" {
                unsafe { state.ui.sidebar.interact_off_button().performClick(None) };
                return ok(json!({"clicked": text}));
            }
            let Ok(id) = id.parse::<u64>() else { return err("id tab tidak valid") };
            let target = state.ui.sidebar.rows().iter().find(|r| r.info.id.0 == id).map(|r| r.view.clone());
            let Some(row) = target else { return err("baris tab tidak ada di sidebar") };
            match part {
                "" => unsafe { row.performClick(None) },
                "close" => {
                    row.set_hover(true);
                    let Some(close) = row.close_button() else { return err("baris tanpa tombol tutup") };
                    unsafe { close.performClick(None) };
                }
                _ => return err("bagian baris tidak dikenal"),
            }
            ok(json!({"clicked": text}))
        }
        "window_layout" => ok(window_layout(state)),
        // "<id>": menggambar ulang sidebar seolah kursor diam di tengah baris itu,
        // lalu melaporkan baris yang tombol tutupnya tampil.
        "sidebar_rerender_pointer" => {
            let Ok(id) = text.unwrap_or_default().parse::<u64>() else { return err("id tidak valid") };
            let found = state.ui.sidebar.rows().iter().find(|r| r.info.id.0 == id).map(|r| r.view.clone());
            let Some(row) = found else { return err("baris tidak ada") };
            let frame = row.convertRect_toView(row.bounds(), None);
            let point = objc2_foundation::NSPoint::new(
                frame.origin.x + frame.size.width / 2.0,
                frame.origin.y + frame.size.height / 2.0,
            );
            let model = browser_core::sidebar::sidebar_model(state.browser.borrow().tabs());
            state.ui.sidebar.render(&model, &state.favicons, &state.ui.target, Some(point));
            let visible: Vec<u64> =
                state.ui.sidebar.rows().iter().filter(|r| r.view.close_visible()).map(|r| r.info.id.0).collect();
            ok(json!({"close_visible": visible}))
        }
        // Kursor masuk ke pita tepi kiri (penangan yang sama dengan tracking area).
        "sidebar_hover_edge" => {
            state.edge_hover(true);
            ok(json!({}))
        }
        // Kursor keluar dari sidebar yang melayang.
        "sidebar_leave_overlay" => {
            state.edge_hover(false);
            state.sidebar_hover(false);
            ok(window_layout(state))
        }
        // "<lebar>": sama dengan menyeret pegangan lalu melepasnya di lebar itu.
        "sidebar_resize" => {
            let Ok(width) = text.unwrap_or_default().parse::<f64>() else { return err("lebar tidak valid") };
            let from = state.ui.sidebar_width();
            state.resize_sidebar(width - from, true);
            ok(window_layout(state))
        }
        // "<id>:<favorite|pinned|today>:<posisi>": sama dengan melepas tab
        // yang diseret di posisi itu (memanggil state.place_tab).
        // "<id>:<id_tujuan>": melepas tab di atas baris tujuan lewat jalur
        // geometri lepasan yang sebenarnya.
        "sidebar_drop_before" => {
            let Some((id, target)) = text.unwrap_or_default().split_once(':') else {
                return err("format: <id>:<id_tujuan>");
            };
            let (Ok(id), Ok(target)) = (id.parse::<u64>(), target.parse::<u64>()) else { return err("id tidak valid") };
            let Some(me) = state.me() else { return err("state hilang") };
            let dropped = state.ui.sidebar.debug_drop_before(&me, id, target);
            ok(json!({"dropped": dropped}))
        }
        "sidebar_drop" => {
            use browser_core::tabs::TabKind;
            let parts: Vec<&str> = text.unwrap_or_default().split(':').collect();
            let [id, kind, at] = parts[..] else { return err("format: <id>:<favorite|pinned|today>:<posisi>") };
            let (Ok(id), Ok(at)) = (id.parse::<u64>(), at.parse::<usize>()) else {
                return err("id atau posisi tidak valid");
            };
            let kind = match kind {
                "favorite" => TabKind::Favorite,
                "pinned" => TabKind::Pinned,
                "today" => TabKind::Today,
                _ => return err("bagian tidak dikenal"),
            };
            state.place_tab(id, kind, at);
            ok(json!({"dropped": id}))
        }
        // "<id>:<judul item>": menjalankan item menu konteks baris itu, seperti
        // klik kanan lalu memilih item tersebut.
        "sidebar_menu" => {
            let text = text.unwrap_or_default();
            let Some((id, title)) = text.split_once(':') else { return err("format: <id>:<judul item>") };
            let Ok(id) = id.parse::<u64>() else { return err("id tab tidak valid") };
            let row = state.ui.sidebar.rows().iter().find(|r| r.info.id.0 == id).map(|r| r.view.clone());
            let Some(row) = row else { return err("baris tab tidak ada di sidebar") };
            let Some(menu) = row.menu() else { return err("baris tanpa menu konteks") };
            let titles: Vec<String> = menu.itemArray().iter().map(|i| i.title().to_string()).collect();
            let Some(index) = titles.iter().position(|t| t == title) else {
                return err(&format!("item tidak ada; tersedia: {}", titles.join(", ")));
            };
            let enabled = menu.itemAtIndex(index as isize).is_some_and(|i| i.isEnabled());
            if !enabled {
                return err("item menu nonaktif");
            }
            menu.performActionForItemAtIndex(index as isize);
            ok(json!({"performed": title}))
        }
        // "<lebar>x<tinggi>" mengubah ukuran area isi jendela (seperti menyeret pojoknya).
        "window_size" => {
            let Some((w, h)) = text.unwrap_or_default().split_once('x') else { return err("format: <lebar>x<tinggi>") };
            let (Ok(w), Ok(h)) = (w.parse::<f64>(), h.parse::<f64>()) else { return err("ukuran tidak valid") };
            state.ui.window.setContentSize(objc2_foundation::NSSize::new(w, h));
            ok(window_layout(state))
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
    let field = &state.ui.sidebar.address;
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

fn rect_json(r: objc2_foundation::NSRect) -> Value {
    json!([r.origin.x, r.origin.y, r.size.width, r.size.height])
}

fn sidebar_state(state: &AppState) -> Value {
    use browser_core::tabs::TabKind;
    let rows = state.ui.sidebar.rows();
    let section = |kind: TabKind| -> Vec<Value> {
        rows.iter()
            .filter(|r| r.info.kind == kind)
            .map(|r| {
                let frame = r.view.convertRect_toView(r.view.bounds(), None);
                json!({
                    "id": r.info.id.0,
                    "title": r.info.title,
                    "url": r.info.url,
                    "active": r.info.active,
                    "asleep": r.info.asleep,
                    "playing": r.info.playing,
                    "icon": if r.favicon { "favicon" } else { "monogram" },
                    "frame": rect_json(frame),
                    "title_lines": r.view.title_lines(),
                })
            })
            .collect()
    };
    json!({
        "favorites": section(TabKind::Favorite),
        "pinned": section(TabKind::Pinned),
        "today": section(TabKind::Today),
        "interact_pill": state.ui.sidebar.interact_visible(),
    })
}

fn window_layout(state: &AppState) -> Value {
    use crate::layout::SidebarMode;
    let (sidebar, card, panel) = state.ui.frames_in_window();
    let mode = match state.ui.sidebar_mode() {
        SidebarMode::Docked => "docked",
        SidebarMode::Hidden => "hidden",
        SidebarMode::Overlay => "overlay",
    };
    json!({
        "mode": mode,
        "sidebar": rect_json(sidebar),
        "card": rect_json(card),
        "panel": panel.map(rect_json),
        "traffic_lights_hidden": state.ui.traffic_lights_hidden(),
        "outline": state.ui.card.outline_visible(),
        "toast": state.ui.card.toast_text(),
        "empty_state": state.ui.card.empty_visible(),
        "theme": state.ui.theme_id(),
        "lights_center": state.ui.traffic_light_center(),
        "movable_by_background": state.ui.window.isMovableByWindowBackground(),
        "backdrop": state.ui.backdrop_info(),
        "address_frame": rect_json(state.ui.sidebar.address.convertRect_toView(state.ui.sidebar.address.bounds(), None)),
    })
}
