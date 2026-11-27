# Rencana build: Windows ZIP saja

Status: **sudah diterapkan, masih tahap rencana** (mudah dibalik dengan `git revert`).
Tujuan: satu artefak saja — ZIP portable Windows — tanpa installer, Linux, atau macOS.

## Hasil akhir

| Workflow | Sebelum | Sesudah |
| --- | --- | --- |
| `.github/workflows/windows.yml` | build + test, lalu installer (Inno) **dan** ZIP | build + test, lalu **ZIP saja** |
| `.github/workflows/release.yml` | job `linux` + `windows`, unggah setup.exe, tar.gz, ZIP | job `windows` saja, unggah ZIP saja |
| `.github/workflows/linux.yml` | ada | **dihapus** |

## Isi ZIP

`YTRust-<versi>-windows-x64.zip`

```
streamtube.exe          # biner (nama internal tetap)
libmpv-2.dll            # dipin + SHA-256 di workflow
streamtube.portable     # penanda mode portable
vulkan/vulkan-1.dll     # loader Vulkan bundled, untuk PC tanpa driver Vulkan
vulkan/LICENSE.txt
README.md
LICENSE.txt
```

Langkah yang tetap dipertahankan dari upstream: build `--locked`, `cargo test`,
unduh libmpv dari rilis shinchiro yang dipin, build Vulkan loader dari tag
terpin, dan cek bahwa libmpv bisa dimuat hanya dengan loader bundled.

## Alur rilis

1. `git tag v0.7.6-alpha && git push origin v0.7.6-alpha` (tag harus sama dengan
   `version` di `Cargo.toml`).
2. Job `check` memvalidasi tag, job `windows` membangun ZIP.
3. Job `publish` membuat **draft release** berisi ZIP + `SHA256SUMS.txt` +
   attestation provenance; publikasi manual dari halaman Releases.

Judul draft sekarang `YTRust <versi>`; nama file ZIP memakai `YTRust-`.

## Konsekuensi yang perlu diketahui

- **Update dalam aplikasi tetap dari upstream.** `src/updater.rs` tidak diubah:
  ia mencari aset `*-setup.exe` dari repo `Avaneesh-Inamdar/Stream-Tube-Desktop`,
  jadi pembaruan otomatis masih berjalan seperti Stream Tube. ZIP milik fork
  tidak dipakai updater (memang disengaja: backend ikut upstream).
- Kalau nanti updater diarahkan ke repo fork, `pick_asset()` harus diajari
  menerima `.zip`; saat ini ia hanya menerima `*setup.exe` (Windows) dan
  `*.tar.gz`/`*.AppImage` (Linux). Itu perubahan backend, bukan frontend.
- Sisa hal "platform lain" yang belum dihapus dan tidak dipakai build:
  `installer/` (Inno), `packaging/linux/`, `mobile/android/`, dan bagian
  Linux/macOS di README. Semuanya tidak ikut terbangun, jadi sengaja dibiarkan
  supaya diff dengan upstream tetap kecil.

## Verifikasi

- Kedua workflow lolos parse YAML (jobs: `check`, `windows`, `publish`).
- Langkah packaging dicek ulang terhadap berkas yang disalin; `pwsh` tidak ada
  di sandbox Linux ini, jadi skrip PowerShell-nya belum dieksekusi lokal.
- Build & test Rust tidak terpengaruh perubahan ini (110 test tetap lulus).
