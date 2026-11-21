//! Script halaman untuk pengelola password. Keduanya dijalankan di content
//! world `PASSWORD_WORLD`, terpisah dari script halaman.

/// Nama content world WebKit untuk script password.
pub const PASSWORD_WORLD: &str = "mb-passwords";
/// Nama handler pesan (hanya ada di content world password).
pub const PASSWORD_HANDLER: &str = "mbPassword";
/// Penangkap login (user script, frame utama).
pub const CAPTURE: &str = include_str!("password_capture.js");
/// Pengisi login (badan fungsi async; argumen `origin`, `username`, `password`).
pub const FILL: &str = include_str!("password_fill.js");
