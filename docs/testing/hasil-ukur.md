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
