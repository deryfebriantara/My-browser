//! Generator password acak.

use crate::crypto::random;

pub const DEFAULT_LENGTH: usize = 20;
pub const MIN_LENGTH: usize = 8;
pub const MAX_LENGTH: usize = 128;

// Tanpa karakter yang mudah tertukar saat dibaca atau diketik ulang
// (0 O o 1 l I), supaya tetap bisa diketik di perangkat lain.
const LOWER: &[u8] = b"abcdefghijkmnpqrstuvwxyz";
const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const DIGITS: &[u8] = b"23456789";
const SYMBOLS: &[u8] = b"!@#$%^&*-_=+?";

/// Indeks acak seragam di `0..n` (rejection sampling, tanpa bias modulo).
fn uniform(n: usize) -> usize {
    assert!(n > 0 && n <= 256);
    let limit = 256 - 256 % n;
    loop {
        let [b] = random::<1>();
        if (b as usize) < limit {
            return b as usize % n;
        }
    }
}

/// Password acak sepanjang `length` (dibatasi 8..=128) yang memuat huruf
/// kecil, huruf besar, angka, dan simbol kalau `symbols`.
pub fn generate(length: usize, symbols: bool) -> String {
    let length = length.clamp(MIN_LENGTH, MAX_LENGTH);
    let mut classes: Vec<&[u8]> = vec![LOWER, UPPER, DIGITS];
    if symbols {
        classes.push(SYMBOLS);
    }
    let alphabet: Vec<u8> = classes.concat();
    loop {
        let password: Vec<u8> = (0..length).map(|_| alphabet[uniform(alphabet.len())]).collect();
        if classes.iter().all(|class| password.iter().any(|c| class.contains(c))) {
            return String::from_utf8(password).expect("alfabet ASCII");
        }
    }
}

/// Perkiraan kekuatan (bit entropi) password hasil `generate`.
pub fn entropy_bits(length: usize, symbols: bool) -> f64 {
    let size = LOWER.len() + UPPER.len() + DIGITS.len() + if symbols { SYMBOLS.len() } else { 0 };
    length.clamp(MIN_LENGTH, MAX_LENGTH) as f64 * (size as f64).log2()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_and_character_classes() {
        for _ in 0..200 {
            let p = generate(DEFAULT_LENGTH, true);
            assert_eq!(p.len(), DEFAULT_LENGTH);
            assert!(p.bytes().any(|c| LOWER.contains(&c)) && p.bytes().any(|c| UPPER.contains(&c)));
            assert!(p.bytes().any(|c| DIGITS.contains(&c)) && p.bytes().any(|c| SYMBOLS.contains(&c)));
            assert!(!p.contains(['0', 'O', 'o', '1', 'l', 'I']));
        }
        assert!(generate(16, false).bytes().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(generate(2, true).len(), MIN_LENGTH);
        assert_eq!(generate(10_000, true).len(), MAX_LENGTH);
    }

    #[test]
    fn passwords_differ_and_entropy_is_high() {
        assert_ne!(generate(20, true), generate(20, true));
        assert!(entropy_bits(DEFAULT_LENGTH, true) > 120.0);
    }

    #[test]
    fn sampling_is_roughly_uniform() {
        let mut counts = [0usize; 69];
        for _ in 0..69_000 {
            counts[uniform(69)] += 1;
        }
        assert!(counts.iter().all(|&c| (700..=1300).contains(&c)), "{counts:?}");
    }
}
