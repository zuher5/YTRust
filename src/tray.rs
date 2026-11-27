use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::Cell;
use std::sync::OnceLock;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PCWSTR, w};

const CALLBACK: u32 = WM_APP + 23;
const KEY_SELECT: u32 = NIN_SELECT | 1;
const ICON_ID: u32 = 1;
const OPEN: i32 = 1;
const QUIT: i32 = 2;
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

thread_local! {
    static HIDDEN: Cell<Option<HWND>> = const { Cell::new(None) };
    static QUITTING: Cell<bool> = const { Cell::new(false) };
}

fn hwnd(window: &Window) -> Option<HWND> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as _)),
        _ => None,
    }
}

fn data(window: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: window, uID: ICON_ID, ..Default::default() }
}

fn add_icon(window: HWND) -> bool {
    let mut icon = data(window);
    icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    icon.uCallbackMessage = CALLBACK;
    for (dest, source) in icon.szTip.iter_mut().zip("YTRust".encode_utf16()) {
        *dest = source;
    }
    // SAFETY: Both the shared icon resource and the main HWND belong to this process.
    unsafe {
        let module = GetModuleHandleW(None).ok().map(|module| HINSTANCE(module.0));
        let Ok(app_icon) = LoadIconW(module, PCWSTR(1usize as *const u16)) else { return false };
        icon.hIcon = app_icon;
        if !Shell_NotifyIconW(NIM_ADD, &icon).as_bool() {
            return false;
        }
        icon.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &icon);
    }
    true
}

fn remove_icon(window: HWND) {
    // SAFETY: the identifier refers only to the notification icon of this window.
    unsafe { let _ = Shell_NotifyIconW(NIM_DELETE, &data(window)); }
}

fn restore_native(window: HWND) {
    remove_icon(window);
    HIDDEN.set(None);
    // SAFETY: This is the HWND passed by Windows to the tray callback.
    unsafe {
        let _ = ShowWindowAsync(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
    }
}

pub(crate) fn is_hidden(window: &Window) -> bool {
    hwnd(window).is_some_and(|window| HIDDEN.get() == Some(window))
}

pub(crate) fn hide(window: &Window) -> bool {
    if QUITTING.replace(false) || !crate::store::settings_snapshot().minimize_to_tray {
        return false;
    }
    let Some(window) = hwnd(window) else { return false };
    if HIDDEN.get() == Some(window) {
        return true;
    }
    // SAFETY: The subclass is installed on the same UI thread that owns the HWND.
    unsafe {
        TASKBAR_CREATED.get_or_init(|| RegisterWindowMessageW(w!("TaskbarCreated")));
        if !SetWindowSubclass(window, Some(window_message), 1, 0).as_bool() || !add_icon(window) {
            return false;
        }
        HIDDEN.set(Some(window));
        if !ShowWindowAsync(window, SW_HIDE).as_bool() {
            HIDDEN.set(None);
            remove_icon(window);
            return false;
        }
    }
    true
}

pub(crate) fn restore(window: &Window) -> bool {
    let Some(window) = hwnd(window).filter(|&window| HIDDEN.get() == Some(window)) else { return false };
    restore_native(window);
    true
}

extern "system" fn window_message(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
    if message == CALLBACK {
        let event = (lparam.0 as usize & 0xffff) as u32;
        match event {
            NIN_SELECT | KEY_SELECT | WM_LBUTTONUP => restore_native(window),
            WM_CONTEXTMENU | WM_RBUTTONUP => {
                match menu(window) {
                    OPEN => restore_native(window),
                    QUIT => {
                        remove_icon(window);
                        HIDDEN.set(None);
                        QUITTING.set(true);
                        // SAFETY: the owning window handles the posted close on its event loop.
                        unsafe { let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)); }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        return LRESULT(0);
    }
    if TASKBAR_CREATED.get() == Some(&message) && HIDDEN.get() == Some(window) && !add_icon(window) {
        restore_native(window);
    }
    if message == WM_NCDESTROY {
        remove_icon(window);
        HIDDEN.set(None);
        // SAFETY: Remove the callback installed by hide before the window is destroyed.
        unsafe { let _ = RemoveWindowSubclass(window, Some(window_message), 1); }
    }
    // SAFETY: Forward unhandled messages to the next procedure on the original HWND.
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}

fn menu(window: HWND) -> i32 {
    // SAFETY: The menu is created, used and destroyed on the main HWND's owning thread.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return 0 };
        let open = HSTRING::from(crate::i18n::t("Open"));
        let quit = HSTRING::from(crate::i18n::t("Quit"));
        let _ = AppendMenuW(menu, MF_STRING, OPEN as usize, &open);
        let _ = AppendMenuW(menu, MF_STRING, QUIT as usize, &quit);
        let _ = SetForegroundWindow(window);
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, cursor.x, cursor.y, None, window, None).0;
        let _ = DestroyMenu(menu);
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        command
    }
}
