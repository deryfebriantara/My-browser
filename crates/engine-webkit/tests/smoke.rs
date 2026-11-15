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
    println!("smoke: semua lulus");
}
