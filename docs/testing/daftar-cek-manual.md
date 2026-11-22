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

Untuk uji manual, tutup browser dulu (⌘Q), set `"sleep_idle_secs": 20` di `~/Library/Application Support/my-browser/settings.json`, lalu buka lagi. Kembalikan ke 1800 (bawaan, 30 menit) dengan cara yang sama setelah selesai.

- [ ] (a) Buka 4 tab ber-URL, aktifkan satu, tunggu 60 detik: tab latar tampil tidur (redup, tooltip "Tidur, dimuat ulang saat dibuka")
- [ ] (b) Video YouTube diputar di tab latar: tab itu tidak pernah tidur
- [ ] (c) Video YouTube di-pause di menit ke-2, ditidurkan, lalu dibuka: lanjut dari sekitar menit ke-2
- [ ] (d) Halaman yang di-scroll jauh, ditidurkan, lalu dibuka: posisi scroll kembali
- [ ] (e) Pengguna menjalankan `sudo memory_pressure -S -l warn`: semua tab latar tidur dalam beberapa detik, kecuali tab yang punya teks belum dikirim
- [ ] (f) ⌘Q lalu buka lagi: semua tab kembali, hanya tab aktif yang dimuat
- [ ] (g) Ketik komentar di YouTube atau form mana pun tanpa mengirimnya, pindah tab, tunggu lewat batas tidur: tab itu tetap bangun dan komentarnya utuh (perbaikan 2026-11-21)
- [ ] (h) Komentar dari (g) dikirim atau dihapus: tab itu tidur lagi di putaran berikutnya
- [ ] (i) Ketik di kotak pencarian YouTube lalu pilih saran (tanpa Enter): tab tetap bisa tidur
- [ ] (j) `sudo memory_pressure -S -l critical`: tab dari (g) ikut ditidurkan

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
- [ ] Minta Claude membuat website kecil, menjalankannya di localhost, lalu memeriksanya: Claude memakai console_logs, network_log, dan screenshot, lalu memperbaiki error yang ditemukan
- [ ] Pilih Claude > Tunjuk Elemen ke Claude (⌘⇧E), arahkan kursor: elemen disorot dengan label ukuran; klik sebuah tombol: tombolnya tidak ikut jalan, tanda "Terpilih" muncul sebentar. Lalu minta Claude "lihat elemen yang saya pilih": Claude menyebut elemen yang benar dan bisa melihat gambarnya
- [ ] Mode Tunjuk Elemen lalu tekan Esc: sorotan hilang, klik berikutnya berjalan normal
- [ ] Minta Claude "cek tampilan HP dan mode gelap": halaman tampil selebar HP di tengah atas jendela, pemberitahuan "Claude: HP 390×844, mode gelap" muncul; setelah Claude selesai (atau lewat menu Claude > Kembalikan Tampilan Normal) tampilan kembali penuh
- [ ] Menu Kembalikan Tampilan Normal nonaktif (abu-abu) di tab yang tidak diemulasi
- [ ] Minta Claude "screenshot seluruh halaman" di halaman panjang: Claude menerima beberapa gambar dan bisa menjelaskan bagian bawah halaman
- [ ] Minta Claude memakai check_page setelah mengubah kode situs lokal: satu panggilan berisi ringkasan error, request gagal, dan screenshot

## Tanya Claude

Buka ulang browser dengan build baru (⌘Q lalu `open dist/my-browser.app`).

- [ ] Tombol ✦ di kanan nav bar dan ⌘J membuka panel di kanan; halaman menyempit (tidak tertutup panel); ⌘J lagi menutupnya
- [ ] Di YouTube atau artikel, tanya "ringkas halaman ini": jawaban muncul bertahap, label "Membaca halaman" tampil di atas jawaban
- [ ] Tunjuk sebuah elemen (⌘⇧E), lalu tanya "jelaskan elemen yang saya tunjuk": Claude membahas elemen yang benar
- [ ] Minta "buka youtube.com": Claude menolak, tidak ada tab baru
- [ ] Tutup panel saat Claude sedang menjawab: jawaban berhenti; buka lagi: riwayat masih ada dan pertanyaan berikutnya nyambung dengan percakapan sebelumnya
- [ ] Monitor Aktivitas: proses `claude` hanya ada selama panel terbuka dan sudah dipakai
- [ ] Klik link di jawaban Claude: terbuka di tab baru
- [ ] Mode terang dan gelap macOS: panel ikut berganti warna

## Pengelola password

Buka ulang browser dengan build baru (⌘Q lalu `open dist/my-browser.app`).

- [ ] Login di sebuah situs: muncul "Simpan password untuk …?"; Simpan meminta membuat master password (ditolak kalau < 10 karakter atau kedua isian beda)
- [ ] Keluar lalu buka halaman login lagi, tekan ⌘\: username dan password terisi, situs bisa dipakai login
- [ ] Password > Kunci Password, lalu ⌘\: diminta master password; master password salah ditolak dengan pesan, yang benar langsung mengisi
- [ ] Login dengan password berbeda: muncul "Perbarui password untuk …?"
- [ ] "Jangan untuk Situs Ini": login berikutnya di situs itu tidak ditanya lagi
- [ ] Kelola Password: cari, Lihat (tersembunyi lagi setelah 20 detik), Salin (clipboard kosong lagi setelah 30 detik), Edit, Hapus (minta konfirmasi), Tambah dengan Buat acak
- [ ] Impor CSV dari Chrome atau app Passwords: jumlah yang diimpor sesuai, muncul pengingat untuk menghapus berkas CSV
- [ ] Ekspor CSV: meminta master password lagi; master password salah membatalkan ekspor
- [ ] Ganti master password: master password lama salah ditolak; setelah diganti, kunci lalu buka dengan yang baru berhasil dan yang lama ditolak
- [ ] Kunci layar Mac (Ctrl+Cmd+Q) lalu buka lagi: password terkunci
- [ ] Biarkan 10 menit tanpa memakai password: terkunci sendiri
- [ ] Login Google dua langkah: ⌘\ di langkah email, lalu di langkah password

## Izin program MCP

Buka ulang browser dengan build baru (⌘Q lalu `open dist/my-browser.app`).

- [ ] Mulai sesi Claude Code baru dan minta Claude melihat daftar tab: my-browser maju ke depan dengan dialog "Izinkan Claude Code mengontrol my-browser?" yang menampilkan folder proyek sesi itu
- [ ] Selalu Izinkan: Claude langsung mendapat daftar tab; sesi Claude Code berikutnya tidak ditanya lagi
- [ ] Claude > Cabut Izin Program MCP: permintaan berikutnya ditanya lagi
- [ ] Tolak: Claude mendapat pesan penolakan; permintaan berikutnya langsung ditolak sampai browser dibuka ulang
- [ ] Panel Tanya Claude tetap bisa membaca halaman tanpa dialog izin
- [ ] Panel menampilkan email, paket, dan profil di bawah judul; pilihan profil mengganti akun dan memulai percakapan baru
- [ ] Pilih profil yang belum login (mis. ~/.claude-flash): muncul Masuk ke Claude; Masuk membuka halaman login di tab my-browser; setelah disetujui tab login tertutup sendiri dan panel siap dipakai
- [ ] Batal saat menunggu login: kembali ke kartu Masuk
- [ ] Keluar: diminta konfirmasi dengan peringatan bahwa Claude Code di Terminal dengan profil yang sama ikut keluar

## Riwayat dan saran address bar

Buka ulang browser dengan build baru (⌘Q lalu `open dist/my-browser.app`). Riwayat mulai tercatat sejak build ini.

- [ ] Buka beberapa situs, lalu ketik awal alamat yang tadi diketik (mis. `you`): alamat dilengkapi dengan bagian tambahan terseleksi; mengetik lanjut menimpanya, Backspace menghapusnya
- [ ] Daftar saran tampil rapi di bawah address bar dalam mode terang dan gelap, dan ikut bergeser saat jendela dipindah
- [ ] ↑ ↓ memilih saran dan mengisi address bar; Enter membuka; klik mouse pada baris juga membuka
- [ ] Escape pertama menutup daftar, Escape kedua mengembalikan alamat tab
- [ ] Ketik judul tab lain yang sedang terbuka: muncul "Pindah ke tab"; memilihnya pindah ke tab itu tanpa memuat ulang
- [ ] Klik beberapa video di YouTube (tanpa memuat ulang halaman): setiap video muncul di Riwayat dengan judul yang benar
- [ ] Video yang ditidurkan lalu dibuka lagi tidak membuat entri baru (tidak ada `&t=` di Riwayat)
- [ ] ⌘Y membuka jendela Riwayat yang dikelompokkan per hari; cari, ⌘-klik membuka di tab baru, ✕ menghapus satu halaman
- [ ] Riwayat > Hapus Riwayat… > Satu jam terakhir: halaman tadi hilang dari Riwayat dan dari saran address bar
- [ ] IME Jepang (atau input lain yang menyusun huruf) di address bar tidak dilengkapi selama menyusun
- [ ] VoiceOver membacakan baris saran
- [ ] Minta Claude "video YouTube apa yang saya tonton hari ini?": Claude memakai search_history
