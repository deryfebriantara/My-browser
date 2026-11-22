# Pengembangan

Panduan singkat untuk membangun, menguji, dan memeriksa my-browser.

## Susunan crate

| Crate | Isi |
|---|---|
| `crates/core` | Logika browser tanpa macOS: tab, tidur tab, riwayat, saran address bar, sesi, pengaturan, protokol kontrol. Sebagian besar test ada di sini. |
| `crates/engine-webkit` | Implementasi trait `Engine`/`Page` (`crates/core/src/engine.rs`) dengan WKWebView. |
| `crates/app` | Aplikasi AppKit: jendela, menu, address bar, panel, dialog, socket kontrol. |
| `crates/mcp` | Server MCP (`my-browser --mcp`) yang meneruskan tool ke socket kontrol. |
| `crates/vault` | Vault password terenkripsi (tanpa macOS). |
| `crates/memtool`, `crates/blocklist-gen` | Alat ukur memory dan pembuat daftar pemblokir. |

Aturan: logika yang bisa diuji tanpa AppKit ditaruh di `core` sebagai fungsi murni. `app` hanya menerjemahkan event AppKit ke `core` dan menjalankan hasilnya.

## Membangun dan menjalankan

```sh
cargo build                    # debug
scripts/run.sh                 # build release, bundle, lalu buka dist/my-browser.app
scripts/bundle.sh              # hanya membangun dist/my-browser.app (ad-hoc codesign)
```

`scripts/bundle.sh` menghapus bundle lama lalu menyalin yang baru, jadi aman dijalankan selama browser terbuka. Perubahan baru berlaku setelah browser dibuka ulang.

## Pemeriksaan

```sh
scripts/check.sh               # format, clippy tanpa peringatan, semua test, audit dependensi
scripts/check.sh --quick       # format dan clippy saja (sekitar 1 detik kalau cache hangat)
```

- Format mengikuti `rustfmt.toml` (baris sampai 120 karakter). Rapikan dengan `cargo fmt --all`.
- Clippy dijalankan dengan `-D warnings`: satu peringatan pun membuat pemeriksaan gagal.
- Audit dependensi memakai `cargo audit` (database RustSec). Pasang sekali dengan `cargo install cargo-audit --locked`.
- **Git hook:** `.githooks/pre-commit` menjalankan `scripts/check.sh --quick` sebelum setiap commit. Aktifkan sekali per clone dengan `git config core.hooksPath .githooks`. Lewati sekali dengan `git commit --no-verify`.
- **CI:** `.github/workflows/ci.yml` menjalankan `scripts/check.sh` di runner macOS GitHub. Repo ini belum punya remote, jadi workflow itu belum pernah berjalan.

## Test

| Jenis | Perintah | Keterangan |
|---|---|---|
| Unit | `cargo test -p my-browser-core` (juga `-p my-browser-vault`, `-p my-browser-mcp`, `-p my-browser`) | Cepat, tanpa jendela |
| Smoke WebKit | `cargo test -p engine-webkit --test smoke` | WKWebView sungguhan di main thread (`harness = false`), butuh sesi login macOS |
| Benchmark saran | `cargo test --release -p my-browser-core suggest_10k -- --ignored --nocapture` | Batas 5 ms per ketikan untuk 10.000 entri riwayat |
| E2E | Script Python di luar repo | Lihat di bawah |
| Manual | `docs/testing/daftar-cek-manual.md` | Hal yang butuh mata dan tangan pengguna |

### Uji E2E di instance terpisah

Uji E2E menjalankan build debug dengan folder sementara, supaya browser yang sedang dipakai tidak tersentuh:

- `HOME` dan `MYBROWSER_DATA_DIR` diarahkan ke folder sementara. Path-nya harus pendek, misalnya di bawah `$TMPDIR`, karena path socket Unix dibatasi sekitar 104 byte.
- Tool dipanggil lewat `my-browser --mcp` dengan JSON-RPC di stdin dan stdout.
- Instance uji dimatikan lewat pid-nya, bukan dengan menutup aplikasi bernama my-browser.

Pengait khusus build debug (tidak ada di build release; periksa dengan `strings dist/my-browser.app/Contents/MacOS/my-browser`):

| Pengait | Fungsi |
|---|---|
| `MYBROWSER_DIALOG_SELFTEST=<teks>` | Menjawab setiap dialog otomatis; isian rahasia diisi teks itu |
| `MYBROWSER_DIALOG_CHOICE=<n>` | Tombol yang dipilih di dialog (0 = tombol pertama) |
| `MYBROWSER_CHAT_SELFTEST=<berkas>` | Mengirim satu baris berkas ke panel Tanya Claude setiap kali panel dibuka |
| Tool `debug_ui` | Mengetik dan menekan tombol di address bar lewat field editor, mengirim aksi menu, membaca keadaan jendela, keluar (`crates/app/src/debug_ui.rs`) |

`MYBROWSER_DEBUG=1` mencetak log tambahan (event global dan keadaan tidur tab) ke stderr, juga di build release.

## Gaya

- Komentar dan dokumentasi dalam bahasa Indonesia. Menu memakai bahasa Inggris; dialog, panel, dan notifikasi masih bahasa Indonesia.
- Commit memakai awalan Conventional Commits (`feat`, `fix`, `refactor`, `style`, `docs`, `test`).
