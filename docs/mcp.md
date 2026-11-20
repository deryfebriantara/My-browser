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
| `read_page`, `list_elements`, `scroll`, `screenshot` | Membaca halaman | Tidak |
| `youtube` | Status, play, pause, seek, kecepatan | Tidak |
| `click`, `type`, `run_js` | Berinteraksi dengan halaman | **Ya** |

## Izin interaksi

`click`, `type`, dan `run_js` hanya berjalan setelah menu **Claude > Izinkan Claude Berinteraksi** dinyalakan. Izin ini selalu mati setiap browser dibuka. Selama menyala, judul window diberi penanda "Claude dapat berinteraksi" dan setiap aksi Claude tampil di kanan nav bar.

Alasannya: login Anda (Google dan lainnya) ada di browser ini, dan teks halaman yang dibaca Claude bisa berisi instruksi jahat. Nilai input password tidak pernah dikirim oleh `list_elements`.
