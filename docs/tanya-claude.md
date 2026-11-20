# Tanya Claude: chat di samping halaman

Panel chat di kanan jendela untuk bertanya tentang halaman yang sedang dibuka, misalnya "ringkas artikel ini", "bandingkan dengan tab sebelah", atau "kenapa tombol ini berantakan di HP?". Jawaban datang dari Claude Code di Mac Anda, jadi memakai kuota langganan Claude yang sama, tanpa API key.

## Cara pakai

- Buka panel lewat tombol ✦ di kanan nav bar, menu **Claude > Tanya Claude**, atau ⌘J. Tutup dengan cara yang sama.
- Ketik pertanyaan lalu Enter (Shift+Enter untuk baris baru). Tombol **Berhenti** atau Esc menghentikan jawaban.
- **Baru** memulai percakapan baru.
- Setiap pertanyaan otomatis membawa judul dan URL tab aktif, jadi "halaman ini" selalu merujuk ke tab yang sedang Anda lihat.
- Untuk bertanya tentang satu elemen: pilih elemennya dulu lewat **Claude > Tunjuk Elemen ke Claude** (⌘⇧E), lalu tanya "jelaskan elemen yang saya tunjuk".

## Yang bisa dan tidak bisa dilakukan Claude di panel

Bisa: membaca teks halaman, memotret tab, melihat daftar tab, memeriksa elemen dan elemen yang ditunjuk, membaca console dan log jaringan, menggulir, mengontrol video YouTube.

Tidak bisa: membuka URL atau tab, mengklik, mengetik di halaman, menjalankan JavaScript, mengubah file, menjalankan perintah terminal, atau membuka web. Claude dijalankan tanpa tool bawaan (`--tools ""`), hanya dengan tool baca my-browser, dan tool lain ditolak otomatis (`--permission-prompts none`).

Alasannya: teks halaman yang dibaca Claude bisa berisi instruksi jahat. Dengan akses baca saja, instruksi seperti itu tidak bisa membuat Claude membuka situs lain (misalnya untuk mengirim isi halaman ke luar) atau menyentuh file Anda. Panelnya sendiri juga dikunci: tidak memuat apa pun dari internet (CSP `default-src 'none'`), dan link di jawaban dibuka di tab baru, bukan di panel.

## Memory

Proses Claude Code (sekitar 270 MB) baru dijalankan saat pesan pertama dikirim, dan dimatikan saat panel ditutup. Riwayat chat tetap tampil saat panel dibuka lagi, dan percakapannya dilanjutkan dengan `claude --resume`. Selama panel tertutup tidak ada proses Claude yang berjalan. Riwayat hilang saat browser ditutup.

## Pengaturan (`settings.json`)

| Kunci | Bawaan | Fungsi |
|---|---|---|
| `claude_config_dir` | `~/.claude` | Profil Claude Code (`CLAUDE_CONFIG_DIR`), misalnya `~/.claude-work` |
| `claude_path` | dicari otomatis | Lokasi binary `claude`. Dicari di `~/.local/bin`, `~/.claude/local`, Homebrew, lalu nvm (versi Node terbaru) |
| `claude_model` | model bawaan Claude Code | Misalnya `sonnet` untuk jawaban yang lebih cepat |

Lokasi berkas: `~/Library/Application Support/my-browser/settings.json`. Tutup browser dulu sebelum mengedit.

## Kalau muncul "Not logged in" atau "Failed to authenticate"

Login Claude Code untuk profil itu belum ada atau kedaluwarsa. Buka Terminal, jalankan `CLAUDE_CONFIG_DIR=~/.claude claude` (sesuaikan dengan profilnya), lalu ketik `/login`. Panel sengaja selalu memasang `CLAUDE_CONFIG_DIR`, karena tanpa variabel itu Claude Code membaca login bawaan yang bisa berbeda dari profil yang Anda pakai di Terminal.

## Cara kerja

```
panel (WKWebView lokal) ──pesan JSON──▶ my-browser ──stdin/stdout stream-json──▶ claude -p
                                                                                   │
                                          my-browser --mcp ◀──── tool baca ────────┘
```

- `crates/core/src/claude_chat.rs`: argumen CLI, konteks tab, penerjemah event, label tool.
- `crates/engine-webkit/src/panel.rs`: webview panel (tanpa pemblokir, data tidak disimpan, tidak bisa berpindah halaman).
- `crates/app/src/chat.rs`: proses Claude, riwayat, sesi, pesan dari dan ke panel.
- `crates/app/src/chat_panel.html`: tampilan panel dan renderer Markdown yang aman.

Untuk pengujian, build debug punya pengait `MYBROWSER_CHAT_SELFTEST=<berkas>`: setiap kali panel dibuka, satu pertanyaan berikutnya dari berkas itu dikirim, dan transkripnya ditulis ke `<berkas>.out`.
