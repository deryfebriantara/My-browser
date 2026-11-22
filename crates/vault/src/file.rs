//! Berkas vault dan vault yang sedang terbuka.

use crate::crypto::{self, Key};
use crate::entries::{Entry, NewEntry, Offer, new_id};
use crate::hex;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use zeroize::Zeroizing;

const VERSION: u32 = 1;
const ALG: &str = "argon2id";
const SALT_LEN: usize = 16;

/// Parameter Argon2id. Disimpan di berkas, jadi bisa dinaikkan nanti tanpa
/// merusak vault lama.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory dalam KiB.
    pub m_kib: u32,
    /// Jumlah putaran.
    pub t: u32,
    /// Paralelisme.
    pub p: u32,
}

impl KdfParams {
    /// 256 MiB, 3 putaran: sekitar 0,3 detik di Mac Apple Silicon (diukur
    /// dengan test `default_kdf_timing`), jauh di atas rekomendasi minimum
    /// OWASP (19 MiB, 2 putaran). Besarnya memory yang membuat tebakan massal
    /// dengan GPU mahal. Memory hanya dipakai sesaat selama membuka kunci.
    pub const DEFAULT: KdfParams = KdfParams { m_kib: 256 * 1024, t: 3, p: 1 };

    /// Batas atas saat membaca berkas, supaya berkas yang diutak-atik tidak
    /// bisa membuat aplikasi mengalokasikan memory sangat besar.
    fn within_limits(&self) -> bool {
        (8..=2 * 1024 * 1024).contains(&self.m_kib) && (1..=20).contains(&self.t) && (1..=16).contains(&self.p)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kdf {
    pub alg: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sealed {
    pub nonce: String,
    pub ct: String,
}

/// Isi berkas `vault.json`. Tidak ada teks terang di dalamnya selain
/// parameter KDF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VaultFile {
    pub version: u32,
    pub kdf: Kdf,
    /// Kunci vault yang dibungkus kunci dari master password.
    pub key: Sealed,
    /// Daftar entri yang dienkripsi kunci vault.
    pub data: Sealed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultError {
    /// Master password salah (atau bungkusan kunci diubah).
    WrongPassword,
    /// Berkas rusak atau diubah.
    Corrupt(String),
    /// Versi atau parameter yang tidak didukung.
    Unsupported(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultError::WrongPassword => write!(f, "Incorrect master password."),
            VaultError::Corrupt(why) => write!(f, "The vault file is damaged: {why}"),
            VaultError::Unsupported(why) => write!(f, "The vault file isn't supported: {why}"),
        }
    }
}

impl std::error::Error for VaultError {}

/// Associated data untuk bungkusan kunci dan isi: versi dan parameter KDF.
fn header_aad(kdf: &Kdf, part: &str) -> Vec<u8> {
    format!("my-browser-vault|v{VERSION}|{}|m{}|t{}|p{}|{}|{part}", kdf.alg, kdf.m_kib, kdf.t, kdf.p, kdf.salt)
        .into_bytes()
}

fn sealed(nonce: [u8; crypto::NONCE_LEN], ct: Vec<u8>) -> Sealed {
    Sealed { nonce: hex::encode(&nonce), ct: hex::encode(&ct) }
}

fn decode(field: &str, value: &str) -> Result<Vec<u8>, VaultError> {
    hex::decode(value).ok_or_else(|| VaultError::Corrupt(format!("{field} is not hexadecimal")))
}

/// Vault yang sedang terbuka. Kunci dan entri dihapus dari memory saat dilepas.
pub struct Vault {
    key: Key,
    kdf: Kdf,
    wrapped: Sealed,
    entries: Vec<Entry>,
}

impl Vault {
    /// Vault baru yang kosong, dikunci dengan `master`.
    pub fn create(master: &str, params: KdfParams) -> Result<Vault, VaultError> {
        if master.is_empty() {
            return Err(VaultError::Unsupported("empty master password".into()));
        }
        let key = crypto::random_key();
        let (kdf, wrapped) = wrap(&key, master, params)?;
        Ok(Vault { key, kdf, wrapped, entries: Vec::new() })
    }

    /// Membuka berkas dengan master password.
    pub fn unlock(file: &VaultFile, master: &str) -> Result<Vault, VaultError> {
        if file.version != VERSION {
            return Err(VaultError::Unsupported(format!("version {}", file.version)));
        }
        if file.kdf.alg != ALG {
            return Err(VaultError::Unsupported(format!("algorithm {}", file.kdf.alg)));
        }
        let params = KdfParams { m_kib: file.kdf.m_kib, t: file.kdf.t, p: file.kdf.p };
        if !params.within_limits() {
            return Err(VaultError::Unsupported("Argon2id parameters out of range".into()));
        }
        let salt = decode("salt", &file.kdf.salt)?;
        if !(SALT_LEN..=64).contains(&salt.len()) {
            return Err(VaultError::Corrupt("invalid salt length".into()));
        }
        let kek = crypto::derive(master.as_bytes(), &salt, params.m_kib, params.t, params.p)
            .ok_or_else(|| VaultError::Unsupported("invalid Argon2id parameters".into()))?;
        let raw = crypto::open(
            &kek,
            &decode("key.nonce", &file.key.nonce)?,
            &decode("key.ct", &file.key.ct)?,
            &header_aad(&file.kdf, "key"),
        )
        .ok_or(VaultError::WrongPassword)?;
        let key: Key =
            Zeroizing::new(raw.as_slice().try_into().map_err(|_| VaultError::Corrupt("invalid key length".into()))?);
        let json = crypto::open(
            &key,
            &decode("data.nonce", &file.data.nonce)?,
            &decode("data.ct", &file.data.ct)?,
            &header_aad(&file.kdf, "data"),
        )
        .ok_or_else(|| VaultError::Corrupt("contents failed authentication".into()))?;
        let entries: Vec<Entry> =
            serde_json::from_slice(&json).map_err(|e| VaultError::Corrupt(format!("contents can't be read: {e}")))?;
        Ok(Vault { key, kdf: file.kdf.clone(), wrapped: file.key.clone(), entries })
    }

    /// Berkas untuk disimpan. Isi dienkripsi ulang dengan nonce baru.
    pub fn to_file(&self) -> VaultFile {
        let json = Zeroizing::new(serde_json::to_vec(&self.entries).expect("entri selalu bisa diserialisasi"));
        let (nonce, ct) = crypto::seal(&self.key, &json, &header_aad(&self.kdf, "data"));
        VaultFile { version: VERSION, kdf: self.kdf.clone(), key: self.wrapped.clone(), data: sealed(nonce, ct) }
    }

    /// Mengganti master password. Isi tidak perlu dienkripsi ulang.
    pub fn change_master(&mut self, master: &str, params: KdfParams) -> Result<(), VaultError> {
        if master.is_empty() {
            return Err(VaultError::Unsupported("empty master password".into()));
        }
        let (kdf, wrapped) = wrap(&self.key, master, params)?;
        self.kdf = kdf;
        self.wrapped = wrapped;
        Ok(())
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Entri untuk origin persis ini, yang terakhir dipakai lebih dulu.
    pub fn for_origin(&self, origin: &str) -> Vec<&Entry> {
        let mut list: Vec<&Entry> = self.entries.iter().filter(|e| e.origin == origin).collect();
        list.sort_by_key(|e| std::cmp::Reverse(e.last_used.unwrap_or(e.updated)));
        list
    }

    /// Apa yang perlu ditawarkan setelah login dengan password ini.
    pub fn offer(&self, origin: &str, username: &str, password: &str) -> Offer {
        match self.entries.iter().find(|e| e.origin == origin && e.username == username) {
            Some(e) if e.password == password => Offer::Nothing,
            Some(e) => Offer::Update(e.id.clone()),
            None => Offer::Save,
        }
    }

    /// Menambah entri; mengembalikan id-nya.
    pub fn add(&mut self, new: NewEntry, now: u64) -> String {
        let id = new_id();
        self.entries.push(Entry {
            id: id.clone(),
            origin: new.origin.clone(),
            username: new.username.clone(),
            password: new.password.clone(),
            note: new.note.clone(),
            created: now,
            updated: now,
            last_used: None,
        });
        id
    }

    /// Mengubah entri. Field `None` tidak diubah.
    pub fn update(
        &mut self,
        id: &str,
        username: Option<&str>,
        password: Option<&str>,
        note: Option<&str>,
        now: u64,
    ) -> bool {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id) else { return false };
        if let Some(v) = username {
            e.username = v.to_string();
        }
        if let Some(v) = password {
            e.password = v.to_string();
        }
        if let Some(v) = note {
            e.note = v.to_string();
        }
        e.updated = now;
        true
    }

    pub fn delete(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    pub fn mark_used(&mut self, id: &str, now: u64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.last_used = Some(now);
        }
    }

    /// Memasukkan entri impor. Akun yang sama (origin dan username) diganti
    /// passwordnya; yang persis sama dilewati. Hasil: (ditambah, diperbarui).
    pub fn import(&mut self, list: Vec<NewEntry>, now: u64) -> (usize, usize) {
        let (mut added, mut updated) = (0, 0);
        for new in list {
            match self.offer(&new.origin, &new.username, &new.password) {
                Offer::Nothing => {}
                Offer::Update(id) => {
                    self.update(
                        &id,
                        None,
                        Some(&new.password),
                        (!new.note.is_empty()).then_some(new.note.as_str()),
                        now,
                    );
                    updated += 1;
                }
                Offer::Save => {
                    self.add(new, now);
                    added += 1;
                }
            }
        }
        (added, updated)
    }
}

fn wrap(key: &Key, master: &str, params: KdfParams) -> Result<(Kdf, Sealed), VaultError> {
    if !params.within_limits() {
        return Err(VaultError::Unsupported("Argon2id parameters out of range".into()));
    }
    let salt = crypto::random::<SALT_LEN>();
    let kdf = Kdf { alg: ALG.into(), m_kib: params.m_kib, t: params.t, p: params.p, salt: hex::encode(&salt) };
    let kek = crypto::derive(master.as_bytes(), &salt, params.m_kib, params.t, params.p)
        .ok_or_else(|| VaultError::Unsupported("invalid Argon2id parameters".into()))?;
    let (nonce, ct) = crypto::seal(&kek, key.as_ref(), &header_aad(&kdf, "key"));
    Ok((kdf, sealed(nonce, ct)))
}

/// Membaca berkas vault. `Ok(None)` kalau belum ada.
pub fn load(path: &Path) -> Result<Option<VaultFile>, VaultError> {
    match std::fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(VaultError::Corrupt(format!("can't be read: {e}"))),
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| VaultError::Corrupt(format!("not a vault JSON file: {e}"))),
    }
}

/// Menyimpan berkas secara atomik dengan izin 0600 (hanya pengguna ini).
pub fn save(path: &Path, file: &VaultFile) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut out = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
    out.write_all(&serde_json::to_vec_pretty(file).map_err(std::io::Error::other)?)?;
    out.sync_all()?;
    drop(out);
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parameter kecil supaya test cepat; skema dan kodenya sama.
    const FAST: KdfParams = KdfParams { m_kib: 64, t: 1, p: 1 };

    fn new(origin: &str, user: &str, pass: &str) -> NewEntry {
        NewEntry { origin: origin.into(), username: user.into(), password: pass.into(), note: String::new() }
    }

    fn sample() -> Vault {
        let mut v = Vault::create("master-kuat", FAST).unwrap();
        v.add(new("https://github.com", "dery", "pw-github"), 10);
        v.add(new("https://mail.example", "dery@mail", "pw-mail"), 11);
        v
    }

    #[test]
    fn file_opens_with_right_password_only() {
        let file = sample().to_file();
        let v = Vault::unlock(&file, "master-kuat").unwrap();
        assert_eq!(v.entries().len(), 2);
        assert_eq!(v.for_origin("https://github.com")[0].password, "pw-github");
        assert_eq!(Vault::unlock(&file, "master-salah").err(), Some(VaultError::WrongPassword));
        assert_eq!(Vault::unlock(&file, "").err(), Some(VaultError::WrongPassword));
    }

    #[test]
    fn file_contains_no_plaintext() {
        let text = serde_json::to_string(&sample().to_file()).unwrap();
        for secret in ["pw-github", "dery", "github.com", "mail.example", "master-kuat"] {
            assert!(!text.contains(secret), "{secret} terlihat di berkas");
        }
    }

    #[test]
    fn tampering_is_detected() {
        let file = sample().to_file();
        let mut data = file.clone();
        let mut ct = hex::decode(&data.data.ct).unwrap();
        ct[3] ^= 0x40;
        data.data.ct = hex::encode(&ct);
        assert!(matches!(Vault::unlock(&data, "master-kuat"), Err(VaultError::Corrupt(_))), "isi diubah");

        let mut header = file.clone();
        header.kdf.t = 2;
        assert_eq!(Vault::unlock(&header, "master-kuat").err(), Some(VaultError::WrongPassword), "parameter diubah");

        let mut swapped = file.clone();
        swapped.data = sample().to_file().data;
        assert!(Vault::unlock(&swapped, "master-kuat").is_err(), "isi dari vault lain");
    }

    #[test]
    fn hostile_parameters_are_rejected_before_any_work() {
        let mut file = sample().to_file();
        file.kdf.m_kib = u32::MAX;
        assert!(matches!(Vault::unlock(&file, "master-kuat"), Err(VaultError::Unsupported(_))));
        let mut file = sample().to_file();
        file.version = 2;
        assert!(matches!(Vault::unlock(&file, "master-kuat"), Err(VaultError::Unsupported(_))));
        let mut file = sample().to_file();
        file.kdf.salt = "00".into();
        assert!(matches!(Vault::unlock(&file, "master-kuat"), Err(VaultError::Corrupt(_))));
        assert!(Vault::create("", FAST).is_err());
    }

    #[test]
    fn each_save_uses_a_fresh_nonce() {
        let v = sample();
        let (a, b) = (v.to_file(), v.to_file());
        assert_ne!(a.data.nonce, b.data.nonce);
        assert_ne!(a.data.ct, b.data.ct);
        assert_eq!(a.key, b.key, "bungkusan kunci tetap sama sampai master password diganti");
    }

    #[test]
    fn changing_master_password_keeps_entries() {
        let mut v = sample();
        v.change_master("master-baru", FAST).unwrap();
        let file = v.to_file();
        assert_eq!(Vault::unlock(&file, "master-kuat").err(), Some(VaultError::WrongPassword));
        assert_eq!(Vault::unlock(&file, "master-baru").unwrap().entries().len(), 2);
    }

    #[test]
    fn origin_matching_is_exact() {
        let v = sample();
        for other in [
            "https://github.com.evil.example",
            "http://github.com",
            "https://evil-github.com",
            "https://www.github.com",
        ] {
            assert!(v.for_origin(other).is_empty(), "{other}");
        }
    }

    #[test]
    fn offers_save_update_or_nothing() {
        let v = sample();
        assert_eq!(v.offer("https://github.com", "dery", "pw-github"), Offer::Nothing);
        assert!(matches!(v.offer("https://github.com", "dery", "baru"), Offer::Update(_)));
        assert_eq!(v.offer("https://github.com", "lain", "x"), Offer::Save);
    }

    #[test]
    fn edit_delete_and_recent_first() {
        let mut v = sample();
        let id = v.add(new("https://github.com", "kedua", "pw2"), 20);
        assert_eq!(v.for_origin("https://github.com")[0].username, "kedua");
        let first = v.for_origin("https://github.com")[1].id.clone();
        v.mark_used(&first, 30);
        assert_eq!(v.for_origin("https://github.com")[0].username, "dery");
        assert!(v.update(&id, None, Some("pw3"), Some("catatan"), 40));
        assert_eq!(v.get(&id).unwrap().password, "pw3");
        assert_eq!(v.get(&id).unwrap().note, "catatan");
        assert!(v.delete(&id));
        assert!(!v.delete(&id));
        assert!(!v.update("tidak-ada", None, None, None, 0));
    }

    #[test]
    fn import_adds_updates_and_skips() {
        let mut v = sample();
        let (added, updated) = v.import(
            vec![
                new("https://github.com", "dery", "pw-github"),
                new("https://github.com", "dery", "ganti"),
                new("https://baru.example", "x", "y"),
            ],
            50,
        );
        assert_eq!((added, updated), (1, 1));
        assert_eq!(v.for_origin("https://github.com")[0].password, "ganti");
    }

    #[test]
    fn save_and_load_with_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("vault.json");
        assert_eq!(load(&path).unwrap(), None);
        let file = sample().to_file();
        save(&path, &file).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(load(&path).unwrap(), Some(file.clone()));
        save(&path, &file).unwrap();
        std::fs::write(&path, "{bukan").unwrap();
        assert!(matches!(load(&path), Err(VaultError::Corrupt(_))));
    }

    /// Waktu membuka kunci dengan parameter bawaan:
    /// `cargo test --release -p my-browser-vault -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn default_kdf_timing() {
        let start = std::time::Instant::now();
        let v = Vault::create("master-kuat", KdfParams::DEFAULT).unwrap();
        println!("buat: {:?}", start.elapsed());
        let file = v.to_file();
        let start = std::time::Instant::now();
        Vault::unlock(&file, "master-kuat").unwrap();
        println!("buka: {:?}", start.elapsed());
    }
}
