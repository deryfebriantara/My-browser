//! Content blocker: kompilasi daftar aturan sekali (disimpan WebKit di disk),
//! lalu pasang ke setiap page kecuali page yang host-nya dikecualikan.

use block2::RcBlock;
use browser_core::blocking::host_matches;
use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak};
use objc2_foundation::{NSArray, NSError, NSString};
use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore, WKUserContentController};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

const PREFIX: &str = "blocklist-";

/// FNV-1a 64-bit, cukup untuk membedakan versi isi daftar.
fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

struct Registered {
    controller: Weak<WKUserContentController>,
    enabled: Rc<Cell<bool>>,
}

#[derive(Default)]
pub(crate) struct Blocker {
    lists: RefCell<Vec<Retained<WKContentRuleList>>>,
    pages: RefCell<Vec<Registered>>,
    exceptions: RefCell<Vec<String>>,
}

impl Blocker {
    pub(crate) fn set_exceptions(&self, hosts: &[String]) {
        *self.exceptions.borrow_mut() = hosts.to_vec();
    }

    pub(crate) fn should_block(&self, host: &str) -> bool {
        !host_matches(host, &self.exceptions.borrow())
    }

    /// Mendaftarkan controller page baru. Kembaliannya adalah status
    /// "pemblokir aktif" page itu, dipegang navigation delegate.
    pub(crate) fn register(&self, controller: &WKUserContentController) -> Rc<Cell<bool>> {
        let enabled = Rc::new(Cell::new(true));
        for list in self.lists.borrow().iter() {
            unsafe { controller.addContentRuleList(list) };
        }
        let mut pages = self.pages.borrow_mut();
        pages.retain(|p| p.controller.load().is_some());
        pages.push(Registered { controller: Weak::from(controller), enabled: enabled.clone() });
        enabled
    }

    /// Menyalakan/mematikan semua daftar untuk satu page.
    pub(crate) fn apply(&self, controller: &WKUserContentController, enabled: &Cell<bool>, block: bool) {
        if enabled.get() == block {
            return;
        }
        enabled.set(block);
        unsafe {
            if block {
                for list in self.lists.borrow().iter() {
                    controller.addContentRuleList(list);
                }
            } else {
                controller.removeAllContentRuleLists();
            }
        }
    }

    fn add_ready_list(&self, list: Retained<WKContentRuleList>) {
        for page in self.pages.borrow().iter() {
            if let (Some(controller), true) = (page.controller.load(), page.enabled.get()) {
                unsafe { controller.addContentRuleList(&list) };
            }
        }
        self.lists.borrow_mut().push(list);
    }

    /// Membaca `blocklist-*.json` di `dir`, mencari versi terkompilasi di
    /// penyimpanan WebKit, mengompilasi yang belum ada, dan menghapus versi
    /// lama. Semua langkah asinkron; page yang sudah hidup ikut dipasangi.
    pub(crate) fn load(self: &Rc<Self>, mtm: MainThreadMarker, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            eprintln!("my-browser: folder blocklist tidak ditemukan: {}", dir.display());
            return;
        };
        let mut files: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(PREFIX)))
            .collect();
        files.sort();

        let Some(store) = (unsafe { WKContentRuleListStore::defaultStore(mtm) }) else {
            eprintln!("my-browser: WKContentRuleListStore tidak tersedia");
            return;
        };
        let mut wanted: Vec<String> = Vec::new();
        for (i, path) in files.iter().enumerate() {
            let Ok(json) = std::fs::read_to_string(path) else { continue };
            let id = format!("{PREFIX}{i}-{:016x}", fnv1a64(json.as_bytes()));
            wanted.push(id.clone());
            self.lookup_or_compile(&store, id, json);
        }
        Self::remove_stale(&store, wanted);
    }

    fn lookup_or_compile(self: &Rc<Self>, store: &Retained<WKContentRuleListStore>, id: String, json: String) {
        let me = self.clone();
        let store_in = store.clone();
        let id_ns = NSString::from_str(&id);
        let id_in = id.clone();
        let json = RefCell::new(Some(json));
        let on_lookup = RcBlock::new(move |list: *mut WKContentRuleList, _error: *mut NSError| {
            if let Some(list) = unsafe { Retained::retain(list) } {
                me.add_ready_list(list);
                return;
            }
            let Some(json) = json.borrow_mut().take() else { return };
            let started = Instant::now();
            let me = me.clone();
            let id_log = id_in.clone();
            let on_compile = RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
                match unsafe { Retained::retain(list) } {
                    Some(list) => {
                        eprintln!("my-browser: {id_log} dikompilasi dalam {} ms", started.elapsed().as_millis());
                        me.add_ready_list(list);
                    }
                    None => {
                        let msg = unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string());
                        eprintln!("my-browser: gagal mengompilasi {id_log}: {}", msg.unwrap_or_default());
                    }
                }
            });
            unsafe {
                store_in.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
                    Some(&NSString::from_str(&id_in)),
                    Some(&NSString::from_str(&json)),
                    Some(&on_compile),
                );
            }
        });
        unsafe { store.lookUpContentRuleListForIdentifier_completionHandler(Some(&id_ns), Some(&on_lookup)) };
    }

    fn remove_stale(store: &Retained<WKContentRuleListStore>, wanted: Vec<String>) {
        let store_in = store.clone();
        let on_list = RcBlock::new(move |ids: *mut NSArray<NSString>| {
            let Some(ids) = (unsafe { ids.as_ref() }) else { return };
            // WebKit memanggil completion handler ini tanpa cek nil, jadi
            // wajib diisi meskipun hasilnya tidak dipakai.
            let on_removed = RcBlock::new(|_error: *mut NSError| {});
            for id in ids.iter() {
                let id = id.to_string();
                if id.starts_with(PREFIX) && !wanted.contains(&id) {
                    unsafe {
                        store_in.removeContentRuleListForIdentifier_completionHandler(
                            Some(&NSString::from_str(&id)),
                            Some(&on_removed),
                        );
                    }
                }
            }
        });
        unsafe { store.getAvailableContentRuleListIdentifiers(Some(&on_list)) };
    }
}
