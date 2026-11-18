//! krosshair: a crosshair overlay drawn through the RivaTuner Statistics Server OSD.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod autorun;
mod dialogs;
mod displays;
mod model;
mod overlay;
mod raster;
mod rtss;
mod store;
mod tray;
mod ui;
mod update;

use gpui::{
    prelude::*, px, size, App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions,
};
use std::sync::Arc;

/// True when another copy is already running, in which case its window is brought to the front.
/// Only one copy may run, whatever its configuration: RTSS keeps one image for the whole OSD, so
/// two copies draw their crosshairs from each other's atlas and make the game stutter.
fn already_running() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, SetLastError, ERROR_ALREADY_EXISTS, WIN32_ERROR};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetForegroundWindow, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };
    unsafe {
        SetLastError(WIN32_ERROR(0));
        // The handle is never closed: the mutex lives as long as this process.
        let _ = CreateMutexW(None, false, w!("krosshair-single-instance"));
        if GetLastError() != ERROR_ALREADY_EXISTS {
            return false;
        }
        // gpui's window class plus the title, so a folder named krosshair in Explorer doesn't match.
        if let Ok(window) = FindWindowW(w!("Zed::Window"), w!("krosshair")) {
            if IsIconic(window).as_bool() {
                let _ = ShowWindow(window, SW_RESTORE);
            } else if !IsWindowVisible(window).as_bool() {
                // Closed to the tray: hidden, but not minimized.
                let _ = ShowWindow(window, SW_SHOW);
            }
            // While a minimized window is being restored, Windows may not let it come to the
            // front yet: keep asking for half a second.
            for _ in 0..10 {
                let _ = SetForegroundWindow(window);
                if GetForegroundWindow() == window {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        true
    }
}

fn main() {
    // Put in place by an update: the copy it replaced is still quitting and holds the lock.
    let args: Vec<String> = std::env::args().collect();
    if let Some(pid) = args.iter().position(|a| a == update::AFTER_FLAG).and_then(|i| args.get(i + 1)?.parse().ok()) {
        update::after_update(pid);
    }
    if already_running() {
        return;
    }
    let autorun = std::env::args().any(|a| a == autorun::FLAG);
    Application::new().with_assets(assets::Assets).run(move |cx: &mut App| {
        ui::theme::init(cx);
        let overlay = Arc::new(overlay::Overlay::start());

        let bounds = Bounds::centered(None, size(px(1200.), px(760.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(980.), px(640.))),
            // The window draws its own header with the window buttons.
            titlebar: Some(TitlebarOptions { title: Some("krosshair".into()), appears_transparent: true, traffic_light_position: None }),
            focus: !autorun,
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            // Minimize hides the window to the notification area (when that setting is on).
            let hwnd = match raw_window_handle::HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
                Ok(raw_window_handle::RawWindowHandle::Win32(h)) => Some(windows::Win32::Foundation::HWND(h.hwnd.get() as _)),
                _ => None,
            };
            if let Some(hwnd) = hwnd {
                tray::install(hwnd);
            }
            // The crosshair lives in RTSS, not in this process: take it down before leaving.
            let on_close = overlay.clone();
            window.on_window_should_close(cx, move |_, _| {
                on_close.shutdown();
                true
            });
            let view = cx.new(|cx| {
                let mut view = ui::Krosshair::new(overlay.clone(), cx);
                // `--screen gallery|editor|layer|settings` opens that screen right away (for development).
                let mut args = std::env::args().skip(1);
                if args.next().as_deref() == Some("--screen") {
                    view.open_screen(args.next().as_deref().unwrap_or(""), cx);
                }
                view
            });
            // Started by Windows at sign-in: stay out of the way, the crosshair is what matters.
            if autorun && let Some(hwnd) = hwnd {
                tray::start_minimized(hwnd);
            }
            view
        })
        .expect("can't open the window");

        cx.on_window_closed(|cx| cx.quit()).detach();
        if !autorun {
            cx.activate(true);
        }
    });
}
