# Hasil Pengukuran Memory

Alat ukur: `scripts/ukur-memory.sh` (crate `memtool`). Angka adalah physical footprint (`proc_pid_rusage`, `ri_phys_footprint`), sama dengan kolom Memory di Activity Monitor. Proses pembantu dikelompokkan ke aplikasi lewat responsible pid.

Mesin: arm64, RAM 16 GB, macOS 15.7.7, Safari 18.6.

## Verifikasi atribusi (Task 2, 2026-11-15)

- `my-browser` (prototipe, YouTube terbuka): 9 proses, termasuk 2 `com.apple.WebKit.WebContent`, `com.apple.WebKit.GPU`, dan `com.apple.WebKit.Networking`. Atribusi berhasil.
- Safari (beberapa tab milik pengguna ditambah satu tab Wikipedia): 14 proses, termasuk 3 `com.apple.WebKit.WebContent`. Atribusi berhasil.

Pengamatan: prototipe dengan satu tab YouTube memakai dua proses WebContent (601 MB dan 328 MB). Kemungkinan satu di antaranya proses lama dari halaman login Google yang masih ditahan WebKit. Ini sesuai risiko di spec bagian 8 dan perlu diperhatikan di Task 9.

## Pengukuran awal: satu tab YouTube (Task 2)

| Browser | Video | Total footprint | Jumlah proses | Proses aplikasi saja |
|---|---|---|---|---|
| my-browser (prototipe) | https://youtu.be/83H3YAm2woI, 1080p, diukur 18:32:58 setelah diputar 2 menit | 1.415,4 MB | 10 | 36,8 MB |
| Safari 18.6 | video sama, 1080p, diukur 18:36:22 setelah diputar 2 menit | 1.812,3 MB | 17 | 58,8 MB |

Rincian proses terbesar:

| Proses | my-browser | Safari |
|---|---|---|
| WebContent tab YouTube | 839,4 MB | 1.487,5 MB |
| WebContent lain | 327,3 MB (satu proses lama, diduga sisa halaman login Google) | 32,5 MB (tiga proses kecil) |
| WebKit GPU | 126,6 MB | 145,2 MB |
| Proses aplikasi | 36,8 MB | 58,8 MB |

Batas pengukuran ini: masing-masing hanya satu sampel, diambil 2 menit setelah video diputar. Selisih buffer video antar pengambilan bisa besar, jadi angka di atas belum bisa dipakai untuk menyimpulkan selisih pasti antara kedua browser. Yang cukup jelas: proses aplikasi prototipe lebih kecil (36,8 MB banding 58,8 MB), dan proses WebContent lama sebesar 327 MB di prototipe adalah memory yang bisa dihemat.

## Tab tidur dan proses WebContent (Task 9, 2026-11-15)

Pengaturan uji: folder data terpisah (`MYBROWSER_DATA_DIR`), `sleep_idle_secs` 20, empat tab dari argumen (`id.wikipedia.org/wiki/Indonesia`, `github.com/servo/servo`, `example.com`, `www.rust-lang.org`), tab terakhir aktif. Diukur dengan `scripts/ukur-memory.sh --detail my-browser`.

| Waktu | Proses WebContent | Total my-browser |
|---|---|---|
| t+22 detik, sebelum tick pertama | 4 | 399,0 MB |
| t+40 detik, 10 detik setelah tick pertama menidurkan 3 tab | 1 | 122,9 MB |

Bug yang ditemukan dan diperbaiki sebelum angka di atas: page yang dibuat di `main()` sebelum `app.run()` (tab sesi yang dipulihkan dan URL dari argumen) tidak pernah dibebaskan. Saat dilepas, `WKWebView`-nya masih punya 31 retain karena WebKit meng-autorelease objek itu puluhan kali saat dibuat, dan di luar `app.run()` tidak ada autorelease pool. Akibatnya proses WebContent tab yang ditidurkan tetap hidup (diamati: 4 proses tetap hidup lebih dari 120 detik). Perbaikan: seluruh persiapan di `main()` dibungkus `autoreleasepool` (`crates/app/src/main.rs`). Setelah perbaikan, retain saat dilepas = 1 dan prosesnya mati.

Keputusan CHECKPOINT: proses WebContent tab yang tidur mati dalam 10 detik, jadi Task 12 (API privat) tidak diperlukan.

## Pemblokir iklan (Task 10, 2026-11-15)

Daftar aturan: EasyList + EasyPrivacy (diunduh 2026-11-15) dikonversi `blocklist-gen` menjadi 131.509 aturan WebKit dalam 3 daftar (14 MB JSON); 3.627 filter tidak bisa dikonversi dan dilewati. Kompilasi pertama: 763, 1.183, dan 1.265 ms per daftar (berjalan paralel); peluncuran berikutnya memakai hasil kompilasi dari penyimpanan WebKit.

Satu tab `https://www.detik.com`, diukur 30 detik setelah aplikasi dibuka, folder data terpisah per percobaan:

| Percobaan | Proses WebContent | Total my-browser |
|---|---|---|
| Pemblokir aktif, kompilasi pertama | 142,2 MB | 190,8 MB |
| detik.com dikecualikan (`blocker_exceptions`) | 455,1 MB | 529,6 MB |
| Pemblokir aktif, daftar dari cache | 118,0 MB | 162,6 MB |

Satu sampel per percobaan. Belum dicek secara visual apakah ada bagian situs yang rusak; itu masuk daftar cek manual.

## Perbaikan pemblokir: tombol "Lewati" YouTube (2026-11-20)

Laporan pengguna: iklan video YouTube tidak punya tombol "Lewati".

Penyebab, dibuktikan di WebKit dengan program diagnostik yang memuat halaman dengan daftar aturan tertentu lalu memeriksa elemen uji yang disisipkan ke halaman:

1. EasyList menyembunyikan `.video-ads` di semua situs (`##.video-ads`) dan mengecualikan YouTube (`youtube.com#@#.video-ads`). Crate adblock mengubah pengecualian itu menjadi aturan sembunyi dengan `unless-domain`, sementara aturan umumnya tetap berlaku di mana-mana. Akibatnya wadah UI iklan YouTube, termasuk tombol "Lewati", ikut tersembunyi, padahal video iklannya tetap diputar.
2. `@@||www.youtube.com^$generichide` (dan 180 situs lain, termasuk GitHub, Gmail, dan Wikipedia) dibuang crate adblock. Akibatnya 13.637 aturan sembunyi umum tetap berlaku di situs-situs itu.
3. Domain di aturan kosmetik ditulis tanpa awalan `*`. Di WebKit, `youtube.com` hanya cocok dengan host yang persis sama, jadi aturan khusus YouTube tidak berlaku di `www.youtube.com`.

Temuan perilaku WebKit yang menentukan bentuk perbaikan: `ignore-previous-rules` yang hanya cocok dengan URL dokumen tidak cukup. Untuk setiap subresource yang tidak terkena pengecualian (mis. CSS dari ytimg.com), WebKit memasang lagi stylesheet aturan umum. Karena itu generichide ditulis sebagai satu aturan dengan `url-filter` `.*` dan `if-domain` berisi semua situsnya, ditaruh di daftar khusus aturan sembunyi umum.

Hasil (1 = tersembunyi, 0 = tampil; baris terakhir menghitung muatan dari jaringan iklan):

| Uji | Tanpa pemblokir | Daftar lama | Daftar baru |
|---|---|---|---|
| YouTube, elemen `.video-ads` (wadah tombol Lewati) | 0 | 1 | 0 |
| YouTube, selector umum `.video-ads-container` | 0 | 1 | 0 |
| YouTube, aturan khusus youtube.com | 0 | 0 | 1 |
| GitHub, selector umum | 0 | 1 | 0 |
| detik.com, selector umum (situs tanpa pengecualian) | 0 | 1 | 1 |
| detik.com, muatan doubleclick/googlesyndication/adservice | 5 | 0 | 0 |

Batas: iklan video YouTube tetap diputar, karena dikirim dari server yang sama dengan videonya dan content blocker WebKit tidak bisa membedakannya. Perbaikan ini hanya mengembalikan tombol "Lewati" untuk iklan yang memang bisa dilewati.

Bug lain yang ditemukan saat pengujian ini: menghapus daftar aturan lama dari penyimpanan WebKit dengan completion handler kosong membuat proses crash (SIGSEGV di `removeContentRuleListForIdentifier:completionHandler:`). Ini terjadi setiap kali isi daftar aturan berubah, termasuk saat daftar baru ini dipasang. Diperbaiki dan dijaga oleh smoke test.
