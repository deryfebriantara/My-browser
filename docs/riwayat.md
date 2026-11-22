# Riwayat dan saran address bar

my-browser mencatat halaman yang dibuka, menyarankan halaman itu saat Anda mengetik di address bar, dan menyediakan jendela Riwayat untuk mencari dan menghapusnya. Semua saran berasal dari riwayat dan tab di Mac ini. Ketikan Anda tidak pernah dikirim ke mesin pencari sebelum Anda menekan Enter.

## Cara pakai

- **Address bar.** Ketik sebagian alamat atau judul. Alamat yang sering Anda buka dilengkapi langsung (misalnya `you` menjadi `youtube.com`), dengan bagian tambahan terseleksi. Lanjut mengetik untuk menimpanya; Backspace menghapusnya.
  - ↑ ↓ memilih saran, Enter membuka saran yang disorot, klik juga bisa.
  - Escape pertama menutup daftar saran, Escape kedua mengembalikan alamat tab.
  - Tab menerima pelengkapan.
  - Baris **Pindah ke tab** membuka tab yang sudah terbuka, tanpa memuat ulang. Kalau tab asalnya kosong, tab itu ditutup.
  - Baris terakhir selalu **Cari “…”** di mesin pencari.
- **Jendela Riwayat:** **History > Show All History** (⌘Y). Daftar dikelompokkan per hari, bisa dicari. Klik membuka di tab ini, ⌘-klik atau tombol ↗ membuka di tab baru, tombol ✕ menghapus satu halaman.
- **Hapus Riwayat:** **History > Clear History…** atau tombol di jendela Riwayat. Pilihannya satu jam terakhir, hari ini, hari ini dan kemarin, atau semua riwayat.

## Yang dicatat

Satu entri per halaman, berisi URL, judul, jumlah kunjungan, jumlah kali alamatnya diketik, serta waktu kunjungan pertama dan terakhir (`crates/core/src/history.rs:25`).

- **Halaman SPA ikut tercatat.** Contohnya klik video di YouTube, yang mengganti alamat lewat `history.pushState` tanpa memuat halaman baru. WebKit melaporkan perubahan alamat seperti itu saat halaman tidak sedang loading, dan browser membedakannya dari muat halaman biasa (`crates/engine-webkit/src/observer.rs:49`, `crates/core/src/browser.rs:486`).
- **Yang tidak menambah hitungan:** muat ulang, membangunkan tab yang tidur, memulihkan sesi saat browser dibuka, dan membuka lagi tab yang ditutup. Setiap tab mengingat halaman terakhir yang sudah dicatat (`crates/core/src/history.rs:93`, `crates/core/src/browser.rs:90`, `:295`).
- **URL dibersihkan sebelum dicatat** (`crates/core/src/history.rs:56`):
  - fragment (`#bagian`) dan nama pengguna atau password di URL dibuang;
  - parameter pelacak dibuang: `utm_*`, `fbclid`, `gclid`, `dclid`, `msclkid`, `mc_eid`, `igshid` (`:22`);
  - di halaman tonton YouTube, parameter `t` dibuang. Parameter ini ditambahkan saat tab ditidurkan (lihat `docs/tab-tidur.md`), jadi tanpa pembersihan setiap tidur dan bangun akan membuat entri baru.
- **Tidak dicatat:** alamat selain http/https (`about:`, `data:`, `file:`), URL lebih dari 4096 byte, dan URL yang dialihkan (redirect). Yang tercatat hanya alamat tujuan akhirnya.
- **Ketikan dibedakan dari pencarian dan link.** Alamat yang diketik di address bar menaikkan hitungan ketikan. Pencarian dan link tidak (`crates/core/src/browser.rs:334`).
- Riwayat bisa dimatikan dengan `"history_enabled": false` di `settings.json`.

## Penyimpanan

- Berkas: `~/Library/Application Support/my-browser/history.json`, izin 0600 sejak dibuat (`crates/core/src/persist.rs:26`). Isinya tidak dienkripsi, sama seperti riwayat Safari dan Chrome.
- Batasnya 10.000 halaman dan 90 hari. Halaman yang terakhir dibuka paling lama dibuang lebih dulu (`crates/core/src/history.rs:16`, `:18`). Judul dipotong di 300 karakter.
- Disimpan paling sering sekali per 2 menit selama browsing (`crates/app/src/state.rs:38`), selalu saat keluar, dan langsung setelah ada yang dihapus.
- **Hapus Riwayat menghapus entri utuh** yang terakhir dibuka dalam rentang yang dipilih (`crates/core/src/history.rs:186`). Kunjungan tidak disimpan satu per satu, jadi halaman yang juga dibuka sebelum rentang itu ikut terhapus. Ini sengaja: lebih aman menghapus terlalu banyak daripada terlalu sedikit.
- **Yang tidak ikut terhapus:** tab yang terbuka, daftar mundur/maju di tab, `session.json`, daftar tab yang baru ditutup, cookie, login, dan cache.

## Cara saran dipilih

`crates/core/src/suggest.rs`, dipanggil setiap kali isi address bar berubah.

1. **Baris pertama** adalah tindakan bawaan saat Enter ditekan:
   - halaman yang dilengkapi inline, kalau ada;
   - kalau tidak, "Buka alamat" untuk teks yang berupa alamat;
   - kalau bukan alamat, "Cari “…”".
2. **"Pindah ke tab":** paling banyak 3 tab yang cocok (`:13`).
3. **Halaman dari riwayat**, berperingkat, tanpa duplikat.
4. **"Cari “…”"** di baris terakhir. Totalnya paling banyak 8 baris (`:11`).

Kecocokan (`crates/core/src/suggest.rs:236`), dari yang paling kuat:
1. awal host (`you` ke `youtube.com`);
2. awal alamat tanpa skema (`github.com/serv`);
3. setiap kata ada di awal kata judul atau URL;
4. setiap kata muncul di mana saja. Kata kurang dari 3 huruf tetap harus di awal kata, supaya `ub` tidak mencocokkan `youtube`.

Bobotnya jumlah kunjungan ditambah tiga kali jumlah ketikan, lalu dikurangi menurut umur: penuh sampai 4 hari, lalu 0,7, 0,5, dan 0,3 setelah sebulan (`:263`).

**Pelengkapan inline** (`:277`) hanya untuk teks ASCII tanpa spasi:
- Tanpa `/`: host yang pernah diketik minimal sekali, atau dikunjungi minimal tiga kali (`:341`).
- Dengan `/`: hanya alamat yang pernah diketik (`:300`).

Halaman yang hanya sekali dibuka lewat link tetap muncul sebagai saran, tetapi tidak dilengkapi otomatis. Pelengkapan juga tidak terjadi setelah Backspace atau Delete, saat kursor tidak di akhir teks, dan saat IME (mis. input Jepang) sedang menyusun huruf (`crates/core/src/address_edit.rs`).

**Kecepatan:** pencocokan berjalan di main thread pada setiap ketikan. Dengan 10.000 entri, rata-ratanya 1,3 ms per ketikan di build release, diukur dengan test `suggest_10k_is_fast` (`cargo test --release -p my-browser-core suggest_10k -- --ignored --nocapture`). Versi pertama butuh 31,5 ms karena host dicari linear dan setiap entri dialokasikan ulang. Sekarang host dicari lewat `HashMap` dan pencocokan ASCII tanpa alokasi.

**Daftar saran native:** jendela anak tanpa bingkai di bawah address field, tanpa WebView (`crates/app/src/suggest_popup.rs`). Menampilkannya tidak menambah proses WebContent (diuji E2E). Jendela tanpa bingkai tidak pernah menjadi key window, jadi kursor tetap di address bar.

## Claude dan riwayat

Tool MCP `search_history` (baca saja) mencari riwayat. Parameternya `query` (semua kata harus ada di judul atau URL), `limit` (bawaan 20, maksimal 100), dan `days`. Hasilnya URL, judul, waktu terakhir dibuka dalam ISO 8601 berzona, dan jumlah kunjungan (`crates/app/src/control.rs:534`).

- Tool ini juga tersedia di panel Tanya Claude, yang hanya mendapat tool baca (`crates/core/src/claude_chat.rs:27`).
- Program MCP luar tetap harus diizinkan lewat dialog izin program. Teks dialognya menyebut "membaca riwayat".
- Judul halaman berasal dari situs dan bisa berisi instruksi jahat, jadi hasil tool dan deskripsinya mengingatkan Claude untuk tidak mengikutinya. Panel Tanya Claude tidak bisa membuka URL, jadi riwayat tidak bisa dikirim keluar dari situ.
- Program MCP yang sudah diizinkan sebelum fitur ini ada otomatis bisa membaca riwayat, tanpa ditanya lagi. Cabut lewat **Claude > Revoke MCP Client Access** kalau tidak diinginkan.

## Kode

- `crates/core/src/history.rs`: entri, pembersihan URL, pencatatan, pencarian, batas, berkas.
- `crates/core/src/suggest.rs`: peringkat saran dan pelengkapan inline.
- `crates/core/src/address_edit.rs`: perilaku keyboard address bar (murni, tanpa AppKit).
- `crates/core/src/browser.rs`: kapan kunjungan dicatat, `navigate_from_address`, `open_suggestion`.
- `crates/engine-webkit/src/observer.rs`: membedakan pushState dari muat halaman.
- `crates/app/src/address_bar.rs` dan `suggest_popup.rs`: field editor dan daftar saran.
- `crates/app/src/history_window.rs` dan `.html`: jendela Riwayat dan Hapus Riwayat (`clear_dialog`, `:186`). Judul hanya disisipkan sebagai teks (`history_window.html:91`).

## Pengujian

- **Unit test core:** 14 di `history.rs`, 15 untuk pencatatan dan saran di `browser.rs`, 17 di `suggest.rs` (plus satu benchmark), dan 9 di `address_edit.rs`.
- **Smoke test WebKit:** pushState dan `#fragment` dilaporkan sebagai navigasi dalam dokumen; muat halaman tetap `UrlChanged`.
- **E2E di instance uji (24 dari 24 lulus):**
  - redirect, SPA, dan fragment;
  - reload dan tidur-bangun tidak menambah hitungan;
  - mengetik alamat, inline, Backspace, panah, Escape dua tahap, dan "Pindah ke tab";
  - letak daftar saran, dan proses WebContent yang tidak bertambah;
  - izin 0600 dan isi tetap utuh setelah dibuka ulang;
  - jendela Riwayat, hapus satu entri, dan Hapus Riwayat.
- **Pengait uji khusus build debug:** `debug_ui` di socket kontrol (`crates/app/src/debug_ui.rs`) mengetik lewat `insertText:` dan menekan tombol lewat `doCommandBySelector:` di field editor sungguhan. Pengait ini tidak ada di build release.

## Yang belum terverifikasi

- Ketikan di E2E dikirim lewat field editor (`insertText:` dan `doCommandBySelector:`), bukan keyboard fisik. Mengetik langsung, klik mouse pada baris saran, VoiceOver, dan IME Jepang belum dicoba. Semuanya masuk daftar cek manual.
- Pencatatan klik video di YouTube sungguhan belum dicoba. Yang diuji adalah halaman lokal yang memanggil `pushState`.
- Daftar saran baru dilihat dalam mode gelap.

## Yang tidak dicakup

- **Bookmark** belum ada.
- **Saran dari mesin pencari** sengaja tidak dibuat, karena keputusan pengguna: ketikan tidak boleh dikirim ke luar.
- **Sinkronisasi riwayat** antarperangkat dan **impor riwayat** dari browser lain belum ada.
- **Mode privat** (jendela tanpa riwayat) belum ada; yang ada hanya mematikan riwayat lewat `history_enabled`.
