//! Colors and fonts of the interface.

use gpui::{rgb, rgba, App, Hsla, Rgba, SharedString};
use std::borrow::Cow;
use std::sync::OnceLock;

// IBM Carbon, the g100 (darkest) theme of @carbon/themes 11; the token each color comes from.
/// `background`
pub const BG: u32 = 0x161616;
/// `layer-01`
pub const SURFACE: u32 = 0x262626;
/// `layer-02`
pub const RAISED: u32 = 0x393939;
/// `layer-hover-02`
pub const HOVER: u32 = 0x474747;
/// `text-primary`
pub const TEXT: u32 = 0xf4f4f4;
/// `text-helper`
pub const MUTED: u32 = 0xa8a8a8;
/// `border-strong-01`
pub const FAINT: u32 = 0x6f6f6f;
/// `link-primary`
pub const ACCENT: u32 = 0x78a9ff;
/// `interactive`: the track of a switch that is on.
pub const BLUE: u32 = 0x4589ff;
/// `support-error`
pub const DANGER: u32 = 0xfa4d56;

pub fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// `color` with an alpha of 0 to 255 (`rgba` wants 0xRRGGBBAA).
pub fn alpha(color: u32, a: u8) -> Rgba {
    rgba((color << 8) | a as u32)
}

/// Hairline between panels: the text color at 16%.
pub fn border() -> Rgba {
    alpha(TEXT, 0x29)
}

static FONTS: OnceLock<(SharedString, SharedString)> = OnceLock::new();

/// The interface fonts of the design, built into the program (SIL Open Font License, the license
/// texts are next to the files in assets/fonts). Only the weights the interface uses.
const FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/Archivo-Regular.ttf"),
    include_bytes!("../../assets/fonts/Archivo-Medium.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexMono-Medium.ttf"),
];

/// Loads the built-in Archivo and IBM Plex Mono. Should that fail, the closest fonts that ship
/// with Windows take their place.
pub fn init(cx: &App) {
    let _ = cx.text_system().add_fonts(FONT_FILES.iter().map(|&f| Cow::Borrowed(f)).collect());
    let names = cx.text_system().all_font_names();
    let pick = |wanted: &str, fallback: &str| -> SharedString {
        if names.iter().any(|n| n == wanted) { wanted.to_owned().into() } else { fallback.to_owned().into() }
    };
    let _ = FONTS.set((pick("Archivo", "Segoe UI"), pick("IBM Plex Mono", "Consolas")));
}

pub fn sans() -> SharedString {
    FONTS.get().map(|f| f.0.clone()).unwrap_or_else(|| "Segoe UI".into())
}

pub fn mono() -> SharedString {
    FONTS.get().map(|f| f.1.clone()).unwrap_or_else(|| "Consolas".into())
}
