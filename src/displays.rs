//! The displays connected to the computer, with the names Windows' display settings show.

use windows::core::PCWSTR;
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{ERROR_SUCCESS, POINT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromPoint, DEVMODEW, ENUM_CURRENT_SETTINGS, MONITORINFO, MONITORINFOEXW,
    MONITOR_DEFAULTTOPRIMARY,
};

#[derive(Clone, PartialEq, Debug)]
pub struct Display {
    /// Windows' name for it, like `\\.\DISPLAY1`.
    pub id: String,
    /// The monitor's own name, like "DELL U2720Q"; "Display 1" when it gives none.
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

impl Display {
    /// "DELL U2720Q · 2560 × 1440"
    pub fn label(&self) -> String {
        format!("{} · {} × {}", self.name, self.width, self.height)
    }
}

/// A NUL-terminated wide string from a fixed buffer.
fn text(wide: &[u16]) -> String {
    String::from_utf16_lossy(&wide[..wide.iter().position(|&c| c == 0).unwrap_or(wide.len())])
}

/// The number Windows gives a display (`\\.\DISPLAY2` is 2).
fn number(id: &str) -> u32 {
    id.trim_start_matches(r"\\.\DISPLAY").parse().unwrap_or(u32::MAX)
}

/// The displays in use, in Windows' order. Empty if Windows doesn't say.
pub fn list() -> Vec<Display> {
    let primary = primary_id();
    let mut out: Vec<Display> = Vec::new();
    unsafe {
        let (mut paths_n, mut modes_n) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths_n, &mut modes_n) != ERROR_SUCCESS {
            return out;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); paths_n as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); modes_n as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut paths_n, paths.as_mut_ptr(), &mut modes_n, modes.as_mut_ptr(), None)
            != ERROR_SUCCESS
        {
            return out;
        }
        paths.truncate(paths_n as usize);
        for path in &paths {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                    adapterId: path.sourceInfo.adapterId,
                    id: path.sourceInfo.id,
                },
                ..Default::default()
            };
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0 {
                continue;
            }
            let id = text(&source.viewGdiDeviceName);
            // Duplicated displays show one picture: one entry.
            if out.iter().any(|d| d.id == id) {
                continue;
            }
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                    size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                    adapterId: path.targetInfo.adapterId,
                    id: path.targetInfo.id,
                },
                ..Default::default()
            };
            let name =
                if DisplayConfigGetDeviceInfo(&mut target.header) == 0 { text(&target.monitorFriendlyDeviceName) } else { String::new() };
            let mut mode = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
            let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
            let (width, height) = if EnumDisplaySettingsW(PCWSTR(wide.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode).as_bool() {
                (mode.dmPelsWidth, mode.dmPelsHeight)
            } else {
                (0, 0)
            };
            out.push(Display { primary: primary.as_deref() == Some(id.as_str()), id, name, width, height });
        }
    }
    out.sort_by_key(|d| number(&d.id));
    for d in &mut out {
        if d.name.is_empty() {
            d.name = format!("Display {}", number(&d.id));
        }
    }
    out
}

/// Windows' name for the primary display.
fn primary_id() -> Option<String> {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        GetMonitorInfoW(monitor, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool().then(|| text(&info.szDevice))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_have_names_sizes_and_one_primary() {
        // A machine without a display (a build server) has nothing to check.
        let list = list();
        if list.is_empty() {
            return;
        }
        assert_eq!(list.iter().filter(|d| d.primary).count(), 1, "{list:?}");
        assert!(list.iter().all(|d| !d.name.is_empty() && d.width > 0 && d.height > 0), "{list:?}");
    }

    #[test]
    fn numbers_from_windows_names() {
        assert_eq!(number(r"\\.\DISPLAY2"), 2);
        assert_eq!(number("something else"), u32::MAX);
    }
}
