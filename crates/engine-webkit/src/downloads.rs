//! Pusat download milik engine. Download dipegang di sini (bukan oleh page),
//! sehingga tetap berjalan dan pemberitahuannya tetap sampai walaupun tab
//! asalnya ditutup atau ditidurkan.

use crate::download_names;
use crate::emitter::Emitter;
use browser_core::engine::PageEvent;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_foundation::{NSData, NSError, NSObject, NSObjectProtocol, NSString, NSURL, NSURLResponse};
use objc2_web_kit::{WKDownload, WKDownloadDelegate};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

fn downloads_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Downloads")
}

fn key(download: &WKDownload) -> usize {
    download as *const WKDownload as usize
}

struct Entry {
    _download: Retained<WKDownload>,
    path: Option<PathBuf>,
    /// Penghitung "sibuk" milik page asal; selama > 0 tab tidak ditidurkan.
    busy: Rc<Cell<usize>>,
}

pub(crate) struct CenterIvars {
    sink: RefCell<Option<Emitter>>,
    active: RefCell<HashMap<usize, Entry>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MBDownloadCenter"]
    #[ivars = CenterIvars]
    pub(crate) struct DownloadCenter;

    unsafe impl NSObjectProtocol for DownloadCenter {}

    unsafe impl WKDownloadDelegate for DownloadCenter {
        #[unsafe(method(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:))]
        fn decide_destination(
            &self,
            download: &WKDownload,
            _response: &NSURLResponse,
            suggested: &NSString,
            handler: &block2::DynBlock<dyn Fn(*mut NSURL)>,
        ) {
            let path = download_names::reserve_on_disk(&downloads_dir(), &suggested.to_string());
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            if let Some(entry) = self.ivars().active.borrow_mut().get_mut(&key(download)) {
                entry.path = Some(path);
            }
            handler.call((Retained::as_ptr(&url).cast_mut(),));
        }

        #[unsafe(method(downloadDidFinish:))]
        fn did_finish(&self, download: &WKDownload) {
            if let Some(path) = self.finish(download) {
                self.emit(PageEvent::DownloadFinished(path));
            }
        }

        #[unsafe(method(download:didFailWithError:resumeData:))]
        fn did_fail(&self, download: &WKDownload, error: &NSError, _resume: Option<&NSData>) {
            self.finish(download);
            self.emit(PageEvent::DownloadFailed(error.localizedDescription().to_string()));
        }
    }
);

impl DownloadCenter {
    pub(crate) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(CenterIvars { sink: RefCell::new(None), active: RefCell::default() });
        unsafe { msg_send![super(this), init] }
    }

    pub(crate) fn set_sink(&self, sink: Box<dyn Fn(PageEvent)>) {
        *self.ivars().sink.borrow_mut() = Some(Emitter::new(sink));
    }

    /// Mengambil alih download yang baru dimulai sebuah page.
    pub(crate) fn start(&self, download: &WKDownload, busy: Rc<Cell<usize>>) {
        busy.set(busy.get() + 1);
        let entry = Entry { _download: download.retain(), path: None, busy };
        self.ivars().active.borrow_mut().insert(key(download), entry);
        unsafe { download.setDelegate(Some(ProtocolObject::from_ref(self))) };
    }

    fn finish(&self, download: &WKDownload) -> Option<PathBuf> {
        let entry = self.ivars().active.borrow_mut().remove(&key(download))?;
        entry.busy.set(entry.busy.get().saturating_sub(1));
        if let Some(path) = &entry.path {
            download_names::release(path);
        }
        entry.path
    }

    fn emit(&self, event: PageEvent) {
        if let Some(sink) = self.ivars().sink.borrow().as_ref() {
            sink.emit(event);
        }
    }
}
