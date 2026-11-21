//! Vault password my-browser: berkas terenkripsi yang hanya bisa dibuka
//! dengan master password.
//!
//! Skema:
//! - master password → Argon2id (salt acak 16 byte, parameter tersimpan di
//!   berkas) → kunci pembungkus 32 byte;
//! - kunci vault: 32 byte acak, dibungkus XChaCha20-Poly1305 dengan kunci
//!   pembungkus;
//! - isi (daftar entri, JSON) dienkripsi XChaCha20-Poly1305 dengan kunci
//!   vault, dengan nonce 24 byte acak baru setiap kali disimpan;
//! - kepala berkas (versi dan parameter KDF) menjadi associated data, jadi
//!   berkas yang kepalanya diubah gagal dibuka.
//!
//! Kunci vault dibungkus terpisah supaya master password bisa diganti tanpa
//! mengenkripsi ulang isi, dan supaya cara buka lain (Touch ID lewat Secure
//! Enclave) nanti cukup menambah satu bungkusan lagi.
//!
//! Kunci dan entri dihapus dari memory saat dilepas (`zeroize`). Rust tidak
//! bisa menjamin tidak ada salinan lain (mis. saat string tumbuh), jadi ini
//! upaya terbaik, bukan jaminan.

mod crypto;
pub mod csv;
mod entries;
mod file;
pub mod generator;
mod hex;

pub use entries::{Entry, NewEntry, Offer, origin_of};
pub use file::{KdfParams, Vault, VaultError, VaultFile, load, save};
