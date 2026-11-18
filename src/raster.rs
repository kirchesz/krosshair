//! Turns editor layers into pixels. What the editor shows and what goes to the overlay both come
//! from here, so the preview is exactly what appears in game.
//!
//! Coordinates: 0,0 is the screen center, which on an even-sized screen is a corner between four
//! pixels. Pixel `i` covers `[i, i + 1)`. Shapes with an even thickness are symmetric around that
//! corner. Shapes with an odd thickness cannot be: they are centered on pixel 0,0 (the one right
//! and below the corner), the same pixel as the middle cell of the custom grid.

use crate::model::{EditLayer, Shape};
use anyhow::{Context as _, Result};
use std::path::Path;
use std::sync::Arc;

/// Straight-alpha RGBA image.
#[derive(Clone, PartialEq, Debug)]
pub struct Bitmap {
    pub w: u32,
    pub h: u32,
    pub px: Vec<[u8; 4]>,
}

impl Bitmap {
    pub fn new(w: u32, h: u32) -> Self {
        Self { w, h, px: vec![[0; 4]; (w * h) as usize] }
    }

    pub fn get(&self, x: i32, y: i32) -> [u8; 4] {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return [0; 4];
        }
        self.px[(y as u32 * self.w + x as u32) as usize]
    }

    /// Draws `src` over this image with its top left corner at `dx, dy`. Parts outside are cut off.
    pub fn draw(&mut self, src: &Bitmap, dx: i32, dy: i32, opacity: f32) {
        for sy in 0..src.h as i32 {
            let y = sy + dy;
            if y < 0 || y >= self.h as i32 {
                continue;
            }
            for sx in 0..src.w as i32 {
                let x = sx + dx;
                if x < 0 || x >= self.w as i32 {
                    continue;
                }
                let s = src.px[(sy as u32 * src.w + sx as u32) as usize];
                let sa = s[3] as f32 / 255.0 * opacity;
                if sa <= 0.0 {
                    continue;
                }
                let d = &mut self.px[(y as u32 * self.w + x as u32) as usize];
                let da = d[3] as f32 / 255.0;
                let oa = sa + da * (1.0 - sa);
                for c in 0..3 {
                    let v = (s[c] as f32 * sa + d[c] as f32 * da * (1.0 - sa)) / oa;
                    d[c] = v.round().clamp(0.0, 255.0) as u8;
                }
                d[3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    /// Multiplies every color by `mask` (0xRRGGBB). White leaves the image as it is.
    pub fn tinted(&self, mask: u32) -> Bitmap {
        if mask & 0xffffff == 0xffffff {
            return self.clone();
        }
        let m = [(mask >> 16) as u8, (mask >> 8) as u8, mask as u8];
        let mut out = self.clone();
        for p in &mut out.px {
            for c in 0..3 {
                p[c] = ((p[c] as u32 * m[c] as u32 + 127) / 255) as u8;
            }
        }
        out
    }

    pub fn is_blank(&self) -> bool {
        self.px.iter().all(|p| p[3] == 0)
    }

    fn from_rgba(img: &image::RgbaImage) -> Bitmap {
        let (w, h) = img.dimensions();
        Bitmap { w, h, px: img.pixels().map(|p| p.0).collect() }
    }

    /// Reads a PNG, or the first frame of a GIF.
    pub fn load(path: &Path) -> Result<Bitmap> {
        let img = image::open(path).with_context(|| format!("can't read {}", path.display()))?.into_rgba8();
        Ok(Bitmap::from_rgba(&img))
    }

    /// This image `k` times bigger, every pixel becoming a `k` x `k` square.
    pub fn scaled(&self, k: u32) -> Bitmap {
        if k <= 1 {
            return self.clone();
        }
        let mut out = Bitmap::new(self.w * k, self.h * k);
        for y in 0..out.h {
            let src = ((y / k) * self.w) as usize;
            let dst = (y * out.w) as usize;
            for x in 0..out.w {
                out.px[dst + x as usize] = self.px[src + (x / k) as usize];
            }
        }
        out
    }


    /// This image at `tw` x `th`, each new pixel the average of the area of the old image it
    /// covers. Shrunk, thin lines turn half transparent instead of disappearing; enlarged by a
    /// fraction, pixels grow unevenly and their edges blend.
    pub fn resampled(&self, tw: u32, th: u32) -> Bitmap {
        if (tw, th) == (self.w, self.h) {
            return self.clone();
        }
        // Premultiplied, so transparent pixels don't darken the colors next to them.
        let src: Vec<[f32; 4]> = self
            .px
            .iter()
            .map(|p| {
                let a = p[3] as f32 / 255.0;
                [p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, p[3] as f32]
            })
            .collect();
        let rows = resample(&src, self.w as usize, self.h as usize, tw as usize, true);
        let out = resample(&rows, tw as usize, self.h as usize, th as usize, false);
        let px = out
            .iter()
            .map(|p| {
                let a = p[3] / 255.0;
                let c = |v: f32| if a > 0.0 { (v / a).round().clamp(0.0, 255.0) as u8 } else { 0 };
                [c(p[0]), c(p[1]), c(p[2]), p[3].round().clamp(0.0, 255.0) as u8]
            })
            .collect();
        Bitmap { w: tw, h: th, px }
    }

    /// This image, with its top left corner at `at`, turned `degrees` clockwise (on a screen,
    /// with y going down) around `pivot`. Returns the turned image and where its top left corner
    /// is, in the same coordinates. Each new pixel averages 4 x 4 points of the old image, so
    /// slanted edges are smoothed; on quarter turns around a pixel corner every point of a pixel
    /// lands in one old pixel, so it is an exact copy.
    pub fn rotated(&self, at: (f32, f32), pivot: (f32, f32), degrees: i32) -> (Bitmap, (i32, i32)) {
        let (sin, cos) = match degrees.rem_euclid(360) {
            0 => (0., 1.),
            90 => (1., 0.),
            180 => (0., -1.),
            270 => (-1., 0.),
            d => (d as f32).to_radians().sin_cos(),
        };
        let turn = |x: f32, y: f32| {
            let (dx, dy) = (x - pivot.0, y - pivot.1);
            (pivot.0 + dx * cos - dy * sin, pivot.1 + dx * sin + dy * cos)
        };
        let (w, h) = (self.w as f32, self.h as f32);
        let corners = [turn(at.0, at.1), turn(at.0 + w, at.1), turn(at.0, at.1 + h), turn(at.0 + w, at.1 + h)];
        // A hair of slack so a corner landing exactly on a pixel edge doesn't add a column.
        let x0 = (corners.iter().map(|c| c.0).fold(f32::MAX, f32::min) + 1e-3).floor() as i32;
        let y0 = (corners.iter().map(|c| c.1).fold(f32::MAX, f32::min) + 1e-3).floor() as i32;
        let x1 = (corners.iter().map(|c| c.0).fold(f32::MIN, f32::max) - 1e-3).ceil() as i32;
        let y1 = (corners.iter().map(|c| c.1).fold(f32::MIN, f32::max) - 1e-3).ceil() as i32;
        let (ow, oh) = ((x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32);
        let mut out = Bitmap::new(ow, oh);
        const N: i32 = 4;
        for oy in 0..oh {
            for ox in 0..ow {
                let mut acc = [0f32; 4];
                for sy in 0..N {
                    for sx in 0..N {
                        let px = x0 as f32 + ox as f32 + (sx as f32 + 0.5) / N as f32;
                        let py = y0 as f32 + oy as f32 + (sy as f32 + 0.5) / N as f32;
                        // Back into the old image: turn the other way.
                        let (dx, dy) = (px - pivot.0, py - pivot.1);
                        let (u, v) = (pivot.0 + dx * cos + dy * sin - at.0, pivot.1 - dx * sin + dy * cos - at.1);
                        if u < 0. || v < 0. || u >= w || v >= h {
                            continue;
                        }
                        let p = self.px[(v as u32 * self.w + u as u32) as usize];
                        let a = p[3] as f32;
                        acc[0] += p[0] as f32 * a;
                        acc[1] += p[1] as f32 * a;
                        acc[2] += p[2] as f32 * a;
                        acc[3] += a;
                    }
                }
                if acc[3] > 0. {
                    let c = |v: f32| (v / acc[3]).round().clamp(0., 255.) as u8;
                    let alpha = (acc[3] / (N * N) as f32).round().clamp(0., 255.) as u8;
                    out.px[(oy * ow + ox) as usize] = [c(acc[0]), c(acc[1]), c(acc[2]), alpha];
                }
            }
        }
        (out, (x0, y0))
    }

    /// The part `x, y, w, h` of this image.
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Bitmap {
        let mut out = Bitmap::new(w, h);
        for row in 0..h {
            let from = ((y + row) * self.w + x) as usize;
            let to = (row * w) as usize;
            out.px[to..to + w as usize].copy_from_slice(&self.px[from..from + w as usize]);
        }
        out
    }

    /// The smallest rectangle holding every visible pixel: `x0, y0, x1, y1` with the ends
    /// excluded. `None` for a fully transparent image.
    pub fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..self.h {
            for x in 0..self.w {
                if self.px[(y * self.w + x) as usize][3] != 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        (x1 > 0).then_some((x0, y0, x1, y1))
    }

    pub fn save_png(&self, path: &Path) -> Result<()> {
        let raw: Vec<u8> = self.px.iter().flatten().copied().collect();
        image::save_buffer(path, &raw, self.w, self.h, image::ExtendedColorType::Rgba8)
            .with_context(|| format!("can't write {}", path.display()))
    }
}

/// Area averaging along one axis of a `w` x `h` image: across (`horizontal`) to `to` columns, or
/// down to `to` rows. Each output pixel averages the source span it covers, partly covered source
/// pixels counting by the covered part.
fn resample(src: &[[f32; 4]], w: usize, h: usize, to: usize, horizontal: bool) -> Vec<[f32; 4]> {
    let from = if horizontal { w } else { h };
    let ratio = from as f32 / to as f32;
    let (ow, oh) = if horizontal { (to, h) } else { (w, to) };
    let mut out = vec![[0.0; 4]; ow * oh];
    for o in 0..to {
        let (start, end) = (o as f32 * ratio, (o + 1) as f32 * ratio);
        let first = start.floor() as usize;
        let last = (end.ceil() as usize).min(from);
        for i in first..last {
            let weight = ((i + 1) as f32).min(end) - (i as f32).max(start);
            if weight <= 0.0 {
                continue;
            }
            let k = weight / ratio;
            for line in 0..(if horizontal { h } else { w }) {
                let (s, d) = if horizontal { (line * w + i, line * ow + o) } else { (i * w + line, o * ow + line) };
                for c in 0..4 {
                    out[d][c] += src[s][c] * k;
                }
            }
        }
    }
    out
}

/// The frames of an animated crosshair, all of the same size, with how long each one stays.
#[derive(Clone, PartialEq, Debug)]
pub struct Anim {
    pub frames: Vec<Arc<Bitmap>>,
    /// Milliseconds, one per frame.
    pub delays: Vec<u32>,
}

/// A gallery picture: the still image (the first frame of a GIF) and the animation, if any.
#[derive(Clone, PartialEq, Debug)]
pub struct Picture {
    pub still: Arc<Bitmap>,
    pub anim: Option<Arc<Anim>>,
}

/// GIFs with more pixels than this in all frames together are shown as a still image. The overlay
/// atlas holds at most 2048 x 2048 (see `overlay::ATLAS_MAX`), so more could never be drawn; the
/// limit keeps a huge file from eating memory.
const ANIM_MAX_PIXELS: u64 = 2048 * 2048;
/// RTSS shows every frame up to about 60 changes a second; faster ones would be skipped.
const MIN_DELAY_MS: u32 = 20;

impl Anim {
    /// The frame showing `elapsed_ms` after the start (the animation loops), and how long until
    /// the next one.
    pub fn at(&self, elapsed_ms: u64) -> (usize, u32) {
        timeline_at(&self.delays, elapsed_ms)
    }

    /// Reads a GIF. `None` when it has a single frame, is too big, or can't be read; the caller
    /// then shows it as a still image.
    pub fn load_gif(path: &Path) -> Option<Anim> {
        use image::AnimationDecoder;
        let file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
        let decoder = image::codecs::gif::GifDecoder::new(file).ok()?;
        let mut anim = Anim { frames: Vec::new(), delays: Vec::new() };
        let mut pixels = 0u64;
        for frame in decoder.into_frames() {
            let frame = frame.ok()?;
            let (num, den) = frame.delay().numer_denom_ms();
            let ms = num.checked_div(den).unwrap_or(0);
            // Like browsers do: a missing or near zero delay means 100 ms.
            anim.delays.push(if ms < MIN_DELAY_MS { 100 } else { ms });
            let bmp = Bitmap::from_rgba(frame.buffer());
            pixels += bmp.w as u64 * bmp.h as u64;
            if pixels > ANIM_MAX_PIXELS {
                return None;
            }
            anim.frames.push(Arc::new(bmp));
        }
        // The decoder gives every frame the full canvas size; anything else is not usable here.
        let first = anim.frames.first()?;
        let same_size = anim.frames.iter().all(|f| f.w == first.w && f.h == first.h);
        (anim.frames.len() > 1 && same_size).then_some(anim)
    }
}

/// Position in a looping timeline of `delays`: (index, milliseconds until the next entry).
pub fn timeline_at(delays: &[u32], elapsed_ms: u64) -> (usize, u32) {
    let total: u64 = delays.iter().map(|&d| d as u64).sum();
    if total == 0 {
        return (0, u32::MAX);
    }
    let mut t = elapsed_ms % total;
    for (i, &d) in delays.iter().enumerate() {
        if t < d as u64 {
            return (i, (d as u64 - t) as u32);
        }
        t -= d as u64;
    }
    (0, delays[0])
}

/// A filled area. The outline of a part is the same part grown by the outline width.
#[derive(Clone, Copy)]
enum Part {
    /// x0, y0, x1, y1
    Rect(f32, f32, f32, f32),
    /// center (same on both axes), inner radius, outer radius
    Ring(f32, f32, f32),
}

impl Part {
    fn contains(&self, x: f32, y: f32, grow: f32) -> bool {
        match *self {
            Part::Rect(x0, y0, x1, y1) => {
                x >= x0 - grow && x < x1 + grow && y >= y0 - grow && y < y1 + grow
            }
            Part::Ring(c, r_in, r_out) => {
                let d2 = (x - c) * (x - c) + (y - c) * (y - c);
                let lo = (r_in - grow).max(0.0);
                let hi = r_out + grow;
                d2 >= lo * lo && d2 <= hi * hi
            }
        }
    }
}

fn parts(layer: &EditLayer) -> Vec<Part> {
    let f = |v: i32| v as f32;
    match layer.shape {
        Shape::Cross | Shape::T => {
            let (t, g, l) = (layer.thickness, layer.gap, layer.len);
            let odd = t % 2;
            // Across the arm: t pixels starting here.
            let a0 = -(t / 2);
            let a1 = a0 + t;
            let mut v = vec![
                Part::Rect(f(g), f(a0), f(g + l), f(a1)),                 // right
                Part::Rect(f(-g - l + odd), f(a0), f(-g + odd), f(a1)),   // left
                Part::Rect(f(a0), f(g), f(a1), f(g + l)),                 // bottom
            ];
            if layer.shape == Shape::Cross {
                v.push(Part::Rect(f(a0), f(-g - l + odd), f(a1), f(-g + odd))); // top
            }
            v
        }
        Shape::Circle => {
            let c = 0.5 * (layer.thickness % 2) as f32;
            let half = layer.thickness as f32 / 2.0;
            vec![Part::Ring(c, layer.radius as f32 - half, layer.radius as f32 + half)]
        }
        Shape::Dot => {
            let d = layer.dot;
            if d <= 2 {
                let a0 = -(d / 2);
                vec![Part::Rect(f(a0), f(a0), f(a0 + d), f(a0 + d))]
            } else {
                vec![Part::Ring(0.5 * (d % 2) as f32, 0.0, d as f32 / 2.0)]
            }
        }
        Shape::Custom => layer
            .pixels
            .iter()
            .map(|&(x, y)| Part::Rect(f(x), f(y), f(x + 1), f(y + 1)))
            .collect(),
    }
}

/// The point a layer turns around: the middle of its shape. Shapes with an odd thickness sit
/// on pixel 0,0, so their middle is that pixel's center; even ones on the corner at 0,0.
fn pivot(layer: &EditLayer) -> f32 {
    let odd = match layer.shape {
        Shape::Cross | Shape::T | Shape::Circle => layer.thickness % 2,
        Shape::Dot => layer.dot % 2,
        Shape::Custom => 1,
    };
    0.5 * odd as f32
}

/// Half size of the working canvas. Large enough for the biggest shape the editor allows, at any
/// angle, with the widest outline.
const WORK_HALF: i32 = 36;
/// Samples per pixel side for anti-aliasing round shapes.
const SAMPLES: i32 = 4;

/// One layer on the working canvas, without its opacity.
fn render_layer(layer: &EditLayer) -> Bitmap {
    let parts = parts(layer);
    let size = (WORK_HALF * 2) as u32;
    let mut bmp = Bitmap::new(size, size);
    if parts.is_empty() {
        return bmp;
    }
    let total = (SAMPLES * SAMPLES) as f32;
    let rgb = [(layer.color >> 16) as u8, (layer.color >> 8) as u8, layer.color as u8];
    let line = [(layer.outline_color >> 16) as u8, (layer.outline_color >> 8) as u8, layer.outline_color as u8];
    let grow = layer.outline_width.clamp(1, crate::model::MAX_OUTLINE) as f32;
    let line_alpha = layer.outline_opacity.min(100) as f32 / 100.0;
    // Turning the shape is done by turning every sample point the other way, so edges stay
    // smooth at any angle and quarter turns land exactly on pixels.
    let (sin, cos) = match layer.rotation.rem_euclid(360) {
        0 => (0., 1.),
        90 => (1., 0.),
        180 => (0., -1.),
        270 => (-1., 0.),
        d => (d as f32).to_radians().sin_cos(),
    };
    let c = pivot(layer);
    for iy in -WORK_HALF..WORK_HALF {
        for ix in -WORK_HALF..WORK_HALF {
            let (mut fill, mut edge) = (0, 0);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let (px, py) = (ix as f32 + (sx as f32 + 0.5) / SAMPLES as f32, iy as f32 + (sy as f32 + 0.5) / SAMPLES as f32);
                    let (dx, dy) = (px - c, py - c);
                    let (x, y) = (c + dx * cos + dy * sin, c - dx * sin + dy * cos);
                    if parts.iter().any(|p| p.contains(x, y, 0.0)) {
                        fill += 1;
                        edge += 1;
                    } else if layer.outline && parts.iter().any(|p| p.contains(x, y, grow)) {
                        edge += 1;
                    }
                }
            }
            // The fill and the outline never overlap: each covers its share of the pixel, the
            // outline's share weighed by its opacity.
            let (wf, wo) = (fill as f32, (edge - fill) as f32 * line_alpha);
            if wf + wo <= 0.0 {
                continue;
            }
            let p = &mut bmp.px[((iy + WORK_HALF) as u32 * size + (ix + WORK_HALF) as u32) as usize];
            for ch in 0..3 {
                p[ch] = ((rgb[ch] as f32 * wf + line[ch] as f32 * wo) / (wf + wo)).round() as u8;
            }
            p[3] = ((wf + wo) / total * 255.0).round() as u8;
        }
    }
    bmp
}

/// Cuts the transparent border off, keeping the center in the middle. The result has even sides.
fn crop_centered(bmp: &Bitmap) -> Bitmap {
    let (cx, cy) = ((bmp.w / 2) as i32, (bmp.h / 2) as i32);
    let (mut half_w, mut half_h) = (1, 1);
    for y in 0..bmp.h as i32 {
        for x in 0..bmp.w as i32 {
            if bmp.get(x, y)[3] != 0 {
                let (rx, ry) = (x - cx, y - cy);
                half_w = half_w.max(if rx < 0 { -rx } else { rx + 1 });
                half_h = half_h.max(if ry < 0 { -ry } else { ry + 1 });
            }
        }
    }
    let mut out = Bitmap::new((half_w * 2) as u32, (half_h * 2) as u32);
    out.draw(bmp, half_w - cx, half_h - cy, 1.0);
    out
}

/// The whole crosshair, layers drawn bottom to top, cropped to its content.
pub fn render(layers: &[EditLayer]) -> Bitmap {
    let size = (WORK_HALF * 2) as u32;
    let mut canvas = Bitmap::new(size, size);
    for layer in layers {
        canvas.draw(&render_layer(layer), 0, 0, layer.opacity as f32 / 100.0);
    }
    crop_centered(&canvas)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque_cols(bmp: &Bitmap, y: i32) -> Vec<i32> {
        (0..bmp.w as i32).filter(|&x| bmp.get(x, y)[3] == 255 && bmp.get(x, y)[1] > 0).map(|x| x - bmp.w as i32 / 2).collect()
    }

    #[test]
    fn even_cross_is_symmetric() {
        let layer = EditLayer { len: 3, thickness: 2, gap: 0, outline: false, ..EditLayer::new(Shape::Cross) };
        let bmp = render(&[layer]);
        assert_eq!((bmp.w, bmp.h), (6, 6));
        // The horizontal arm fills rows -1 and 0 across the whole width.
        assert_eq!(opaque_cols(&bmp, 2), vec![-3, -2, -1, 0, 1, 2]);
        assert_eq!(opaque_cols(&bmp, 3), vec![-3, -2, -1, 0, 1, 2]);
        assert_eq!(opaque_cols(&bmp, 0), vec![-1, 0]);
    }

    #[test]
    fn odd_cross_is_centered_on_pixel_zero() {
        let layer = EditLayer { len: 3, thickness: 1, gap: 0, outline: false, ..EditLayer::new(Shape::Cross) };
        let bmp = render(&[layer]);
        // Arms cover -2..=2 around pixel 0, the canvas stays even: 3 pixels each side.
        assert_eq!((bmp.w, bmp.h), (6, 6));
        assert_eq!(opaque_cols(&bmp, 3), vec![-2, -1, 0, 1, 2]);
    }

    #[test]
    fn outline_adds_one_black_pixel() {
        let layer = EditLayer { dot: 2, color: 0x57c87f, ..EditLayer::new(Shape::Dot) };
        let bmp = render(&[layer]);
        assert_eq!((bmp.w, bmp.h), (4, 4));
        assert_eq!(bmp.get(0, 0), [0, 0, 0, 255]);
        assert_eq!(bmp.get(1, 1), [0x57, 0xc8, 0x7f, 255]);
    }

    #[test]
    fn outline_width_and_color() {
        let layer = EditLayer { dot: 2, color: 0x57c87f, outline_width: 2, outline_color: 0xffffff, ..EditLayer::new(Shape::Dot) };
        let bmp = render(&[layer]);
        assert_eq!((bmp.w, bmp.h), (6, 6));
        assert_eq!(bmp.get(0, 0), [255, 255, 255, 255]);
        assert_eq!(bmp.get(2, 2), [0x57, 0xc8, 0x7f, 255]);
    }

    #[test]
    fn outline_opacity_fades_only_the_outline() {
        let layer = EditLayer { dot: 2, color: 0x57c87f, outline_color: 0xffffff, outline_opacity: 40, ..EditLayer::new(Shape::Dot) };
        let bmp = render(&[layer]);
        assert_eq!((bmp.w, bmp.h), (4, 4));
        assert_eq!(bmp.get(0, 0), [255, 255, 255, 102]);
        assert_eq!(bmp.get(1, 1), [0x57, 0xc8, 0x7f, 255]);
    }

    #[test]
    fn editor_rotation_turns_the_shape() {
        let t = EditLayer { len: 4, thickness: 2, gap: 1, outline: false, ..EditLayer::new(Shape::T) };
        let turned = |deg| render(&[EditLayer { rotation: deg, ..t.clone() }]);
        // A quarter turn of a T is exact: the stem that pointed down points left.
        let r = turned(90);
        assert!(r.px.iter().all(|p| p[3] == 0 || p[3] == 255));
        let (w, h) = (r.w as i32, r.h as i32);
        assert_eq!(r.get(0, h / 2)[3], 255, "stem on the left");
        assert_eq!(r.get(w - 1, h / 2)[3], 0, "nothing on the right");
        // A cross turned a quarter is the same cross; at 45 degrees its edges are smoothed.
        let cross = EditLayer { len: 4, thickness: 2, gap: 1, outline: false, ..EditLayer::new(Shape::Cross) };
        assert_eq!(render(&[EditLayer { rotation: 90, ..cross.clone() }]), render(std::slice::from_ref(&cross)));
        let x = render(&[EditLayer { rotation: 45, ..cross }]);
        assert!(x.px.iter().any(|p| p[3] > 0 && p[3] < 255));
    }

    #[test]
    fn gap_leaves_the_middle_empty() {
        let layer = EditLayer { len: 2, thickness: 2, gap: 2, outline: false, ..EditLayer::new(Shape::Cross) };
        let bmp = render(&[layer]);
        assert_eq!(opaque_cols(&bmp, 4), vec![-4, -3, 2, 3]);
    }

    #[test]
    fn scaling_repeats_pixels() {
        let mut b = Bitmap::new(2, 1);
        b.px = vec![[255, 0, 0, 255], [0, 0, 255, 128]];
        let s = b.scaled(2);
        assert_eq!((s.w, s.h), (4, 2));
        let (r, bl) = (b.px[0], b.px[1]);
        assert_eq!(s.px, vec![r, r, bl, bl, r, r, bl, bl]);
        assert_eq!(b.scaled(1), b);
    }

    #[test]
    fn shrinking_averages_the_area() {
        let mut b = Bitmap::new(4, 2);
        b.px.fill([255, 255, 255, 255]);
        let s = b.resampled(2, 1);
        assert_eq!((s.w, s.h), (2, 1));
        assert!(s.px.iter().all(|p| *p == [255, 255, 255, 255]));
        // One opaque red pixel of four: a quarter covered, still pure red.
        let mut b = Bitmap::new(2, 2);
        b.px[0] = [255, 0, 0, 255];
        assert_eq!(b.resampled(1, 1).px, vec![[255, 0, 0, 64]]);
        // 3 -> 2 columns: the white middle pixel is split between both, a third of each.
        let mut b = Bitmap::new(3, 1);
        b.px = vec![[0, 0, 0, 255], [255, 255, 255, 255], [0, 0, 0, 0]];
        let s = b.resampled(2, 1);
        assert_eq!(s.px, vec![[85, 85, 85, 255], [255, 255, 255, 85]]);
    }

    #[test]
    fn enlarging_by_a_fraction_blends_the_edges() {
        // 2 -> 3 columns: the middle pixel is half of each.
        let mut b = Bitmap::new(2, 1);
        b.px = vec![[0, 0, 0, 255], [255, 255, 255, 255]];
        assert_eq!(b.resampled(3, 1).px, vec![[0, 0, 0, 255], [128, 128, 128, 255], [255, 255, 255, 255]]);
    }

    #[test]
    fn quarter_turns_are_exact() {
        // A 3 x 2 image with its top left at 0,0, turned around 0,0.
        let mut b = Bitmap::new(3, 2);
        for (i, p) in b.px.iter_mut().enumerate() {
            *p = [i as u8 * 40, 0, 0, 255];
        }
        let (r, at) = b.rotated((0., 0.), (0., 0.), 90);
        assert_eq!((r.w, r.h, at), (2, 3, (-2, 0)));
        // Clockwise on a screen: the top row becomes the right column, read downwards.
        assert_eq!([r.get(1, 0), r.get(1, 1), r.get(1, 2)], [b.px[0], b.px[1], b.px[2]]);
        assert_eq!([r.get(0, 0), r.get(0, 1), r.get(0, 2)], [b.px[3], b.px[4], b.px[5]]);
        let (r, at) = b.rotated((0., 0.), (0., 0.), 180);
        assert_eq!((r.w, r.h, at), (3, 2, (-3, -2)));
        assert_eq!(r.get(2, 1), b.px[0]);
        let (r, _) = b.rotated((0., 0.), (0., 0.), 360);
        assert_eq!(r, b);
    }

    #[test]
    fn other_angles_smooth_the_edges() {
        let mut b = Bitmap::new(4, 4);
        b.px.fill([255, 255, 255, 255]);
        let (r, _) = b.rotated((-2., -2.), (0., 0.), 45);
        // About 4 * sqrt(2) wide, with partly covered pixels along the slanted edges.
        assert!(r.w == 6 || r.w == 7);
        assert!(r.px.iter().any(|p| p[3] > 0 && p[3] < 255));
        assert!(r.px.iter().filter(|p| p[3] > 0).all(|p| p[0] == 255));
    }

    #[test]
    fn timeline_loops() {
        let delays = [50, 100, 30];
        assert_eq!(timeline_at(&delays, 0), (0, 50));
        assert_eq!(timeline_at(&delays, 49), (0, 1));
        assert_eq!(timeline_at(&delays, 50), (1, 100));
        assert_eq!(timeline_at(&delays, 179), (2, 1));
        assert_eq!(timeline_at(&delays, 180), (0, 50));
        assert_eq!(timeline_at(&delays, 180 * 7 + 160), (2, 20));
        assert_eq!(timeline_at(&[], 5).0, 0);
    }

    #[test]
    fn tint_multiplies_colors() {
        let mut bmp = Bitmap::new(1, 1);
        bmp.px[0] = [255, 128, 0, 200];
        assert_eq!(bmp.tinted(0xff0000).px[0], [255, 0, 0, 200]);
        assert_eq!(bmp.tinted(0xffffff), bmp);
    }
}
