//! Batas antara logika browser dan mesin render. Implementasi mesin (WebKit
//! sekarang, Servo nanti) hanya perlu memenuhi dua trait di sini.
//!
//! Aturan: event tidak pernah dikirim sinkron dari dalam pemanggilan method
//! `Engine` atau `Page`. Mesin selalu menjadwalkannya ke putaran run loop
//! berikutnya, supaya pemanggil boleh memegang borrow saat memanggil method.

use std::path::PathBuf;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsTag {
    ScrollY,
    VideoTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PageEvent {
    UrlChanged(Url),
    TitleChanged(String),
    LoadingChanged(bool),
    LoadFinished,
    NavState { can_back: bool, can_forward: bool },
    OpenInNewTab(Url),
    /// `true`: memutar media, memakai kamera/mikrofon, atau sedang download.
    MediaState(bool),
    Number { tag: JsTag, value: Option<f64> },
    /// Navigasi terakhir berubah menjadi download; halaman tidak berganti.
    DownloadStarted,
    DownloadFinished(PathBuf),
    DownloadFailed(String),
    /// Halaman gagal dimuat (DNS, server mati, offline). Berisi pesan yang bisa dibaca pengguna.
    LoadFailed(String),
    ProcessCrashed,
}

/// Satu halaman. Melepas (`Drop`) page berarti melepas halaman dan memory-nya.
pub trait Page {
    type View: Clone;
    fn load(&self, url: &Url);
    fn go_back(&self);
    fn go_forward(&self);
    fn reload(&self);
    fn stop(&self);
    /// Dibalas dengan `PageEvent::MediaState`.
    fn request_media_state(&self);
    /// Dibalas dengan `PageEvent::Number`.
    fn request_number(&self, tag: JsTag, script: &str);
    fn run_script(&self, script: &str);
    /// Menjalankan script yang menghasilkan string (biasanya JSON). Balasan
    /// lewat `reply`, tidak pernah sinkron dari dalam pemanggilan ini.
    fn evaluate(&self, script: &str, reply: Box<dyn FnOnce(Result<String, String>)>);
    /// Gambar halaman yang sedang tampil sebagai JPEG, lebar paling besar
    /// `max_width` poin. Balasan lewat `reply`, tidak pernah sinkron.
    fn snapshot(&self, max_width: f64, reply: Box<dyn FnOnce(Result<Vec<u8>, String>)>);
    /// Pid proses yang merender halaman ini, kalau mesin mengetahuinya.
    fn process_id(&self) -> Option<i32>;
    fn view(&self) -> Self::View;
}

pub trait Engine {
    type Page: Page;
    fn create_page(&self, on_event: Box<dyn Fn(PageEvent)>) -> Self::Page;
    fn set_blocker_exceptions(&self, hosts: &[String]);
    /// Penerima event yang tidak terikat ke satu tab (selesai/gagal download),
    /// supaya pemberitahuan tetap sampai walaupun tab asalnya sudah ditutup.
    fn set_global_sink(&self, sink: Box<dyn Fn(PageEvent)>);
}
