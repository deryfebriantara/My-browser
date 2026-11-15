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
