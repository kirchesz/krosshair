//! Minimizing or closing the window, as the tray setting says, hides it to an icon in the
//! notification area. A click on the icon brings the window back; its menu also quits.
//!
//! gpui has no tray support, so this subclasses gpui's window: the extra messages are handled
//! here and everything else goes on to gpui.
//!
//! Until the tray choice is made, the first minimize or close is held: the window asks what they
//! should do (see [`held`]) and then does it with [`minimize`] or [`close`].

use crate::model::TrayMode;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::OnceLock;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::Shell::{
    DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP,
    NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NINF_KEY, NIN_SELECT, NOTIFYICONDATAW,
    NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetSystemMetrics, IsIconic, IsWindowVisible,
    PostMessageW, RegisterWindowMessageW, SetForegroundWindow, SetMenuDefaultItem, ShowWindow, ShowWindowAsync,
    TrackPopupMenu,
    HICON, HTMINBUTTON, HTNOWHERE, ICONINFO, MF_SEPARATOR, MF_STRING, SC_MINIMIZE, SIZE_MINIMIZED, SM_CXSMICON,
    SWP_SHOWWINDOW, SW_HIDE, SW_MINIMIZE, SW_RESTORE, SW_SHOW, SW_SHOWMINNOACTIVE, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WINDOWPOS, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_NCDESTROY, WM_NCLBUTTONUP, WM_NULL,
    WM_SETTINGCHANGE, WM_SIZE, WM_SYSCOMMAND, WM_WINDOWPOSCHANGED,
};

/// The icon's messages.
const WM_TRAY: u32 = WM_APP + 1;
/// Sent to itself after the window has been minimized, to hide it.
const WM_TO_TRAY: u32 = WM_APP + 2;
const ICON_ID: u32 = 1;
const SUBCLASS_ID: usize = 0x6b72;
const MENU_OPEN: usize = 1;
const MENU_QUIT: usize = 2;
/// Enter or Space on the icon.
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

/// The tray setting, as the index of a [`TrayMode`].
static MODE: AtomicU8 = AtomicU8::new(TrayMode::OnMinimize as u8);

pub fn set_mode(mode: TrayMode) {
    MODE.store(mode as u8, Ordering::Relaxed);
}

fn mode() -> TrayMode {
    TrayMode::ALL[(MODE.load(Ordering::Relaxed) as usize).min(TrayMode::ALL.len() - 1)]
}

/// What the user did that is waiting for the tray choice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Held {
    Minimize,
    Close,
}

/// The tray choice hasn't been made yet: minimize and close are held, and the mode doesn't apply
/// (a minimize that can't be held, such as Show desktop, goes to the taskbar).
static ASK: AtomicBool = AtomicBool::new(false);
static HELD: OnceLock<async_channel::Sender<Held>> = OnceLock::new();

pub fn set_ask(ask: bool) {
    ASK.store(ask, Ordering::Relaxed);
}

fn asking() -> bool {
    ASK.load(Ordering::Relaxed)
}

/// Where held minimizes and closes arrive. Call once.
pub fn held() -> async_channel::Receiver<Held> {
    let (tx, rx) = async_channel::unbounded();
    let _ = HELD.set(tx);
    rx
}

/// Holds a minimize or close and tells the window, which shows itself if it was minimized or in
/// the tray (closed from the taskbar, say), so the question can be seen.
fn hold(hwnd: HWND, what: Held) {
    if let Some(tx) = HELD.get() {
        let _ = tx.try_send(what);
    }
    unsafe {
        if IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
            restore(hwnd);
        }
    }
}

/// Minimizes the window, as the tray setting says. Call on the window's thread.
pub fn minimize() {
    if let Some(hwnd) = WINDOW.get() {
        unsafe {
            let _ = ShowWindowAsync(hwnd, SW_MINIMIZE);
        }
    }
}

/// Quits krosshair whatever the tray setting, as Quit in the icon's menu: the overlay is taken
/// down on the way out.
pub fn quit() {
    QUITTING.set(true);
    close();
}

/// Closes the window, as the tray setting says: hidden to the tray, or krosshair quits.
pub fn close() {
    if let Some(hwnd) = WINDOW.get() {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// Starts the window minimized without taking the focus: to the tray when the tray is used at
/// all, on close as well as on minimize.
pub fn start_minimized(hwnd: HWND) {
    TO_TRAY_ONCE.set(mode() != TrayMode::Never);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            // Started minimized already (a shortcut set to "Minimized"): no WM_SIZE will come,
            // and minimizing again would show the window after it went to the tray.
            let _ = PostMessageW(Some(hwnd), WM_TO_TRAY, WPARAM(0), LPARAM(0));
        } else {
            let _ = ShowWindowAsync(hwnd, SW_SHOWMINNOACTIVE);
        }
    }
}

thread_local! {
    /// The window, once [`install`]ed.
    static WINDOW: Cell<Option<HWND>> = const { Cell::new(None) };
    /// The icon is in the notification area.
    static SHOWN: Cell<bool> = const { Cell::new(false) };
    static ICON: Cell<Option<HICON>> = const { Cell::new(None) };
    /// The message Explorer broadcasts when the taskbar is created again (after Explorer
    /// restarts): the icon has to be added again.
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    /// The next minimize goes to the tray whatever the mode: the start with Windows.
    static TO_TRAY_ONCE: Cell<bool> = const { Cell::new(false) };
    /// Quit was chosen: the close that follows really closes, even "on close".
    static QUITTING: Cell<bool> = const { Cell::new(false) };
}

/// Makes minimizing the window hide it to the notification area. Call on the window's thread.
pub fn install(hwnd: HWND) {
    WINDOW.set(Some(hwnd));
    unsafe {
        TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
        let _ = SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, 0);
    }
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
    unsafe {
        match msg {
            // Not asked yet: the window's minimize button, held. gpui still gets the release,
            // but not on its button, so it doesn't minimize.
            WM_NCLBUTTONUP if wp.0 == HTMINBUTTON as usize && asking() => {
                hold(hwnd, Held::Minimize);
                return DefSubclassProc(hwnd, msg, WPARAM(HTNOWHERE as usize), lp);
            }
            // The taskbar button, Win+Down, the window menu.
            WM_SYSCOMMAND if wp.0 & 0xfff0 == SC_MINIMIZE as usize && asking() => {
                hold(hwnd, Held::Minimize);
                return LRESULT(0);
            }
            // The close button, Alt+F4, the taskbar. Not after Quit in the tray menu.
            WM_CLOSE if asking() && !QUITTING.get() => {
                hold(hwnd, Held::Close);
                return LRESULT(0);
            }
            // Hide only once gpui has handled the minimize, and outside of it.
            WM_SIZE if wp.0 == SIZE_MINIMIZED as usize && to_tray_on_minimize() => {
                let result = DefSubclassProc(hwnd, msg, wp, lp);
                let _ = PostMessageW(Some(hwnd), WM_TO_TRAY, WPARAM(0), LPARAM(0));
                return result;
            }
            WM_TO_TRAY => {
                if IsIconic(hwnd).as_bool() && to_tray_on_minimize() {
                    hide(hwnd);
                }
                TO_TRAY_ONCE.set(false);
                return LRESULT(0);
            }
            // The close button, Alt+F4: hide instead, krosshair goes on. Not after Quit.
            WM_CLOSE if mode() == TrayMode::OnClose && !QUITTING.get() => {
                hide(hwnd);
                return LRESULT(0);
            }
            // Shown again, by the icon or by a second copy of the program. (ShowWindow with
            // SW_RESTORE sends no WM_SHOWWINDOW, so watch the position change instead.)
            WM_WINDOWPOSCHANGED if (*(lp.0 as *const WINDOWPOS)).flags.contains(SWP_SHOWWINDOW) => {
                remove_icon(hwnd)
            }
            WM_TRAY => {
                // NOTIFYICON_VERSION_4: the event is in the low word, the anchor point in wParam.
                match lp.0 as u32 & 0xffff {
                    NIN_SELECT | NIN_KEYSELECT => restore(hwnd),
                    WM_CONTEXTMENU => {
                        let x = (wp.0 & 0xffff) as i16 as i32;
                        let y = ((wp.0 >> 16) & 0xffff) as i16 as i32;
                        menu(hwnd, x, y);
                    }
                    _ => {}
                }
                return LRESULT(0);
            }
            // The light or dark taskbar may have changed: draw the icon for it.
            WM_SETTINGCHANGE if SHOWN.get() => {
                set_icon(hwnd, NIM_MODIFY);
            }
            WM_NCDESTROY => {
                remove_icon(hwnd);
                if let Some(icon) = ICON.take() {
                    let _ = DestroyIcon(icon);
                }
                let _ = RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID);
            }
            _ if msg != 0 && msg == TASKBAR_CREATED.get() && SHOWN.get() => {
                set_icon(hwnd, NIM_ADD);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

/// A minimize hides the window to the tray: the setting says so (once it is chosen), or this is
/// the start with Windows.
fn to_tray_on_minimize() -> bool {
    (mode() == TrayMode::OnMinimize && !asking()) || TO_TRAY_ONCE.get()
}

/// The window off the screen and the taskbar, the icon in the notification area.
fn hide(hwnd: HWND) {
    add_icon(hwnd);
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

/// The window back: as it was before it was minimized, or before it was closed to the tray
/// (maximized stays maximized).
pub fn restore(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, if IsIconic(hwnd).as_bool() { SW_RESTORE } else { SW_SHOW });
        let _ = SetForegroundWindow(hwnd);
    }
}

fn menu(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, MENU_OPEN, w!("Open krosshair"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit"));
        let _ = SetMenuDefaultItem(menu, MENU_OPEN as u32, 0);
        // Without this the menu stays open when the user clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, x, y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        match chosen.0 as usize {
            MENU_OPEN => restore(hwnd),
            // The usual way out, so the overlay is taken down as with the close button.
            MENU_QUIT => {
                QUITTING.set(true);
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            _ => {}
        }
    }
}

fn add_icon(hwnd: HWND) {
    if !SHOWN.get() {
        set_icon(hwnd, NIM_ADD);
    }
}

fn remove_icon(hwnd: HWND) {
    if SHOWN.replace(false) {
        let data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: ICON_ID,
            ..Default::default()
        };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        }
    }
}

/// Adds the icon, or redraws it, for the current light or dark taskbar.
fn set_icon(hwnd: HWND, how: windows::Win32::UI::Shell::NOTIFY_ICON_MESSAGE) {
    let side = unsafe { GetSystemMetrics(SM_CXSMICON) }.clamp(16, 64);
    let Some(icon) = make_icon(side, if light_taskbar() { [0x1f, 0x1f, 0x1f] } else { [0xff, 0xff, 0xff] }) else {
        return;
    };
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    for (to, from) in data.szTip.iter_mut().zip("krosshair".encode_utf16()) {
        *to = from;
    }
    unsafe {
        if Shell_NotifyIconW(how, &data).as_bool() {
            if how == NIM_ADD {
                data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
            }
            SHOWN.set(true);
        }
        if let Some(old) = ICON.replace(Some(icon)) {
            let _ = DestroyIcon(old);
        }
    }
}

/// Windows paints the taskbar light: the icon has to be dark to be seen.
fn light_taskbar() -> bool {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    found.is_ok() && value == 1
}

fn make_icon(side: i32, rgb: [u8; 3]) -> Option<HICON> {
    let px = logo(side as usize, rgb);
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: side,
                // Negative: rows go from the top down.
                biHeight: -side,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        // BGRA with straight alpha.
        let out = std::slice::from_raw_parts_mut(bits as *mut [u8; 4], px.len());
        for (o, [r, g, b, a]) in out.iter_mut().zip(px) {
            *o = [b, g, r, a];
        }
        // The mask is ignored where the color bitmap has alpha, but it must exist: all zeros.
        let mask_bytes = vec![0u8; (side as usize).div_ceil(16) * 2 * side as usize];
        let mask = CreateBitmap(side, side, 1, 1, Some(mask_bytes.as_ptr() as *const _));
        let icon = CreateIconIndirect(&ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        });
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon.ok()
    }
}

/// The app's logo (a ring with four ticks, as in the window) as RGBA pixels, side x side, row by
/// row. Drawn in a 24-unit box like the SVG icon, with 4x4 samples per pixel for smooth edges.
fn logo(side: usize, rgb: [u8; 3]) -> Vec<[u8; 4]> {
    const STROKE: f32 = 2.0;
    const LINES: [(f32, f32, f32, f32); 4] =
        [(12., 2., 12., 7.), (12., 17., 12., 22.), (2., 12., 7., 12.), (17., 12., 22., 12.)];
    let unit = 24. / side as f32;
    let inside = |u: f32, v: f32| {
        let ring = ((u - 12.).hypot(v - 12.) - 7.).abs();
        let ticks = LINES.iter().map(|&(x0, y0, x1, y1)| {
            // Distance to the segment: round ends, as the SVG's stroke-linecap.
            let (dx, dy) = (x1 - x0, y1 - y0);
            let t = (((u - x0) * dx + (v - y0) * dy) / (dx * dx + dy * dy)).clamp(0., 1.);
            (u - x0 - t * dx).hypot(v - y0 - t * dy)
        });
        ticks.fold(ring, f32::min) <= STROKE / 2.
    };
    let mut out = Vec::with_capacity(side * side);
    for y in 0..side {
        for x in 0..side {
            let mut hits = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let u = (x as f32 + (sx as f32 + 0.5) / 4.) * unit;
                    let v = (y as f32 + (sy as f32 + 0.5) / 4.) * unit;
                    hits += inside(u, v) as u32;
                }
            }
            out.push([rgb[0], rgb[1], rgb[2], (hits * 255 / 16) as u8]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_is_a_ring_with_ticks() {
        let px = logo(24, [255, 255, 255]);
        let alpha = |x: usize, y: usize| px[y * 24 + x][3];
        assert_eq!(alpha(12, 12), 0, "the middle is empty");
        assert_eq!(alpha(0, 0), 0, "so are the corners");
        assert!(alpha(12, 5) > 200, "the ring");
        assert!(alpha(11, 3) > 100 && alpha(20, 11) > 100, "the ticks");
    }
}
