//! Batas antara logika browser dan mesin render. Implementasi mesin (WebKit
//! sekarang, Servo nanti) hanya perlu memenuhi dua trait di sini.
//!
//! Aturan: event tidak pernah dikirim sinkron dari dalam pemanggilan method
//! `Engine` atau `Page`. Mesin selalu menjadwalkannya ke putaran run loop
//! berikutnya, supaya pemanggil boleh memegang borrow saat memanggil method.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsTag {
    ScrollY,
    VideoTime,
}

/// Teks rahasia (password). Tidak pernah ikut tercetak lewat `Debug` (log
/// debug mencetak event), dan isinya dihapus dari memory saat dilepas.
#[derive(Clone, PartialEq, Eq, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct Secret(String);

impl Secret {
    pub fn new(text: String) -> Self {
        Self(text)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret(<disembunyikan>)")
    }
}

/// Persegi dalam poin CSS, relatif terhadap pojok kiri atas dokumen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Skema warna yang dilaporkan ke halaman lewat `prefers-color-scheme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    /// Mengikuti macOS.
    #[default]
    System,
    Light,
    Dark,
}

/// Emulasi perangkat satu tab. Bawaan: seukuran area konten, user agent
/// desktop, dan warna mengikuti macOS.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Emulation {
    /// Ukuran viewport (lebar, tinggi) dalam poin CSS.
    pub viewport: Option<(f64, f64)>,
    /// User agent Safari iPhone, supaya situs menyajikan versi HP.
    pub mobile: bool,
    pub color_scheme: ColorScheme,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PageEvent {
    /// URL berubah karena navigasi yang memuat dokumen baru (dimulai atau dialihkan).
    UrlChanged(Url),
    /// URL berubah tanpa memuat dokumen baru: `history.pushState`,
    /// `replaceState`, atau `#fragment` (mis. klik video di YouTube).
    SameDocumentNavigation(Url),
    TitleChanged(String),
    LoadingChanged(bool),
    LoadFinished,
    NavState {
        can_back: bool,
        can_forward: bool,
    },
    OpenInNewTab(Url),
    /// Balasan `request_activity`. `media`: memutar media, memakai
    /// kamera/mikrofon, atau sedang download. `draft`: ada teks yang sudah
    /// diketik pengguna di frame utama tapi belum dikirim.
    Activity {
        media: bool,
        draft: bool,
    },
    Number {
        tag: JsTag,
        value: Option<f64>,
    },
    /// Navigasi terakhir berubah menjadi download; halaman tidak berganti.
    DownloadStarted,
    DownloadFinished(PathBuf),
    DownloadFailed(String),
    /// Halaman gagal dimuat (DNS, server mati, offline). Berisi pesan yang bisa dibaca pengguna.
    LoadFailed(String),
    ProcessCrashed,
    /// Pengguna mengirim form login di frame utama. `origin` berasal dari
    /// frame pengirim (bukan dari halaman), jadi tidak bisa dipalsukan.
    CredentialSubmitted {
        origin: String,
        username: String,
        password: Secret,
    },
}

/// Satu halaman. Melepas (`Drop`) page berarti melepas halaman dan memory-nya.
pub trait Page {
    type View: Clone;
    fn load(&self, url: &Url);
    fn go_back(&self);
    fn go_forward(&self);
    fn reload(&self);
    fn stop(&self);
    /// Dibalas dengan `PageEvent::Activity`.
    fn request_activity(&self);
    /// Dibalas dengan `PageEvent::Number`.
    fn request_number(&self, tag: JsTag, script: &str);
    fn run_script(&self, script: &str);
    /// Menjalankan script yang menghasilkan string (biasanya JSON). Balasan
    /// lewat `reply`, tidak pernah sinkron dari dalam pemanggilan ini.
    fn evaluate(&self, script: &str, reply: Box<dyn FnOnce(Result<String, String>)>);
    /// Seperti `evaluate`, tetapi `function_body` adalah badan fungsi async
    /// (boleh `await`); nilai yang di-`return` (string) menjadi balasan.
    fn evaluate_async(&self, function_body: &str, reply: Box<dyn FnOnce(Result<String, String>)>);
    /// Gambar halaman yang sedang tampil sebagai JPEG, lebar paling besar
    /// `max_width` piksel. Balasan lewat `reply`, tidak pernah sinkron.
    fn snapshot(&self, max_width: f64, reply: Box<dyn FnOnce(Result<Vec<u8>, String>)>);
    /// Gambar bagian dokumen `area` (boleh di luar layar) sebagai JPEG,
    /// `scale` piksel per poin CSS. Halaman dirender ulang, bukan difoto,
    /// jadi video dan canvas WebGL bisa kosong. Balasan tidak pernah sinkron.
    fn capture(&self, area: Rect, scale: f64, reply: Box<dyn FnOnce(Result<Vec<u8>, String>)>);
    /// User agent Safari iPhone (`true`) atau bawaan. Berlaku untuk request
    /// berikutnya; halaman perlu dimuat ulang supaya situs ikut berubah.
    fn set_mobile_user_agent(&self, mobile: bool);
    fn set_color_scheme(&self, scheme: ColorScheme);
    /// Mengisi form login di frame utama, hanya kalau origin halaman masih
    /// `origin`. Dijalankan di dunia script terpisah yang tidak terlihat
    /// oleh script halaman. Balasan JSON (`filled_password`,
    /// `filled_username`) atau `{"error": ...}`; tidak pernah sinkron.
    fn fill_login(
        &self,
        origin: &str,
        username: &str,
        password: &Secret,
        reply: Box<dyn FnOnce(Result<String, String>)>,
    );
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
    /// Melupakan petunjuk HTTP/3 (header `Alt-Svc`) yang sudah dipelajari dari
    /// situs, supaya koneksi baru memakai HTTP/2. Bawaan: tidak melakukan apa-apa.
    fn forget_http3_hints(&self) {}
}
