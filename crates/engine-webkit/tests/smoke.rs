//! Smoke test WebKitEngine dengan WKWebView sungguhan. Berjalan di main thread
//! (harness = false) karena AppKit dan WebKit mewajibkannya.

use browser_core::engine::{Engine, JsTag, Page, PageEvent};
use engine_webkit::WebKitEngine;
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use url::Url;

/// Tempat balasan gambar dari callback mesin.
type ImageSlot = Rc<RefCell<Option<Result<Vec<u8>, String>>>>;

const PAGE: &str = "data:text/html,%3Ctitle%3Ehalo%3C/title%3E%3Cp%3Ex%3C/p%3E";

fn spin_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        // Sengaja tanpa autorelease pool per putaran: NSApplication yang diam
        // (tanpa event dari pengguna) juga tidak mengosongkan pool-nya, jadi
        // callback mesin wajib membungkus dirinya sendiri dengan pool.
        unsafe {
            NSRunLoop::currentRunLoop()
                .runMode_beforeDate(NSDefaultRunLoopMode, &NSDate::dateWithTimeIntervalSinceNow(0.05));
        }
        if done() {
            return true;
        }
    }
    false
}

/// Halaman tiruan player YouTube. Tombol skip mengganti judul saat diklik.
fn fake_player(ad_showing: bool, skip_button: bool) -> String {
    let class = if ad_showing { "html5-video-player ad-showing" } else { "html5-video-player" };
    let button = if skip_button {
        r#"<button class="ytp-skip-ad-button" onclick="document.title='skipped'">Skip</button>"#
    } else {
        ""
    };
    format!(r#"<title>start</title><div id="movie_player" class="{class}"><video></video><div class="video-ads ytp-ad-module">{button}</div></div>"#)
}

fn load_and_wait(page: &engine_webkit::WebKitPage, events: &Rc<RefCell<Vec<PageEvent>>>, html: &str, base: &str) {
    events.borrow_mut().clear();
    page.load_html(html, &Url::parse(base).unwrap());
    let loaded = spin_until(Duration::from_secs(10), || events.borrow().contains(&PageEvent::LoadFinished));
    assert!(loaded, "halaman tiruan tidak selesai dimuat");
}

fn number(page: &engine_webkit::WebKitPage, events: &Rc<RefCell<Vec<PageEvent>>>, js: &str) -> Option<f64> {
    events.borrow_mut().clear();
    page.request_number(JsTag::ScrollY, js);
    spin_until(Duration::from_secs(5), || events.borrow().iter().any(|e| matches!(e, PageEvent::Number { .. })));
    events.borrow().iter().find_map(|e| match e {
        PageEvent::Number { value, .. } => *value,
        _ => None,
    })
}

fn title_becomes(events: &Rc<RefCell<Vec<PageEvent>>>, title: &str, within: Duration) -> bool {
    spin_until(within, || events.borrow().contains(&PageEvent::TitleChanged(title.into())))
}

/// Pemblokir bawaan melewati iklan YouTube: tombol "Lewati" diklik, iklan
/// yang tidak bisa dilewati dibisukan dan dipercepat, lalu keadaan video
/// pengguna dikembalikan. Hanya di YouTube, dan ikut mati bersama
/// pengecualian pemblokir.
fn youtube_ad_skip(engine: &WebKitEngine) {
    const YT: &str = "https://www.youtube.com/watch?v=abc";
    let events: Rc<RefCell<Vec<PageEvent>>> = Rc::default();
    let sink = events.clone();
    let page = engine.create_page(Box::new(move |e| sink.borrow_mut().push(e)));

    load_and_wait(&page, &events, &fake_player(true, true), "https://example.com/");
    assert!(!title_becomes(&events, "skipped", Duration::from_secs(2)), "skip iklan aktif di luar YouTube");
    println!("ok: skip iklan tidak aktif di luar YouTube");

    load_and_wait(&page, &events, &fake_player(true, true), YT);
    assert!(title_becomes(&events, "skipped", Duration::from_secs(3)), "tombol Lewati tidak diklik");
    println!("ok: tombol Lewati diklik otomatis");

    load_and_wait(&page, &events, &fake_player(false, false), YT);
    page.run_script("var v=document.querySelector('video');v.playbackRate=1.5;v.muted=false;");
    spin_until(Duration::from_secs(1), || false);
    page.run_script("document.getElementById('movie_player').classList.add('ad-showing')");
    spin_until(Duration::from_secs(1), || false);
    let state = "(function(){var v=document.querySelector('video');return v.playbackRate*10+(v.muted?1:0)})()";
    assert_eq!(number(&page, &events, state), Some(161.0), "iklan tanpa tombol Lewati tidak dibisukan dan dipercepat 16x");
    page.run_script("document.getElementById('movie_player').classList.remove('ad-showing')");
    spin_until(Duration::from_secs(1), || false);
    assert_eq!(number(&page, &events, state), Some(15.0), "kecepatan 1,5x dan suara pengguna tidak dikembalikan");
    println!("ok: iklan tanpa tombol Lewati dibisukan dan dipercepat, lalu dikembalikan");

    engine.set_blocker_exceptions(&["youtube.com".to_string()]);
    load_and_wait(&page, &events, &fake_player(true, true), YT);
    assert!(!title_becomes(&events, "skipped", Duration::from_secs(2)), "skip iklan tetap aktif saat pemblokir dimatikan untuk YouTube");
    engine.set_blocker_exceptions(&[]);
    load_and_wait(&page, &events, &fake_player(true, true), YT);
    assert!(title_becomes(&events, "skipped", Duration::from_secs(3)), "skip iklan tidak aktif lagi setelah pengecualian dicabut");
    println!("ok: skip iklan ikut pengecualian pemblokir");
}

fn evaluate(page: &engine_webkit::WebKitPage, script: &str) -> Result<String, String> {
    let slot: Rc<RefCell<Option<Result<String, String>>>> = Rc::default();
    let sink = slot.clone();
    page.evaluate(script, Box::new(move |r| *sink.borrow_mut() = Some(r)));
    assert!(slot.borrow().is_none(), "balasan evaluate tidak boleh sinkron");
    assert!(spin_until(Duration::from_secs(5), || slot.borrow().is_some()), "evaluate tidak dibalas");
    slot.borrow_mut().take().unwrap()
}

fn json(page: &engine_webkit::WebKitPage, script: &str) -> serde_json::Value {
    let text = evaluate(page, script).expect("script berhasil");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("bukan JSON ({e}): {text}"))
}

fn evaluate_async(page: &engine_webkit::WebKitPage, body: &str) -> Result<String, String> {
    let slot: Rc<RefCell<Option<Result<String, String>>>> = Rc::default();
    let sink = slot.clone();
    page.evaluate_async(body, Box::new(move |r| *sink.borrow_mut() = Some(r)));
    assert!(slot.borrow().is_none(), "balasan evaluate_async tidak boleh sinkron");
    assert!(spin_until(Duration::from_secs(10), || slot.borrow().is_some()), "evaluate_async tidak dibalas");
    slot.borrow_mut().take().unwrap()
}

fn json_async(page: &engine_webkit::WebKitPage, body: &str) -> serde_json::Value {
    let text = evaluate_async(page, body).expect("script berhasil");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("bukan JSON ({e}): {text}"))
}

fn messages(logs: &serde_json::Value) -> Vec<String> {
    logs["entries"].as_array().unwrap().iter().map(|e| e["message"].as_str().unwrap().to_string()).collect()
}

/// Alat developer: console_logs, network_log, wait_for, inspect_element.
fn developer_tools(engine: &WebKitEngine) {
    use browser_core::control::{LogLevel, WaitState};
    use browser_core::page_scripts as js;
    let events: Rc<RefCell<Vec<PageEvent>>> = Rc::default();
    let sink = events.clone();
    let page = engine.create_page(Box::new(move |e| sink.borrow_mut().push(e)));
    let html = r#"<title>dev</title>
        <div id="box" class="kartu utama" style="width:120px;height:40px;color:rgb(255, 0, 0);display:flex">Kotak</div>
        <script>
          console.log('halo', {a: 1});
          console.warn('awas');
          fetch('data:application/json,{"ok":true}');
          fetch('http://127.0.0.1:9/api').catch(function(){});
          var x = new XMLHttpRequest(); x.open('GET', 'data:text/plain,xhr-ok'); x.send();
          setTimeout(function(){ Promise.reject(new Error('janji gagal')); fungsiTidakAda(); }, 50);
          setTimeout(function(){ var p = document.createElement('p'); p.id = 'telat'; p.textContent = 'Muncul belakangan'; document.body.appendChild(p); }, 3000);
        </script>"#;
    load_and_wait(&page, &events, html, "http://localhost:8000/");
    spin_until(Duration::from_secs(2), || false);

    let logs = json(&page, &js::console_logs(LogLevel::All, 0, 100));
    let all = messages(&logs);
    assert!(all.contains(&r#"halo {"a":1}"#.to_string()), "{all:?}");
    assert!(all.contains(&"awas".to_string()), "{all:?}");
    assert!(all.iter().any(|m| m.contains("fungsiTidakAda")), "error tak tertangkap: {all:?}");
    assert!(all.iter().any(|m| m.contains("Unhandled promise rejection") && m.contains("janji gagal")), "{all:?}");
    let errors = json(&page, &js::console_logs(LogLevel::Error, 0, 100));
    assert!(errors["entries"].as_array().unwrap().iter().all(|e| e["level"] == "error"));
    assert!(errors["returned"].as_u64().unwrap() >= 2);
    let last = logs["last_seq"].as_u64().unwrap();
    assert_eq!(json(&page, &js::console_logs(LogLevel::All, last, 100))["returned"], 0);
    println!("ok: console_logs (console, error tak tertangkap, promise gagal, filter level dan since)");

    let net = json(&page, &js::network_log(false, None, false, 50));
    let entries = net["entries"].as_array().unwrap();
    let data_fetch = entries.iter().find(|e| e["type"] == "fetch" && e["url"].as_str().unwrap().starts_with("data:")).expect("fetch data tercatat");
    assert_eq!(data_fetch["status"], 200);
    assert!(data_fetch["body"].as_str().unwrap_or_default().contains("ok"), "isi respons situs lokal disimpan: {data_fetch}");
    let xhr = entries.iter().find(|e| e["type"] == "xhr").expect("xhr tercatat");
    assert_eq!(xhr["body"], "xhr-ok", "{xhr}");
    let failed = json(&page, &js::network_log(true, None, false, 50));
    assert_eq!(failed["returned"], 1, "{failed}");
    assert!(failed["entries"][0]["url"].as_str().unwrap().contains("127.0.0.1:9"));
    assert!(failed["entries"][0]["error"].is_string());
    assert_eq!(json(&page, &js::network_log(false, Some("127.0.0.1"), false, 50))["returned"], 1);
    println!("ok: network_log (fetch, xhr, gagal, filter, isi respons lokal)");

    let found = json_async(&page, &js::wait_for(Some("#telat"), None, WaitState::Visible, 5000));
    assert_eq!(found["found"], true, "{found}");
    assert!(found["waited_ms"].as_u64().unwrap() > 200, "seharusnya benar-benar menunggu: {found}");
    assert_eq!(json_async(&page, &js::wait_for(None, Some("Kotak"), WaitState::Visible, 1000))["found"], true);
    let missing = json_async(&page, &js::wait_for(Some("#tidak-ada"), None, WaitState::Present, 300));
    assert!(missing["error"].as_str().unwrap().contains("Waktu habis"), "{missing}");
    let invalid = json_async(&page, &js::wait_for(Some("##"), None, WaitState::Present, 300));
    assert!(invalid["error"].as_str().unwrap().contains("Selector tidak valid"), "{invalid}");
    println!("ok: wait_for (muncul belakangan, teks, waktu habis, selector rusak)");

    let info = json(&page, &js::inspect_element(Some("#box"), None, None));
    assert_eq!(info["element"], "div#box.kartu.utama", "{info}");
    assert_eq!(info["box"]["width"], 120);
    assert_eq!(info["styles"]["color"], "rgb(255, 0, 0)");
    assert_eq!(info["styles"]["display"], "flex");
    assert!(info["outer_html"].as_str().unwrap().contains(r#"id="box""#));
    assert!(json(&page, &js::inspect_element(Some("#tidak-ada"), None, None))["error"].is_string());
    println!("ok: inspect_element");

    let remote = r#"<title>remote</title><script>fetch('data:application/json,{"rahasia":1}');</script>"#;
    engine.set_blocker_exceptions(&["example.com".to_string()]);
    load_and_wait(&page, &events, remote, "https://example.com/");
    spin_until(Duration::from_secs(1), || false);
    let net = json(&page, &js::network_log(false, None, false, 50));
    assert!(net["entries"][0].get("body").is_none(), "isi respons situs biasa tidak boleh disimpan: {net}");
    assert!(net["note"].is_string());
    engine.set_blocker_exceptions(&[]);
    println!("ok: situs biasa: tanpa isi respons, dan pencatat tetap ada saat pemblokir dimatikan");
}

/// Script kontrol Claude di halaman sungguhan: baca teks, daftar elemen,
/// ketik, klik, run_js, error JS, screenshot, dan pid proses halaman.
fn page_scripts(engine: &WebKitEngine) {
    use browser_core::page_scripts as js;
    let events: Rc<RefCell<Vec<PageEvent>>> = Rc::default();
    let sink = events.clone();
    let page = engine.create_page(Box::new(move |e| sink.borrow_mut().push(e)));
    let html = r#"<title>form</title><p>Halo dunia</p>
        <input id="q" placeholder="Cari"><input type="password" value="rahasia">
        <button onclick="document.title='klik:'+document.getElementById('q').value">Kirim</button>
        <a href="https://example.com/x">Tautan</a><div style="display:none"><button>Tersembunyi</button></div>"#;
    load_and_wait(&page, &events, html, "https://example.com/");

    let read = json(&page, &js::read_page(5));
    assert_eq!(read["title"], "form");
    assert_eq!(read["text"], "Halo ");
    assert_eq!(read["truncated"], true);
    println!("ok: read_page");

    let list = json(&page, js::LIST_ELEMENTS);
    assert_eq!(list["count"], 4, "{list}");
    assert!(!list.to_string().contains("rahasia"), "nilai password tidak boleh dikirim: {list}");
    assert!(!list.to_string().contains("Tersembunyi"));
    assert_eq!(list["elements"][2]["text"], "Kirim");
    println!("ok: list_elements (tanpa nilai password dan elemen tersembunyi)");

    assert_eq!(json(&page, &js::type_text(1, "kopi \"susu\"", false))["typed"], 1);
    assert_eq!(json(&page, &js::click(3))["clicked"], 3);
    assert!(title_becomes(&events, "klik:kopi \"susu\"", Duration::from_secs(3)), "klik tidak menjalankan onclick");
    assert!(json(&page, &js::click(99))["error"].is_string());
    println!("ok: type_text dan click");

    assert_eq!(json_async(&page, &js::run_js("document.title.length"))["result"], 16);
    assert_eq!(json_async(&page, &js::run_js("Promise.resolve(41 + 1)"))["result"], 42, "Promise harus ditunggu");
    assert!(json_async(&page, &js::run_js("nggak_ada()"))["error"].as_str().unwrap().contains("ReferenceError"));
    let err = evaluate(&page, "(function(){throw new Error('rusak')})()").unwrap_err();
    assert!(err.contains("rusak"), "{err}");
    println!("ok: run_js dan error JavaScript");

    let pid = page.process_id();
    assert!(pid.is_some_and(|p| p > 0), "pid proses halaman: {pid:?}");
    println!("ok: process_id");

    page.view().setFrameSize(objc2_foundation::NSSize::new(800.0, 600.0));
    let shot: ImageSlot = Rc::default();
    let sink = shot.clone();
    page.snapshot(400.0, Box::new(move |r| *sink.borrow_mut() = Some(r)));
    assert!(spin_until(Duration::from_secs(10), || shot.borrow().is_some()), "snapshot tidak dibalas");
    let bytes = shot.borrow_mut().take().unwrap().expect("snapshot berhasil");
    assert_eq!(&bytes[..2], &[0xFF, 0xD8], "bukan JPEG");
    println!("ok: snapshot JPEG {} byte", bytes.len());
}

fn capture(page: &engine_webkit::WebKitPage, area: browser_core::engine::Rect, scale: f64) -> Vec<u8> {
    let shot: ImageSlot = Rc::default();
    let sink = shot.clone();
    page.capture(area, scale, Box::new(move |r| *sink.borrow_mut() = Some(r)));
    assert!(shot.borrow().is_none(), "balasan capture tidak boleh sinkron");
    assert!(spin_until(Duration::from_secs(10), || shot.borrow().is_some()), "capture tidak dibalas");
    shot.borrow_mut().take().unwrap().expect("capture berhasil")
}

/// Ukuran (piksel) dan warna satu piksel JPEG, dibulatkan ke 0/255 per kanal.
fn decode(jpeg: &[u8]) -> objc2::rc::Retained<objc2_app_kit::NSBitmapImageRep> {
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "bukan JPEG");
    let data = objc2_foundation::NSData::with_bytes(jpeg);
    objc2_app_kit::NSBitmapImageRep::imageRepWithData(&data).expect("JPEG bisa dibaca")
}

fn size(jpeg: &[u8]) -> (isize, isize) {
    let rep = decode(jpeg);
    (rep.pixelsWide(), rep.pixelsHigh())
}

fn color(jpeg: &[u8], x: isize, y: isize) -> &'static str {
    let rep = decode(jpeg);
    let space = objc2_app_kit::NSColorSpace::deviceRGBColorSpace();
    let c = rep.colorAtX_y(x, y).and_then(|c| c.colorUsingColorSpace(&space)).expect("warna piksel");
    let bit = |v: f64| v > 0.5;
    match (bit(c.redComponent()), bit(c.greenComponent()), bit(c.blueComponent())) {
        (true, false, false) => "merah",
        (true, true, false) => "kuning",
        (false, true, false) => "hijau",
        (false, false, true) => "biru",
        (true, false, true) => "magenta",
        (true, true, true) => "putih",
        (false, false, false) => "hitam",
        _ => "lain",
    }
}

fn mouse(page: &engine_webkit::WebKitPage, kind: &str, x: f64, y: f64) {
    let js = format!(
        "document.elementFromPoint({x},{y}).dispatchEvent(new MouseEvent('{kind}',{{bubbles:true,cancelable:true,clientX:{x},clientY:{y}}}));'ok'"
    );
    evaluate(page, &js).expect("event mouse terkirim");
}

/// Alat visual: screenshot halaman penuh dan elemen (termasuk di luar
/// layar), mode gelap/terang, user agent HP, dan mode Tunjuk Elemen.
fn visual_tools(engine: &WebKitEngine) {
    use browser_core::capture;
    use browser_core::engine::{ColorScheme, Rect};
    use browser_core::page_scripts as js;
    let events: Rc<RefCell<Vec<PageEvent>>> = Rc::default();
    let sink = events.clone();
    let page = engine.create_page(Box::new(move |e| sink.borrow_mut().push(e)));
    page.view().setFrameSize(objc2_foundation::NSSize::new(800.0, 600.0));
    let html = r#"<title>warna</title><style>body{margin:0}div{height:600px}</style>
        <div style="height:300px;background:#f00"></div><div style="height:300px;background:#ff0"></div>
        <div style="background:#0f0"></div><div style="background:#00f;position:relative">
        <span id="tombol" style="position:absolute;left:100px;top:300px;width:120px;height:40px;background:#f0f"></span></div>"#;
    load_and_wait(&page, &events, html, "http://localhost:8000/");

    let doc = json(&page, &js::document_size());
    assert_eq!((doc["width"].as_f64(), doc["height"].as_f64()), (Some(800.0), Some(1800.0)), "{doc}");
    let plan = capture::full_page(800.0, 1800.0);
    assert_eq!(plan.tiles.len(), 2);
    let first = capture(&page, plan.tiles[0], plan.scale);
    let second = capture(&page, plan.tiles[1], plan.scale);
    assert_eq!(size(&first), (800, 1600));
    assert_eq!(size(&second), (800, 200));
    assert_eq!(
        [color(&first, 10, 10), color(&first, 10, 450), color(&first, 10, 1000), color(&second, 10, 100)],
        ["merah", "kuning", "hijau", "biru"],
        "isi potongan halaman penuh (termasuk di luar layar) salah"
    );
    println!("ok: screenshot halaman penuh dipotong per 1600 px, bagian di luar layar ikut tergambar");

    let b = json(&page, &js::element_box("#tombol"));
    let area = Rect {
        x: b["x"].as_f64().unwrap(),
        y: b["y"].as_f64().unwrap(),
        width: b["width"].as_f64().unwrap(),
        height: b["height"].as_f64().unwrap(),
    };
    assert_eq!(area, Rect { x: 100.0, y: 1500.0, width: 120.0, height: 40.0 });
    let plan = capture::element(area);
    let shot = capture(&page, plan.tiles[0], plan.scale);
    assert_eq!(size(&shot), (272, 112), "elemen kecil diperbesar 2x plus padding");
    assert_eq!((color(&shot, 136, 56), color(&shot, 4, 4)), ("magenta", "biru"));
    assert!(json(&page, &js::element_box("#tidak-ada"))["error"].is_string());
    println!("ok: screenshot elemen di luar layar, diperbesar 2x");

    let dark = "String(matchMedia('(prefers-color-scheme: dark)').matches)";
    let system = evaluate(&page, dark).unwrap();
    page.set_color_scheme(ColorScheme::Light);
    assert!(spin_until(Duration::from_secs(2), || evaluate(&page, dark).unwrap() == "false"), "mode terang");
    page.set_color_scheme(ColorScheme::Dark);
    assert!(spin_until(Duration::from_secs(2), || evaluate(&page, dark).unwrap() == "true"), "mode gelap");
    page.set_color_scheme(ColorScheme::System);
    assert!(spin_until(Duration::from_secs(2), || evaluate(&page, dark).unwrap() == system), "kembali ke sistem");
    println!("ok: prefers-color-scheme terang/gelap/sistem per tab");

    page.set_mobile_user_agent(true);
    let agent = evaluate(&page, "navigator.userAgent").unwrap();
    assert!(agent.contains("iPhone") && agent.contains("Mobile/"), "{agent}");
    page.set_mobile_user_agent(false);
    assert!(evaluate(&page, "navigator.userAgent").unwrap().contains("Macintosh"));
    println!("ok: user agent Safari iPhone bisa dipasang dan dilepas");

    let menu = r#"<title>menu</title><style>body{margin:0}a{display:block;height:40px}</style>
        <ul class="menu"><li><a class="item" href="javascript:void 0">Satu</a></li>
        <li><a class="item" href="javascript:void 0" onclick="document.title='diklik'">Dua</a></li></ul>"#;
    load_and_wait(&page, &events, menu, "http://localhost:8000/");
    let target = json(&page, "JSON.stringify(document.querySelectorAll('a.item')[1].getBoundingClientRect())");
    let (x, y) = (target["x"].as_f64().unwrap() + 5.0, target["y"].as_f64().unwrap() + 5.0);
    assert!(json(&page, &js::picked_element(None))["error"].as_str().unwrap().contains("Tunjuk Elemen"));

    page.run_script(js::ELEMENT_PICKER);
    spin_until(Duration::from_millis(200), || false);
    mouse(&page, "mousemove", x, y);
    mouse(&page, "mousedown", x, y);
    mouse(&page, "click", x, y);
    let overlay = "String(!!document.querySelector('[data-mb-overlay]'))";
    assert_eq!(evaluate(&page, overlay).unwrap(), "true", "tanda Terpilih tampil sebentar setelah memilih");
    let picked = json(&page, &js::picked_element(None));
    assert_eq!(evaluate(&page, overlay).unwrap(), "false", "tanda Terpilih harus dibuang sebelum elemen difoto");
    let selector = picked["selector"].as_str().expect("selector").to_string();
    assert_eq!(picked["text"], "Dua", "{picked}");
    assert!(picked["document_box"]["width"].as_f64().unwrap() > 0.0);
    let same = format!("String(document.querySelector({:?})===document.querySelectorAll('a.item')[1])", selector);
    assert_eq!(evaluate(&page, &same).unwrap(), "true", "selector {selector} tidak menunjuk elemen yang dipilih");
    assert!(!title_becomes(&events, "diklik", Duration::from_millis(300)), "klik saat memilih tidak boleh sampai ke halaman");
    println!("ok: Tunjuk Elemen: elemen terpilih, selector unik ({selector}), klik tidak sampai ke halaman");

    page.run_script("var li=document.querySelectorAll('li')[1];li.innerHTML=li.innerHTML;");
    assert_eq!(json(&page, &js::picked_element(None))["text"], "Dua", "elemen yang dirender ulang dicari lewat selector");

    page.run_script(js::ELEMENT_PICKER);
    spin_until(Duration::from_millis(200), || false);
    mouse(&page, "click", x, y);
    assert!(spin_until(Duration::from_secs(3), || evaluate(&page, overlay).unwrap() == "false"), "tanda Terpilih tidak hilang sendiri");

    page.run_script(js::ELEMENT_PICKER);
    spin_until(Duration::from_millis(200), || false);
    page.run_script("dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}))");
    spin_until(Duration::from_millis(200), || false);
    mouse(&page, "click", x, y);
    assert!(title_becomes(&events, "diklik", Duration::from_secs(2)), "setelah Esc, klik harus sampai ke halaman lagi");
    assert_eq!(evaluate(&page, "String(!document.querySelector('[data-mb-overlay]'))").unwrap(), "true");
    println!("ok: Esc membatalkan mode Tunjuk Elemen");
}

/// Panel HTML lokal: pesan dua arah, tidak bisa berpindah halaman, dan
/// webview-nya bebas setelah panel dilepas.
fn panel_bridge(mtm: MainThreadMarker) {
    let html = r#"<script>
        function post(o){ webkit.messageHandlers.mb.postMessage(JSON.stringify(o)); }
        window.mbReceive = function(m){
          if (m.t === 'go') { location.href = 'https://example.com/'; setTimeout(function(){ post({t:'href', href: location.href}); }, 800); }
          else post({t:'echo', got: m.text});
        };
        post({t:'ready'});
    </script>"#;
    let inbox: Rc<RefCell<Vec<serde_json::Value>>> = Rc::default();
    let sink = inbox.clone();
    // Pool per langkah, seperti event loop AppKit di aplikasi: objek yang
    // di-autorelease saat membuat webview harus dikuras supaya bisa bebas.
    let panel = objc2::rc::autoreleasepool(|_| {
        engine_webkit::PanelView::new(mtm, html, Box::new(move |text| sink.borrow_mut().push(serde_json::from_str(&text).unwrap())))
    });
    let has = |t: &str| inbox.borrow().iter().any(|m| m["t"] == t);
    assert!(spin_until(Duration::from_secs(10), || has("ready")), "panel tidak mengirim ready");
    objc2::rc::autoreleasepool(|_| panel.send(&serde_json::json!({"t": "echo", "text": "halo \u{2028} \"kutip\" </script>"}).to_string()));
    assert!(spin_until(Duration::from_secs(5), || has("echo")), "pesan ke panel tidak sampai");
    assert_eq!(inbox.borrow().iter().find(|m| m["t"] == "echo").unwrap()["got"], "halo \u{2028} \"kutip\" </script>");
    println!("ok: panel: pesan dua arah (termasuk U+2028, kutip, dan </script>)");

    objc2::rc::autoreleasepool(|_| panel.send(r#"{"t":"go"}"#));
    assert!(spin_until(Duration::from_secs(5), || has("href")), "panel tidak melapor");
    assert_eq!(inbox.borrow().iter().find(|m| m["t"] == "href").unwrap()["href"], "about:blank", "panel tidak boleh berpindah halaman");
    println!("ok: panel: navigasi ke situs lain diblokir");

    let weak = objc2::rc::autoreleasepool(|_| objc2::rc::Weak::from(&*panel.view()));
    objc2::rc::autoreleasepool(|_| drop(panel));
    assert!(spin_until(Duration::from_secs(5), || weak.load().is_none()), "webview panel masih hidup setelah dilepas");
    println!("ok: panel: webview dibebaskan setelah ditutup");
}

fn main() {
    let mtm = MainThreadMarker::new().expect("smoke test harus di main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let engine = WebKitEngine::new(mtm);
    let url = Url::parse(PAGE).unwrap();

    let events: Rc<RefCell<Vec<PageEvent>>> = Rc::default();
    let sink = events.clone();
    let page = engine.create_page(Box::new(move |e| sink.borrow_mut().push(e)));

    page.load(&url);
    assert!(events.borrow().is_empty(), "event tidak boleh dikirim sinkron dari dalam load()");
    println!("ok: tidak ada event sinkron");

    let loaded = spin_until(Duration::from_secs(10), || {
        let ev = events.borrow();
        ev.contains(&PageEvent::TitleChanged("halo".into())) && ev.contains(&PageEvent::LoadFinished)
    });
    assert!(loaded, "TitleChanged(halo) dan LoadFinished tidak datang; event: {:?}", events.borrow());
    println!("ok: TitleChanged dan LoadFinished");

    page.request_number(JsTag::ScrollY, "1+1");
    let number = spin_until(Duration::from_secs(5), || {
        events.borrow().contains(&PageEvent::Number { tag: JsTag::ScrollY, value: Some(2.0) })
    });
    assert!(number, "request_number tidak dibalas Some(2.0)");
    println!("ok: request_number");

    page.request_media_state();
    let media = spin_until(Duration::from_secs(5), || events.borrow().contains(&PageEvent::MediaState(false)));
    assert!(media, "request_media_state tidak dibalas MediaState(false)");
    println!("ok: request_media_state");
    drop(page);

    for i in 0..3 {
        let done: Rc<RefCell<bool>> = Rc::default();
        let flag = done.clone();
        let p = engine.create_page(Box::new(move |e| {
            if e == PageEvent::LoadFinished {
                *flag.borrow_mut() = true;
            }
        }));
        p.load(&url);
        assert!(spin_until(Duration::from_secs(10), || *done.borrow()), "siklus {i}: LoadFinished tidak datang");
        drop(p);
        spin_until(Duration::from_millis(200), || false);
    }
    println!("ok: tiga siklus buat dan lepas page tanpa crash");

    // Melepas page harus membebaskan WKWebView-nya; kalau tidak, proses
    // WebContent tidak pernah dilepas dan penidur tab tidak menghemat apa pun.
    // Page B dibuat DAN dilepas di dalam callback event (seperti tab baru dari
    // link, lalu ditidurkan). Callback berjalan dari run loop tanpa pool luar,
    // jadi mesin wajib membungkus pengiriman event dengan autorelease pool;
    // kalau tidak, autorelease saat membuat WKWebView bocor dan B tidak pernah
    // dibebaskan.
    let engine = Rc::new(engine);
    let holder: Rc<RefCell<Option<engine_webkit::WebKitPage>>> = Rc::default();
    let b_loaded: Rc<RefCell<bool>> = Rc::default();
    let page_a = objc2::rc::autoreleasepool(|_| {
        let engine_in = engine.clone();
        let slot = holder.clone();
        let flag = b_loaded.clone();
        let a = engine.create_page(Box::new(move |e| {
            if e != PageEvent::LoadFinished || slot.borrow().is_some() {
                return;
            }
            let flag = flag.clone();
            let slot_in = slot.clone();
            let b = engine_in.create_page(Box::new(move |e| match e {
                PageEvent::LoadFinished => *flag.borrow_mut() = true,
                PageEvent::MediaState(_) => drop(slot_in.borrow_mut().take()),
                _ => {}
            }));
            b.load(&Url::parse(PAGE).unwrap());
            *slot.borrow_mut() = Some(b);
        }));
        a.load(&url);
        a
    });
    assert!(spin_until(Duration::from_secs(10), || *b_loaded.borrow()), "page B tidak selesai dimuat");
    let weak = objc2::rc::autoreleasepool(|_| objc2::rc::Weak::from(&*holder.borrow().as_ref().unwrap().view()));
    objc2::rc::autoreleasepool(|_| holder.borrow().as_ref().unwrap().request_media_state());
    let freed = spin_until(Duration::from_secs(5), || weak.load().is_none());
    assert!(freed, "WKWebView masih hidup setelah page dilepas (retain cycle?)");
    println!("ok: WKWebView dibebaskan setelah page dibuat dan dilepas di dalam callback");
    drop(page_a);

    youtube_ad_skip(&engine);
    page_scripts(&engine);
    developer_tools(&engine);
    visual_tools(&engine);
    panel_bridge(mtm);

    // Daftar aturan yang isinya berubah (mis. setelah update-filters.sh)
    // membuat daftar lama di penyimpanan WebKit usang dan dihapus. WebKit
    // memanggil completion handler penghapusan tanpa cek nil, jadi handler
    // kosong membuat proses crash.
    let base = std::env::temp_dir().join(format!("mb-smoke-blocklist-{}", std::process::id()));
    let rule = |sel: &str| format!(r#"[{{"trigger":{{"url-filter":".*"}},"action":{{"type":"css-display-none","selector":"{sel}"}}}}]"#);
    for (name, sel) in [("a", ".mb-smoke-a"), ("b", ".mb-smoke-b")] {
        std::fs::create_dir_all(base.join(name)).unwrap();
        std::fs::write(base.join(name).join("blocklist-0.json"), rule(sel)).unwrap();
    }
    engine.load_blocklists(&base.join("a"));
    spin_until(Duration::from_secs(3), || false);
    engine.load_blocklists(&base.join("b"));
    spin_until(Duration::from_secs(3), || false);
    let _ = std::fs::remove_dir_all(&base);
    println!("ok: daftar aturan usang dihapus tanpa crash");
    println!("smoke: semua lulus");
}
