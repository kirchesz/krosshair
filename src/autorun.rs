//! Starting with Windows: a "krosshair" value under the user's Run key that starts this program
//! with [`FLAG`]. Windows keeps the setting, so it is read from there rather than the config.

use anyhow::{bail, Result};
use std::os::windows::ffi::OsStrExt;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};

/// Started by Windows at sign-in: the window starts minimized.
pub const FLAG: &str = "--autorun";
const VALUE: &str = "krosshair";

/// The Run key, or the key named by `KROSSHAIR_RUN_KEY` (used for testing without touching the
/// real one).
fn key() -> HSTRING {
    match std::env::var("KROSSHAIR_RUN_KEY") {
        Ok(key) => key.into(),
        Err(_) => r"Software\Microsoft\Windows\CurrentVersion\Run".into(),
    }
}

/// The command line Windows runs at sign-in, if starting with Windows is on.
pub fn command() -> Option<String> {
    let (key, value) = (key(), HSTRING::from(VALUE));
    let mut size = 0u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, &key, &value, RRF_RT_REG_SZ, None, None, Some(&mut size)).ok().ok()?;
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        RegGetValueW(HKEY_CURRENT_USER, &key, &value, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
            .ok()
            .ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

/// The command that starts this copy of the program with Windows.
pub fn this_command() -> Result<String> {
    let exe = std::env::current_exe()?;
    Ok(format!("\"{}\" {FLAG}", exe.display()))
}

/// Turns starting with Windows on (for this copy of the program) or off.
pub fn set(on: bool) -> Result<()> {
    let (key, value) = (key(), HSTRING::from(VALUE));
    let code = if on {
        let text: Vec<u16> = std::ffi::OsStr::new(&this_command()?).encode_wide().chain([0]).collect();
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &key,
                &value,
                REG_SZ.0,
                Some(text.as_ptr().cast()),
                (text.len() * 2) as u32,
            )
        }
    } else {
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &key, PCWSTR(value.as_ptr())) } {
            e if e == ERROR_FILE_NOT_FOUND => return Ok(()),
            e => e,
        }
    };
    if code.is_err() {
        bail!("can't {} starting with Windows: {}", if on { "turn on" } else { "turn off" }, code.to_hresult().message());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_on_and_off_in_a_test_key() {
        // SAFETY of the environment change: no other test reads this variable.
        unsafe { std::env::set_var("KROSSHAIR_RUN_KEY", format!(r"Software\krosshair-test-{}", std::process::id())) };
        set(false).unwrap();
        assert_eq!(command(), None);
        set(true).unwrap();
        assert_eq!(command(), Some(this_command().unwrap()));
        assert!(command().unwrap().ends_with(" --autorun"));
        set(false).unwrap();
        assert_eq!(command(), None);
        let test_key = key();
        unsafe {
            let _ = windows::Win32::System::Registry::RegDeleteKeyW(HKEY_CURRENT_USER, &test_key);
        }
    }
}
