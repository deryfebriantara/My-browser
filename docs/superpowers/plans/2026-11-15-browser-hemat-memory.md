# Browser Hemat Memory: Rencana Implementasi

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Browser macOS untuk YouTube dan browsing santai, ditulis dengan Rust di atas WebKit, dengan pemakaian RAM jauh di bawah Chrome.

**Architecture:** Cargo workspace. `crates/core` berisi semua logika (tab, kebijakan tidur, address bar, sesi, controller `Browser<E: Engine>`) tanpa dependensi macOS dan diuji dengan mesin palsu. `crates/engine-webkit` mengimplementasikan trait `Engine`/`Page` di atas `WKWebView`. `crates/app` adalah UI AppKit tipis yang meneruskan aksi ke `Browser` dan menggambar ulang dari daftar `UiChange`.

**Tech Stack:** Rust stable (edition 2024), `objc2` 0.6, `objc2-foundation`/`objc2-app-kit`/`objc2-web-kit` 0.3, `block2` 0.6, `dispatch2` 0.3, `url` 2, `serde`/`serde_json` 1, `adblock` 0.13 (fitur `content-blocking`), `libc` 0.2, `tempfile` 3 (dev).

**Spec:** `docs/superpowers/specs/2026-11-15-browser-hemat-memory-design.md`

## Global Constraints

- Hanya macOS arm64. `MACOSX_DEPLOYMENT_TARGET = "14.0"` dan `LSMinimumSystemVersion` 14.0.
- Mesin render: `WKWebView` sistem. Tidak membundel mesin sendiri.
- `crates/core` tidak boleh bergantung pada crate macOS. `cargo tree -p my-browser-core | grep -E 'objc2|block2|dispatch2'` harus kosong.
- `core` dan logika `app` bicara ke mesin hanya lewat trait `Engine`/`Page`. Satu-satunya yang lewat dari mesin ke UI adalah `NSView` (`Page::View`).
- AppKit dan WebKit hanya dipanggil dari main thread (`MainThreadMarker`).
- Event mesin tidak pernah dikirim sinkron dari dalam pemanggilan method `Engine`/`Page`. Engine selalu menjadwalkannya ke main queue.
- Kemasan: `dist/my-browser.app`, ad-hoc codesign (`codesign --force --deep -s -`), tanpa App Store dan tanpa notarisasi. Aplikasi selalu dijalankan sebagai bundle lewat `scripts/run.sh`, karena `Info.plist` memuat izin ATS dan kamera.
- Nilai bawaan: tab ditidurkan setelah 300 detik tidak dilihat, paling banyak 3 tab bangun, mesin pencari `https://www.google.com/search?q={}`, tick setiap 30 detik.
- Data pengguna: `~/Library/Application Support/my-browser/settings.json` dan `session.json`.
- Teks UI berbahasa Indonesia (keputusan penulis rencana, mengikuti bahasa pengguna).
- Setiap task diakhiri commit. Pesan commit diakhiri baris `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Langkah bertanda **GATE** atau **CHECKPOINT** berhenti dan melapor ke pengguna. Jangan lanjut ke task berikutnya sebelum pengguna menjawab.

## Review Focus

1. Proses WebContent crash (umum pada tab YouTube yang berat). Tab aktif dimuat ulang sekali; crash kedua dalam 10 detik tidak dimuat ulang lagi dan memunculkan pemberitahuan. Tab latar yang crash ditidurkan. Test: Task 6.
2. Berkas `session.json`/`settings.json` rusak, terpotong, atau versinya lain. Aplikasi tetap terbuka dengan nilai bawaan dan satu tab kosong, tidak panic. Test: Task 5.
3. Input address bar di luar kasus umum: `localhost:3000`, alamat IPv4, skema `javascript:`, spasi di tepi, input kosong, kata kunci non-ASCII. Test: Task 3.
4. Event terlambat dari halaman yang tabnya sudah ditutup atau ditidurkan (balasan `MediaState`, `Number`, `TitleChanged`). Event diabaikan, tidak panic, dan tidak membangunkan tab. Test: Task 6.
5. Nama berkas download dari server berisi path (`../../etc/passwd`) atau kosong. Berkas tetap tersimpan di `~/Downloads` dengan nama aman. Test: Task 11.

---

### Task 1: Toolchain, workspace, prototipe minimal, dan GATE login Google

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, `.gitignore`
- Create: `crates/core/Cargo.toml`, `crates/core/src/lib.rs`
- Create: `crates/engine-webkit/Cargo.toml`, `crates/engine-webkit/src/lib.rs`, `crates/engine-webkit/src/user_agent.rs`
- Create: `crates/app/Cargo.toml`, `crates/app/src/main.rs`
- Create: `resources/Info.plist`, `scripts/bundle.sh`, `scripts/run.sh`
- Create: `docs/testing/daftar-cek-manual.md`

**Interfaces:**
- Produces: `engine_webkit::user_agent::safari_application_name(safari_version: &str) -> String`, `engine_webkit::user_agent::installed_safari_version() -> Option<String>`, `engine_webkit::user_agent::FALLBACK_SAFARI_VERSION: &str = "18.6"`.
- Produces: nama package `my-browser-core` (lib `browser_core`), `engine-webkit` (lib `engine_webkit`), `my-browser` (bin `my-browser`).

- [ ] **Step 1: Pasang Rust**

Run: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable && source "$HOME/.cargo/env" && cargo --version`
Expected: `cargo 1.8x` atau lebih baru (edition 2024 butuh 1.85+).

- [ ] **Step 2: Buat workspace**

`Cargo.toml`: `[workspace] members = ["crates/*"]`, `resolver = "3"`, `[workspace.package] edition = "2024"`, dan `[workspace.dependencies]` berisi versi di Tech Stack. `rust-toolchain.toml`: `channel = "stable"`, `targets = ["aarch64-apple-darwin"]`. `.cargo/config.toml`: `[env] MACOSX_DEPLOYMENT_TARGET = "14.0"`. `.gitignore`: `/target`, `/dist`, `/resources/filters`, `/resources/blocklist`.

- [ ] **Step 3: Tulis test user agent yang gagal di `crates/engine-webkit/src/user_agent.rs`**

```rust
#[test]
fn app_name_matches_safari_format() {
    assert_eq!(safari_application_name("18.6"), "Version/18.6 Safari/605.1.15");
}

#[test]
fn installed_safari_version_is_numeric() {
    let v = installed_safari_version().expect("Safari terpasang di /Applications");
    assert!(v.split('.').next().unwrap().parse::<u32>().is_ok());
}
```

- [ ] **Step 4: Jalankan test, pastikan gagal**

Run: `cargo test -p engine-webkit user_agent`
Expected: FAIL, fungsi belum ada.

- [ ] **Step 5: Implementasikan kedua fungsi**

`installed_safari_version` membaca `CFBundleShortVersionString` dari `/Applications/Safari.app/Contents/Info.plist` (lewat `NSDictionary::dictionaryWithContentsOfFile`). Di mesin pengguna nilainya `18.6`.

- [ ] **Step 6: Jalankan test, pastikan lulus**

Run: `cargo test -p engine-webkit user_agent`
Expected: 2 passed.

- [ ] **Step 7: Prototipe `crates/app/src/main.rs`**

Satu `NSWindow` 1280x800 berjudul `my-browser` berisi satu `WKWebView` yang mengisi window, memuat `https://www.youtube.com`. Konfigurasi: `preferences.setElementFullscreenEnabled(true)`, `setApplicationNameForUserAgent(safari_application_name(installed atau FALLBACK))`, `setWebsiteDataStore(WKWebsiteDataStore::defaultDataStore())`. Menu minimal: Keluar (⌘Q) dan menu Edit standar (Urungkan, Ulangi, Potong, Salin, Tempel, Pilih Semua) supaya ⌘V berfungsi di form login. Menutup window mengakhiri aplikasi.

- [ ] **Step 8: Bundle dan script jalan**

`resources/Info.plist` memuat: `CFBundleName`/`CFBundleExecutable` = `my-browser`, `CFBundleIdentifier` = `local.my-browser`, `CFBundlePackageType` = `APPL`, `LSMinimumSystemVersion` = `14.0`, `NSHighResolutionCapable` = true, `NSAppTransportSecurity` → `NSAllowsArbitraryLoadsInWebContent` = true, `NSCameraUsageDescription` = "Situs web meminta akses kamera.", `NSMicrophoneUsageDescription` = "Situs web meminta akses mikrofon.". Tanpa dua kunci terakhir, macOS mematikan aplikasi saat situs meminta kamera.

`scripts/bundle.sh`: `cargo build --release -p my-browser`, menyusun `dist/my-browser.app/Contents/{MacOS,Resources}`, menyalin binary dan `Info.plist`, lalu `codesign --force --deep -s - dist/my-browser.app`. `scripts/run.sh`: menjalankan `bundle.sh` lalu `open dist/my-browser.app`.

- [ ] **Step 9: Verifikasi prototipe jalan**

Run: `scripts/run.sh`
Expected: window terbuka dan halaman YouTube tampil.

- [ ] **Step 10: GATE login Google (dilakukan pengguna)**

Minta pengguna: login lewat tombol "Sign in" YouTube (password dan kode HP), putar satu video, coba tombol layar penuh, keluar dengan ⌘Q, lalu buka lagi dengan `open dist/my-browser.app`. Catat hasilnya di `docs/testing/daftar-cek-manual.md` (bagian "Task 1").
Lulus: login diterima, video berputar, layar penuh berfungsi, dan setelah dibuka lagi masih login.
Kalau login ditolak: **berhenti**, laporkan pesan yang muncul ke pengguna, dan jangan lanjut ke Task 2.

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m "feat: workspace dan prototipe WKWebView (gate login Google)"
```

---

### Task 2: Alat ukur memory (`memtool`) dan CHECKPOINT pengukuran awal

**Files:**
- Create: `crates/memtool/Cargo.toml`, `crates/memtool/src/lib.rs`, `crates/memtool/src/sys.rs`, `crates/memtool/src/main.rs`
- Create: `scripts/ukur-memory.sh`, `scripts/skenario-urls.txt`, `docs/testing/hasil-ukur.md`

**Interfaces:**
- Produces: `memtool::ProcInfo { pid: i32, name: String, responsible_pid: i32, footprint: u64 }`, `memtool::AppUsage { app: String, processes: Vec<ProcInfo>, footprint: u64 }`, `memtool::group(procs: &[ProcInfo], apps: &[&str]) -> Vec<AppUsage>`.
- Produces: perintah `scripts/ukur-memory.sh [--detail] <nama-app>...`, mencetak satu baris per app (`<app>  <n> proses  <MB> MB`) dan, dengan `--detail`, satu baris per proses.

- [ ] **Step 1: Tulis test yang gagal di `crates/memtool/src/lib.rs`**

```rust
fn p(pid: i32, name: &str, resp: i32, mb: u64) -> ProcInfo {
    ProcInfo { pid, name: name.into(), responsible_pid: resp, footprint: mb * 1024 * 1024 }
}

#[test]
fn groups_helpers_under_responsible_app() {
    let procs = vec![
        p(10, "Safari", 10, 200),
        p(11, "com.apple.WebKit.WebContent", 10, 300),
        p(12, "com.apple.WebKit.Networking", 10, 50),
        p(20, "Mail", 20, 100),
        p(21, "com.apple.WebKit.WebContent", 20, 80),
    ];
    let out = group(&procs, &["Safari"]);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].processes.len(), 3);
    assert_eq!(out[0].footprint, 550 * 1024 * 1024);
}

#[test]
fn missing_app_reports_zero() {
    let out = group(&[], &["Safari"]);
    assert_eq!(out[0].processes.len(), 0);
    assert_eq!(out[0].footprint, 0);
}
```

- [ ] **Step 2: Jalankan test, pastikan gagal**

Run: `cargo test -p memtool`
Expected: FAIL, `group` belum ada.

- [ ] **Step 3: Implementasikan `group`**

Proses utama sebuah app adalah proses dengan `name == app` dan `responsible_pid == pid`. Anggotanya adalah semua proses yang `responsible_pid`-nya sama dengan pid proses utama.

- [ ] **Step 4: Jalankan test, pastikan lulus**

Run: `cargo test -p memtool`
Expected: 2 passed.

- [ ] **Step 5: Implementasikan `sys.rs` dan `main.rs`**

`sys::list_processes() -> Vec<ProcInfo>` memakai `proc_listallpids`, `proc_name`, dan `proc_pid_rusage(RUSAGE_INFO_V4).ri_phys_footprint` (sama dengan kolom Memory di Activity Monitor). Responsible pid diambil dari `responsibility_get_pid_responsible_for_pid`, dideklarasikan sendiri sebagai `extern "C"` karena tidak ada di header publik. `scripts/ukur-memory.sh` menjalankan `cargo run -q --release -p memtool -- "$@"`. `scripts/skenario-urls.txt` berisi lima URL: `https://www.kompas.com`, `https://www.detik.com`, `https://id.wikipedia.org/wiki/Indonesia`, `https://github.com/servo/servo`, `https://www.reddit.com/r/rust/`.

- [ ] **Step 6: Verifikasi atribusi proses WebKit**

Run: `open -a Safari https://id.wikipedia.org && sleep 5 && scripts/ukur-memory.sh --detail Safari`
Expected: baris Safari memuat paling sedikit satu `com.apple.WebKit.WebContent`. Kalau tidak ada, atribusi gagal: catat di `hasil-ukur.md`, ganti cara ukur menjadi "jumlahkan semua proses `com.apple.WebKit.*` dengan hanya satu aplikasi WebKit terbuka", dan laporkan ke pengguna.

- [ ] **Step 7: Ukur prototipe dan Safari, satu tab YouTube**

Dengan bantuan pengguna: putar video YouTube yang sama di prototipe dan di Safari (bergantian, hanya satu yang hidup), set kualitas 1080p, tunggu 2 menit, jalankan `scripts/ukur-memory.sh my-browser` atau `scripts/ukur-memory.sh Safari`. Tulis tabel di `docs/testing/hasil-ukur.md` (tanggal, URL video, footprint total, jumlah proses, footprint proses aplikasi saja).

- [ ] **Step 8: CHECKPOINT**

Laporkan angka ke pengguna. Kalau prototipe lebih berat dari Safari, sebutkan selisihnya secara terbuka. Lanjut ke Task 3 setelah pengguna menjawab.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat: memtool untuk mengukur physical footprint per aplikasi"
```

---

### Task 3: Address bar (`omnibox`)

**Files:**
- Create: `crates/core/src/omnibox.rs`
- Modify: `crates/core/src/lib.rs` (tambah `pub mod omnibox;`), `crates/core/Cargo.toml` (dependensi `url`)

**Interfaces:**
- Produces: `browser_core::omnibox::parse_input(input: &str, search_template: &str) -> Option<url::Url>`.

- [ ] **Step 1: Tulis test yang gagal**

```rust
const G: &str = "https://www.google.com/search?q={}";
fn parse(s: &str) -> Option<String> { parse_input(s, G).map(|u| u.to_string()) }

#[test] fn domain_gets_https() { assert_eq!(parse("youtube.com").unwrap(), "https://youtube.com/"); }
#[test] fn explicit_http_kept() { assert_eq!(parse("http://example.com").unwrap(), "http://example.com/"); }
#[test] fn domain_with_path_kept() { assert_eq!(parse("github.com/servo/servo").unwrap(), "https://github.com/servo/servo"); }
#[test] fn words_become_search() { assert_eq!(parse("resep nasi goreng").unwrap(), "https://www.google.com/search?q=resep+nasi+goreng"); }
#[test] fn trims_whitespace() { assert_eq!(parse("  youtube.com  ").unwrap(), "https://youtube.com/"); }
#[test] fn empty_is_none() { assert!(parse("   ").is_none()); }
#[test] fn localhost_uses_http() { assert_eq!(parse("localhost:3000").unwrap(), "http://localhost:3000/"); }
#[test] fn ipv4_uses_http() { assert_eq!(parse("192.168.1.1").unwrap(), "http://192.168.1.1/"); }
#[test] fn numeric_tld_is_search() { assert_eq!(parse("v1.2").unwrap(), "https://www.google.com/search?q=v1.2"); }
#[test] fn javascript_scheme_is_search() { assert!(parse("javascript:alert(1)").unwrap().starts_with("https://www.google.com/search?q=javascript")); }
#[test] fn non_ascii_query_encoded() { assert_eq!(parse("café").unwrap(), "https://www.google.com/search?q=caf%C3%A9"); }
#[test] fn about_blank_allowed() { assert_eq!(parse("about:blank").unwrap(), "about:blank"); }
```

- [ ] **Step 2: Jalankan test, pastikan gagal**

Run: `cargo test -p my-browser-core omnibox`
Expected: FAIL, `parse_input` belum ada.

- [ ] **Step 3: Implementasikan `parse_input`**

Aturan berurutan, berhenti di aturan pertama yang cocok:

1. Trim. Kosong → `None`.
2. Diawali `http://` atau `https://` (tanpa peduli huruf besar/kecil) → `Url::parse`. Kalau gagal → pencarian.
3. Diawali `about:` → `Url::parse`.
4. Mengandung spasi → pencarian.
5. Host = teks sebelum `/` pertama, tanpa akhiran `:<angka>`. Host `localhost` atau alamat IPv4 → `http://` + teks.
6. Host mengandung titik dan label terakhirnya minimal 2 huruf ASCII → `https://` + teks.
7. Selain itu → pencarian.

Pencarian mengganti `{}` di template dengan `url::form_urlencoded::byte_serialize(teks)`.

- [ ] **Step 4: Jalankan test, pastikan lulus**

Run: `cargo test -p my-browser-core omnibox`
Expected: 12 passed.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): penguraian input address bar"
```

---

### Task 4: Daftar tab dan kebijakan tidur

**Files:**
- Create: `crates/core/src/tabs.rs`, `crates/core/src/sleep.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces di `browser_core::tabs`:
  - `TabId(pub u64)` (`Copy, Eq, Hash, Debug`)
  - `Tab { id, url: Option<Url>, title: String, scroll_y: f64, video_time: Option<f64>, awake: bool, last_viewed: Instant, media_active: bool, sleep_pending: bool, restore_scroll: bool, loading: bool, can_back: bool, can_forward: bool, last_crash: Option<Instant> }`
  - `TabList` dengan `new(now) -> TabList` (satu tab kosong aktif), `open(url: Option<Url>, now, activate: bool) -> TabId` (disisipkan tepat setelah tab aktif), `close(id, now) -> Option<Tab>`, `reopen_closed(now) -> Option<TabId>`, `activate(id, now)`, `activate_index(index: usize, now)`, `activate_next(now)`, `activate_previous(now)`, `active() -> TabId`, `get(id) -> Option<&Tab>`, `get_mut(id) -> Option<&mut Tab>`, `iter() -> impl Iterator<Item = &Tab>`, `snapshots() -> Vec<TabSnapshot>`.
- Produces di `browser_core::sleep`:
  - `TabSnapshot { id: TabId, awake: bool, active: bool, last_viewed: Instant, media_active: bool }`
  - `SleepPolicy { idle_after: Duration, max_awake: usize }`, `Default` = 300 detik dan 3
  - `Pressure { Normal, Warning, Critical }`
  - `tabs_to_sleep(tabs: &[TabSnapshot], now: Instant, policy: &SleepPolicy, pressure: Pressure) -> Vec<TabId>`

- [ ] **Step 1: Tulis test kebijakan tidur yang gagal di `sleep.rs`**

```rust
fn snap(id: u64, awake: bool, active: bool, idle_s: u64, media: bool, now: Instant) -> TabSnapshot {
    TabSnapshot { id: TabId(id), awake, active, last_viewed: now - Duration::from_secs(idle_s), media_active: media }
}
fn run(tabs: &[TabSnapshot], now: Instant, p: Pressure) -> Vec<TabId> {
    tabs_to_sleep(tabs, now, &SleepPolicy::default(), p)
}

#[test] fn default_policy_is_300s_and_3_tabs() {
    let p = SleepPolicy::default();
    assert_eq!((p.idle_after, p.max_awake), (Duration::from_secs(300), 3));
}
#[test] fn sleeps_background_tab_after_idle_timeout() {
    let n = Instant::now() + Duration::from_secs(10_000);
    assert_eq!(run(&[snap(1, true, true, 0, false, n), snap(2, true, false, 301, false, n)], n, Pressure::Normal), vec![TabId(2)]);
}
#[test] fn keeps_tab_idle_less_than_timeout() {
    let n = Instant::now() + Duration::from_secs(10_000);
    assert!(run(&[snap(1, true, true, 0, false, n), snap(2, true, false, 299, false, n)], n, Pressure::Normal).is_empty());
}
#[test] fn never_sleeps_active_or_media_tab() {
    let n = Instant::now() + Duration::from_secs(10_000);
    assert!(run(&[snap(1, true, true, 9_000, false, n), snap(2, true, false, 9_000, true, n)], n, Pressure::Critical).is_empty());
}
#[test] fn caps_awake_tabs_sleeping_least_recently_viewed() {
    let n = Instant::now() + Duration::from_secs(10_000);
    let t = [snap(1, true, true, 0, false, n), snap(2, true, false, 10, false, n), snap(3, true, false, 20, false, n), snap(4, true, false, 30, false, n)];
    assert_eq!(run(&t, n, Pressure::Normal), vec![TabId(4)]);
}
#[test] fn media_tabs_count_toward_cap_but_are_skipped() {
    let n = Instant::now() + Duration::from_secs(10_000);
    let t = [snap(1, true, true, 0, false, n), snap(2, true, false, 100, true, n), snap(3, true, false, 50, false, n), snap(4, true, false, 10, false, n)];
    assert_eq!(run(&t, n, Pressure::Normal), vec![TabId(3)]);
}
#[test] fn pressure_sleeps_all_eligible_background_tabs() {
    let n = Instant::now() + Duration::from_secs(10_000);
    let t = [snap(1, true, true, 0, false, n), snap(2, true, false, 5, false, n), snap(3, false, false, 5, false, n)];
    assert_eq!(run(&t, n, Pressure::Warning), vec![TabId(2)]);
}
```

- [ ] **Step 2: Tulis test daftar tab yang gagal di `tabs.rs`**

Nama test dan isi yang dipastikan:

- `open_inserts_after_active_and_activates`: tab A aktif, B di kanan; `open(.., activate: true)` → urutan A, baru, B, dan yang baru aktif.
- `closing_active_activates_right_neighbor_else_left`: tutup tab tengah yang aktif → tab kanannya aktif; tutup tab paling kanan yang aktif → tab kirinya aktif.
- `closing_last_tab_leaves_one_blank_tab`: setelah `close` satu-satunya tab, `iter().count() == 1`, tab itu `url == None` dan aktif.
- `reopen_closed_restores_url_title_scroll_asleep`: tutup tab ber-URL dengan `scroll_y = 640.0` → `reopen_closed` menghasilkan tab baru dengan URL, judul, dan `scroll_y` yang sama, `awake == false`, di posisi semula. Stack tab tertutup menyimpan paling banyak 20 entri.
- `activate_index_8_selects_last`: `activate_index(8, ..)` (⌘9) memilih tab terakhir walaupun jumlah tab kurang dari 9.
- `activate_next_and_previous_wrap`: dari tab terakhir, `activate_next` → tab pertama; dari tab pertama, `activate_previous` → tab terakhir.
- `deactivated_tab_gets_last_viewed_now`: `activate(B, t1)` saat A aktif → `A.last_viewed == t1`.

- [ ] **Step 3: Jalankan test, pastikan gagal**

Run: `cargo test -p my-browser-core -- sleep tabs`
Expected: FAIL, tipe belum ada.

- [ ] **Step 4: Implementasikan `tabs_to_sleep` dan `TabList`**

`tabs_to_sleep`: kandidat adalah tab yang `awake && !active && !media_active`. Kalau `pressure != Normal`, semua kandidat ditidurkan. Kalau tidak: (a) kandidat dengan `now - last_viewed >= idle_after`; (b) kalau jumlah tab bangun yang tersisa (termasuk aktif dan media) masih lebih dari `max_awake`, tambahkan kandidat sisa mulai dari `last_viewed` paling lama sampai jumlahnya tidak lebih dari `max_awake` atau kandidat habis. Urutan hasil: paling lama tidak dilihat lebih dulu.

- [ ] **Step 5: Jalankan test, pastikan lulus**

Run: `cargo test -p my-browser-core -- sleep tabs`
Expected: 14 passed.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): daftar tab dan kebijakan tidur"
```

---

### Task 5: Pengaturan dan sesi

**Files:**
- Create: `crates/core/src/settings.rs`, `crates/core/src/session.rs`, `crates/core/src/persist.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/Cargo.toml` (`serde`, `serde_json`; dev `tempfile`)

**Interfaces:**
- Consumes: `TabList`, `Tab`, `SleepPolicy` (Task 4).
- Produces:
  - `browser_core::persist::app_support_dir(home: &Path) -> PathBuf` → `<home>/Library/Application Support/my-browser`
  - `browser_core::persist::write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()>` (tulis ke `<path>.tmp` lalu `rename`)
  - `Settings { search_template: String, blocker_exceptions: Vec<String>, sleep_idle_secs: u64, max_awake: usize }`, `#[serde(default)]`, `Settings::load(path: &Path) -> Settings`, `Settings::save(&self, path: &Path) -> io::Result<()>`, `Settings::sleep_policy(&self) -> SleepPolicy`
  - `SessionFile { version: u32, active_index: Option<usize>, tabs: Vec<SavedTab> }`, `SavedTab { url: String, title: String, scroll_y: f64 }`, `SessionFile::load(path: &Path) -> Option<SessionFile>`, `SessionFile::save(&self, path: &Path) -> io::Result<()>`, konstanta `SESSION_VERSION: u32 = 1`
  - `TabList::to_session(&self) -> SessionFile`, `TabList::from_session(s: &SessionFile, now: Instant) -> TabList`

- [ ] **Step 1: Tulis test yang gagal**

```rust
#[test] fn settings_defaults() {
    let s = Settings::default();
    assert_eq!(s.search_template, "https://www.google.com/search?q={}");
    assert_eq!((s.sleep_idle_secs, s.max_awake), (300, 3));
    assert!(s.blocker_exceptions.is_empty());
}
#[test] fn settings_missing_fields_use_defaults() {
    let s: Settings = serde_json::from_str(r#"{"max_awake":5}"#).unwrap();
    assert_eq!((s.max_awake, s.sleep_idle_secs), (5, 300));
}
#[test] fn corrupt_settings_file_gives_defaults() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("settings.json");
    std::fs::write(&p, "{bukan json").unwrap();
    assert_eq!(Settings::load(&p), Settings::default());
}
#[test] fn corrupt_or_old_session_is_none() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("session.json");
    std::fs::write(&p, r#"{"version":1,"tabs":[{"url":"https://a.com""#).unwrap();
    assert!(SessionFile::load(&p).is_none());
    std::fs::write(&p, r#"{"version":99,"active_index":null,"tabs":[]}"#).unwrap();
    assert!(SessionFile::load(&p).is_none());
}
#[test] fn session_roundtrip_restores_all_tabs_asleep() {
    // TabList dengan 3 tab ber-URL, tab ke-2 aktif → to_session → save → load → from_session:
    // 3 tab, URL/judul/scroll_y sama, tab ke-2 aktif, semua awake == false.
}
#[test] fn session_skips_blank_tabs() {
    // TabList: 1 tab ber-URL + 1 tab kosong → to_session().tabs.len() == 1
}
#[test] fn empty_session_gives_one_blank_tab() {
    // from_session(SessionFile { version: 1, active_index: None, tabs: vec![] }) → 1 tab kosong aktif
}
#[test] fn save_leaves_no_tmp_file() {
    // setelah save, isi direktori hanya "session.json"
}
```

- [ ] **Step 2: Jalankan test, pastikan gagal**

Run: `cargo test -p my-browser-core -- settings session`
Expected: FAIL.

- [ ] **Step 3: Implementasikan**

`Settings::load` dan `SessionFile::load` mencetak satu baris peringatan ke stderr kalau berkas rusak, lalu mengembalikan bawaan/`None`. Berkas yang tidak ada tidak memicu peringatan. `save` membuat direktori induk kalau belum ada.

- [ ] **Step 4: Jalankan test, pastikan lulus**

Run: `cargo test -p my-browser-core -- settings session`
Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): pengaturan dan sesi tersimpan sebagai JSON"
```

---

### Task 6: Trait mesin dan controller `Browser`

**Files:**
- Create: `crates/core/src/engine.rs`, `crates/core/src/youtube.rs`, `crates/core/src/browser.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: Task 3, 4, 5.
- Produces di `browser_core::engine`:

```rust
pub enum JsTag { ScrollY, VideoTime }

pub enum PageEvent {
    UrlChanged(Url),
    TitleChanged(String),
    LoadingChanged(bool),
    LoadFinished,
    NavState { can_back: bool, can_forward: bool },
    OpenInNewTab(Url),
    MediaState(bool),                      // true: memutar media, memakai kamera/mikrofon, atau sedang download
    Number { tag: JsTag, value: Option<f64> },
    DownloadFinished(PathBuf),
    DownloadFailed(String),
    ProcessCrashed,
}

pub trait Page {
    type View: Clone;
    fn load(&self, url: &Url);
    fn go_back(&self);
    fn go_forward(&self);
    fn reload(&self);
    fn stop(&self);
    fn request_media_state(&self);                 // dibalas PageEvent::MediaState
    fn request_number(&self, tag: JsTag, script: &str); // dibalas PageEvent::Number
    fn run_script(&self, script: &str);
    fn view(&self) -> Self::View;
}                                                  // Drop melepas halaman dan memory-nya

pub trait Engine {
    type Page: Page;
    fn create_page(&self, on_event: Box<dyn Fn(PageEvent)>) -> Self::Page;
    fn set_blocker_exceptions(&self, hosts: &[String]);
}
```

- Produces di `browser_core::youtube`: `is_youtube_watch(url: &Url) -> bool`, `with_resume_time(url: &Url, seconds: f64) -> Url`, `VIDEO_TIME_JS: &str = "(function(){var v=document.querySelector('video');return v?v.currentTime:null;})()"`, dan di `browser_core::browser`: `SCROLL_Y_JS: &str = "window.scrollY"`.
- Produces di `browser_core::browser`:
  - `pub type SinkFactory = Box<dyn Fn(TabId) -> Box<dyn Fn(PageEvent)>>;`
  - `pub enum UiChange { Tabs, Active, Nav, Settings, Notice(String) }`
  - `Browser<E: Engine>` dengan: `new(engine: E, settings: Settings, session: Option<&SessionFile>, sinks: SinkFactory, now: Instant) -> Self`, `tabs(&self) -> &TabList`, `settings(&self) -> &Settings`, `active_view(&self) -> Option<<E::Page as Page>::View>`, `new_tab(&mut self, now)`, `close_tab(&mut self, id, now)`, `reopen_closed_tab(&mut self, now)`, `activate(&mut self, id, now)`, `activate_index(&mut self, index, now)`, `activate_next(&mut self, now)`, `activate_previous(&mut self, now)`, `navigate(&mut self, input: &str, now) -> bool`, `go_back(&mut self)`, `go_forward(&mut self)`, `reload_or_stop(&mut self)`, `tick(&mut self, now, pressure: Pressure)`, `handle_event(&mut self, id: TabId, event: PageEvent, now)`, `take_changes(&mut self) -> Vec<UiChange>`, `session_if_dirty(&mut self) -> Option<SessionFile>`.

Perilaku `Browser` yang dipastikan test:

- Tab kosong tidak punya page. Page dibuat saat `navigate` atau saat tab tidur diaktifkan.
- Saat tab berhenti aktif, `Browser` meminta `request_number(ScrollY, SCROLL_Y_JS)` dan menyimpan balasannya ke `tab.scroll_y`.
- `tick`: hitung `tabs_to_sleep` dari cache `media_active`, tandai `sleep_pending`, lalu minta `request_media_state` (dan `request_number(VideoTime, ..)` untuk halaman YouTube watch) ke setiap tab latar yang bangun. Balasan `MediaState(false)` untuk tab `sleep_pending` yang masih latar → tab ditidurkan. Balasan `MediaState(true)` → `sleep_pending` dibatalkan.
- Menidurkan tab: kalau YouTube watch dan `video_time >= 1`, `tab.url = with_resume_time(url, video_time)`; page dilepas; `awake = false`; `restore_scroll = scroll_y > 0`.
- Membangunkan tab: page baru, `load(url)`, dan saat `LoadFinished` dengan `restore_scroll` → `run_script("window.scrollTo(0, <scroll_y dibulatkan>)")`.
- `OpenInNewTab(url)` membuka tab baru di sebelah tab pengirim, aktif, langsung dimuat. Konsekuensinya, popup yang butuh `window.opener` tidak didukung di versi pertama.
- `ProcessCrashed`: tab aktif → `reload()`, kecuali crash sebelumnya kurang dari 10 detik lalu (tidak reload, `UiChange::Notice("Halaman ini crash berulang kali. Tekan ⌘R untuk memuat ulang.")`). Tab latar → ditidurkan.
- Event untuk `TabId` yang tidak ada atau tab yang sedang tidur diabaikan.
- `DownloadFinished`/`DownloadFailed` diabaikan dulu; Task 11 yang menanganinya.

- [ ] **Step 1: Tulis test YouTube yang gagal di `youtube.rs`**

```rust
fn u(s: &str) -> Url { Url::parse(s).unwrap() }

#[test] fn adds_t_param() {
    assert_eq!(with_resume_time(&u("https://www.youtube.com/watch?v=abc"), 125.7).as_str(), "https://www.youtube.com/watch?v=abc&t=125s");
}
#[test] fn replaces_existing_t_keeps_others() {
    assert_eq!(with_resume_time(&u("https://www.youtube.com/watch?v=abc&t=10s&list=PL1"), 60.0).as_str(), "https://www.youtube.com/watch?v=abc&list=PL1&t=60s");
}
#[test] fn under_one_second_unchanged() {
    assert_eq!(with_resume_time(&u("https://www.youtube.com/watch?v=abc"), 0.4).as_str(), "https://www.youtube.com/watch?v=abc");
}
#[test] fn detects_watch_pages_only() {
    assert!(is_youtube_watch(&u("https://m.youtube.com/watch?v=x")));
    assert!(!is_youtube_watch(&u("https://www.youtube.com/")));
    assert!(!is_youtube_watch(&u("https://youtube.com.evil.com/watch?v=x")));
}
```

- [ ] **Step 2: Tulis mesin palsu dan test `Browser` yang gagal di `browser.rs` (`#[cfg(test)]`)**

Mesin palsu: `FakeEngine` mencatat setiap pemanggilan ke `Rc<RefCell<Log>>` sebagai `Call { Load(String), Reload, Stop, Back, Forward, MediaState, Number(JsTag), Script(String) }`, menyimpan jumlah page hidup (`live`, berkurang di `Drop`), dan menyimpan argumen terakhir `set_blocker_exceptions`. `SinkFactory` di test tidak dipakai: test memanggil `handle_event` langsung.

Nama test dan isi yang dipastikan:

- `navigate_creates_page_and_loads_parsed_url`: `navigate("youtube.com")` → `live == 1`, log berisi `Load("https://youtube.com/")`.
- `new_tab_has_no_page_until_navigation`: `new_tab` → `live` tidak bertambah.
- `deactivating_tab_requests_scroll_position`: tab A bangun, aktifkan B → log berisi `Number(ScrollY)` untuk A.
- `tick_sleeps_idle_tab_after_media_state_false`: A latar 301 detik → `tick` → log berisi `MediaState`; `handle_event(A, MediaState(false))` → `live` turun 1, `A.awake == false`.
- `media_state_true_cancels_pending_sleep`: seperti di atas, balasan `MediaState(true)` → A tetap bangun, `sleep_pending == false`.
- `waking_restores_scroll_after_load_finished`: A punya `scroll_y = 640.0`, ditidurkan, lalu diaktifkan → `Load(url A)`; `handle_event(A, LoadFinished)` → `Script("window.scrollTo(0, 640)")`.
- `youtube_tab_resumes_at_cached_video_time`: A = `https://www.youtube.com/watch?v=abc`, `Number { VideoTime, Some(125.7) }`, ditidurkan → `A.url` = `https://www.youtube.com/watch?v=abc&t=125s`.
- `open_in_new_tab_opens_next_to_opener_and_activates`.
- `crash_in_active_tab_reloads_once_then_notices`: `ProcessCrashed` dua kali dalam 5 detik → satu `Reload`, satu `UiChange::Notice`.
- `crash_in_background_tab_puts_it_to_sleep`.
- `late_events_for_closed_or_sleeping_tab_are_ignored`: tutup A, lalu `handle_event(A, TitleChanged("x"))` dan `handle_event(A, MediaState(false))` → tidak panic, tidak ada tab baru, `live` tidak berubah. Sama untuk tab tidur dengan `Number { ScrollY, .. }`.
- `title_and_url_events_update_tab_and_dirty_session`: `TitleChanged`/`UrlChanged` → nilai tab berubah, `session_if_dirty()` mengembalikan `Some` sekali lalu `None`.
- `session_restore_wakes_only_active_tab`: sesi 3 tab, aktif index 1 → `live == 1` dan satu `Load`.

- [ ] **Step 3: Jalankan test, pastikan gagal**

Run: `cargo test -p my-browser-core -- youtube browser`
Expected: FAIL.

- [ ] **Step 4: Implementasikan `engine.rs`, `youtube.rs`, `browser.rs`**

`Browser` menyimpan `HashMap<TabId, E::Page>`. Setiap perubahan tab, judul, URL, atau urutan menandai sesi kotor. Setiap perubahan yang terlihat menambah `UiChange` yang sesuai.

- [ ] **Step 5: Jalankan test dan cek batas dependensi**

Run: `cargo test -p my-browser-core && cargo tree -p my-browser-core | grep -E 'objc2|block2|dispatch2' ; echo "exit=$?"`
Expected: semua test lulus, `grep` tidak mencetak apa pun, dan `exit=1`.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): trait mesin dan controller Browser"
```

---

### Task 7: Mesin WebKit

**Files:**
- Create: `crates/engine-webkit/src/page.rs`, `crates/engine-webkit/src/observer.rs`, `crates/engine-webkit/src/delegates.rs`, `crates/engine-webkit/tests/smoke.rs`
- Modify: `crates/engine-webkit/src/lib.rs`, `crates/engine-webkit/Cargo.toml` (`[[test]] name = "smoke"`, `harness = false`), `crates/app/src/main.rs` (prototipe memakai engine)

**Interfaces:**
- Consumes: `Engine`, `Page`, `PageEvent`, `JsTag` (Task 6), `user_agent` (Task 1).
- Produces: `engine_webkit::WebKitEngine::new(mtm: MainThreadMarker) -> WebKitEngine`, `impl Engine for WebKitEngine { type Page = WebKitPage; }`, `impl Page for WebKitPage { type View = Retained<NSView>; }`.

Kebutuhan implementasi:

- Setiap page punya `WKWebViewConfiguration` dan `WKUserContentController` sendiri. Data store bersama `WKWebsiteDataStore::defaultDataStore()`. Fullscreen dan user agent sama seperti prototipe Task 1. `setInspectable(true)` hanya di build debug.
- KVO pada `title`, `URL`, `loading`, `canGoBack`, `canGoForward` menghasilkan `TitleChanged`, `UrlChanged`, `LoadingChanged`, `NavState`.
- Navigation delegate: `didFinishNavigation` → `LoadFinished`; `webViewWebContentProcessDidTerminate` → `ProcessCrashed`.
- UI delegate: `createWebViewWithConfiguration:...` mengirim `OpenInNewTab(url request)` dan mengembalikan `nil`.
- `request_media_state`: `requestMediaPlaybackState` bernilai Playing, atau `cameraCaptureState`/`microphoneCaptureState` Active → `MediaState(true)`.
- `request_number`: `evaluateJavaScript` → `NSNumber` → `Some(f64)`, selain itu `None`.
- Semua event dijadwalkan ke main queue (`dispatch2`), tidak pernah dipanggil langsung di dalam method `Page`.
- `Drop`: `stopLoading`, lepas semua observer KVO, kosongkan delegate, `removeFromSuperview`. Observer yang tidak dilepas membuat aplikasi crash.

- [ ] **Step 1: Tulis smoke test yang gagal di `tests/smoke.rs`**

`main()` membuat `NSApplication`, `WebKitEngine`, dan page dengan sink yang mendorong event ke `Rc<RefCell<Vec<PageEvent>>>`. Run loop diputar sampai kondisi terpenuhi atau batas 10 detik. Yang dipastikan, dengan `assert!` dan keluar non-zero kalau gagal:

1. `load("data:text/html,<title>halo</title><p>x</p>")`, dan tepat setelah `load` kembali, vektor event masih kosong (tidak ada event sinkron).
2. Run loop → ada `TitleChanged("halo")` dan `LoadFinished`.
3. `request_number(JsTag::ScrollY, "1+1")` → `Number { tag: ScrollY, value: Some(2.0) }`.
4. `request_media_state()` → `MediaState(false)`.
5. Tiga kali buat page, `load` halaman yang sama, tunggu `LoadFinished`, lalu drop. Tidak crash.

- [ ] **Step 2: Jalankan, pastikan gagal**

Run: `cargo test -p engine-webkit --test smoke`
Expected: FAIL (kompilasi gagal karena `WebKitEngine` belum ada).

- [ ] **Step 3: Implementasikan `WebKitEngine`, `WebKitPage`, observer, dan delegate**

`set_blocker_exceptions` cukup menyimpan daftar host di `Rc<RefCell<Vec<String>>>`. Pemakaiannya datang di Task 10.

- [ ] **Step 4: Jalankan, pastikan lulus**

Run: `cargo test -p engine-webkit`
Expected: unit test dan `smoke` lulus.

- [ ] **Step 5: Prototipe memakai engine**

`crates/app/src/main.rs` membuat satu page lewat `WebKitEngine` (bukan `WKWebView` langsung) dan menempel `page.view()` ke window.
Run: `scripts/run.sh`
Expected: YouTube tampil dan login dari Task 1 masih ada.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(engine-webkit): implementasi Engine/Page di atas WKWebView"
```

---

### Task 8: UI aplikasi

**Files:**
- Create: `crates/app/src/state.rs`, `crates/app/src/window.rs`, `crates/app/src/nav_bar.rs`, `crates/app/src/tab_strip.rs`, `crates/app/src/menu.rs`, `crates/app/src/actions.rs`
- Modify: `crates/app/src/main.rs`, `docs/testing/daftar-cek-manual.md`

**Interfaces:**
- Consumes: `Browser<WebKitEngine>` dan seluruh API-nya (Task 6), `WebKitEngine` (Task 7).
- Produces: `app::state::AppState { browser: RefCell<Browser<WebKitEngine>>, .. }` di dalam `Rc`, dan `AppState::refresh(&self)` yang menguras `take_changes()` lalu memperbarui view. Task 9 sampai 11 menambah handler ke sini.

Kebutuhan:

- Window 1280x800, minimum 800x500. Dari atas ke bawah: nav bar 36pt, tab strip 28pt, area konten yang diisi `active_view()`. Window ditutup → aplikasi keluar.
- Nav bar: tombol mundur, maju, muat ulang/berhenti (SF Symbols `chevron.left`, `chevron.right`, `arrow.clockwise`, `xmark`), lalu address field yang melebar. Tombol mundur/maju mengikuti `can_back`/`can_forward`. Ikon muat ulang berganti `xmark` saat `loading`.
- Address field menampilkan URL tab aktif (kosong untuk tab kosong). Enter → `navigate`. Escape → kembali ke URL tab aktif. Tab kosong yang aktif membuat address field langsung fokus.
- Tab strip: satu tombol judul per tab (lebar 80 sampai 200pt, judul dipotong di ujung) dengan tombol tutup kecil, lalu tombol `plus`. Tab aktif ditandai. Kalau tab terlalu banyak, strip bisa digeser horizontal.
- Judul window = judul tab aktif, atau `my-browser` kalau kosong.
- Menu (teks Indonesia): aplikasi (Keluar my-browser ⌘Q); Berkas (Tab Baru ⌘T, Tutup Tab ⌘W, Buka Lagi Tab yang Ditutup ⇧⌘T, Buka Alamat ⌘L); Edit (Urungkan ⌘Z, Ulangi ⇧⌘Z, Potong ⌘X, Salin ⌘C, Tempel ⌘V, Pilih Semua ⌘A, memakai selector first responder standar); Tampilan (Muat Ulang ⌘R, Layar Penuh ⌃⌘F); Riwayat (Mundur ⌘[, Maju ⌘]); Jendela (Tab Berikutnya ⌃⇥ dan ⇧⌘], Tab Sebelumnya ⌃⇧⇥ dan ⇧⌘[, Tab 1 sampai 9 ⌘1 sampai ⌘9).
- Sink factory menangkap `Weak<AppState>`. Event → `browser.borrow_mut().handle_event(id, ev, Instant::now())`, borrow dilepas, lalu `refresh()`. Borrow `RefCell` tidak boleh ditahan selama memanggil method AppKit.
- Aplikasi dibuka dengan satu tab kosong. Sesi baru dipulihkan di Task 9.

- [ ] **Step 1: Tulis daftar cek manual Task 8 di `docs/testing/daftar-cek-manual.md`**

Satu checkbox per kebutuhan di atas, ditambah: ⌘V di address field menempel teks, mengetik `youtube.com` lalu Enter memuat YouTube, mengetik `resep nasi goreng` membuka pencarian Google, link `target=_blank` membuka tab baru di sebelahnya, tombol layar penuh YouTube berfungsi.

- [ ] **Step 2: Implementasikan modul UI dan sambungkan ke `Browser`**

- [ ] **Step 3: Verifikasi**

Run: `cargo test --workspace && scripts/run.sh`
Expected: semua test lulus, aplikasi terbuka, dan semua checkbox Task 8 dicentang. Checkbox yang gagal dicatat beserta alasannya.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(app): window, nav bar, tab strip, dan menu"
```

---

### Task 9: Tab tidur di aplikasi, sesi, dan CHECKPOINT proses WebContent

**Files:**
- Create: `crates/app/src/timer.rs`, `crates/app/src/memory_pressure.rs`, `crates/app/src/persistence.rs`
- Modify: `crates/app/src/main.rs`, `crates/app/src/state.rs`, `crates/app/src/tab_strip.rs`, `docs/testing/daftar-cek-manual.md`, `docs/testing/hasil-ukur.md`

**Interfaces:**
- Consumes: `Browser::tick`, `Browser::session_if_dirty`, `Settings::load/save`, `SessionFile::load/save`, `app_support_dir` (Task 5, 6), `scripts/ukur-memory.sh --detail` (Task 2).

Kebutuhan:

- `NSTimer` berulang 30 detik → `tick(now, Pressure::Normal)`, lalu simpan `session_if_dirty()` kalau ada.
- Dispatch source memory pressure (warn dan critical) → `tick(now, Warning|Critical)` saat itu juga.
- Saat mulai: `Settings::load` dan `SessionFile::load` dari `app_support_dir($HOME)`, lalu `Browser::new(.., session.as_ref(), ..)`. Saat `applicationWillTerminate`: sesi dan pengaturan selalu disimpan.
- Tab tidur di tab strip: teks `NSColor::secondaryLabelColor`, tooltip "Tidur, dimuat ulang saat dibuka".

- [ ] **Step 1: Tambah daftar cek manual Task 9**

Untuk uji manual, set `"sleep_idle_secs": 20` di `settings.json`. Checkbox: (a) 4 tab ber-URL, aktifkan satu, tunggu 60 detik → tab latar tampil tidur; (b) video YouTube diputar di tab latar → tab itu tidak pernah tidur; (c) video YouTube yang di-pause di menit ke-2, ditidurkan, lalu dibuka → lanjut dari sekitar menit ke-2; (d) halaman yang di-scroll jauh, ditidurkan, lalu dibuka → posisi scroll kembali; (e) pengguna menjalankan `sudo memory_pressure -S -l warn` → semua tab latar tidur dalam beberapa detik; (f) ⌘Q lalu buka lagi → semua tab kembali, hanya tab aktif yang dimuat. Kembalikan `sleep_idle_secs` ke 300 setelah selesai.

- [ ] **Step 2: Implementasikan timer, memory pressure, persistence, dan tanda tab tidur**

- [ ] **Step 3: Verifikasi daftar cek**

Run: `scripts/run.sh`
Expected: checkbox (a) sampai (f) dicentang. Langkah (e) dijalankan pengguna karena butuh `sudo`.

- [ ] **Step 4: Ukur proses WebContent setelah tab tidur**

Buka 4 tab ber-URL (`sleep_idle_secs` 20). Jalankan `scripts/ukur-memory.sh --detail my-browser` sebelum tab tidur, lalu 30 detik dan 120 detik setelah tab latar tidur. Catat jumlah `com.apple.WebKit.WebContent` dan total footprint di `hasil-ukur.md`.

- [ ] **Step 5: CHECKPOINT**

Kalau proses WebContent untuk tab yang tidur masih ada setelah 120 detik, Task 12 diperlukan. Kalau tidak, Task 12 dilewati. Tulis keputusannya di `hasil-ukur.md` dan laporkan ke pengguna.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(app): penidur tab, memory pressure, dan pemulihan sesi"
```

---

### Task 10: Pemblokir iklan dan tracker

**Files:**
- Create: `crates/blocklist-gen/Cargo.toml`, `crates/blocklist-gen/src/lib.rs`, `crates/blocklist-gen/src/main.rs`, `scripts/update-filters.sh`
- Create: `crates/core/src/blocking.rs`, `crates/engine-webkit/src/blocker.rs`
- Modify: `crates/core/src/browser.rs`, `crates/core/src/lib.rs`, `crates/engine-webkit/src/lib.rs`, `crates/engine-webkit/src/delegates.rs`, `crates/app/src/menu.rs`, `crates/app/src/main.rs`, `scripts/bundle.sh`, `docs/testing/daftar-cek-manual.md`

**Interfaces:**
- Produces: `blocklist_gen::convert(filter_text: &str) -> (Vec<serde_json::Value>, usize)` (aturan WebKit, jumlah filter yang dilewati), `blocklist_gen::chunk(rules: Vec<serde_json::Value>, max: usize) -> Vec<Vec<serde_json::Value>>`, konstanta `MAX_RULES_PER_LIST: usize = 50_000`.
- Produces: `browser_core::blocking::normalize_host(host: &str) -> String`, `browser_core::blocking::host_matches(host: &str, exceptions: &[String]) -> bool`.
- Produces: `Browser::toggle_blocker_exception(&mut self) -> Option<bool>` (status pengecualian yang baru untuk host tab aktif; `None` kalau tab tidak punya host) dan `Browser::active_host_excepted(&self) -> bool` untuk tanda centang menu.
- Produces: `WebKitEngine::load_blocklists(&self, dir: &Path)`.

Kebutuhan:

- `scripts/update-filters.sh` mengunduh `https://easylist.to/easylist/easylist.txt` dan `https://easylist.to/easylist/easyprivacy.txt` ke `resources/filters/`, lalu menjalankan `cargo run --release -p blocklist-gen -- resources/filters resources/blocklist`, yang menulis `blocklist-0.json`, `blocklist-1.json`, dan seterusnya. `scripts/bundle.sh` menjalankan script ini kalau `resources/blocklist/` belum ada, lalu menyalinnya ke `Contents/Resources/blocklist/`.
- `ignore-previous-rules` di WebKit hanya berlaku di dalam satu daftar. Karena itu `chunk` memisahkan aturan `ignore-previous-rules`, membagi aturan lainnya per `max - jumlah_pengecualian`, dan menambahkan seluruh aturan pengecualian di akhir setiap potongan.
- `load_blocklists`: identifier `blocklist-<i>-<hash FNV-1a 64-bit isi, hex>`. Cari dulu dengan `lookUpContentRuleListForIdentifier`, kompilasi kalau belum ada, lalu hapus identifier lama yang tidak dipakai lagi (`getAvailableContentRuleListIdentifiers`). Daftar yang siap dipasang ke semua page hidup dan semua page baru. Lama kompilasi pertama dicatat ke stderr.
- Pengecualian per situs: di `decidePolicyForNavigationAction` untuk main frame, kalau host tujuan cocok dengan `host_matches`, semua daftar dilepas dari `WKUserContentController` page itu; kalau tidak cocok, dipasang lagi.
- Menu Tampilan → "Matikan Pemblokir untuk Situs Ini", bercentang sesuai `active_host_excepted()`. Klik → `toggle_blocker_exception()` → simpan pengaturan saat ada `UiChange::Settings`.
- YouTube tidak dikecualikan secara bawaan. Kalau YouTube rusak di daftar cek, tambahkan `youtube.com` ke nilai bawaan `blocker_exceptions` dan catat alasannya.

- [ ] **Step 1: Tulis test yang gagal**

```rust
// crates/blocklist-gen/src/lib.rs
#[test] fn converts_network_cosmetic_and_exception_rules() {
    let (rules, _) = convert("||ads.example.com^\n##.ad-banner\n@@||good.example.com^\n");
    assert!(rules.iter().any(|r| r["action"]["type"] == "block"
        && r["trigger"]["url-filter"].as_str().unwrap().contains("ads\\.example\\.com")));
    assert!(rules.iter().any(|r| r["action"]["type"] == "css-display-none" && r["action"]["selector"] == ".ad-banner"));
    assert!(rules.iter().any(|r| r["action"]["type"] == "ignore-previous-rules"));
}
#[test] fn chunk_appends_all_exceptions_to_every_chunk() {
    let block = |i: usize| serde_json::json!({"trigger": {"url-filter": format!("a{i}")}, "action": {"type": "block"}});
    let ignore = |i: usize| serde_json::json!({"trigger": {"url-filter": format!("b{i}")}, "action": {"type": "ignore-previous-rules"}});
    let rules: Vec<_> = (0..120_000).map(block).chain((0..1_000).map(ignore)).collect();
    let chunks = chunk(rules, 50_000);
    assert!(chunks.iter().all(|c| c.len() <= 50_000));
    assert!(chunks.iter().all(|c| c[c.len() - 1_000..].iter().all(|r| r["action"]["type"] == "ignore-previous-rules")));
    let blocks: usize = chunks.iter().map(|c| c.len() - 1_000).sum();
    assert_eq!(blocks, 120_000);
}

// crates/core/src/blocking.rs
#[test] fn normalize_strips_www_and_lowercases() { assert_eq!(normalize_host("WWW.Detik.com"), "detik.com"); }
#[test] fn matches_exact_and_subdomains_only() {
    let ex = vec!["detik.com".to_string()];
    assert!(host_matches("detik.com", &ex));
    assert!(host_matches("news.detik.com", &ex));
    assert!(!host_matches("notdetik.com", &ex));
}
```

Di `browser.rs`:

- `toggle_exception_updates_settings_engine_and_reloads`: tab aktif `https://www.detik.com/x` → `Some(true)`, `settings().blocker_exceptions == ["detik.com"]`, mesin palsu menerima `["detik.com"]`, log berisi `Reload`, ada `UiChange::Settings`. Toggle kedua → `Some(false)` dan daftarnya kosong.
- `toggle_on_blank_tab_returns_none`.

- [ ] **Step 2: Jalankan test, pastikan gagal**

Run: `cargo test -p blocklist-gen -p my-browser-core`
Expected: FAIL.

- [ ] **Step 3: Implementasikan `convert`/`chunk` dengan crate `adblock` (fitur `content-blocking`, lihat docs.rs `adblock` 0.13.5 untuk API konversinya), lalu `blocking.rs` dan toggle di `Browser`**

- [ ] **Step 4: Jalankan test, pastikan lulus**

Run: `cargo test -p blocklist-gen -p my-browser-core`
Expected: semua lulus.

- [ ] **Step 5: Implementasikan `blocker.rs`, pengecualian di navigation delegate, menu, dan langkah bundle**

- [ ] **Step 6: Verifikasi manual**

Run: `rm -rf resources/blocklist && scripts/run.sh`
Expected: `resources/blocklist/` terisi; stderr mencetak lama kompilasi pertama. Daftar cek: https://www.detik.com menampilkan lebih sedikit iklan dibanding Safari tanpa content blocker; "Matikan Pemblokir untuk Situs Ini" memunculkan iklan lagi setelah reload otomatis dan centangnya ikut berubah; YouTube masih memutar video dan login tetap ada.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: pemblokir iklan dengan WKContentRuleList dan pengecualian per situs"
```

---

### Task 11: Download sederhana

**Files:**
- Create: `crates/core/src/downloads.rs`, `crates/engine-webkit/src/download.rs`
- Modify: `crates/core/src/browser.rs`, `crates/core/src/lib.rs`, `crates/engine-webkit/src/delegates.rs`, `crates/engine-webkit/src/page.rs`, `crates/app/src/nav_bar.rs`, `crates/app/src/state.rs`, `docs/testing/daftar-cek-manual.md`

**Interfaces:**
- Produces: `browser_core::downloads::unique_path(dir: &Path, suggested: &str, exists: impl Fn(&Path) -> bool) -> PathBuf`.
- Consumes: `PageEvent::DownloadFinished/DownloadFailed`, `UiChange::Notice` (Task 6).

Kebutuhan:

- Navigation delegate: response yang tidak bisa ditampilkan (`!canShowMIMEType`) atau aksi dengan `shouldPerformDownload` → download. Delegate download menentukan tujuan dengan `unique_path(~/Downloads, suggestedFilename, |p| p.exists())`.
- Selama ada download aktif dari sebuah page, `request_media_state` page itu dibalas `true` supaya tab tidak ditidurkan dan download tidak terputus.
- `Browser`: `DownloadFinished(path)` → `Notice("Selesai diunduh: <nama berkas>")`; `DownloadFailed(msg)` → `Notice("Gagal mengunduh: <msg>")`.
- Nav bar menampilkan `Notice` sebagai label di kanan selama 6 detik.

- [ ] **Step 1: Tulis test yang gagal**

```rust
fn ex<'a>(names: &'a [&'a str]) -> impl Fn(&Path) -> bool + 'a {
    move |p| names.iter().any(|n| p.file_name().unwrap() == *n)
}
#[test] fn keeps_name_when_free() { assert_eq!(unique_path(Path::new("/d"), "a.pdf", ex(&[])), Path::new("/d/a.pdf")); }
#[test] fn adds_counter_before_extension() { assert_eq!(unique_path(Path::new("/d"), "a.pdf", ex(&["a.pdf", "a (1).pdf"])), Path::new("/d/a (2).pdf")); }
#[test] fn counter_without_extension() { assert_eq!(unique_path(Path::new("/d"), "README", ex(&["README"])), Path::new("/d/README (1)")); }
#[test] fn strips_path_components() { assert_eq!(unique_path(Path::new("/d"), "../../etc/passwd", ex(&[])), Path::new("/d/passwd")); }
#[test] fn empty_name_becomes_download() { assert_eq!(unique_path(Path::new("/d"), "", ex(&[])), Path::new("/d/download")); }
```

Di `browser.rs`: `download_finished_emits_notice` (`DownloadFinished("/Users/x/Downloads/a.pdf")` → `Notice("Selesai diunduh: a.pdf")`) dan `download_failed_emits_notice`.

- [ ] **Step 2: Jalankan test, pastikan gagal**

Run: `cargo test -p my-browser-core -- downloads browser`
Expected: FAIL.

- [ ] **Step 3: Implementasikan `unique_path` dan penanganan event di `Browser`**

- [ ] **Step 4: Jalankan test, pastikan lulus**

Run: `cargo test -p my-browser-core`
Expected: semua lulus.

- [ ] **Step 5: Implementasikan download di engine dan label pemberitahuan di nav bar**

- [ ] **Step 6: Verifikasi manual**

Run: `scripts/run.sh`
Expected: mengunduh sebuah PDF dua kali menghasilkan `nama.pdf` dan `nama (1).pdf` di `~/Downloads`, dan label "Selesai diunduh: ..." muncul lalu hilang.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: download sederhana ke ~/Downloads"
```

---

### Task 12 (bersyarat): API privat WebKit untuk pengaturan memory

Kerjakan hanya kalau CHECKPOINT Task 9 menyatakan proses WebContent tab tidur masih hidup setelah 120 detik.

**Files:**
- Create: `crates/engine-webkit/src/private.rs`
- Modify: `crates/engine-webkit/src/lib.rs`, `docs/testing/hasil-ukur.md`

**Interfaces:**
- Produces: `engine_webkit::private::try_set_bool(obj: &AnyObject, selector: &str, value: bool) -> bool` (`true` kalau `respondsToSelector:` dan setter dipanggil), `engine_webkit::private::apply_memory_tuning(config: &WKWebViewConfiguration) -> Vec<&'static str>` (nama pengaturan yang berhasil diterapkan; dicetak ke stderr saat mulai).

- [ ] **Step 1: Tulis test yang gagal di `private.rs`**

```rust
#[test] fn unknown_selector_returns_false() {
    let o = NSObject::new();
    assert!(!try_set_bool(&o, "setBukanPropertiSungguhan:", true));
}
#[test] fn known_selector_returns_true_and_sets() {
    let r = NSMutableURLRequest::new();
    assert!(try_set_bool(&r, "setHTTPShouldHandleCookies:", false));
    assert!(!r.HTTPShouldHandleCookies());
}
```

- [ ] **Step 2: Jalankan test, pastikan gagal**

Run: `cargo test -p engine-webkit private`
Expected: FAIL.

- [ ] **Step 3: Implementasikan `try_set_bool`**

- [ ] **Step 4: Jalankan test, pastikan lulus**

Run: `cargo test -p engine-webkit private`
Expected: 2 passed.

- [ ] **Step 5: Implementasikan `apply_memory_tuning`**

Kandidat, diverifikasi dulu terhadap sumber WebKit yang sesuai Safari 18.6: `_WKProcessPoolConfiguration` dengan `setUsesWebProcessCache:` NO dan `setPrewarmsProcessesAutomatically:` NO, lalu satu `WKProcessPool` bersama yang dibuat dengan `_initWithConfiguration:` dan dipasang ke konfigurasi setiap page. Semua panggilan privat hanya ada di `private.rs`.

- [ ] **Step 6: Ukur ulang seperti Task 9 Step 4**

Expected: jumlah proses WebContent untuk tab tidur turun dalam 120 detik. Pengaturan yang tidak mengubah hasil ukur dibuang. Tulis hasilnya di `hasil-ukur.md`.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(engine-webkit): pengaturan memory lewat API privat dengan pengaman"
```

---

### Task 13: Pengukuran akhir

**Files:**
- Modify: `docs/testing/hasil-ukur.md`, `docs/testing/daftar-cek-manual.md`

- [ ] **Step 1: Jalankan seluruh daftar cek manual pada build release**

Run: `cargo test --workspace && scripts/run.sh`
Expected: semua test lulus. Semua checkbox dicentang, atau yang gagal dicatat beserta alasannya.

- [ ] **Step 2: Perbandingan utama (dengan bantuan pengguna)**

Bergantian, hanya satu browser hidup: Chrome dengan profil asli pengguna (catat status Memory Saver), Safari bawaan tanpa content blocker, lalu `my-browser` dengan pengaturan bawaan. Di setiap browser: satu tab YouTube memutar video 1080p dan lima tab dari `scripts/skenario-urls.txt`. Tunggu 10 menit tanpa menyentuh tab selain YouTube, lalu jalankan `scripts/ukur-memory.sh "Google Chrome" Safari my-browser`.

- [ ] **Step 3: Perbandingan tambahan**

`my-browser` dengan `"sleep_idle_secs": 100000` dan `"max_awake": 10` (semua tab bangun) dibandingkan Safari dengan tab yang sama. Hasilnya informasi saja, bukan penentu lulus/gagal.

- [ ] **Step 4: Isi tabel target di `hasil-ukur.md`**

| Ukuran | Target | Hasil |
|---|---|---|
| Total `my-browser` dibanding Chrome | paling banyak 50% | |
| Total `my-browser` dibanding Safari | lebih rendah | |
| Proses aplikasi saja | paling banyak 100 MB | |
| Proses WebContent untuk tab tidur | tidak ada | |

- [ ] **Step 5: Laporkan ke pengguna**

Sampaikan tabel apa adanya, termasuk target yang tidak tercapai.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "docs: hasil pengukuran akhir"
```
