//! Menjalankan permintaan kontrol dari server MCP di main thread.

use crate::state::{AppState, display_title};
use browser_core::control::{ControlRequest as R, ControlResponse, TabInfo};
use browser_core::engine::Page;
use browser_core::page_scripts as js;
use browser_core::tabs::TabId;
use serde_json::{Value, json};
use std::rc::Rc;
use std::time::Instant;

pub type Reply = Box<dyn FnOnce(ControlResponse)>;

/// Lebar screenshot dalam piksel.
const SCREENSHOT_WIDTH: f64 = 1280.0;

const INTERACTION_OFF: &str = "Interaksi Claude dimatikan. Minta pengguna menyalakan menu \
Claude > Izinkan Claude Berinteraksi di my-browser, lalu coba lagi.";

fn ok(value: Value) -> ControlResponse {
    ControlResponse::Ok(value)
}

fn err(message: impl Into<String>) -> ControlResponse {
    ControlResponse::Error(message.into())
}

pub fn tab_infos(state: &AppState) -> Vec<TabInfo> {
    let b = state.browser.borrow();
    let active = b.tabs().active();
    b.tabs()
        .iter()
        .map(|t| TabInfo {
            id: t.id.0,
            title: display_title(t),
            url: t.url.as_ref().map(|u| u.to_string()),
            active: t.id == active,
            asleep: !t.awake && t.url.is_some(),
            loading: t.loading,
            loads: t.loads,
        })
        .collect()
}

/// Tab tujuan: `tab_id` kalau ada, kalau tidak tab aktif.
fn target(state: &AppState, tab_id: Option<u64>) -> Result<TabId, ControlResponse> {
    let b = state.browser.borrow();
    match tab_id {
        None => Ok(b.tabs().active()),
        Some(id) if b.tabs().get(TabId(id)).is_some() => Ok(TabId(id)),
        Some(id) => Err(err(format!("Tab {id} tidak ada. Panggil list_tabs untuk melihat id tab."))),
    }
}

/// Hasil script halaman: JSON dengan kunci `error` menjadi error.
fn script_result(result: Result<String, String>) -> ControlResponse {
    match result {
        Err(e) => err(format!("Script halaman gagal: {e}")),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(value) => match value.get("error").and_then(Value::as_str) {
                Some(message) => err(message),
                None => ok(value),
            },
            Err(_) => ok(Value::String(text)),
        },
    }
}

/// Menjalankan script di page tab tujuan. Tab yang tidur tidak dibangunkan.
fn run_on_page(state: &AppState, tab_id: Option<u64>, script: String, reply: Reply) {
    let id = match target(state, tab_id) {
        Ok(id) => id,
        Err(e) => return reply(e),
    };
    let b = state.browser.borrow();
    match b.page(id) {
        Some(page) => page.evaluate(&script, Box::new(move |result| reply(script_result(result)))),
        None => {
            drop(b);
            reply(err(format!("Tab {} sedang tidur atau kosong. Panggil activate_tab atau navigate dulu.", id.0)))
        }
    }
}

fn notice(state: &AppState, message: &str) {
    state.ui.nav.show_notice(message);
}

pub fn handle(state: &Rc<AppState>, request: R, reply: Reply) {
    if request.needs_interaction() && !state.claude_interact.get() {
        return reply(err(INTERACTION_OFF));
    }
    let now = Instant::now();
    match request {
        R::ListTabs => reply(ok(json!({"tabs": tab_infos(state)}))),
        R::OpenTab { url, activate } => {
            let id = state.browser.borrow_mut().open_tab(&url, activate, now);
            state.refresh();
            reply(match id {
                Some(id) => ok(json!({"tab_id": id.0})),
                None => err("URL kosong."),
            })
        }
        R::CloseTab { tab_id } => match target(state, Some(tab_id)) {
            Err(e) => reply(e),
            Ok(_) => {
                state.close_tab(tab_id);
                reply(ok(json!({"closed": tab_id})))
            }
        },
        R::ActivateTab { tab_id } => match target(state, Some(tab_id)) {
            Err(e) => reply(e),
            Ok(_) => {
                state.select_tab(tab_id);
                reply(ok(json!({"tab_id": tab_id})))
            }
        },
        R::Navigate { url, tab_id } => match target(state, tab_id) {
            Err(e) => reply(e),
            Ok(id) => {
                let loaded = state.browser.borrow_mut().navigate_tab(id, &url, now);
                state.refresh();
                reply(if loaded { ok(json!({"tab_id": id.0})) } else { err("URL kosong.") })
            }
        },
        R::GoBack { tab_id } | R::GoForward { tab_id } | R::Reload { tab_id } => {
            let id = match target(state, tab_id) {
                Ok(id) => id,
                Err(e) => return reply(e),
            };
            if state.browser.borrow().page(id).is_none() {
                return reply(err(format!("Tab {} sedang tidur atau kosong.", id.0)));
            }
            {
                let mut b = state.browser.borrow_mut();
                match request {
                    R::GoBack { .. } => b.go_back_in(id),
                    R::GoForward { .. } => b.go_forward_in(id),
                    _ => b.reload_in(id),
                }
            }
            state.refresh();
            reply(ok(json!({"tab_id": id.0})))
        }
        R::SleepTab { tab_id } => match target(state, Some(tab_id)) {
            Err(e) => reply(e),
            Ok(id) => {
                let requested = state.browser.borrow_mut().request_sleep(id);
                state.refresh();
                reply(if requested {
                    ok(json!({"requested": true, "note": "Tab ditidurkan dalam sekejap kalau tidak sedang memutar media."}))
                } else {
                    err("Tab aktif, tab kosong, atau tab yang sudah tidur tidak bisa ditidurkan.")
                })
            }
        },
        R::MemoryUsage => reply(ok(memory_usage(state))),
        R::ReadPage { tab_id, max_chars } => {
            run_on_page(state, tab_id, js::read_page(max_chars.unwrap_or(js::DEFAULT_MAX_CHARS)), reply)
        }
        R::ListElements { tab_id } => run_on_page(state, tab_id, js::LIST_ELEMENTS.into(), reply),
        R::Scroll { direction, tab_id } => run_on_page(state, tab_id, js::scroll(direction), reply),
        R::Youtube { action, value, tab_id } => run_on_page(state, tab_id, js::youtube(action, value), reply),
        R::Screenshot { tab_id } => screenshot(state, tab_id, reply),
        R::Click { element, tab_id } => {
            notice(state, &format!("Claude: klik elemen {element}"));
            run_on_page(state, tab_id, js::click(element), reply)
        }
        R::Type { element, text, submit, tab_id } => {
            notice(state, &format!("Claude: mengisi elemen {element}"));
            run_on_page(state, tab_id, js::type_text(element, &text, submit), reply)
        }
        R::RunJs { code, tab_id } => {
            notice(state, "Claude: menjalankan JavaScript");
            run_on_page(state, tab_id, js::run_js(&code), reply)
        }
    }
}

fn screenshot(state: &AppState, tab_id: Option<u64>, reply: Reply) {
    let id = match target(state, tab_id) {
        Ok(id) => id,
        Err(e) => return reply(e),
    };
    let b = state.browser.borrow();
    if id != b.tabs().active() {
        drop(b);
        return reply(err("Screenshot hanya untuk tab aktif. Panggil activate_tab dulu."));
    }
    match b.page(id) {
        Some(page) => page.snapshot(
            SCREENSHOT_WIDTH,
            Box::new(move |result| {
                reply(match result {
                    Ok(jpeg) => ok(json!({"image_base64": base64(&jpeg)})),
                    Err(e) => err(format!("Screenshot gagal: {e}")),
                })
            }),
        ),
        None => {
            drop(b);
            reply(err("Tab aktif kosong."))
        }
    }
}

fn megabytes(bytes: u64) -> f64 {
    (bytes as f64 / 1_048_576.0 * 10.0).round() / 10.0
}

/// Physical footprint proses browser dan proses WebKit miliknya, plus per tab.
fn memory_usage(state: &AppState) -> Value {
    let me = std::process::id() as i32;
    let b = state.browser.borrow();
    let tab_pids: Vec<(TabId, Option<i32>)> =
        b.tabs().iter().map(|t| (t.id, b.page(t.id).and_then(|p| p.process_id()))).collect();
    // Proses tab dikenali dari pid-nya; proses WebKit lain (GPU, jaringan)
    // hanya lewat responsible pid, yang berlaku kalau dibuka lewat open.
    let procs: Vec<_> = memtool::sys::list_processes()
        .into_iter()
        .filter(|p| p.pid == me || p.responsible_pid == me || tab_pids.iter().any(|(_, pid)| *pid == Some(p.pid)))
        .collect();
    let total: u64 = procs.iter().map(|p| p.footprint).sum();
    let tabs: Vec<Value> = b
        .tabs()
        .iter()
        .zip(&tab_pids)
        .map(|(t, (_, pid))| {
            let memory = pid.and_then(|pid| procs.iter().find(|p| p.pid == pid)).map(|p| megabytes(p.footprint));
            json!({"id": t.id.0, "title": display_title(t), "asleep": !t.awake, "pid": pid, "memory_mb": memory})
        })
        .collect();
    let processes: Vec<Value> = procs
        .iter()
        .map(|p| json!({"pid": p.pid, "name": p.name, "memory_mb": megabytes(p.footprint)}))
        .collect();
    let mut out = json!({"total_mb": megabytes(total), "processes": processes, "tabs": tabs});
    if !procs.iter().any(|p| p.name.contains("WebKit.GPU")) {
        out["note"] = json!("Proses GPU dan jaringan WebKit hanya terhitung kalau my-browser dibuka lewat Finder atau perintah open.");
    }
    out
}

/// Base64 standar (RFC 4648) dengan padding.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        let cases = [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("foobar", "Zm9vYmFy")];
        for (input, expected) in cases {
            assert_eq!(base64(input.as_bytes()), expected, "{input}");
        }
    }

    #[test]
    fn page_script_errors_become_control_errors() {
        assert_eq!(script_result(Ok(r#"{"error":"Elemen 3 tidak ditemukan"}"#.into())), err("Elemen 3 tidak ditemukan"));
        assert_eq!(script_result(Ok(r#"{"title":"x"}"#.into())), ok(json!({"title": "x"})));
        assert_eq!(script_result(Ok("bukan json".into())), ok(json!("bukan json")));
        assert!(matches!(script_result(Err("rusak".into())), ControlResponse::Error(m) if m.contains("rusak")));
    }
}
