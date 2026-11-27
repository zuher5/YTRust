//! The app icon is the official YouTube icon: the window and taskbar icon, the
//! desktop / Start menu shortcuts (Windows) and the menu icon (Linux).
//!
//! The rasters come from `tools/youtube_icon.py` (assets/youtube.png, and
//! assets/streamtube.ico for Windows resource 1, see assets/streamtube.rc).
//! The update/About hero shows the same icon (`ui::youtube_icon`).
use gpui_kit::*;

/// Accent colours the update dialog's gradient can be built from.
pub const ACCENTS: [u32; 7] = [0xFF0033, 0xF0307E, 0xAB47BC, 0x3EA6FF, 0x00BFA5, 0x2BA640, 0xFFA000];
/// The brand colour: its logo is the original gradient.
pub const BRAND: u32 = 0xF0307E;

/// The closest accent that has an icon.
pub fn icon_accent(accent: u32) -> u32 {
    if ACCENTS.contains(&accent) {
        return accent;
    }
    let d = |a: u32, b: u32| {
        let c = |s: u32| ((a >> s) & 255) as i32 - ((b >> s) & 255) as i32;
        c(16).pow(2) + c(8).pow(2) + c(0).pow(2)
    };
    *ACCENTS.iter().min_by_key(|a| d(**a, accent)).unwrap_or(&BRAND)
}

fn hls(c: u32) -> (f64, f64, f64) {
    let (r, g, b) = (((c >> 16) & 255) as f64 / 255., ((c >> 8) & 255) as f64 / 255., (c & 255) as f64 / 255.);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.;
    if mx == mn {
        return (0., l, 0.);
    }
    let d = mx - mn;
    let s = if l <= 0.5 { d / (mx + mn) } else { d / (2. - mx - mn) };
    let h = if mx == r {
        (g - b) / d + if g < b { 6. } else { 0. }
    } else if mx == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    } / 6.;
    (h, l, s)
}

fn from_hls(h: f64, l: f64, s: f64) -> u32 {
    let f = |p: f64, q: f64, mut t: f64| {
        t = t.rem_euclid(1.0);
        if t < 1. / 6. {
            p + (q - p) * 6. * t
        } else if t < 0.5 {
            q
        } else if t < 2. / 3. {
            p + (q - p) * (2. / 3. - t) * 6.
        } else {
            p
        }
    };
    let (r, g, b) = if s == 0. {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1. + s) } else { l + s - l * s };
        let p = 2. * l - q;
        (f(p, q, h + 1. / 3.), f(p, q, h), f(p, q, h - 1. / 3.))
    };
    let c = |x: f64| (x.clamp(0., 1.) * 255.).round() as u32;
    (c(r) << 16) | (c(g) << 8) | c(b)
}

fn shift(c: u32, dh: f64, sk: f64, lk: f64) -> u32 {
    let (h, l, s) = hls(c);
    from_hls((h + dh / 360.).rem_euclid(1.), (l * lk).clamp(0., 1.), (s * sk).clamp(0., 1.))
}

/// The three gradient stops for an accent, used by the update dialog's hero.
pub fn accent_stops(accent: u32) -> [u32; 3] {
    if accent == BRAND {
        return [0xFF5A5F, 0xF0307E, 0x7A3CF5];
    }
    [shift(accent, -12., 1.0, 1.18), accent, shift(accent, 32., 0.95, 0.72)]
}

/// Applies the app icon everywhere outside the app's own UI.
///
/// The icon is the same in every accent colour (it is the YouTube icon), so the
/// only thing that matters is which one is already installed.
pub fn apply(cx: &mut App) {
    #[cfg(windows)]
    {
        for w in cx.windows() {
            let _ = w.update(cx, |_, window, _| set_window_icon(window));
        }
        update_shortcuts();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = cx;
        update_menu_icon();
    }
}

#[cfg(windows)]
pub fn set_window_icon(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    #[link(name = "user32")]
    unsafe extern "system" {
        fn LoadImageW(hinst: isize, name: *const u16, kind: u32, cx: i32, cy: i32, flags: u32) -> isize;
        fn SendMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
        fn GetSystemMetrics(i: i32) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> isize;
    }
    const IMAGE_ICON: u32 = 1;
    const WM_SETICON: u32 = 0x0080;
    const SM_CXICON: i32 = 11;
    const SM_CXSMICON: i32 = 49;
    let Ok(h) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::Win32(w) = h.as_raw() else { return };
    let id = 1usize as *const u16; // MAKEINTRESOURCE(1): the app icon resource
    unsafe {
        let inst = GetModuleHandleW(std::ptr::null());
        let big_size = GetSystemMetrics(SM_CXICON).max(32);
        let small_size = GetSystemMetrics(SM_CXSMICON).max(16);
        let big = LoadImageW(inst, id, IMAGE_ICON, big_size, big_size, 0);
        let small = LoadImageW(inst, id, IMAGE_ICON, small_size, small_size, 0);
        if big != 0 {
            SendMessageW(w.hwnd.get(), WM_SETICON, 1, big);
        }
        if small != 0 {
            SendMessageW(w.hwnd.get(), WM_SETICON, 0, small);
        }
    }
}

/// Points the app's shortcuts (desktop, Start menu, taskbar pin) at the matching icon.
#[cfg(windows)]
fn update_shortcuts() {
    let Ok(exe) = std::env::current_exe() else { return };
    // an update reinstalls the shortcuts with the default icon, so the version and
    // the exe's timestamp are part of what "already done" means
    let built = std::fs::metadata(&exe).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let exe = exe.display().to_string();
    let key = format!("{exe}|{}|{built}", env!("CARGO_PKG_VERSION"));
    if crate::store::kv_get("shortcut_icon").as_deref() == Some(key.as_str()) {
        return;
    }
    let id = 1;
    std::thread::spawn(move || {
        // .lnk files are COM objects; PowerShell's WScript.Shell edits them simply
        let script = format!(
            r#"$exe = '{exe}'
$shell = New-Object -ComObject WScript.Shell
$dirs = @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs'), (Join-Path $env:APPDATA 'Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar'))
foreach ($d in $dirs) {{
  if (-not (Test-Path $d)) {{ continue }}
  Get-ChildItem -Path $d -Filter *.lnk -Recurse -Depth 1 -ErrorAction SilentlyContinue | ForEach-Object {{
    $l = $shell.CreateShortcut($_.FullName)
    if ($l.TargetPath -ieq $exe) {{ $l.IconLocation = "$exe,-{id}"; $l.Save() }}
  }}
}}"#,
            exe = exe.replace('\'', "''"),
        );
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script]);
        crate::api::streams::hide_window(&mut c);
        if c.output().map(|o| o.status.success()).unwrap_or(false) {
            crate::store::kv_set("shortcut_icon", &key);
            // ask Explorer to redraw icons
            #[link(name = "shell32")]
            unsafe extern "system" {
                fn SHChangeNotify(event: i32, flags: u32, a: *const std::ffi::c_void, b: *const std::ffi::c_void);
            }
            const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
            unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, 0, std::ptr::null(), std::ptr::null()) };
        }
    });
}

/// The app icon (assets/youtube.png) is written into the user's icon theme, so the
/// launcher shows it regardless of where the binary was installed.
#[cfg(target_os = "linux")]
fn update_menu_icon() {
    let png: &[u8] = include_bytes!("../../assets/youtube.png");
    let Some(base) = dirs::data_dir() else { return };
    let dir = base.join("icons/hicolor/256x256/apps");
    let path = dir.join("streamtube.png");
    if std::fs::read(&path).map(|b| b == png).unwrap_or(false) {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::write(&path, png).is_ok() {
        // icon caches look at the folder's modification time
        let _ = std::process::Command::new("touch").arg(base.join("icons/hicolor")).status();
        let _ = std::process::Command::new("gtk-update-icon-cache").args(["-q", "-t", "-f"]).arg(base.join("icons/hicolor")).status();
    }
}
