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
    fn view(&self) -> Self::View;
}

pub trait Engine {
    type Page: Page;
    fn create_page(&self, on_event: Box<dyn Fn(PageEvent)>) -> Self::Page;
    fn set_blocker_exceptions(&self, hosts: &[String]);
}
