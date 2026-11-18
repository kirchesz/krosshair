//! Native Windows dialogs.

use std::sync::Mutex;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::UI::Controls::Dialogs::{ChooseColorW, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW};

/// The "custom colors" row of the color dialog, kept while the app runs.
static CUSTOM_COLORS: Mutex<[COLORREF; 16]> = Mutex::new([COLORREF(0x00ff_ffff); 16]);

/// Shows the system color picker. Blocks until it is closed, so call it off the main thread.
/// Colors are 0xRRGGBB. `None` when the dialog was cancelled.
pub fn choose_color(initial: u32) -> Option<u32> {
    let to_ref = |c: u32| COLORREF(((c & 0xff) << 16) | (c & 0xff00) | ((c >> 16) & 0xff));
    let mut custom = *CUSTOM_COLORS.lock().unwrap();
    let mut cc = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        rgbResult: to_ref(initial),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    let ok = unsafe { ChooseColorW(&mut cc) }.as_bool();
    *CUSTOM_COLORS.lock().unwrap() = custom;
    ok.then(|| {
        let c = cc.rgbResult.0;
        ((c & 0xff) << 16) | (c & 0xff00) | ((c >> 16) & 0xff)
    })
}
