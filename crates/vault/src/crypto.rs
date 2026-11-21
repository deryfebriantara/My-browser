//! Primitif kriptografi: Argon2id, XChaCha20-Poly1305, dan angka acak dari
//! sistem operasi.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

pub(crate) const KEY_LEN: usize = 32;
pub(crate) const NONCE_LEN: usize = 24;

pub(crate) type Key = Zeroizing<[u8; KEY_LEN]>;

/// Byte acak dari generator kriptografis sistem operasi.
pub(crate) fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("generator acak sistem tersedia");
    bytes
}

pub(crate) fn random_key() -> Key {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    getrandom::fill(key.as_mut()).expect("generator acak sistem tersedia");
    key
}

/// Kunci dari master password lewat Argon2id. `None` kalau parameter tidak sah.
pub(crate) fn derive(password: &[u8], salt: &[u8], m_kib: u32, t: u32, p: u32) -> Option<Key> {
    let params = Params::new(m_kib, t, p, Some(KEY_LEN)).ok()?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon.hash_password_into(password, salt, key.as_mut()).ok()?;
    Some(key)
}

/// Enkripsi terautentikasi dengan nonce acak baru. Hasil: (nonce, ciphertext).
pub(crate) fn seal(key: &Key, plaintext: &[u8], aad: &[u8]) -> ([u8; NONCE_LEN], Vec<u8>) {
    let nonce = random::<NONCE_LEN>();
    let cipher = XChaCha20Poly1305::new(key.as_ref().into());
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plaintext, aad })
        .expect("enkripsi XChaCha20-Poly1305 tidak gagal untuk ukuran wajar");
    (nonce, ciphertext)
}

/// Dekripsi; `None` kalau kunci salah atau data/associated data diubah.
pub(crate) fn open(key: &Key, nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if nonce.len() != NONCE_LEN {
        return None;
    }
    let cipher = XChaCha20Poly1305::new(key.as_ref().into());
    cipher.decrypt(XNonce::from_slice(nonce), Payload { msg: ciphertext, aad }).ok().map(Zeroizing::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_data_opens_only_with_same_key_and_aad() {
        let key = random_key();
        let (nonce, ct) = seal(&key, b"rahasia", b"kepala");
        assert_eq!(open(&key, &nonce, &ct, b"kepala").as_deref().map(|v| v.as_slice()), Some(&b"rahasia"[..]));
        assert!(open(&key, &nonce, &ct, b"kepala lain").is_none(), "associated data berbeda");
        assert!(open(&random_key(), &nonce, &ct, b"kepala").is_none(), "kunci berbeda");
        let mut tampered = ct.clone();
        tampered[0] ^= 1;
        assert!(open(&key, &nonce, &tampered, b"kepala").is_none(), "ciphertext diubah");
        assert!(open(&key, &nonce[..12], &ct, b"kepala").is_none(), "nonce salah panjang");
    }

    #[test]
    fn nonces_are_fresh() {
        let key = random_key();
        let (a, ca) = seal(&key, b"x", b"");
        let (b, cb) = seal(&key, b"x", b"");
        assert_ne!(a, b);
        assert_ne!(ca, cb);
    }

    #[test]
    fn argon2id_is_deterministic_per_salt_and_rejects_bad_params() {
        let a = derive(b"pw", b"salt-16-bytes!!!", 64, 1, 1).unwrap();
        let b = derive(b"pw", b"salt-16-bytes!!!", 64, 1, 1).unwrap();
        let c = derive(b"pw", b"salt-lain-16byte", 64, 1, 1).unwrap();
        assert_eq!(*a, *b);
        assert_ne!(*a, *c);
        assert!(derive(b"pw", b"salt-16-bytes!!!", 1, 1, 1).is_none(), "memory di bawah minimum Argon2");
        assert!(derive(b"pw", b"pendek", 64, 1, 1).is_none(), "salt terlalu pendek");
    }
}
