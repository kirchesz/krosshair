//! Data types: the crosshair being drawn in the editor, and the saved configuration.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Half size of the custom pixel grid: cells go from -GRID_HALF to +GRID_HALF on both axes.
pub const GRID_HALF: i32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Cross,
    T,
    Circle,
    Dot,
    Custom,
}

impl Shape {
    pub const ALL: [Shape; 5] = [Shape::Cross, Shape::T, Shape::Circle, Shape::Dot, Shape::Custom];

    pub fn name(self) -> &'static str {
        match self {
            Shape::Cross => "Cross",
            Shape::T => "T",
            Shape::Circle => "Circle",
            Shape::Dot => "Dot",
            Shape::Custom => "Custom",
        }
    }
}

/// One layer of a crosshair in the editor. All sizes are screen pixels.
#[derive(Clone, PartialEq, Debug)]
pub struct EditLayer {
    pub shape: Shape,
    /// 0xRRGGBB
    pub color: u32,
    /// Percent, 20..=100.
    pub opacity: u8,
    /// An outline around the shape, [`EditLayer::outline_width`] pixels of
    /// [`EditLayer::outline_color`].
    pub outline: bool,
    /// 1 to [`MAX_OUTLINE`].
    pub outline_width: i32,
    /// 0xRRGGBB
    pub outline_color: u32,
    /// Percent, 10..=100, on top of [`EditLayer::opacity`].
    pub outline_opacity: u8,
    pub len: i32,
    pub thickness: i32,
    pub gap: i32,
    pub radius: i32,
    pub dot: i32,
    /// Painted cells of the custom grid, (x, y) with 0,0 in the middle.
    pub pixels: BTreeSet<(i32, i32)>,
    /// Degrees clockwise around the middle of the shape, 0 to 359.
    pub rotation: i32,
}

pub const MAX_OUTLINE: i32 = 3;

impl EditLayer {
    pub fn new(shape: Shape) -> Self {
        Self {
            shape,
            color: 0xffffff,
            opacity: 100,
            outline: true,
            outline_width: 1,
            outline_color: 0x000000,
            outline_opacity: 100,
            len: 6,
            thickness: 2,
            gap: 3,
            radius: 9,
            dot: 2,
            pixels: BTreeSet::new(),
            rotation: 0,
        }
    }
}

/// A crosshair from the gallery placed on the screen.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct LayerRef {
    /// File name inside the gallery folder.
    pub file: String,
    /// Cell of the 3x3 screen grid, row by row: 0 = top left, 4 = center, 8 = bottom right.
    pub anchor: u8,
    pub ox: i32,
    pub oy: i32,
    pub on: bool,
    /// Degrees clockwise, 0 to 359, around the layer's own middle. The crosshair rotation adds
    /// to it.
    #[serde(default)]
    pub rotation: i32,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub layers: Vec<LayerRef>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Config {
    pub gallery_dir: PathBuf,
    /// Global color mask, 0xRRGGBB. Every crosshair color is multiplied by it; white changes nothing.
    pub mask: u32,
    /// Overlay on/off.
    pub power: bool,
    /// Every crosshair is drawn this many times bigger, offsets included: [`SCALE_MIN`] to
    /// [`SCALE_MAX`] in steps of [`SCALE_STEP`]. Whole values repeat pixels and stay sharp
    /// ([`scale_is_sharp`]); the others blend them.
    #[serde(default = "one")]
    pub scale: f32,
    /// The crosshair turns this many degrees clockwise, 0 to 359. Quarter turns stay sharp
    /// ([`rotation_is_sharp`]); other angles smooth the edges.
    #[serde(default)]
    pub rotation: i32,
    /// When the window goes to the notification area instead of the taskbar or quitting.
    /// Configurations from before it had `hide_to_tray: bool`, see `store::parse_config`.
    #[serde(default)]
    pub tray: TrayMode,
    /// The tray choice was made: asked on the first minimize or close, or set in the settings.
    /// Until then those two are held and the window asks.
    #[serde(default)]
    pub tray_asked: bool,
    /// Look for a newer version on GitHub (see `crate::update`).
    #[serde(default = "yes")]
    pub check_updates: bool,
    pub current: usize,
    pub profiles: Vec<Profile>,
}

pub const SCALE_MIN: f32 = 0.5;
pub const SCALE_MAX: f32 = 4.0;
/// Steps per 1x: the scale moves in steps of 0.05.
pub const SCALE_STEPS_PER_UNIT: f32 = 20.0;

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

/// When the window hides to the notification area. Hidden, the crosshair stays on; a click on
/// the icon brings the window back.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayMode {
    /// Minimizing goes to the taskbar, closing quits.
    Never,
    /// Minimizing hides the window; closing quits.
    #[default]
    OnMinimize,
    /// Closing hides the window, so krosshair goes on running; minimizing goes to the taskbar.
    /// Quit from the icon's menu ends it.
    OnClose,
}

impl TrayMode {
    pub const ALL: [TrayMode; 3] = [TrayMode::Never, TrayMode::OnMinimize, TrayMode::OnClose];

    pub fn label(self) -> &'static str {
        match self {
            TrayMode::Never => "Never",
            TrayMode::OnMinimize => "On minimize",
            TrayMode::OnClose => "On close",
        }
    }
}

/// A usable scale from whatever the configuration holds: within the range, on a step. Computed
/// as a count of steps divided by [`SCALE_STEPS_PER_UNIT`], so whole values come out exact.
pub fn clean_scale(scale: f32) -> f32 {
    let s = if scale.is_finite() { scale.clamp(SCALE_MIN, SCALE_MAX) } else { 1.0 };
    (s * SCALE_STEPS_PER_UNIT).round() / SCALE_STEPS_PER_UNIT
}

/// Whole scales enlarge by repeating pixels, so the crosshair stays sharp.
pub fn scale_is_sharp(scale: f32) -> bool {
    clean_scale(scale).fract() == 0.0
}

/// Any angle as 0 to 359 degrees.
pub fn clean_rotation(degrees: i32) -> i32 {
    degrees.rem_euclid(360)
}

/// Quarter turns move pixels exactly, so the crosshair stays sharp.
pub fn rotation_is_sharp(degrees: i32) -> bool {
    clean_rotation(degrees) % 90 == 0
}

/// The scale as people read it: "1×", "1.5×", "1.05×".
pub fn scale_label(scale: f32) -> String {
    let text = format!("{:.2}", clean_scale(scale));
    format!("{}×", text.trim_end_matches('0').trim_end_matches('.'))
}

impl Config {
    pub fn profile(&self) -> &Profile {
        &self.profiles[self.current.min(self.profiles.len() - 1)]
    }

    pub fn profile_mut(&mut self) -> &mut Profile {
        let i = self.current.min(self.profiles.len() - 1);
        &mut self.profiles[i]
    }
}

pub const ANCHOR_NAMES: [&str; 9] = [
    "top left",
    "top center",
    "top right",
    "middle left",
    "center",
    "middle right",
    "bottom left",
    "bottom center",
    "bottom right",
];

/// Crosshairs written to a new gallery folder on first start: (file stem, layers).
pub fn presets() -> Vec<(&'static str, Vec<EditLayer>)> {
    let white = |shape: Shape| EditLayer { color: 0xffffff, ..EditLayer::new(shape) };
    let cross = |len, thickness, gap| EditLayer { len, thickness, gap, ..white(Shape::Cross) };
    let t = |len, thickness, gap| EditLayer { len, thickness, gap, ..white(Shape::T) };
    let circle = |radius, thickness| EditLayer { radius, thickness, ..white(Shape::Circle) };
    let dot = |dot| EditLayer { dot, ..white(Shape::Dot) };
    vec![
        ("cross", vec![cross(8, 2, 0)]),
        ("cross-gap", vec![cross(6, 2, 3)]),
        ("cross-small", vec![cross(4, 2, 0)]),
        ("cross-bold", vec![cross(6, 4, 0)]),
        ("cross-dot", vec![cross(6, 2, 4), dot(2)]),
        ("t-shape", vec![t(6, 2, 3)]),
        ("dot", vec![dot(4)]),
        ("dot-small", vec![dot(2)]),
        ("ring", vec![circle(9, 2)]),
        ("ring-dot", vec![circle(9, 2), dot(2)]),
        ("ring-cross", vec![circle(10, 2), cross(4, 2, 0)]),
        ("ring-ticks", vec![circle(8, 2), cross(5, 2, 11)]),
    ]
}
