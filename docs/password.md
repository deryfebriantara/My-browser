# Pengelola password

my-browser menyimpan password dalam vault terenkripsi yang hanya bisa dibuka dengan master password Anda.

## Cara pakai

- **Menyimpan:** login di sebuah situs seperti biasa. Setelah form dikirim, muncul tawaran "Simpan password untuk github.com?". Pilihannya **Simpan**, **Jangan Sekarang**, atau **Jangan untuk Situs Ini**. Pertama kali menyimpan, Anda diminta membuat master password (minimal 10 karakter).
- **Mengisi:** di halaman login, tekan ⌘\ atau pilih **Passwords > Fill Password**. Kalau ada beberapa akun untuk situs itu, pilih salah satu. Login bertahap (misalnya Google): tekan ⌘\ di langkah username, lalu sekali lagi di langkah password.
- **Kelola:** **Passwords > Manage Passwords…** (⌥⌘P) untuk mencari, melihat, menyalin, menambah, mengedit, dan menghapus password, impor dan ekspor CSV, serta mengganti master password.
- **Password acak:** **Passwords > Copy Random Password** menyalin password 20 karakter ke clipboard, misalnya untuk form daftar. Di jendela Kelola, tombol **Buat acak** mengisi kolom password.
- **Kunci:** **Passwords > Lock Passwords**.

## Enkripsi

| Bagian | Cara |
|---|---|
| Master password → kunci | Argon2id, 256 MiB, 3 putaran, salt acak 16 byte (sekitar 0,3 detik di Mac Apple Silicon). Memory sebesar itu membuat tebakan massal dengan GPU sangat mahal |
| Kunci vault | 32 byte acak, dibungkus XChaCha20-Poly1305 dengan kunci dari master password |
| Isi vault | Dienkripsi XChaCha20-Poly1305 dengan kunci vault, memakai nonce 24 byte acak baru setiap kali disimpan |
| Integritas | Versi dan parameter berkas diikat sebagai associated data; berkas yang diubah sedikit pun gagal dibuka |
| Berkas | `~/Library/Application Support/my-browser/vault.json`, izin 0600, ditulis atomik. Tidak ada situs, username, atau password dalam teks terang |
| Pustaka | RustCrypto (`argon2`, `chacha20poly1305`, `zeroize`) |

Kunci vault dibungkus terpisah, jadi mengganti master password tidak perlu mengenkripsi ulang isi. Cara buka lain (Touch ID lewat Secure Enclave) nanti cukup menambah satu bungkusan lagi.

## Perlindungan lain

- **Situs tiruan:** password hanya ditawarkan untuk origin yang persis sama (skema, host, port). `github.com` tidak cocok dengan `github.com.evil.example`, `http://github.com`, atau `www.github.com`.
- **Tidak mengisi otomatis:** pengisian hanya terjadi setelah Anda memintanya (⌘\), tidak pernah saat halaman dimuat. Dengan begitu, form tersembunyi tidak bisa menyedot password. Pengisian hanya di frame utama, dan origin halaman dicek lagi tepat sebelum mengisi.
- **Script halaman tidak bisa ikut campur:** penangkap login dan pengisi berjalan di content world WebKit terpisah. Script situs tidak bisa melihat kodenya maupun mengirim pesan palsu. Origin penyimpan ditentukan WebKit dari frame, bukan dari isi halaman.
- **Terkunci otomatis:** setelah 10 menit tidak dipakai (`password_lock_minutes`), saat layar tidur atau dikunci, saat berganti pengguna, dan saat Mac tidur. Saat terkunci, kunci dan entri dihapus dari memory.
- **Clipboard:** password yang disalin ditandai "concealed", supaya pengelola clipboard tidak mencatatnya, dan dibersihkan setelah 30 detik kalau clipboard belum diganti.
- **Ekspor dan ganti master password** selalu meminta master password lagi, walaupun vault sedang terbuka.
- **Claude tidak bisa mengakses vault:** tidak ada tool MCP atau perintah socket untuk password. `run_js` ditolak di halaman yang kolom password-nya terisi, dan `list_elements` tidak pernah mengirim isi kolom password.
- Kolom master password memakai NSSecureTextField: selama aktif, ketikan tidak bisa dibaca aplikasi lain.

## Batasan yang perlu diketahui

- **Master password tidak bisa dipulihkan.** Kalau lupa, isi vault hilang. Simpan di tempat aman.
- Halaman yang sudah diisi password bisa membaca isian itu. Ini berlaku di semua password manager.
- Upaya menghapus rahasia dari memory adalah upaya terbaik: Rust dan WebKit bisa menyimpan salinan sementara yang tidak bisa dihapus paksa. Password yang ditampilkan dengan **Lihat** disembunyikan lagi setelah 20 detik.
- Belum ada sinkronisasi ke perangkat lain. Passkey belum didukung (butuh izin khusus Apple untuk browser). Form login di dalam iframe belum diisi.
- Saat vault terkunci, akun yang tersimpan dikenali dari pembukaan terakhir (hanya di memory). Setelah browser dibuka ulang, login pertama bisa memunculkan tawaran simpan lagi; memilih Simpan akan meminta master password lalu memberi tahu bahwa password itu sudah tersimpan.
- Host di "Jangan untuk Situs Ini" disimpan di `settings.json` tanpa enkripsi (hanya nama situs).

## Impor dari browser lain

Di jendela **Passwords > Manage Passwords…**, tombol **Impor CSV** menerima ekspor Chrome, Safari/app Passwords (File > Ekspor), Firefox, dan Bitwarden. Akun yang sama (situs dan username) diperbarui passwordnya. Setelah impor, hapus berkas CSV itu (juga dari Sampah), karena isinya password tanpa enkripsi.

## Rencana: Touch ID

Touch ID yang benar-benar melindungi kunci (lewat Secure Enclave) butuh aplikasi yang ditandatangani sertifikat Apple Developer dengan entitlement keychain. Aplikasi yang ditandatangani ad-hoc ditolak dengan error `-34018` (sudah dicoba). Setelah sertifikat tersedia (akun gratis "Personal Team" di Xcode kemungkinan cukup untuk pemakaian sendiri), kunci vault bisa dibungkus kunci Secure Enclave sebagai cara buka kedua, dengan master password tetap sebagai cadangan.

## Pengaturan (`settings.json`)

| Kunci | Bawaan | Fungsi |
|---|---|---|
| `password_offer_save` | `true` | Tawarkan simpan setelah login |
| `password_lock_minutes` | `10` | Kunci setelah sekian menit tidak dipakai |
| `password_never_save` | `[]` | Host yang tidak pernah ditawari |

## Kode

- `crates/vault`: format berkas, kriptografi, entri, generator, CSV (tanpa dependensi macOS).
- `crates/core/src/password_capture.js`, `password_fill.js`: penangkap dan pengisi login.
- `crates/engine-webkit/src/passwords.rs`: penerima pesan dari content world password.
- `crates/app/src/passwords.rs`: tawaran simpan, isi, kunci otomatis, clipboard.
- `crates/app/src/password_manager.rs` dan `.html`: jendela Kelola Password.
- `crates/app/src/dialogs.rs`: dialog master password dan konfirmasi.

Untuk pengujian, build debug punya pengait `MYBROWSER_DIALOG_SELFTEST=<master>`, yang menjawab setiap dialog otomatis dengan master password itu. Pengait ini tidak ada di build release.
