//! Teks yang sudah diketik pengguna tetapi belum dikirim (komentar, form,
//! pesan). Tab dengan teks seperti itu tidak ditidurkan, kecuali saat memory
//! kritis, karena halaman yang dimuat ulang kehilangan isiannya.

/// Content world untuk pelacak draft. Sama dengan script password, supaya
/// setiap halaman tidak menanggung satu content world lagi.
pub const DRAFT_WORLD: &str = crate::password_scripts::PASSWORD_WORLD;

/// Pelacak (user script di frame utama, content world `DRAFT_WORLD`).
pub const TRACKER: &str = include_str!("draft_tracker.js");

/// Ekspresi di content world `DRAFT_WORLD`: `true` kalau masih ada draft.
pub const CHECK: &str = "!!(window.__mbDraft && window.__mbDraft.pending())";
