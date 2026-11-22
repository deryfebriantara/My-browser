# Mengontrol my-browser dari Claude Code (MCP)

Claude Code bisa melihat dan mengatur tab, membaca halaman, membuat screenshot, dan (dengan izin) berinteraksi dengan halaman di my-browser.

## Pemasangan

```sh
scripts/pasang-mcp.sh            # profil ~/.claude
scripts/pasang-mcp.sh ~/.claude-work   # profil lain
```

Skrip ini mendaftarkan `dist/my-browser.app/Contents/MacOS/my-browser --mcp` sebagai server MCP scope user. Mulai sesi Claude Code baru supaya tool-nya muncul (cek dengan `/mcp`). Kalau browser belum berjalan saat tool dipanggil, server MCP membukanya otomatis.

## Cara kerja

```
Claude Code ──stdio JSON-RPC──▶ my-browser --mcp ──control.sock──▶ my-browser (jendela)
```

- `my-browser --mcp` adalah binary yang sama dalam mode tanpa jendela (crate `crates/mcp`).
- Browser mendengarkan di `~/Library/Application Support/my-browser/control.sock` (mode 0600, hanya pengguna yang sama). Tidak ada port jaringan.
- Protokol antar keduanya: `crates/core/src/control.rs`, satu baris JSON per permintaan.

## Tool

| Tool | Fungsi | Butuh izin interaksi |
|---|---|---|
| `list_tabs`, `open_tab`, `close_tab`, `activate_tab` | Mengatur tab | Tidak |
| `navigate`, `go_back`, `go_forward`, `reload` | Navigasi, menunggu halaman selesai dimuat | Tidak |
| `sleep_tab`, `memory_usage` | Menidurkan tab, memory per tab | Tidak |
| `search_history` | Mencari riwayat halaman yang pernah dibuka: URL, judul, waktu terakhir, jumlah kunjungan (lihat `docs/riwayat.md`) | Tidak |
| `read_page`, `list_elements`, `scroll` | Membaca halaman | Tidak |
| `screenshot` | Layar tab aktif; `full_page` untuk seluruh halaman (dipotong per 1600 px, maks 6 gambar); `selector` untuk satu elemen (elemen kecil diperbesar 2x) | Tidak |
| `emulate` | Meniru HP (390×844 + user agent iPhone), tablet, desktop, ukuran khusus, mode gelap/terang; `device=reset` untuk kembali | Tidak |
| `get_selection` | Elemen yang Anda tunjuk lewat **Claude > Tunjuk Elemen ke Claude**: selector, style, HTML, dan gambarnya | Tidak |
| `check_page` | Satu panggilan: reload (atau buka url), tunggu, error console, request gagal, screenshot | Tidak |
| `youtube` | Status, play, pause, seek, kecepatan | Tidak |
| `console_logs` | Console, error tak tertangkap (sumber dan stack), promise gagal | Tidak |
| `network_log` | Request fetch/XHR: status, durasi, error; isi respons untuk situs lokal | Tidak |
| `wait_for` | Menunggu elemen atau teks muncul, terlihat, atau hilang | Tidak |
| `inspect_element` | Posisi, ukuran, computed style, outerHTML satu elemen | Tidak |
| `click`, `type`, `run_js` | Berinteraksi dengan halaman (`run_js` menunggu Promise) | **Ya** |

## Membangun website bersama Claude

Minta Claude membuat atau memperbaiki website, misalnya "buatkan landing page kedai kopi, jalankan, dan cek di browser". Alur yang dipakai Claude:

1. Menulis kode dan menjalankan server lokal (`npm run dev`, `python3 -m http.server`, dan sejenisnya).
2. `navigate` ke `http://localhost:<port>`.
3. Setelah setiap perubahan: `check_page`. Isinya sama dengan `reload` lalu `console_logs` (warn dan error), `network_log` (`failed_only`), dan `screenshot`, tetapi dalam satu panggilan. Untuk halaman yang memuat data belakangan, isi `wait_for` dengan selector yang ditunggu.
4. Tampilan di layar lain: `emulate` dengan `device=mobile` atau `tablet`, dan `color_scheme=dark`. Setelah selesai, `emulate device=reset`. Anda melihat perubahannya di jendela: halaman tampil selebar HP di tengah atas. Kalau Claude lupa mengembalikan, pakai menu **Claude > Kembalikan Tampilan Normal**.
5. Detail tampilan: `screenshot` dengan `selector`, `full_page`, atau `inspect_element`.

### Menunjuk elemen ke Claude

Daripada menjelaskan "tombol oranye di bawah judul", pilih **Claude > Tunjuk Elemen ke Claude** (⌘⇧E). Elemen di bawah kursor disorot; klik elemen yang dimaksud (Esc untuk batal). Klik ini tidak diteruskan ke halaman, jadi tombol dan link tidak ikut jalan. Setelah itu cukup bilang ke Claude, misalnya "perbesar elemen yang saya pilih". Claude memanggil `get_selection` dan menerima selector unik, teks, posisi, computed style, outerHTML, dan potongan gambar elemen itu.

### Catatan screenshot

Screenshot biasa adalah foto layar tab aktif, termasuk video. `full_page`, `selector`, dan gambar `get_selection` dirender ulang dari halaman lewat PDF WebKit, sehingga bagian di luar layar ikut tergambar dan bisa dipakai untuk tab latar. Video dan canvas WebGL bisa tampil kosong di gambar jenis ini, dan gambar yang lazy-load di bawah layar mungkin belum dimuat.

Pencatat console dan network terpasang di awal setiap halaman dan dibatasi jumlahnya (200 catatan console, 300 request). Isi respons hanya disimpan untuk situs lokal (`localhost`, IP lokal, `.local`, `.test`, `file:`), karena isi respons situs sungguhan bisa berisi data pribadi. Mesinnya WebKit, jadi hasilnya sekaligus menunjukkan perilaku di Safari.

## Izin program (auth MCP)

Socket kontrol hanya bisa dibuka oleh user macOS yang sama. Itu berarti program apa pun yang Anda jalankan, jadi setiap program juga harus diizinkan dulu.

- Saat sebuah program tersambung pertama kali, my-browser maju ke depan dan bertanya: "Izinkan Claude Code mengontrol my-browser?". Dialog itu menampilkan lokasi program, pid, dan folder kerjanya, sehingga Anda bisa mengenali sesi Claude Code mana yang meminta. Pilihannya:
  - **Selalu Izinkan:** lokasi program disimpan di `mcp_allowed_clients` di `settings.json`.
  - **Izinkan Sekali:** berlaku sampai koneksi itu putus.
  - **Tolak:** berlaku sampai browser ditutup.
- Permintaan pertama menunggu jawaban Anda sampai 35 detik. Kalau Anda menjawab setelah itu, jawabannya tetap berlaku untuk permintaan berikutnya.
- **Identitas dari kernel**, bukan dari pengakuan program. Pid peer socket menunjuk ke `my-browser --mcp`, dan yang dianggap meminta akses adalah induk proses itu (misalnya `claude`). Program asing yang menjalankan `my-browser --mcp` sendiri akan tampil dengan namanya sendiri.
- **Panel Tanya Claude** dikenali dari silsilah proses (proses Claude-nya adalah anak browser ini), jadi tidak ditanya. Browser juga membatasinya ke perintah baca saja, sebagai lapisan kedua di atas daftar tool panel.
- Program yang diizinkan bisa membaca riwayat lewat `search_history`, dan teks dialognya menyebut hal itu. Program yang sudah diizinkan sebelum tool ini ada tidak ditanya ulang.
- Menu **Claude > Cabut Izin Program MCP** menghapus semua izin; setiap program akan ditanya lagi.
- Izin disimpan per lokasi program. Setelah Claude Code diperbarui ke lokasi lain (misalnya versi Node di nvm berganti), Anda akan ditanya sekali lagi.

## Izin interaksi

`click`, `type`, dan `run_js` hanya berjalan setelah menu **Claude > Izinkan Claude Berinteraksi** dinyalakan. Izin ini selalu mati setiap browser dibuka. Selama menyala, judul window diberi penanda "Claude dapat berinteraksi" dan setiap aksi Claude tampil di kanan nav bar.

Alasannya: login Anda (Google dan lainnya) ada di browser ini, dan teks halaman yang dibaca Claude bisa berisi instruksi jahat. Nilai input password tidak pernah dikirim oleh `list_elements`.
