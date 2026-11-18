//! Puts the layers of a profile on screen through the RTSS on-screen display.
//!
//! RTSS draws sprites from one PNG (`<LI>` loads it, `<I>` draws a part of it) and has nine
//! "sticky" positions, a 3x3 grid over the screen that it places itself whatever the window size,
//! OSD zoom or OSD origin are. Every profile layer becomes one sprite at its anchor, moved by its
//! offset with a layer extent `<E>`. All pictures, and every frame of the animated ones, are packed
//! into one atlas PNG. An animation is played by rewriting the OSD text with the coordinates of the
//! next frame when its time comes.
//!
//! Measured on RTSS 7.3.5:
//! - The tag order has to be `<Pn><Ln>`; `<Ln><Pn>` lands in the wrong place.
//! - Sprites are stacked in text order: what comes later is drawn on top.
//! - `<E=w,h,origin>` makes the layer `w` x `h` and puts its content in the `origin` cell (0..8,
//!   the 3x3 grid again) of that box. It is exact at every anchor. It can only enlarge.
//! - A layer lasts until the next `<L>` tag, and RTSS appends its own text (framerate counter)
//!   after ours. Left in our layer it makes the layer bigger and shifts the sprite, so the text
//!   ends with `\b<P=0,0><L254>`, and a second slot restores the text size.
//! - `<S=1>` before `<I>` shrinks the text line to almost nothing. With `<S=10>` the line was still
//!   4 zoomed pixels tall in a 29 px font, and sprites smaller than that sat up to 2 px too high.
//! - `<LI>` fails on long paths, so the atlas lives in `%ProgramData%\krosshair`.
//! - An atlas up to 2048 x 2048 is drawn; with a side of 4096 or more it is not.
//! - Rewriting the text up to 60 times a second, every frame reaches the screen (`tools/animtest`).
//! - Sizes and positions in tags are in zoomed pixels: with the profile's "On-Screen Display zoom"
//!   at 2, `<I=12,12,...>` covers 24 x 24 screen pixels. `<I>` takes the shown size and the part of
//!   the atlas separately, so a sprite drawn at 1/zoom of its size is exactly 1:1 and sharp. Offsets
//!   and extents can then only be multiples of the zoom, so the part of the atlas taken is widened
//!   with transparent pixels to whole zoomed pixels. `<FNT=face,height,weight,zoom>` would set the
//!   zoom too, but for the whole OSD: the framerate counter and every other program's text with it.

use crate::model::LayerRef;
use crate::raster::{timeline_at, Bitmap, Picture};
use crate::rtss;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{channel, sync_channel, RecvTimeoutError, Sender, SyncSender};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SLOT: &str = "krosshair";
/// The slot after the crosshair's that restores the text size.
const SLOT_RESET: &str = "krosshair_reset";
const RESET_TEXT: &str = "<P=0,0><S=100>";
/// Atlas file names start with this.
const ATLAS_PREFIX: &str = "osd_";
/// Transparent pixels around every picture in the atlas. A sprite at zoom `z` may take up to
/// `z - 1` of them on each side, and scaled sprites don't pick up a neighbor.
const ATLAS_PAD: u32 = 4;
/// The highest zoom the crosshair stays pixel exact at (RTSS offers 1 to 5).
const MAX_ZOOM: u32 = ATLAS_PAD + 1;
/// Largest atlas side RTSS draws (with the default font texture size 2048).
pub const ATLAS_MAX: u32 = 2048;
/// RTSS layer of the first profile layer. 254 closes them, so this leaves room for 54.
const FIRST_LAYER: usize = 200;
pub const MAX_LAYERS: usize = 54;
/// How often the overlay checks which zoom the game uses.
const ZOOM_POLL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// One profile layer on screen.
#[derive(Clone, PartialEq, Debug)]
pub struct Draw {
    /// Cell of the 3x3 screen grid.
    pub anchor: u8,
    /// Position among the visible layers, which sets its RTSS layer and stacking.
    pub layer: usize,
    /// Where its picture goes relative to the anchor point, in screen pixels: `[start, end)`.
    pub xr: (i32, i32),
    pub yr: (i32, i32),
    /// Where its picture is in the atlas, one entry per frame (a still picture has one).
    pub rects: Vec<Rect>,
    /// How long each frame stays, in milliseconds. Empty for a still picture.
    pub delays: Vec<u32>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Frame {
    pub atlas: Bitmap,
    pub draws: Vec<Draw>,
}

/// What [`compose`] made, and what it had to leave out.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Composed {
    pub frame: Option<Frame>,
    pub note: Option<String>,
}

/// Where a picture of `size` goes on one axis relative to the anchor point: `[start, start + size)`.
fn span(cell: u8, size: i32, offset: i32) -> (i32, i32) {
    let start = match cell {
        0 => offset,
        1 => offset - size / 2,
        _ => offset - size,
    };
    (start, start + size)
}

/// The part of the axis an anchor can show: from the screen edge inwards for the edge cells.
fn visible(cell: u8) -> (i32, i32) {
    match cell {
        0 => (0, i32::MAX),
        1 => (i32::MIN, i32::MAX),
        _ => (i32::MIN, 0),
    }
}

/// The layer extent and alignment (0 start, 1 middle, 2 end) that put content `[a, b)` where it
/// belongs on one axis, and whether the extent differs from the content.
fn extent(cell: u8, a: i32, b: i32) -> (i32, u8, bool) {
    match cell {
        0 => (b, 2, a != 0),
        1 => {
            // RTSS centers the layer on the middle of the screen: make it symmetric.
            let half = (-a).max(b);
            let align = if -a == b { 1 } else if b == half { 2 } else { 0 };
            (2 * half, align, 2 * half != b - a)
        }
        _ => (-a, 0, b != 0),
    }
}

/// Distinct pictures already in the atlas, by size and pixels, to their index.
type Seen = HashMap<(u32, u32, Vec<[u8; 4]>), usize>;

/// A layer cut down to what is visible, before packing.
struct Cut {
    anchor: u8,
    layer: usize,
    xr: (i32, i32),
    yr: (i32, i32),
    /// Indices into the list of distinct pictures, one per frame.
    pics: Vec<usize>,
    delays: Vec<u32>,
}

/// A layer's frames ready to be cut, at the crosshair scale and angle.
struct Prepared {
    frames: Vec<Arc<Bitmap>>,
    /// Whole-step enlargement still to apply (repeating pixels, done in `cut` after cropping).
    k: i32,
    /// Where the top left of `frames` (after `k`) is, relative to the anchor point, in screen
    /// pixels.
    at: (i32, i32),
    /// The middle of the whole picture, before cropping, relative to the anchor point.
    middle: (f32, f32),
}

/// Union of the visible pixels of all frames: `x0, y0, x1, y1`.
fn union_bounds(frames: &[Arc<Bitmap>]) -> Option<(u32, u32, u32, u32)> {
    frames.iter().filter_map(|f| f.bounds()).reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
}

/// The frames of a layer at `scale`, placed relative to its anchor point (its offset is already
/// at the crosshair scale). Whole steps are left to `cut` (sharp); anything else is resampled
/// here, after cropping to the visible pixels, so big pictures don't grow in memory first.
fn prepare(frames: Vec<Arc<Bitmap>>, scale: f32, layer: &LayerRef) -> Option<Prepared> {
    let (col, row) = (layer.anchor % 3, layer.anchor / 3);
    let (w, h) = (frames[0].w, frames[0].h);
    let place = |full: (i32, i32)| {
        let (xs, ys) = (span(col, full.0, layer.ox), span(row, full.1, layer.oy));
        ((xs.0, ys.0), (xs.0 as f32 + full.0 as f32 / 2., ys.0 as f32 + full.1 as f32 / 2.))
    };
    if scale.fract() == 0.0 {
        let k = scale as i32;
        let (at, middle) = place((w as i32 * k, h as i32 * k));
        return Some(Prepared { frames, k, at, middle });
    }
    let (bx0, by0, bx1, by1) = union_bounds(&frames)?;
    let sc = |v: u32| (v as f32 * scale).round() as i32;
    let (tx0, ty0) = (sc(bx0), sc(by0));
    let (tx1, ty1) = (sc(bx1).max(tx0 + 1), sc(by1).max(ty0 + 1));
    let frames = frames
        .iter()
        .map(|f| Arc::new(f.crop(bx0, by0, bx1 - bx0, by1 - by0).resampled((tx1 - tx0) as u32, (ty1 - ty0) as u32)))
        .collect();
    let (at, middle) = place((sc(w).max(1), sc(h).max(1)));
    Some(Prepared { frames, k: 1, at: (at.0 + tx0, at.1 + ty0), middle })
}

/// Turns a layer: first `own` degrees clockwise around its own middle, then the crosshair's
/// `crosshair` degrees. For that a layer at the screen center turns around the center, its offset
/// with it, so a crosshair of several layers turns as a whole; elsewhere a layer turns around its
/// own middle again and stays in place. Both turns are done as one, so the picture is resampled
/// once, and when they add up to a quarter turn it moves pixels exactly.
fn rotate(prep: Prepared, own: i32, crosshair: i32, anchor: u8) -> Prepared {
    let total = (own + crosshair).rem_euclid(360);
    let around_center = anchor == 4 && crosshair != 0;
    if total == 0 && !around_center {
        return prep;
    }
    // Turning around the screen center is the same as turning around the layer's middle and then
    // moving that middle to where the turn takes it. With no turn of its own, the layer turns
    // around the center directly, which needs no rounding of that move.
    let (pivot, shift) = if anchor == 4 && own == 0 {
        ((0., 0.), (0, 0))
    } else {
        let m = prep.middle;
        // On a quarter turn a pivot on a pixel corner maps pixels onto pixels.
        let m = if total % 90 == 0 { (m.0.round(), m.1.round()) } else { m };
        let shift = if around_center {
            let (sin, cos) = (crosshair as f32).to_radians().sin_cos();
            ((m.0 * cos - m.1 * sin - m.0).round() as i32, (m.0 * sin + m.1 * cos - m.1).round() as i32)
        } else {
            (0, 0)
        };
        (m, shift)
    };
    let pivot = if total % 90 == 0 { (pivot.0.round(), pivot.1.round()) } else { pivot };
    let k = prep.k as u32;
    let mut at = prep.at;
    let frames = prep
        .frames
        .iter()
        .map(|f| {
            let (bmp, top_left) = f.scaled(k).rotated((prep.at.0 as f32, prep.at.1 as f32), pivot, total);
            at = (top_left.0 + shift.0, top_left.1 + shift.1);
            Arc::new(bmp)
        })
        .collect();
    Prepared { frames, k: 1, at, middle: prep.middle }
}

/// Cuts a layer: its frames cropped to the visible pixels, deduplicated into `pics`.
fn cut(
    index: usize,
    layer: &LayerRef,
    prep: &Prepared,
    delays: &[u32],
    pics: &mut Vec<Bitmap>,
    seen: &mut Seen,
) -> Option<Cut> {
    let (col, row) = (layer.anchor % 3, layer.anchor / 3);
    let (k, frames) = (prep.k, &prep.frames);
    // Everything below is in screen pixels relative to the anchor point.
    let (a, b, c, d) = union_bounds(frames)?;
    let (ox, oy) = prep.at;
    let (mut x0, mut y0, mut x1, mut y1) = (ox + a as i32 * k, oy + b as i32 * k, ox + c as i32 * k, oy + d as i32 * k);
    // Cut off what falls past the screen edge.
    let (vx, vy) = (visible(col), visible(row));
    x0 = x0.max(vx.0);
    x1 = x1.min(vx.1);
    y0 = y0.max(vy.0);
    y1 = y1.min(vy.1);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }


    let ids = frames
        .iter()
        .map(|f| {
            // Enlarge only the source pixels that are needed, then take the exact part.
            let (lx0, ly0, lx1, ly1) = (x0 - ox, y0 - oy, x1 - ox, y1 - oy);
            let (sx0, sy0) = (lx0 / k, ly0 / k);
            let (sx1, sy1) = ((lx1 + k - 1) / k, (ly1 + k - 1) / k);
            let src = f.crop(sx0 as u32, sy0 as u32, (sx1 - sx0) as u32, (sy1 - sy0) as u32);
            let big = src.scaled(k as u32);
            let pic = big.crop((lx0 - sx0 * k) as u32, (ly0 - sy0 * k) as u32, (x1 - x0) as u32, (y1 - y0) as u32);
            *seen.entry((pic.w, pic.h, pic.px.clone())).or_insert_with(|| {
                pics.push(pic);
                pics.len() - 1
            })
        })
        .collect();
    Some(Cut {
        anchor: layer.anchor,
        layer: index,
        xr: (x0, x1),
        yr: (y0, y1),
        pics: ids,
        delays: delays.to_vec(),
    })
}

/// Atlas width and height, and where each picture goes in it.
type Packed = (u32, u32, Vec<(u32, u32)>);

/// Places the pictures in rows, tallest first, with [`ATLAS_PAD`] transparent pixels around each.
/// `None` when they don't fit in the atlas.
fn pack(pics: &[Bitmap]) -> Option<Packed> {
    let mut order: Vec<usize> = (0..pics.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(pics[i].h));
    let mut at = vec![(0, 0); pics.len()];
    let (mut x, mut y, mut row_h, mut width) = (ATLAS_PAD, ATLAS_PAD, 0u32, 0u32);
    for i in order {
        let (w, h) = (pics[i].w, pics[i].h);
        if w + 2 * ATLAS_PAD > ATLAS_MAX {
            return None;
        }
        if x > ATLAS_PAD && x + w + ATLAS_PAD > ATLAS_MAX {
            y += row_h + ATLAS_PAD;
            x = ATLAS_PAD;
            row_h = 0;
        }
        at[i] = (x, y);
        width = width.max(x + w + ATLAS_PAD);
        row_h = row_h.max(h);
        x += w + ATLAS_PAD;
    }
    let height = y + row_h + ATLAS_PAD;
    (height <= ATLAS_MAX).then_some((width, height, at))
}

/// Builds the atlas and the draws for the visible layers. `picture` gives the picture of a
/// gallery file. Every picture and offset is `scale` times bigger: whole steps repeat pixels, so
/// they stay sharp; other values average them. Offsets are rounded to whole pixels. Then the
/// crosshair turns `rotation` degrees clockwise (see [`rotate`]). When the animations don't fit
/// in the atlas, the biggest ones are shown as still pictures, and the note says so.
pub fn compose(
    layers: &[LayerRef],
    picture: impl Fn(&str) -> Option<Picture>,
    mask: u32,
    scale: f32,
    rotation: i32,
) -> Composed {
    let scale = crate::model::clean_scale(scale);
    let rotation = rotation.rem_euclid(360);
    let at = |v: i32| (v as f32 * scale).round() as i32;
    let mut notes = Vec::new();
    let mut shown: Vec<(LayerRef, Picture)> = layers
        .iter()
        .filter(|l| l.on)
        .filter_map(|l| Some((LayerRef { ox: at(l.ox), oy: at(l.oy), ..l.clone() }, picture(&l.file)?)))
        .collect();
    if shown.len() > MAX_LAYERS {
        shown.truncate(MAX_LAYERS);
        notes.push(format!("only the first {MAX_LAYERS} layers are shown"));
    }

    // Layers whose animation is dropped to make the atlas fit.
    let mut stilled: Vec<usize> = Vec::new();
    loop {
        let mut pics = Vec::new();
        let mut seen = HashMap::new();
        let mut cuts = Vec::new();
        // Distinct pixels each animated layer adds, to pick which one to still first.
        let mut weight: Vec<(usize, u64)> = Vec::new();
        for (i, (layer, pic)) in shown.iter().enumerate() {
            let anim = pic.anim.as_ref().filter(|_| !stilled.contains(&i));
            let (frames, delays) = match anim {
                Some(a) => (a.frames.clone(), a.delays.clone()),
                None => (vec![pic.still.clone()], Vec::new()),
            };
            let before = pics.len();
            let prepared = prepare(frames, scale, layer).map(|p| rotate(p, layer.rotation, rotation, layer.anchor));
            if let Some(c) = prepared.and_then(|p| cut(i, layer, &p, &delays, &mut pics, &mut seen)) {
                if anim.is_some() {
                    weight.push((i, pics[before..].iter().map(|p| p.w as u64 * p.h as u64).sum()));
                }
                cuts.push(c);
            }
        }
        if cuts.is_empty() {
            return Composed { frame: None, note: join(notes) };
        }

        let Some((w, h, at)) = pack(&pics) else {
            match weight.iter().max_by_key(|w| w.1) {
                Some(&(i, _)) => {
                    stilled.push(i);
                    continue;
                }
                None => {
                    notes.push(format!("the crosshair is too big for RTSS (over {ATLAS_MAX} px)"));
                    return Composed { frame: None, note: join(notes) };
                }
            }
        };

        let mut atlas = Bitmap::new(w, h);
        for (pic, &(x, y)) in pics.iter().zip(&at) {
            atlas.draw(pic, x as i32, y as i32, 1.0);
        }
        let draws = cuts
            .into_iter()
            .map(|c| Draw {
                anchor: c.anchor,
                layer: c.layer,
                xr: c.xr,
                yr: c.yr,
                rects: c.pics.iter().map(|&p| Rect { x: at[p].0, y: at[p].1, w: pics[p].w, h: pics[p].h }).collect(),
                delays: c.delays,
            })
            .collect();
        if !stilled.is_empty() {
            let names: Vec<&str> = stilled.iter().map(|&i| shown[i].0.file.as_str()).collect();
            notes.push(format!("too big to animate in RTSS, shown still: {}", names.join(", ")));
        }
        return Composed { frame: Some(Frame { atlas: atlas.tinted(mask), draws }), note: join(notes) };
    }
}

fn join(notes: Vec<String>) -> Option<String> {
    (!notes.is_empty()).then(|| {
        let text = notes.join("; ");
        let mut chars = text.chars();
        chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
    })
}

/// `[a, b)` widened to whole multiples of `z`.
fn snap(a: i32, b: i32, z: i32) -> (i32, i32) {
    (a.div_euclid(z) * z, -(-b).div_euclid(z) * z)
}

/// The tags that draw picture `r` of a layer at zoom `zoom`, 1:1 on screen.
fn sprite(d: &Draw, r: Rect, zoom: u32) -> String {
    let z = zoom.clamp(1, MAX_ZOOM) as i32;
    let (col, row) = (d.anchor % 3, d.anchor / 3);
    let (x0, x1) = snap(d.xr.0, d.xr.1, z);
    let (y0, y1) = snap(d.yr.0, d.yr.1, z);
    let (ex, ax, need_x) = extent(col, x0 / z, x1 / z);
    let (ey, ay, need_y) = extent(row, y0 / z, y1 / z);
    let mut tags = format!("<P{}><L{}>", d.anchor, FIRST_LAYER + d.layer);
    if need_x || need_y {
        tags += &format!("<E={ex},{ey},{}>", ay * 3 + ax);
    }
    // The part of the atlas: the picture plus the transparent pixels that widen it.
    let (sx, sy) = (r.x as i32 - (d.xr.0 - x0), r.y as i32 - (d.yr.0 - y0));
    let (sw, sh) = (x1 - x0, y1 - y0);
    tags + &format!("<S=1><I={},{},{sx},{sy},{sw},{sh}>", sw / z, sh / z)
}

/// The OSD text that draws every layer, with frame `current[i]` of draw `i`, at zoom `zoom`.
fn hypertext(path: &str, draws: &[Draw], current: &[usize], zoom: u32) -> String {
    let mut text = format!("<LI=\"{path}\">");
    for (d, &f) in draws.iter().zip(current) {
        text += &sprite(d, d.rects[f.min(d.rects.len() - 1)], zoom);
    }
    text + "\u{8}<P=0,0><L254>"
}

/// The zoom to draw for: `KROSSHAIR_ZOOM` when set (for testing), else the game's.
fn current_zoom() -> Option<u32> {
    let forced = std::env::var("KROSSHAIR_ZOOM").ok().and_then(|z| z.parse().ok());
    forced.or_else(rtss::game_zoom)
}

fn image_dir() -> PathBuf {
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    base.join("krosshair")
}

fn fnv1a(hash: &mut u32, bytes: &[u8]) {
    for b in bytes {
        *hash = (*hash ^ *b as u32).wrapping_mul(0x0100_0193);
    }
}

/// What is on screen: the frame, the atlas file written for it, and where the animations are.
struct Shown {
    frame: Frame,
    path: PathBuf,
    /// When the animations started.
    start: Instant,
    /// The frame each draw shows.
    current: Vec<usize>,
    /// Whether the atlases of earlier frames were removed (after the first write of this one).
    cleaned: bool,
    /// The game's "On-Screen Display zoom" the text is written for.
    zoom: u32,
}

impl Shown {
    fn animated(&self) -> bool {
        self.frame.draws.iter().any(|d| !d.delays.is_empty())
    }

    fn text(&self) -> String {
        hypertext(&self.path.to_string_lossy(), &self.frame.draws, &self.current, self.zoom)
    }

    /// Moves the animations to now. Returns whether a frame changed and how long until the next
    /// change.
    fn advance(&mut self) -> (bool, Option<Duration>) {
        let elapsed = self.start.elapsed().as_millis() as u64;
        let (mut changed, mut next) = (false, None::<u32>);
        for (d, cur) in self.frame.draws.iter().zip(self.current.iter_mut()) {
            if d.delays.is_empty() {
                continue;
            }
            let (i, left) = timeline_at(&d.delays, elapsed);
            if i != *cur {
                *cur = i;
                changed = true;
            }
            next = Some(next.map_or(left, |n| n.min(left)));
        }
        (changed, next.map(|ms| Duration::from_millis(ms as u64)))
    }
}

/// Writes the atlas as `osd_<content hash>_<time>.png`. The name is new every time: RTSS caches
/// images by path, and a path it once failed to load stays broken.
fn store(frame: Frame, zoom: u32) -> Option<Shown> {
    let mut hash = 0x811c_9dc5u32;
    fnv1a(&mut hash, &frame.atlas.w.to_le_bytes());
    for p in &frame.atlas.px {
        fnv1a(&mut hash, p);
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u32);
    let dir = image_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{ATLAS_PREFIX}{hash:08x}_{stamp:08x}.png"));
    frame.atlas.save_png(&path).ok()?;
    path.to_str()?;
    let current = vec![0; frame.draws.len()];
    Some(Shown { frame, path, start: Instant::now(), current, cleaned: false, zoom })
}

/// Deletes the atlas files except `keep`.
fn remove_atlases(keep: Option<&std::path::Path>) {
    let Ok(entries) = std::fs::read_dir(image_dir()) else { return };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(ATLAS_PREFIX) && name.ends_with(".png") && Some(e.path().as_path()) != keep {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Asks Windows for 1 ms timer precision while an animation plays, so frames change on time.
struct FineTimer(bool);

impl FineTimer {
    fn set(&mut self, on: bool) {
        use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
        if on != self.0 {
            unsafe {
                if on {
                    timeBeginPeriod(1);
                } else {
                    timeEndPeriod(1);
                }
            }
            self.0 = on;
        }
    }
}

impl Drop for FineTimer {
    fn drop(&mut self) {
        self.set(false);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// Nothing is shown: the overlay is off or the profile has no visible layers.
    Off,
    /// The crosshair is in the RTSS OSD.
    Active,
    /// RTSS isn't running (or has no free OSD slot).
    NoRtss,
}

enum Cmd {
    Show(Frame),
    Hide,
    Quit(SyncSender<()>),
}

/// Handle to the thread that keeps the RTSS slots up to date.
pub struct Overlay {
    tx: Sender<Cmd>,
    status: Arc<AtomicU8>,
}

impl Overlay {
    pub fn start() -> Overlay {
        let (tx, rx) = channel::<Cmd>();
        let status = Arc::new(AtomicU8::new(Status::Off as u8));
        let shared = status.clone();
        std::thread::spawn(move || {
            let set = |s: Status| shared.store(s as u8, Ordering::Relaxed);
            let mut shown: Option<Shown> = None;
            let mut timer = FineTimer(false);
            // True while the text still has to be written (new text, or the OSD was locked).
            let mut pending = false;
            // Kept when no game is hooked, so the text doesn't change in between.
            let mut zoom = current_zoom().unwrap_or(1);
            loop {
                let mut wait = if pending { Duration::from_millis(2) } else { ZOOM_POLL };
                if let Some(next) = shown.as_mut().and_then(|s| s.advance().1) {
                    wait = wait.min(next);
                }
                timer.set(shown.as_ref().is_some_and(Shown::animated));
                match rx.recv_timeout(wait) {
                    Ok(Cmd::Show(frame)) => {
                        shown = store(frame, zoom);
                        pending = shown.is_some();
                    }
                    Ok(Cmd::Hide) => {
                        shown = None;
                        pending = false;
                        release();
                        set(Status::Off);
                    }
                    Ok(Cmd::Quit(done)) => {
                        release();
                        let _ = done.send(());
                        return;
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        release();
                        return;
                    }
                }
                // The atlas file disappeared (something cleaned the folder): write it again.
                if shown.as_ref().is_some_and(|s| !s.path.exists()) {
                    shown = shown.take().and_then(|s| store(s.frame, zoom));
                    pending = shown.is_some();
                }
                let Some(now) = shown.as_mut() else { continue };
                // The game, or its zoom setting, changed: redraw at the new zoom.
                zoom = current_zoom().unwrap_or(zoom);
                if now.zoom != zoom {
                    now.zoom = zoom;
                    pending = true;
                }
                if now.advance().0 {
                    pending = true;
                }
                // A still crosshair is checked four times a second, which rewrites the slot after
                // RTSS was restarted. An animation rewrites it with every frame anyway.
                if pending || (!now.animated() && !rtss::holds(SLOT, &now.text())) {
                    match push(&now.text()) {
                        rtss::Update::Written => {
                            pending = false;
                            set(Status::Active);
                            if !now.cleaned {
                                // Only now is the previous atlas no longer referenced.
                                remove_atlases(Some(&now.path));
                                now.cleaned = true;
                            }
                        }
                        rtss::Update::Busy => pending = true,
                        rtss::Update::Unavailable => {
                            pending = false;
                            set(Status::NoRtss);
                        }
                    }
                }
            }
        });
        Overlay { tx, status }
    }

    /// Shows the frame, or hides the overlay when there is none.
    pub fn set(&self, frame: Option<Frame>) {
        let _ = self.tx.send(match frame {
            Some(frame) => Cmd::Show(frame),
            None => Cmd::Hide,
        });
    }

    pub fn status(&self) -> Status {
        match self.status.load(Ordering::Relaxed) {
            1 => Status::Active,
            2 => Status::NoRtss,
            _ => Status::Off,
        }
    }

    /// Removes the crosshair from the OSD and waits until that is done.
    pub fn shutdown(&self) {
        let (done, wait) = sync_channel(1);
        if self.tx.send(Cmd::Quit(done)).is_ok() {
            let _ = wait.recv_timeout(Duration::from_secs(2));
        }
    }
}

/// The crosshair slot has to come before the reset slot, so it is always written first.
fn push(text: &str) -> rtss::Update {
    match rtss::update(SLOT, text) {
        rtss::Update::Written => rtss::update(SLOT_RESET, RESET_TEXT),
        other => other,
    }
}

fn release() {
    rtss::release(SLOT);
    rtss::release(SLOT_RESET);
    remove_atlases(None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Anim;

    fn solid(w: u32, h: u32) -> Arc<Bitmap> {
        let mut b = Bitmap::new(w, h);
        b.px.fill([255, 255, 255, 255]);
        Arc::new(b)
    }

    fn still(bmp: Arc<Bitmap>) -> Option<Picture> {
        Some(Picture { still: bmp, anim: None })
    }

    fn layer(anchor: u8, ox: i32, oy: i32) -> LayerRef {
        LayerRef { file: "a.png".into(), anchor, ox, oy, on: true, rotation: 0 }
    }

    fn one(layers: &[LayerRef], pic: Option<Picture>) -> Frame {
        compose(layers, |_| pic.clone(), 0xffffff, 1.0, 0).frame.unwrap()
    }

    /// The tags of draw `i`, first frame, at `zoom`.
    fn tags(f: &Frame, i: usize, zoom: u32) -> String {
        sprite(&f.draws[i], f.draws[i].rects[0], zoom)
    }

    #[test]
    fn centered_layer_needs_no_extent() {
        let f = one(&[layer(4, 0, 0)], still(solid(6, 4)));
        // Pictures sit ATLAS_PAD pixels into the atlas.
        assert_eq!(tags(&f, 0, 1), "<P4><L200><S=1><I=6,4,4,4,6,4>");
    }

    #[test]
    fn center_offset_uses_a_symmetric_extent() {
        // 3 px right, 2 px up: content x [0, 6), y [-4, 0).
        let f = one(&[layer(4, 3, -2)], still(solid(6, 4)));
        assert_eq!(tags(&f, 0, 1), "<P4><L200><E=12,8,2><S=1><I=6,4,4,4,6,4>");
        // Odd size, no offset: x [-2, 3) goes right of the middle, like the editor centers it.
        let f = one(&[layer(4, 0, 0)], still(solid(5, 4)));
        assert_eq!(tags(&f, 0, 1), "<P4><L200><E=6,4,5><S=1><I=5,4,4,4,5,4>");
    }

    #[test]
    fn edge_offsets_move_into_the_screen() {
        let f = one(&[layer(0, 5, 7)], still(solid(2, 2)));
        assert_eq!(tags(&f, 0, 1), "<P0><L200><E=7,9,8><S=1><I=2,2,4,4,2,2>");
        let f = one(&[layer(8, -5, -7)], still(solid(2, 2)));
        assert_eq!(tags(&f, 0, 1), "<P8><L200><E=7,9,0><S=1><I=2,2,4,4,2,2>");
        let f = one(&[layer(5, -11, 6)], still(solid(20, 20)));
        assert_eq!(tags(&f, 0, 1), "<P5><L200><E=31,32,6><S=1><I=20,20,4,4,20,20>");
    }

    // The zoom 2 cases below are the ones measured exact and sharp on the bench.
    #[test]
    fn zoom_draws_at_one_over_zoom() {
        let f = one(&[layer(4, 0, 0)], still(solid(12, 12)));
        assert_eq!(tags(&f, 0, 2), "<P4><L200><S=1><I=6,6,4,4,12,12>");
        assert_eq!(tags(&f, 0, 3), "<P4><L200><S=1><I=4,4,4,4,12,12>");
    }

    #[test]
    fn zoom_widens_odd_offsets_with_transparent_pixels() {
        // x [-3, 9) becomes [-4, 10), y [-7, 5) becomes [-8, 6): one more pixel on every side.
        let f = one(&[layer(4, 3, -1)], still(solid(12, 12)));
        assert_eq!(tags(&f, 0, 2), "<P4><L200><E=10,8,2><S=1><I=7,7,3,3,14,14>");
        let f = one(&[layer(0, 5, 7)], still(solid(12, 12)));
        assert_eq!(tags(&f, 0, 2), "<P0><L200><E=9,10,8><S=1><I=7,7,3,3,14,14>");
        let f = one(&[layer(8, -5, -7)], still(solid(12, 12)));
        assert_eq!(tags(&f, 0, 2), "<P8><L200><E=9,10,0><S=1><I=7,7,3,3,14,14>");
        // Odd size: x [-2, 3) becomes [-2, 4).
        let f = one(&[layer(4, 0, 0)], still(solid(5, 4)));
        assert_eq!(tags(&f, 0, 2), "<P4><L200><E=4,2,5><S=1><I=3,2,4,4,6,4>");
    }

    #[test]
    fn zoom_above_the_padding_is_capped() {
        let f = one(&[layer(4, 1, 0)], still(solid(4, 4)));
        assert_eq!(tags(&f, 0, 9), tags(&f, 0, MAX_ZOOM));
        // The widened part never reaches past the transparent border around the picture.
        let t = tags(&f, 0, MAX_ZOOM);
        let src_x: i32 = t.split("<I=").nth(1).unwrap().split(',').nth(2).unwrap().parse().unwrap();
        assert!(src_x >= 0);
    }

    #[test]
    fn parts_past_the_screen_edge_are_cut() {
        let f = one(&[layer(0, -1, 0)], still(solid(2, 2)));
        assert_eq!(tags(&f, 0, 1), "<P0><L200><S=1><I=1,2,4,4,1,2>");
        assert!(compose(&[layer(0, -5, 0)], |_| still(solid(2, 2)), 0xffffff, 1.0, 0).frame.is_none());
    }

    #[test]
    fn transparent_margins_are_cropped() {
        let mut b = Bitmap::new(6, 6);
        for (x, y) in [(2, 2), (3, 2), (2, 3), (3, 3)] {
            b.px[y * 6 + x] = [255; 4];
        }
        let f = one(&[layer(4, 0, 0)], still(Arc::new(b)));
        assert_eq!(tags(&f, 0, 1), "<P4><L200><S=1><I=2,2,4,4,2,2>");
        assert_eq!((f.atlas.w, f.atlas.h), (2 + 2 * ATLAS_PAD, 2 + 2 * ATLAS_PAD));
    }

    #[test]
    fn layers_are_drawn_in_order_and_share_pictures() {
        let f = one(&[layer(4, 0, 0), layer(0, 3, 3), layer(4, 1, 0)], still(solid(4, 4)));
        let all: Vec<String> = (0..3).map(|i| tags(&f, i, 1)).collect();
        assert_eq!(
            all,
            [
                "<P4><L200><S=1><I=4,4,4,4,4,4>",
                "<P0><L201><E=7,7,8><S=1><I=4,4,4,4,4,4>",
                "<P4><L202><E=6,4,5><S=1><I=4,4,4,4,4,4>"
            ]
        );
        assert_eq!((f.atlas.w, f.atlas.h), (4 + 2 * ATLAS_PAD, 4 + 2 * ATLAS_PAD));
    }

    #[test]
    fn animation_frames_are_deduplicated() {
        let a = solid(4, 4);
        let mut b = (*a).clone();
        b.px[0] = [255, 0, 0, 255];
        let anim = Anim { frames: vec![a.clone(), Arc::new(b), a.clone()], delays: vec![50, 60, 70] };
        let pic = Some(Picture { still: a, anim: Some(Arc::new(anim)) });
        let f = one(&[layer(4, 0, 0)], pic);
        let d = &f.draws[0];
        assert_eq!(d.delays, vec![50, 60, 70]);
        assert_eq!(d.rects.len(), 3);
        assert_eq!(d.rects[0], d.rects[2]);
        assert_ne!(d.rects[0], d.rects[1]);
    }

    #[test]
    fn too_big_animation_is_shown_still() {
        let a = solid(1500, 1500);
        let mut b = (*a).clone();
        b.px[0] = [255, 0, 0, 255];
        let anim = Anim { frames: vec![a.clone(), Arc::new(b)], delays: vec![100, 100] };
        let pic = Picture { still: a, anim: Some(Arc::new(anim)) };
        let c = compose(&[layer(4, 0, 0)], |_| Some(pic.clone()), 0xffffff, 1.0, 0);
        let f = c.frame.unwrap();
        assert!(f.draws[0].delays.is_empty());
        assert_eq!(f.draws[0].rects.len(), 1);
        assert!(c.note.unwrap().contains("a.png"));
    }

    #[test]
    fn fractional_scales_resample_pictures_and_round_offsets() {
        // 4 x 4 at 3 px right, halved: 2 x 2 at 2 px right (1.5 rounded), x [1, 3).
        let f = compose(&[layer(4, 3, 0)], |_| still(solid(4, 4)), 0xffffff, 0.5, 0).frame.unwrap();
        assert_eq!(tags(&f, 0, 1), "<P4><L200><E=6,2,5><S=1><I=2,2,4,4,2,2>");
        // 2 x 2 at 1.5x: 3 x 3, odd, so right of and below the middle like any odd picture.
        let f = compose(&[layer(4, 0, 0)], |_| still(solid(2, 2)), 0xffffff, 1.5, 0).frame.unwrap();
        assert_eq!(tags(&f, 0, 1), "<P4><L200><E=4,4,8><S=1><I=3,3,4,4,3,3>");
        // Resampling a solid picture keeps it solid.
        assert!(f.atlas.crop(4, 4, 3, 3).px.iter().all(|p| *p == [255, 255, 255, 255]));
        // The same 2 x 2 inside transparent margins lands on the same spot: margins are cropped
        // before resampling, and their scaled size still counts for the position.
        let mut b = Bitmap::new(6, 6);
        for (x, y) in [(2, 2), (3, 2), (2, 3), (3, 3)] {
            b.px[y * 6 + x] = [255; 4];
        }
        let g = compose(&[layer(4, 0, 0)], |_| still(Arc::new(b.clone())), 0xffffff, 1.5, 0).frame.unwrap();
        assert_eq!(tags(&g, 0, 1), tags(&f, 0, 1));
    }

    #[test]
    fn rotation_turns_the_center_around_the_middle_of_the_screen() {
        // 4 x 2 at 3 px right of the center: x [1, 5), y [-1, 1). A quarter turn clockwise moves
        // it below the center: x [-1, 1), y [1, 5).
        let f = compose(&[layer(4, 3, 0)], |_| still(solid(4, 2)), 0xffffff, 1.0, 90).frame.unwrap();
        assert_eq!(tags(&f, 0, 1), "<P4><L200><E=2,10,7><S=1><I=2,4,4,4,2,4>");
        // Pixels are moved, not blended.
        assert!(f.atlas.crop(4, 4, 2, 4).px.iter().all(|p| *p == [255, 255, 255, 255]));
        // Angles outside 0..359 wrap.
        let g = compose(&[layer(4, 3, 0)], |_| still(solid(4, 2)), 0xffffff, 1.0, -270).frame.unwrap();
        assert_eq!(tags(&g, 0, 1), tags(&f, 0, 1));
    }

    #[test]
    fn rotation_turns_other_anchors_in_place() {
        // At the top left corner, 5 px right and 7 down: x [5, 9), y [7, 9), middle 7,8. Turned
        // around its middle: x [6, 8), y [6, 10).
        let f = compose(&[layer(0, 5, 7)], |_| still(solid(4, 2)), 0xffffff, 1.0, 90).frame.unwrap();
        assert_eq!(tags(&f, 0, 1), "<P0><L200><E=8,10,8><S=1><I=2,4,4,4,2,4>");
    }

    #[test]
    fn a_layer_turns_around_its_own_middle() {
        // 4 x 2 at 3 px right of the center: x [1, 5), y [-1, 1), middle 3,0. Its own quarter
        // turn keeps it there: x [2, 4), y [-2, 2).
        let l = LayerRef { rotation: 90, ..layer(4, 3, 0) };
        let f = compose(std::slice::from_ref(&l), |_| still(solid(4, 2)), 0xffffff, 1.0, 0).frame.unwrap();
        assert_eq!(tags(&f, 0, 1), "<P4><L200><E=8,4,5><S=1><I=2,4,4,4,2,4>");
        // With the crosshair turned 90 too: 180 around its middle, then the middle goes from 3,0
        // to 0,3: x [-2, 2), y [2, 4).
        let f = compose(std::slice::from_ref(&l), |_| still(solid(4, 2)), 0xffffff, 1.0, 90).frame.unwrap();
        assert_eq!(tags(&f, 0, 1), "<P4><L200><E=4,8,7><S=1><I=4,2,4,4,4,2>");
        // 30 own + 60 crosshair is one sharp quarter turn, not two smoothed ones.
        let l = LayerRef { rotation: 30, ..layer(4, 0, 0) };
        let f = compose(&[l], |_| still(solid(4, 2)), 0xffffff, 1.0, 60).frame.unwrap();
        assert!(f.atlas.px.iter().all(|p| p[3] == 0 || p[3] == 255));
    }

    #[test]
    fn slanted_rotation_smooths_the_edges() {
        let f = compose(&[layer(4, 0, 0)], |_| still(solid(8, 8)), 0xffffff, 1.0, 30).frame.unwrap();
        assert!(f.atlas.px.iter().any(|p| p[3] > 0 && p[3] < 255));
        assert!(f.draws[0].rects[0].w > 8);
    }

    #[test]
    fn scale_is_cleaned_to_the_steps() {
        use crate::model::{clean_scale, scale_is_sharp};
        assert_eq!([clean_scale(0.25), clean_scale(0.77), clean_scale(2.42), clean_scale(9.0)], [0.5, 0.75, 2.4, 4.0]);
        // Whole values are exact, however they were reached.
        assert_eq!(clean_scale(0.05 * 20.0), 1.0);
        assert!((1..=4).all(|w| scale_is_sharp(w as f32 + 0.004)));
        assert_eq!([scale_is_sharp(1.05), scale_is_sharp(0.5), scale_is_sharp(2.95)], [false, false, false]);
        use crate::model::scale_label;
        assert_eq!([scale_label(1.0), scale_label(1.5), scale_label(1.05), scale_label(0.95)], ["1×", "1.5×", "1.05×", "0.95×"]);
        let a = compose(&[layer(4, 0, 0)], |_| still(solid(2, 2)), 0xffffff, 9.0, 0).frame.unwrap();
        assert_eq!(a.draws[0].rects[0].w, 8);
    }

    #[test]
    fn scale_enlarges_pictures_and_offsets() {
        // 2 x 2 at 1 px right, scaled 2x: 4 x 4 at 2 px right, x [0, 4).
        let f = compose(&[layer(4, 1, 0)], |_| still(solid(2, 2)), 0xffffff, 2.0, 0).frame.unwrap();
        assert_eq!(tags(&f, 0, 1), "<P4><L200><E=8,4,5><S=1><I=4,4,4,4,4,4>");
        // At the screen edge the scaled picture is cut like any other.
        let f = compose(&[layer(0, -1, 0)], |_| still(solid(2, 2)), 0xffffff, 2.0, 0).frame.unwrap();
        assert_eq!(tags(&f, 0, 1), "<P0><L200><S=1><I=2,4,4,4,2,4>");
        // Pixels are repeated, not blended.
        let mut b = Bitmap::new(2, 1);
        b.px = vec![[255, 0, 0, 255], [0, 0, 255, 255]];
        let f = compose(&[layer(4, 0, 0)], |_| still(Arc::new(b.clone())), 0xffffff, 3.0, 0).frame.unwrap();
        let p = |x: i32, y: i32| f.atlas.get(ATLAS_PAD as i32 + x, ATLAS_PAD as i32 + y);
        assert_eq!((f.draws[0].rects[0].w, f.draws[0].rects[0].h), (6, 3));
        assert_eq!([p(0, 0), p(2, 2), p(3, 0), p(5, 2)], [b.px[0], b.px[0], b.px[1], b.px[1]]);
    }

    #[test]
    fn hidden_and_missing_layers_are_skipped() {
        let mut off = layer(4, 0, 0);
        off.on = false;
        assert!(compose(&[off], |_| still(solid(2, 2)), 0xffffff, 1.0, 0).frame.is_none());
        assert!(compose(&[layer(4, 0, 0)], |_| None, 0xffffff, 1.0, 0).frame.is_none());
    }

    #[test]
    fn text_draws_the_current_frames_and_closes_the_layer() {
        let r = |x, y| Rect { x, y, w: 4, h: 4 };
        let d = |anchor, layer, rects, delays| Draw { anchor, layer, xr: (-2, 2), yr: (-2, 2), rects, delays };
        let draws = [d(4, 0, vec![r(4, 4)], vec![]), d(4, 1, vec![r(12, 4), r(20, 4)], vec![50, 50])];
        assert_eq!(
            hypertext(r"C:\ProgramData\krosshair\osd_1.png", &draws, &[0, 1], 1),
            "<LI=\"C:\\ProgramData\\krosshair\\osd_1.png\"><P4><L200><S=1><I=4,4,4,4,4,4><P4><L201><S=1><I=4,4,20,4,4,4>\u{8}<P=0,0><L254>"
        );
    }
}