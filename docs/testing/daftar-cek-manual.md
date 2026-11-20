# Daftar Cek Manual

Dijalankan pada build bundle (`scripts/run.sh`). Checkbox yang gagal diberi catatan alasannya.

## Task 1: Prototipe dan GATE login Google

Tanggal: 2026-11-15. macOS 15.7.7, Safari 18.6, arm64.

- [x] Window terbuka dan halaman YouTube tampil
- [x] Login lewat tombol "Sign in" YouTube (password dan kode HP) diterima Google
- [x] Video bisa diputar
- [x] Tombol layar penuh YouTube berfungsi
- [x] Setelah keluar (⌘Q) dan dibuka lagi (`open dist/my-browser.app`), masih login

Catatan: dicek pengguna pada 2026-11-15, semua lancar. GATE lulus.

## Task 8: UI aplikasi

- [ ] Window 1280x800 terbuka; tidak bisa diperkecil di bawah 800x500
- [ ] Dari atas: nav bar, tab strip, area konten
- [ ] Menutup window mengakhiri aplikasi
- [ ] Tombol mundur/maju aktif hanya saat ada riwayat
- [ ] Ikon muat ulang berganti menjadi silang saat halaman sedang dimuat
- [ ] Address field menampilkan URL tab aktif, kosong untuk tab kosong
- [ ] Mengetik `youtube.com` lalu Enter memuat YouTube
- [ ] Mengetik `resep nasi goreng` lalu Enter membuka pencarian Google
- [ ] Escape di address field mengembalikan URL tab aktif
- [ ] Tab kosong yang aktif membuat address field langsung fokus
- [ ] ⌘V di address field menempel teks
- [ ] Tab strip: judul per tab (dipotong kalau panjang), tombol tutup per tab, tombol plus di ujung
- [ ] Tab aktif ditandai
- [ ] Banyak tab: strip bisa digeser horizontal
- [ ] Judul window mengikuti judul tab aktif
- [ ] Link `target=_blank` membuka tab baru di sebelahnya dan langsung aktif
- [ ] Tombol layar penuh YouTube berfungsi
- [ ] Menu Berkas: Tab Baru ⌘T, Tutup Tab ⌘W, Buka Lagi Tab yang Ditutup ⇧⌘T, Buka Alamat ⌘L
- [ ] Menu Tampilan: Muat Ulang ⌘R, Layar Penuh ⌃⌘F
- [ ] Menu Riwayat: Mundur ⌘[, Maju ⌘]
- [ ] Menu Jendela: Tab Berikutnya ⌃⇥ dan ⇧⌘], Tab Sebelumnya ⌃⇧⇥ dan ⇧⌘[, ⌘1 sampai ⌘9

## Task 9: Tab tidur, memory pressure, sesi

Untuk uji manual, tutup browser dulu (⌘Q), set `"sleep_idle_secs": 20` di `~/Library/Application Support/my-browser/settings.json`, lalu buka lagi. Kembalikan ke 300 dengan cara yang sama setelah selesai.

- [ ] (a) Buka 4 tab ber-URL, aktifkan satu, tunggu 60 detik: tab latar tampil tidur (redup, tooltip "Tidur, dimuat ulang saat dibuka")
- [ ] (b) Video YouTube diputar di tab latar: tab itu tidak pernah tidur
- [ ] (c) Video YouTube di-pause di menit ke-2, ditidurkan, lalu dibuka: lanjut dari sekitar menit ke-2
- [ ] (d) Halaman yang di-scroll jauh, ditidurkan, lalu dibuka: posisi scroll kembali
- [ ] (e) Pengguna menjalankan `sudo memory_pressure -S -l warn`: semua tab latar tidur dalam beberapa detik
- [ ] (f) ⌘Q lalu buka lagi: semua tab kembali, hanya tab aktif yang dimuat

## Task 10: Pemblokir iklan

- [ ] https://www.detik.com menampilkan lebih sedikit iklan dibanding Safari tanpa content blocker
- [ ] Tampilan > Matikan Pemblokir untuk Situs Ini: halaman dimuat ulang dan iklan muncul lagi; centang menu ikut berubah
- [ ] Menu itu nonaktif di tab kosong
- [ ] YouTube masih memutar video dan login tetap ada
- [ ] Iklan video YouTube yang bisa dilewati menampilkan tombol "Lewati" (perbaikan 2026-11-20)
- [ ] github.com dan Gmail tampil utuh (aturan sembunyi umum tidak berlaku di situs generichide)
- [ ] Skip iklan YouTube otomatis: iklan yang bisa dilewati hilang dalam sekitar 1 detik; iklan yang tidak bisa dilewati bisu dan selesai dalam 1-2 detik
- [ ] Setelah iklan, kecepatan video (mis. 1,5x) dan suara kembali seperti sebelumnya
- [ ] Tampilan > Matikan Pemblokir untuk Situs Ini di YouTube: iklan diputar normal lagi; nyalakan lagi: iklan dilewati lagi

## Task 11: Download

Sudah diverifikasi otomatis (folder Downloads sementara): dua download bersamaan bernama sama menjadi `gitignore-main.zip` dan `gitignore-main (1).zip`; tab yang navigasinya berubah menjadi download tidak masuk sesi dan tidak mengunduh ulang saat aplikasi dibuka lagi.

- [ ] Mengunduh sebuah PDF lewat link `download` dua kali menghasilkan `nama.pdf` dan `nama (1).pdf` di `~/Downloads`
- [ ] Label "Selesai diunduh: ..." muncul di kanan nav bar lalu hilang setelah sekitar 6 detik

## Perbaikan setelah review akhir

- [ ] Tab kosong (tab pertama atau ⌘T): mengetik URL lalu Enter langsung menampilkan halamannya
- [ ] Klik tab berulang kali (termasuk tab yang sudah aktif) dan tutup tab lewat tombol silang: tidak crash, tanda tab aktif tetap benar
- [ ] Upload berkas (mis. form upload gambar di situs mana pun): jendela pilih berkas muncul
- [ ] Dialog `alert`, `confirm`, dan `prompt` muncul (mis. ketik `javascript` tidak berlaku di address bar; pakai situs uji seperti https://www.w3schools.com/js/tryit.asp?filename=tryjs_alert)
- [ ] Membuka `localhost:59999` (server mati): muncul "Gagal memuat halaman: ..." di kanan nav bar
- [ ] Mulai download lalu langsung tutup tabnya: pemberitahuan "Selesai diunduh" tetap muncul
- [ ] Pengecualian pemblokir dari subdomain (mis. aktifkan di www.detik.com, buka news.detik.com, matikan lagi): pemblokir benar-benar aktif kembali

## MCP Claude

Buka ulang browser dengan build baru (⌘Q lalu `open dist/my-browser.app`), lalu mulai sesi Claude Code baru.

- [ ] `/mcp` di Claude Code menampilkan my-browser terhubung
- [ ] Minta Claude "buka youtube.com di tab baru": tab terbuka di browser
- [ ] Minta Claude membaca halaman aktif dan membuat screenshot
- [ ] Minta Claude mengklik sesuatu: ditolak dengan pesan untuk menyalakan izin
- [ ] Nyalakan Claude > Izinkan Claude Berinteraksi: judul window berpenanda, klik berhasil, pemberitahuan "Claude: klik elemen ..." muncul
- [ ] Matikan lagi: klik ditolak lagi
- [ ] Tutup browser (⌘Q), lalu minta Claude melihat daftar tab: browser terbuka otomatis
