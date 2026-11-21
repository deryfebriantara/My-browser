//! Menjalankan pekerjaan berat (Argon2id saat membuka vault) di thread
//! latar, lalu meneruskan hasilnya ke main thread. Lanjutan (`then`) boleh
//! memegang objek main thread (Rc, objek AppKit), karena ia tidak pernah
//! berpindah thread: ia disimpan di main thread dan hanya hasilnya yang
//! dikirim balik.

use dispatch2::DispatchQueue;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

type Continuation = Box<dyn FnOnce(Box<dyn Any + Send>)>;

thread_local! {
    static PENDING: RefCell<HashMap<u64, Continuation>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(0) };
}

/// Dipanggil dari main thread.
pub fn run<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, then: impl FnOnce(T) + 'static) {
    let id = NEXT.with(|n| {
        n.set(n.get() + 1);
        n.get()
    });
    let continuation: Continuation = Box::new(move |result| {
        if let Ok(result) = result.downcast::<T>() {
            then(*result);
        }
    });
    PENDING.with(|p| p.borrow_mut().insert(id, continuation));
    std::thread::spawn(move || {
        let result: Box<dyn Any + Send> = Box::new(work());
        DispatchQueue::main().exec_async(move || {
            let continuation = PENDING.with(|p| p.borrow_mut().remove(&id));
            if let Some(continuation) = continuation {
                continuation(result);
            }
        });
    });
}
