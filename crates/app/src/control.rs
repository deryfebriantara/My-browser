//! Menjalankan permintaan kontrol dari server MCP di main thread.

use crate::state::{AppState, display_title};
use browser_core::capture::{self, Plan};
use browser_core::control::{
    ControlRequest as R, ControlResponse, Device, LogLevel, TabInfo, WaitState, emulation_for,
};
use browser_core::engine::{ColorScheme, Emulation, Page, Rect};
use browser_core::mcp_access::ClientKind;
use browser_core::page_scripts as js;
use browser_core::tabs::TabId;
use serde_json::{Value, json};
use std::rc::{Rc, Weak};
use std::time::Instant;

pub type Reply = Box<dyn FnOnce(ControlResponse)>;

/// Lebar screenshot dalam piksel.
const SCREENSHOT_WIDTH: f64 = 1280.0;

const INTERACTION_OFF: &str = "Interaksi Claude dimatikan. Minta pengguna menyalakan menu \
Claude > Allow Claude to Interact di my-browser, lalu coba lagi.";

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

/// Cara script dijalankan di halaman.
#[derive(Clone, Copy)]
enum Script {
    /// Ekspresi biasa (`Page::evaluate`).
    Expression,
    /// Badan fungsi async yang boleh `await` (`Page::evaluate_async`).
    AsyncBody,
}

/// Menjalankan script di page tab tujuan. Tab yang tidur tidak dibangunkan.
fn run_on_page(state: &AppState, tab_id: Option<u64>, script: String, reply: Reply) {
    run_script_on_page(state, tab_id, Script::Expression, script, reply)
}

fn run_async_on_page(state: &AppState, tab_id: Option<u64>, body: String, reply: Reply) {
    run_script_on_page(state, tab_id, Script::AsyncBody, body, reply)
}

fn run_script_on_page(state: &AppState, tab_id: Option<u64>, kind: Script, script: String, reply: Reply) {
    let id = match target(state, tab_id) {
        Ok(id) => id,
        Err(e) => return reply(e),
    };
    let b = state.browser.borrow();
    match b.page(id) {
        Some(page) => {
            let done: Box<dyn FnOnce(Result<String, String>)> = Box::new(move |result| reply(script_result(result)));
            match kind {
                Script::Expression => page.evaluate(&script, done),
                Script::AsyncBody => page.evaluate_async(&script, done),
            }
        }
        None => {
            drop(b);
            reply(err(format!("Tab {} sedang tidur atau kosong. Panggil activate_tab atau navigate dulu.", id.0)))
        }
    }
}

fn notice(state: &AppState, message: &str) {
    state.ui.nav.show_notice(message);
}

const PANEL_READ_ONLY: &str = "Panel Tanya Claude hanya boleh membaca halaman.";

pub fn handle(state: &Rc<AppState>, request: R, kind: &ClientKind, reply: Reply) {
    // Lapisan kedua di atas daftar tool panel: ditegakkan browser sendiri.
    if *kind == ClientKind::Panel && !request.is_read_only() {
        return reply(err(PANEL_READ_ONLY));
    }
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
                    ok(
                        json!({"requested": true, "note": "Tab ditidurkan dalam sekejap kalau tidak sedang memutar media dan tidak berisi teks yang belum dikirim pengguna. Cek dengan list_tabs."}),
                    )
                } else {
                    err("Tab aktif, tab kosong, atau tab yang sudah tidur tidak bisa ditidurkan.")
                })
            }
        },
        R::MemoryUsage => reply(ok(memory_usage(state))),
        R::SearchHistory { query, limit, days } => reply(ok(search_history(state, query, limit, days))),
        #[cfg(debug_assertions)]
        R::DebugUi { action, text } => reply(crate::debug_ui::handle(state, &action, text.as_deref())),
        R::ReadPage { tab_id, max_chars } => {
            run_on_page(state, tab_id, js::read_page(max_chars.unwrap_or(js::DEFAULT_MAX_CHARS)), reply)
        }
        R::ListElements { tab_id } => run_on_page(state, tab_id, js::LIST_ELEMENTS.into(), reply),
        R::Scroll { direction, tab_id } => run_on_page(state, tab_id, js::scroll(direction), reply),
        R::Youtube { action, value, tab_id } => run_on_page(state, tab_id, js::youtube(action, value), reply),
        R::Screenshot { tab_id, full_page, selector } => match selector {
            Some(selector) => screenshot_element(state, tab_id, &selector, reply),
            None if full_page => screenshot_full_page(state, tab_id, reply),
            None => screenshot(state, tab_id, reply),
        },
        R::Emulate { device, width, height, color_scheme, tab_id } => {
            emulate(state, tab_id, device, width, height, color_scheme, reply)
        }
        R::GetSelection { tab_id } => get_selection(state, tab_id, reply),
        R::Click { element, tab_id } => {
            notice(state, &format!("Claude: clicking element {element}"));
            run_on_page(state, tab_id, js::click(element), reply)
        }
        R::Type { element, text, submit, tab_id } => {
            notice(state, &format!("Claude: typing into element {element}"));
            run_on_page(state, tab_id, js::type_text(element, &text, submit), reply)
        }
        R::RunJs { code, tab_id } => {
            notice(state, "Claude: running JavaScript");
            run_async_on_page(state, tab_id, js::run_js(&code), reply)
        }
        R::ConsoleLogs { level, since, limit, tab_id } => run_on_page(
            state,
            tab_id,
            js::console_logs(level.unwrap_or(LogLevel::All), since.unwrap_or(0), limit.unwrap_or(100).min(500)),
            reply,
        ),
        R::NetworkLog { failed_only, url_contains, include_resources, limit, tab_id } => run_on_page(
            state,
            tab_id,
            js::network_log(failed_only, url_contains.as_deref(), include_resources, limit.unwrap_or(50).min(300)),
            reply,
        ),
        R::WaitFor { selector, text, state: wait_state, timeout_ms, tab_id } => run_async_on_page(
            state,
            tab_id,
            js::wait_for(
                selector.as_deref(),
                text.as_deref(),
                wait_state.unwrap_or(WaitState::Visible),
                timeout_ms.unwrap_or(5000),
            ),
            reply,
        ),
        R::InspectElement { selector, element, styles, tab_id } => {
            run_on_page(state, tab_id, js::inspect_element(selector.as_deref(), element, styles.as_deref()), reply)
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

const RERENDER_NOTE: &str =
    "Gambar dirender ulang dari halaman (bukan foto layar): video dan canvas WebGL bisa kosong.";

type Tiles = Box<dyn FnOnce(Result<Vec<String>, String>)>;

/// Memotret potongan `plan` satu per satu (tidak sekaligus, supaya memory
/// tetap kecil), lalu memanggil `finish` dengan gambar base64.
fn capture_tiles(state: Weak<AppState>, id: TabId, plan: Rc<Plan>, mut images: Vec<String>, finish: Tiles) {
    let Some(&area) = plan.tiles.get(images.len()) else { return finish(Ok(images)) };
    let Some(state) = state.upgrade() else { return finish(Err("Browser sedang ditutup.".into())) };
    let b = state.browser.borrow();
    let Some(page) = b.page(id) else {
        drop(b);
        return finish(Err(format!("Tab {} ditutup atau tidur saat difoto.", id.0)));
    };
    let weak = Rc::downgrade(&state);
    page.capture(
        area,
        plan.scale,
        Box::new(move |result| match result {
            Err(e) => finish(Err(format!("Screenshot gagal: {e}"))),
            Ok(jpeg) => {
                images.push(base64(&jpeg));
                capture_tiles(weak, id, plan, images, finish)
            }
        }),
    );
}

fn rect_of(value: &Value) -> Option<Rect> {
    let n = |k: &str| value.get(k).and_then(Value::as_f64);
    Some(Rect { x: n("x")?, y: n("y")?, width: n("width")?, height: n("height")? })
}

/// Menjalankan script yang menghasilkan JSON di tab tujuan, lalu meneruskan
/// datanya (atau error) ke `then` bersama id tab.
fn with_page_json(
    state: &Rc<AppState>,
    tab_id: Option<u64>,
    script: &str,
    then: impl FnOnce(TabId, Value) + 'static,
    reply: Reply,
) {
    let id = match target(state, tab_id) {
        Ok(id) => id,
        Err(e) => return reply(e),
    };
    let b = state.browser.borrow();
    let Some(page) = b.page(id) else {
        drop(b);
        return reply(err(format!("Tab {} sedang tidur atau kosong. Panggil activate_tab atau navigate dulu.", id.0)));
    };
    page.evaluate(
        script,
        Box::new(move |result| match script_result(result) {
            ControlResponse::Ok(value) => then(id, value),
            error => reply(error),
        }),
    );
}

fn screenshot_full_page(state: &Rc<AppState>, tab_id: Option<u64>, reply: Reply) {
    let weak = Rc::downgrade(state);
    let (reply, fail) = split_reply(reply);
    with_page_json(
        state,
        tab_id,
        &js::document_size(),
        move |id, size| {
            let (width, height) = (size["width"].as_f64().unwrap_or(0.0), size["height"].as_f64().unwrap_or(0.0));
            if width <= 0.0 || height <= 0.0 {
                return reply(err("Halaman kosong (ukuran dokumen nol)."));
            }
            let plan = Rc::new(capture::full_page(width, height));
            let covered = plan.tiles.iter().map(|t| t.height).sum::<f64>().round();
            let truncated = plan.truncated;
            capture_tiles(
                weak,
                id,
                plan,
                Vec::new(),
                Box::new(move |result| {
                    reply(match result {
                        Err(e) => err(e),
                        Ok(images) => {
                            let mut out = json!({
                                "images_base64": images,
                                "document": {"width": width, "height": height},
                                "note": RERENDER_NOTE,
                            });
                            if truncated {
                                out["truncated"] = json!(format!(
                                    "Halaman setinggi {height} poin; hanya {covered} poin pertama yang difoto. \
                                     Pakai screenshot dengan selector untuk bagian bawah."
                                ));
                            }
                            ok(out)
                        }
                    })
                }),
            );
        },
        fail,
    );
}

fn screenshot_element(state: &Rc<AppState>, tab_id: Option<u64>, selector: &str, reply: Reply) {
    let weak = Rc::downgrade(state);
    let (reply, fail) = split_reply(reply);
    with_page_json(
        state,
        tab_id,
        &js::element_box(selector),
        move |id, value| {
            let Some(area) = rect_of(&value) else { return reply(err("Posisi elemen tidak terbaca.")) };
            let plan = Rc::new(capture::element(area));
            let truncated = plan.truncated;
            capture_tiles(
                weak,
                id,
                plan,
                Vec::new(),
                Box::new(move |result| {
                    reply(match result {
                        Err(e) => err(e),
                        Ok(images) => {
                            let mut out = json!({"images_base64": images, "box": value, "note": RERENDER_NOTE});
                            if truncated {
                                out["truncated"] = json!("Elemen sangat tinggi; hanya bagian atasnya yang difoto.");
                            }
                            ok(out)
                        }
                    })
                }),
            );
        },
        fail,
    );
}

/// Elemen yang terakhir ditunjuk pengguna: detail plus potongan gambarnya.
/// Tanpa tab_id: tab tempat pengguna menunjuk, kalau tab itu masih ada.
fn get_selection(state: &Rc<AppState>, tab_id: Option<u64>, reply: Reply) {
    let picked = state.picked_tab.get().filter(|id| state.browser.borrow().tabs().get(*id).is_some());
    let tab_id = tab_id.or(picked.map(|id| id.0));
    let weak = Rc::downgrade(state);
    let (reply, fail) = split_reply(reply);
    with_page_json(
        state,
        tab_id,
        &js::picked_element(None),
        move |id, mut info| {
            info["tab_id"] = json!(id.0);
            let Some(area) = info.get("document_box").and_then(rect_of).filter(|r| r.width > 0.0 && r.height > 0.0)
            else {
                info["image_note"] = json!("Elemen tidak terlihat (ukurannya nol), jadi tanpa gambar.");
                return reply(ok(info));
            };
            let mut plan = capture::element(area);
            let truncated = plan.tiles.len() > 1;
            plan.tiles.truncate(1);
            capture_tiles(
                weak,
                id,
                Rc::new(plan),
                Vec::new(),
                Box::new(move |result| {
                    match result {
                        Ok(images) => info["images_base64"] = json!(images),
                        Err(e) => info["image_note"] = json!(e),
                    }
                    if truncated {
                        info["image_note"] = json!(
                            "Elemen tinggi; gambar hanya bagian atasnya. Pakai screenshot dengan selector untuk seluruhnya."
                        );
                    }
                    reply(ok(info))
                }),
            );
        },
        fail,
    );
}

/// Satu balasan untuk dua jalur (sukses lewat closure, error lewat
/// `with_page_json`); siapa pun yang duluan, balasan hanya terkirim sekali.
fn split_reply(reply: Reply) -> (Reply, Reply) {
    let slot = Rc::new(std::cell::RefCell::new(Some(reply)));
    let other = slot.clone();
    let send = move |slot: &std::cell::RefCell<Option<Reply>>, response| {
        if let Some(reply) = slot.borrow_mut().take() {
            reply(response)
        }
    };
    (Box::new(move |r| send(&slot, r)), Box::new(move |r| send(&other, r)))
}

fn describe_emulation(e: &Emulation) -> String {
    if *e == Emulation::default() {
        return "normal view".into();
    }
    let mut parts = Vec::new();
    match e.viewport {
        Some((w, h)) => parts.push(format!("{}{}×{}", if e.mobile { "phone " } else { "" }, w, h)),
        None if e.mobile => parts.push("phone user agent".into()),
        None => {}
    }
    match e.color_scheme {
        ColorScheme::Dark => parts.push("dark mode".into()),
        ColorScheme::Light => parts.push("light mode".into()),
        ColorScheme::System => {}
    }
    parts.join(", ")
}

fn emulate(
    state: &Rc<AppState>,
    tab_id: Option<u64>,
    device: Option<Device>,
    width: Option<u32>,
    height: Option<u32>,
    color_scheme: Option<ColorScheme>,
    reply: Reply,
) {
    let id = match target(state, tab_id) {
        Ok(id) => id,
        Err(e) => return reply(e),
    };
    let current = state.browser.borrow().tabs().get(id).map(|t| t.emulation).unwrap_or_default();
    let next = match emulation_for(current, device, width, height, color_scheme) {
        Ok(next) => next,
        Err(e) => return reply(err(e)),
    };
    let reloaded = state.browser.borrow_mut().set_emulation(id, next);
    state.refresh();
    notice(state, &format!("Claude: {}", describe_emulation(&next)));
    let viewport = match next.viewport {
        Some((w, h)) => json!({"width": w, "height": h}),
        None => json!("seukuran jendela"),
    };
    let mut out = json!({
        "tab_id": id.0,
        "viewport": viewport,
        "mobile_user_agent": next.mobile,
        "color_scheme": next.color_scheme,
        "reloaded": reloaded,
    });
    let (area_w, area_h) = state.ui.content_size();
    if let Some((w, h)) = next.viewport
        && (w > area_w || h > area_h)
    {
        out["note"] = json!(format!(
            "Viewport lebih besar dari area jendela ({}×{}); pengguna hanya melihat sebagian, tetapi halaman dan screenshot tetap seukuran viewport.",
            area_w.round(),
            area_h.round()
        ));
    }
    if next != Emulation::default() {
        out["reset_hint"] = json!("Kembalikan dengan emulate device=reset setelah selesai.");
    }
    reply(ok(out))
}

fn megabytes(bytes: u64) -> f64 {
    (bytes as f64 / 1_048_576.0 * 10.0).round() / 10.0
}

/// Physical footprint proses browser dan proses WebKit miliknya, plus per tab.
/// Hasil tool `search_history`: halaman yang cocok, terbaru dulu.
fn search_history(state: &AppState, query: Option<String>, limit: Option<usize>, days: Option<u32>) -> Value {
    let query: String = query.unwrap_or_default().chars().take(200).collect();
    let limit = limit.unwrap_or(20).clamp(1, 100);
    let browser = state.browser.borrow();
    let since = days.map(|d| browser.unix_now().saturating_sub(u64::from(d) * 86_400));
    let (entries, total) = browser.search_history(&query, since, 0, limit);
    browser_core::history::search_json(&entries, total, crate::local_time::offset_at)
}

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
    let processes: Vec<Value> =
        procs.iter().map(|p| json!({"pid": p.pid, "name": p.name, "memory_mb": megabytes(p.footprint)})).collect();
    let mut out = json!({"total_mb": megabytes(total), "processes": processes, "tabs": tabs});
    if !procs.iter().any(|p| p.name.contains("WebKit.GPU")) {
        out["note"] = json!(
            "Proses GPU dan jaringan WebKit hanya terhitung kalau my-browser dibuka lewat Finder atau perintah open."
        );
    }
    out
}

/// Base64 standar (RFC 4648) dengan padding.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n =
            (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
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
        let cases =
            [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("foobar", "Zm9vYmFy")];
        for (input, expected) in cases {
            assert_eq!(base64(input.as_bytes()), expected, "{input}");
        }
    }

    #[test]
    fn emulation_is_described_in_plain_words() {
        let phone = Emulation { viewport: Some((390.0, 844.0)), mobile: true, color_scheme: ColorScheme::Dark };
        assert_eq!(describe_emulation(&phone), "phone 390×844, dark mode");
        assert_eq!(describe_emulation(&Emulation::default()), "normal view");
        let light = Emulation { color_scheme: ColorScheme::Light, ..Emulation::default() };
        assert_eq!(describe_emulation(&light), "light mode");
    }

    #[test]
    fn split_reply_sends_only_once() {
        let count = Rc::new(std::cell::Cell::new(0));
        let c = count.clone();
        let (a, b) = split_reply(Box::new(move |_| c.set(c.get() + 1)));
        a(ok(json!(1)));
        b(err("x"));
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn page_script_errors_become_control_errors() {
        assert_eq!(
            script_result(Ok(r#"{"error":"Elemen 3 tidak ditemukan"}"#.into())),
            err("Elemen 3 tidak ditemukan")
        );
        assert_eq!(script_result(Ok(r#"{"title":"x"}"#.into())), ok(json!({"title": "x"})));
        assert_eq!(script_result(Ok("bukan json".into())), ok(json!("bukan json")));
        assert!(matches!(script_result(Err("rusak".into())), ControlResponse::Error(m) if m.contains("rusak")));
    }
}
