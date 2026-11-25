# Jaringan: HTTP/3 dimatikan

my-browser memakai HTTP/2 (lewat TCP), bukan HTTP/3 (QUIC lewat UDP). Pengaturannya `http3_enabled` di `settings.json`, bawaannya `false` (`crates/core/src/settings.rs`).

## Alasan

Pada 2026-11-25, Hostinger Mail tampil putih setelah login. Pemeriksaan lewat MCP di tab yang bermasalah menemukan:

- Login berhasil (API `auth/me` membalas 200), tetapi sekitar 170 file JS dan CSS aplikasi gagal dimuat pada milidetik yang sama, lalu aplikasi berhenti ("Router initialization failed") dan memuat ulang dirinya.
- File-file itu ada di server, kodenya lolos pemeriksaan sintaks mesin JavaScript Safari, dan pemblokir iklan tidak memblokirnya.
- Koneksinya memakai HTTP/3. Dari tab itu, 30 request bersamaan ke file statis Hostinger gagal semua, dan request berurutan pun kadang gagal.
- Dari Mac yang sama, 60 request bersamaan lewat HTTP/2 (curl) berhasil semua.
- my-browser memakai HTTP/3 karena petunjuk `Alt-Svc` dari Cloudflare yang disimpan di `~/Library/Caches/local.my-browser/WebKit/AlternativeServices/AlternativeService.sqlite`. Setelah petunjuk itu dihapus, Inbox langsung terbuka, lalu putih lagi setelah petunjuknya tercatat ulang.

Kesimpulannya, jaringan pengguna (gateway CGNAT, tanpa VPN di jalurnya) tidak andal untuk QUIC saat banyak request dikirim bersamaan. Safari tidak terkena, dan penyebab pastinya di Safari tidak diperiksa.

## Cara kerja

WebKit tidak punya sakelar publik maupun SPI untuk mematikan HTTP/3 di WKWebView pada macOS 15.7 (diperiksa lewat runtime). Yang tersedia adalah tipe data situs `_WKWebsiteDataTypeAlternativeServices`. Saat `http3_enabled` mati, `Engine::forget_http3_hints` menghapus data itu dari data store bawaan, baik di memory proses jaringan maupun di berkasnya (`crates/engine-webkit/src/lib.rs`):

- saat browser dibuka;
- setiap tick (30 detik);
- sebelum setiap navigasi dan sebelum tab dibangunkan (`crates/core/src/browser.rs`).

Dengan begitu koneksi baru tidak pernah memakai HTTP/3. Koneksi yang sedang berjalan tidak terpengaruh.

## Verifikasi

Dua instance uji, masing-masing membuka mail.hostinger.com, ditutup, lalu dibuka lagi:

| Pengaturan | Protokol muatan kedua |
|---|---|
| `http3_enabled: true` | h3 |
| bawaan (`false`) | h2 |

Test core: `http3_hints_forgotten_at_start_tick_and_navigation_when_disabled` dan `http3_hints_kept_when_enabled` (`crates/core/src/browser/tests.rs`).

## Yang belum terverifikasi dan batasan

- Kegagalan HTTP/3 tidak muncul di instance uji saat verifikasi (30 dari 30 request berhasil), jadi gangguannya bersifat sesekali. Bukti kegagalannya berasal dari tab pengguna.
- Situs yang mengiklankan HTTP/3 lewat DNS (record HTTPS/SVCB) mungkin tetap memakai HTTP/3, karena itu tidak lewat `Alt-Svc`. mail.hostinger.com tidak termasuk.
- Kalau jaringan sudah baik, HTTP/3 bisa dinyalakan lagi dengan `"http3_enabled": true` (tutup browser dulu sebelum mengedit `settings.json`).
