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
