# Fork YTRust — hanya frontend yang diubah

Baseline: `Avaneesh-Inamdar/Stream-Tube-Desktop` @ `ae25300` (0.7.6-alpha).

Tujuan: rebranding visual ringan supaya sinkronisasi dengan upstream tetap mudah.
Backend, i18n, packaging, dan updater dibiarkan apa adanya.

## Yang diubah (frontend saja)

1. **Header tanpa brand** — `src/ui/app.rs::render_topbar`: blok logo + wordmark
   "Stream Tube" di samping search dihapus. `ui::wordmark()` ikut dihapus
   (`src/ui/mod.rs`). Sisanya (menu, back, refresh, search, download, settings,
   avatar, window controls) persis upstream.

2. **Ikon YouTube** — app icon (window/taskbar/shortcut/menu Linux) dan hero
   dialog update/About memakai ikon YouTube resmi:
   - `assets/youtube.svg` (sumber), `assets/youtube.png` 256 px (ikon Linux),
     `assets/streamtube.ico` 16–256 px (Windows resource 1).
   - `tools/youtube_icon.py` membuat kedua raster itu dari geometri SVG.
   - `src/assets.rs` menyajikan `brand/youtube.svg`; hero memanggil
     `ui::youtube_icon()` di `src/ui/app.rs`.
   - `src/ui/app_icon.rs`: ikon tetap (tidak ikut accent), `logo_svg()` dihapus.
   - `assets/accent/` dan `tools/accent_icons.py` dihapus karena tak terpakai lagi.

3. **Nama tampil "YTRust"** hanya di chrome aplikasi:
   - judul jendela (`src/main.rs`),
   - judul jendela Settings (`src/ui/pages/settings.rs`),
   - baris nama di About (`src/ui/pages/settings.rs`),
   - tooltip tray Windows (`src/tray.rs`).

## Yang sengaja dibiarkan seperti upstream

- Nama crate/biner `streamtube`, `app_id`/`StreamWMClass`, `streamtube.desktop`,
  skema `streamtube://`, env `STREAMTUBE_*`, folder data `StreamTube`, DB
  `streamtube.db`.
- Semua teks i18n (`src/i18n/locales/*.json` dan string sumbernya), label
  Settings lain, MPRIS, nama klien audio mpv, pesan error/log.
- `src/updater.rs` tetap menunjuk repo upstream, jadi update mengikuti rilis
  Stream Tube dan tidak menimpa backend.
- README, CONTRIBUTING, docs upstream, `packaging/`, `installer/`, `userscript/`,
  `mobile/`, `flake.nix`, workflow CI. `assets/logo.png` dan `logo.svg` tetap ada
  untuk packaging dan README.

## Berkas yang berubah

```
src/main.rs                     judul jendela
src/ui/app.rs                   header tanpa brand + ikon YouTube di hero
src/ui/mod.rs                   wordmark dihapus, youtube_icon ditambah
src/ui/app_icon.rs              app icon tetap, logo_svg dihapus
src/ui/pages/settings.rs        judul Settings + baris nama About
src/tray.rs                     tooltip tray
src/assets.rs                   brand/youtube.svg
assets/youtube.svg              baru
assets/youtube.png              baru (dipakai di Linux menu icon)
assets/streamtube.ico           diganti ikon YouTube
assets/streamtube.rc            hanya resource 1
build.rs                        memantau assets/youtube.png
assets/accent/                  dihapus (14 berkas, tak terpakai)
tools/accent_icons.py           dihapus
tools/youtube_icon.py           baru
docs/fork-ytrust.md             catatan ini
docs/screenshots/header-after.png
```

## Verifikasi

- `cargo build` sukses, `cargo test` 110 lulus / 0 gagal, jumlah warning sama
  dengan upstream.
- App dijalankan di X11: `_NET_WM_NAME = "YTRust"`, header tanpa logo/teks
  (`docs/screenshots/header-after.png`), ikon Linux tertulis ke
  `~/.local/share/icons/hicolor/256x256/apps/streamtube.png` dan byte-identik
  dengan `assets/youtube.png`.
- Tidak diverifikasi: jalur khusus Windows (resource ico, shortcut `.lnk`).

## Kalau mau lebih jauh

Karena nama di desktop entry/metadata masih "Stream Tube", pintasan di menu
aplikasi Linux akan tampil sebagai Stream Tube sampai `packaging/linux/*` ikut
diubah — itu sengaja, supaya diff dengan upstream tetap kecil.
