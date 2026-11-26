//! Mengambil favicon tanpa cookie, menyimpannya sebagai PNG 32 px, dan
//! menyediakan gambarnya (atau monogram) untuk sidebar.
//!
//! Permintaan memakai sesi `NSURLSession` ephemeral tanpa penyimpanan cookie,
//! jadi tidak pernah membawa login Space mana pun. Balasan diproses di main
//! queue; blok penyelesaian hanya memegang data `Send` (host, kandidat), dan
//! `AppState` dicari lewat thread-local di main thread.

use crate::state::AppState;
use block2::RcBlock;
pub use browser_core::favicon::MAX_ICON_BYTES;
use browser_core::favicon::{self, FaviconCache, ICON_CANDIDATES_JS};
use browser_core::sidebar::site_host;
use browser_core::tabs::{TabId, Url};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{AllocAnyThread, define_class, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSCompositingOperation, NSDeviceRGBColorSpace,
    NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSGraphicsContext, NSImage, NSImageInterpolation,
    NSStringDrawing,
};
use objc2_foundation::{
    NSData, NSDictionary, NSError, NSHTTPURLResponse, NSObject, NSObjectProtocol, NSOperationQueue, NSPoint, NSRect,
    NSSize, NSString, NSURL, NSURLResponse, NSURLSession, NSURLSessionConfiguration, NSURLSessionDataDelegate,
    NSURLSessionDataTask, NSURLSessionDelegate, NSURLSessionResponseDisposition, NSURLSessionTask,
    NSURLSessionTaskDelegate,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::SystemTime;

/// Sisi PNG di cache (piksel): 16 pt di layar Retina.
const ICON_PIXELS: isize = 32;
const TIMEOUT_SECS: f64 = 10.0;
/// Sisi terbesar (piksel) yang mau didekode.
const MAX_ICON_PIXELS: f64 = 1024.0;

thread_local! {
    static OWNER: RefCell<Weak<AppState>> = const { RefCell::new(Weak::new()) };
}

pub struct Favicons {
    cache: FaviconCache,
    memory: RefCell<HashMap<String, Retained<NSImage>>>,
    in_flight: RefCell<HashSet<String>>,
    session: Retained<NSURLSession>,
    /// Unduhan yang sedang berjalan, per `taskIdentifier`.
    tasks: RefCell<HashMap<usize, Fetch>>,
}

/// Satu unduhan kandidat ikon.
struct Fetch {
    host: String,
    candidates: Vec<Url>,
    index: usize,
    body: Vec<u8>,
    failed: bool,
}

define_class!(
    /// Delegate sesi unduhan ikon. Tanpa state: semua data disimpan di
    /// `Favicons` (main thread), karena delegate queue sesi ini main queue.
    #[unsafe(super(NSObject))]
    #[name = "MBFaviconFetchDelegate"]
    struct FetchDelegate;

    unsafe impl NSObjectProtocol for FetchDelegate {}

    unsafe impl NSURLSessionDelegate for FetchDelegate {}

    unsafe impl NSURLSessionTaskDelegate for FetchDelegate {
        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn did_complete(&self, _session: &NSURLSession, task: &NSURLSessionTask, error: Option<&NSError>) {
            with_owner(|state| state.favicons.on_complete(state, task.taskIdentifier(), error.is_none()));
        }
    }

    unsafe impl NSURLSessionDataDelegate for FetchDelegate {
        #[unsafe(method(URLSession:dataTask:didReceiveResponse:completionHandler:))]
        fn did_receive_response(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionDataTask,
            response: &NSURLResponse,
            handler: &block2::DynBlock<dyn Fn(NSURLSessionResponseDisposition)>,
        ) {
            let status = response.downcast_ref::<NSHTTPURLResponse>().map(|r| r.statusCode());
            let allow = status == Some(200) && !exceeds_limit(0, response.expectedContentLength());
            if !allow {
                with_owner(|state| state.favicons.mark_failed(task.taskIdentifier()));
            }
            handler.call((if allow {
                NSURLSessionResponseDisposition::Allow
            } else {
                NSURLSessionResponseDisposition::Cancel
            },));
        }

        #[unsafe(method(URLSession:dataTask:didReceiveData:))]
        fn did_receive_data(&self, _session: &NSURLSession, task: &NSURLSessionDataTask, data: &NSData) {
            let keep =
                with_owner(|state| state.favicons.on_data(task.taskIdentifier(), &data.to_vec())).unwrap_or(false);
            if !keep {
                task.cancel();
            }
        }
    }
);

// SAFETY: FetchDelegate tidak punya state; metodenya hanya berjalan di main
// queue (delegate queue sesi) dan membaca AppState lewat thread-local main thread.
unsafe impl Send for FetchDelegate {}
unsafe impl Sync for FetchDelegate {}

impl FetchDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn with_owner<T>(f: impl FnOnce(&Rc<AppState>) -> T) -> Option<T> {
    OWNER.with(|o| o.borrow().upgrade()).map(|state| f(&state))
}

impl Favicons {
    pub fn new(dir: PathBuf) -> Self {
        let config = session_configuration();
        let delegate = FetchDelegate::new();
        // SAFETY: delegate dipegang sesi; balasan diproses di main queue.
        let session = unsafe {
            NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &config,
                Some(ProtocolObject::from_ref(&*delegate)),
                Some(&NSOperationQueue::mainQueue()),
            )
        };
        Self {
            cache: FaviconCache::new(dir),
            memory: RefCell::default(),
            in_flight: RefCell::default(),
            session,
            tasks: RefCell::default(),
        }
    }

    /// Gambar favicon host dari memory atau disk. None: pakai monogram.
    pub fn image(&self, host: &str) -> Option<Retained<NSImage>> {
        if let Some(image) = self.memory.borrow().get(host) {
            return Some(image.clone());
        }
        let image = image_from_png(&self.cache.load(host)?)?;
        self.memory.borrow_mut().insert(host.to_string(), image.clone());
        Some(image)
    }

    /// Ubin bersudut dengan huruf pertama host, seukuran `size` poin.
    pub fn monogram_image(host: &str, size: f64) -> Retained<NSImage> {
        let (letter, [r, g, b]) = favicon::monogram(host);
        let text = NSString::from_str(&letter.to_string());
        let handler = RcBlock::new(move |rect: NSRect| -> Bool {
            let color = NSColor::colorWithSRGBRed_green_blue_alpha(
                f64::from(r) / 255.0,
                f64::from(g) / 255.0,
                f64::from(b) / 255.0,
                1.0,
            );
            color.setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                rect,
                rect.size.width * 0.22,
                rect.size.width * 0.22,
            )
            .fill();
            let font = NSFont::boldSystemFontOfSize(rect.size.height * 0.62);
            let white = NSColor::whiteColor();
            // SAFETY: kunci atribut dan nilainya bertipe sesuai (NSFont, NSColor).
            unsafe {
                let attrs = NSDictionary::<NSString, AnyObject>::from_slices(
                    &[NSFontAttributeName, NSForegroundColorAttributeName],
                    &[&*font as &AnyObject, &*white as &AnyObject],
                );
                let glyph = text.sizeWithAttributes(Some(&attrs));
                let at = NSPoint::new(
                    rect.origin.x + (rect.size.width - glyph.width) / 2.0,
                    rect.origin.y + (rect.size.height - glyph.height) / 2.0,
                );
                text.drawAtPoint_withAttributes(at, Some(&attrs));
            }
            Bool::YES
        });
        NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(size, size), false, &handler)
    }

    /// Dipanggil saat halaman http(s) selesai dimuat. Kalau ikon host belum
    /// ada atau sudah lebih dari 7 hari, kandidatnya dibaca dari halaman lalu
    /// diambil satu per satu sampai ada yang berhasil.
    pub fn page_loaded(&self, state: &Rc<AppState>, id: TabId) {
        let b = state.browser.borrow();
        let Some(url) = b.tabs().get(id).and_then(|t| t.url.clone()) else { return };
        let Some(host) = site_host(&url) else { return };
        if !self.cache.needs_refresh(&host, SystemTime::now()) || self.in_flight.borrow().contains(&host) {
            return;
        }
        let Some(page) = b.page(id) else { return };
        self.in_flight.borrow_mut().insert(host.clone());
        OWNER.with(|o| *o.borrow_mut() = Rc::downgrade(state));
        let weak = Rc::downgrade(state);
        use browser_core::engine::Page;
        page.evaluate_isolated(
            ICON_CANDIDATES_JS,
            Box::new(move |reply| {
                let Some(state) = weak.upgrade() else { return };
                let candidates = favicon::pick_candidates(&reply.unwrap_or_default(), &url);
                state.favicons.fetch(host, candidates, 0);
            }),
        );
    }

    fn fetch(&self, host: String, candidates: Vec<Url>, index: usize) {
        let Some(target) = candidates.get(index) else {
            self.in_flight.borrow_mut().remove(&host);
            return;
        };
        let Some(nsurl) = NSURL::URLWithString(&NSString::from_str(target.as_str())) else {
            self.fetch(host, candidates, index + 1);
            return;
        };
        let task = self.session.dataTaskWithURL(&nsurl);
        let fetch = Fetch { host, candidates, index, body: Vec::new(), failed: false };
        self.tasks.borrow_mut().insert(task.taskIdentifier(), fetch);
        task.resume();
    }

    fn mark_failed(&self, task: usize) {
        if let Some(fetch) = self.tasks.borrow_mut().get_mut(&task) {
            fetch.failed = true;
        }
    }

    /// Menambah data yang diterima; `false` kalau unduhan harus dihentikan.
    fn on_data(&self, task: usize, bytes: &[u8]) -> bool {
        let mut tasks = self.tasks.borrow_mut();
        let Some(fetch) = tasks.get_mut(&task) else { return false };
        if fetch.failed || exceeds_limit(fetch.body.len() + bytes.len(), -1) {
            fetch.failed = true;
            fetch.body.clear();
            return false;
        }
        fetch.body.extend_from_slice(bytes);
        true
    }

    fn on_complete(&self, state: &Rc<AppState>, task: usize, ok: bool) {
        let Some(fetch) = self.tasks.borrow_mut().remove(&task) else { return };
        let body = (ok && !fetch.failed).then_some(fetch.body);
        self.finished(state, fetch.host, fetch.candidates, fetch.index, body);
    }

    fn finished(&self, state: &Rc<AppState>, host: String, candidates: Vec<Url>, index: usize, body: Option<Vec<u8>>) {
        let Some(png) = body.as_deref().and_then(normalize_icon) else {
            self.fetch(host, candidates, index + 1);
            return;
        };
        if let Err(e) = self.cache.store(&host, &png) {
            eprintln!("my-browser: gagal menyimpan favicon: {e}");
        }
        if let Some(image) = image_from_png(&png) {
            self.memory.borrow_mut().insert(host.clone(), image);
        }
        self.in_flight.borrow_mut().remove(&host);
        state.layout_sidebar();
    }

    /// Setelah riwayat dihapus: ikon host yang tidak lagi muncul di tab atau
    /// riwayat ikut dibuang, supaya cache tidak menjadi jejak situs yang dibuka.
    pub fn forget_unused(&self, state: &AppState) {
        let hosts = known_hosts(state);
        if let Err(e) = self.cache.retain_hosts(&hosts) {
            eprintln!("my-browser: gagal membersihkan cache favicon: {e}");
        }
        self.memory.borrow_mut().retain(|host, _| hosts.contains(host));
    }

    /// Host (dari tab dan riwayat) yang ikonnya ada di cache, beserta jalur berkasnya.
    #[cfg(debug_assertions)]
    pub fn cached(&self, state: &AppState) -> Vec<(String, PathBuf)> {
        let mut hosts: Vec<String> = known_hosts(state).into_iter().collect();
        hosts.sort();
        hosts
            .into_iter()
            .filter(|h| self.cache.load(h).is_some())
            .map(|h| (h.clone(), self.cache.path_for(&h)))
            .collect()
    }
}

/// Sesi ephemeral tanpa cookie dan tanpa cache, dengan batas waktu total
/// (bukan hanya jeda antardata) supaya server yang menetes pelan tidak
/// menahan pengambilan ikon host itu.
pub fn session_configuration() -> Retained<NSURLSessionConfiguration> {
    let config = NSURLSessionConfiguration::ephemeralSessionConfiguration();
    config.setHTTPCookieStorage(None);
    config.setHTTPShouldSetCookies(false);
    config.setURLCache(None);
    config.setTimeoutIntervalForRequest(TIMEOUT_SECS);
    config.setTimeoutIntervalForResource(TIMEOUT_SECS);
    config
}

/// Unduhan ikon dihentikan begitu ukurannya (yang diterima, atau yang
/// diumumkan server) melewati `MAX_ICON_BYTES`. `expected` negatif: tidak diketahui.
pub fn exceeds_limit(received: usize, expected: i64) -> bool {
    received > MAX_ICON_BYTES || usize::try_from(expected).is_ok_and(|e| e > MAX_ICON_BYTES)
}

/// Host semua tab (termasuk alamat simpanan slot) dan semua entri riwayat.
fn known_hosts(state: &AppState) -> HashSet<String> {
    let b = state.browser.borrow();
    let mut hosts = HashSet::new();
    for tab in b.tabs().iter() {
        for url in [&tab.url, &tab.saved_url].into_iter().flatten() {
            hosts.extend(site_host(url));
        }
    }
    for entry in b.history().entries() {
        if let Ok(url) = Url::parse(&entry.url) {
            hosts.extend(site_host(&url));
        }
    }
    hosts
}

fn image_from_png(png: &[u8]) -> Option<Retained<NSImage>> {
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png))?;
    image.setSize(NSSize::new(16.0, 16.0));
    Some(image)
}

/// Bytes ikon dari server menjadi PNG 32×32 piksel. None kalau terlalu besar,
/// tidak bisa dibaca NSImage, atau berukuran nol.
pub fn normalize_icon(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() || bytes.len() > MAX_ICON_BYTES {
        return None;
    }
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))?;
    let size = image.size();
    if !(size.width > 0.0 && size.height > 0.0) {
        return None;
    }
    // Header dibaca tanpa mendekode piksel: ikon raksasa ditolak sebelum digambar.
    let too_big = size.width > MAX_ICON_PIXELS
        || size.height > MAX_ICON_PIXELS
        || image
            .representations()
            .iter()
            .any(|rep| rep.pixelsWide() as f64 > MAX_ICON_PIXELS || rep.pixelsHigh() as f64 > MAX_ICON_PIXELS);
    if too_big {
        return None;
    }
    // SAFETY: planes null meminta NSBitmapImageRep mengalokasikan bufernya sendiri.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            ICON_PIXELS,
            ICON_PIXELS,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    let side = ICON_PIXELS as f64;
    // Rasio dipertahankan: ikon yang tidak persegi diletakkan di tengah.
    let scale = side / size.width.max(size.height);
    let (w, h) = (size.width * scale, size.height * scale);
    let target = NSRect::new(NSPoint::new((side - w) / 2.0, (side - h) / 2.0), NSSize::new(w, h));
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    context.setImageInterpolation(NSImageInterpolation::High);
    image.drawInRect_fromRect_operation_fraction(target, NSRect::ZERO, NSCompositingOperation::Copy, 1.0);
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    // SAFETY: kamus properti kosong.
    unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
        .map(|d| d.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::AllocAnyThread;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage};
    use objc2_foundation::{NSData, NSDictionary};

    /// PNG `side`×`side` piksel yang dibuat lewat NSBitmapImageRep.
    fn png(side: isize) -> Vec<u8> {
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                side,
                side,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                0,
                0,
            )
        }
        .unwrap();
        unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
            .unwrap()
            .to_vec()
    }

    fn pixel_size(bytes: &[u8]) -> (f64, f64) {
        let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes)).unwrap();
        let rep = image.representations().firstObject().unwrap();
        (rep.pixelsWide() as f64, rep.pixelsHigh() as f64)
    }

    #[test]
    fn normalize_scales_png_to_32_px() {
        let out = normalize_icon(&png(64)).expect("PNG 64 px harus diterima");
        assert_eq!(pixel_size(&out), (32.0, 32.0));
    }

    #[test]
    fn normalize_rejects_garbage() {
        assert!(normalize_icon(b"<html>not an icon</html>").is_none());
        assert!(normalize_icon(b"").is_none());
    }

    #[test]
    fn normalize_rejects_oversized_input() {
        let mut big = png(64);
        big.resize(MAX_ICON_BYTES + 1, 0);
        assert!(normalize_icon(&big).is_none());
    }

    #[test]
    fn normalize_rejects_huge_dimensions() {
        // PNG kecil di disk bisa berukuran piksel raksasa (bom dekompresi).
        assert!(normalize_icon(&png(2048)).is_none());
    }

    #[test]
    fn session_configuration_limits_time_and_cookies() {
        let config = session_configuration();
        assert_eq!(config.timeoutIntervalForRequest(), 10.0);
        assert_eq!(config.timeoutIntervalForResource(), 10.0);
        assert!(!config.HTTPShouldSetCookies());
        assert!(config.HTTPCookieStorage().is_none());
        assert!(config.URLCache().is_none());
    }

    #[test]
    fn download_stops_once_over_the_limit() {
        assert!(exceeds_limit(0, (MAX_ICON_BYTES + 1) as i64));
        assert!(!exceeds_limit(0, -1));
        assert!(exceeds_limit(MAX_ICON_BYTES + 1, -1));
        assert!(!exceeds_limit(10, 100));
    }

    #[test]
    fn normalize_rejects_zero_size_image() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"></svg>"#;
        assert!(normalize_icon(svg).is_none());
    }
}
