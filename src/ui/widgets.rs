//! Small building blocks shared by the screens.

use super::theme::*;
use super::Krosshair;
use crate::raster::Bitmap;
use crate::store::GalleryItem;
use gpui::{
    canvas, div, fill, img, point, prelude::*, px, quad, size, svg, transparent_black, AnyView, App, BorderStyle,
    Bounds, Context, CursorStyle, DispatchPhase, Div, ElementId, FontWeight, HitboxBehavior, Hsla, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, Point, Rgba, SharedString, Stateful, Svg,
    WeakEntity, Window,
};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long hover, tooltip and switch transitions take.
pub const ANIM: Duration = Duration::from_millis(120);

/// Progress 0..1 of a transition that started at `since`, eased out (close to the design's
/// `cubic-bezier(.22, .61, .36, 1)`).
pub fn progress(since: Instant) -> f32 {
    let left = 1. - (since.elapsed().as_secs_f32() / ANIM.as_secs_f32()).clamp(0., 1.);
    1. - left * left * left
}

fn channels(hex: u32) -> [f32; 3] {
    [(hex >> 16 & 0xff) as f32 / 255., (hex >> 8 & 0xff) as f32 / 255., (hex & 0xff) as f32 / 255.]
}

/// The color `t` of the way from `from` to `to`.
pub fn mix(from: u32, to: u32, t: f32) -> Hsla {
    let (a, b) = (channels(from), channels(to));
    Rgba { r: a[0] + (b[0] - a[0]) * t, g: a[1] + (b[1] - a[1]) * t, b: a[2] + (b[2] - a[2]) * t, a: 1. }.into()
}

/// `color` at opacity `t`: for backgrounds that appear from nothing.
pub fn fade(color: u32, t: f32) -> Hsla {
    let ch = channels(color);
    Rgba { r: ch[0], g: ch[1], b: ch[2], a: t }.into()
}

/// Smooth hover styling. gpui's own `.hover()` switches styles instantly.
pub trait Fx: Sized {
    /// Tracks hovering of this element (it needs an id) and lets `style` dress it by the hover
    /// amount: 0 at rest, 1 hovered, moving between the two over [`ANIM`].
    fn fx(self, view: &Krosshair, cx: &Context<Krosshair>, style: impl FnOnce(Self, f32) -> Self) -> Self;
}

impl Fx for Stateful<Div> {
    fn fx(mut self, view: &Krosshair, cx: &Context<Krosshair>, style: impl FnOnce(Self, f32) -> Self) -> Self {
        let id = self.interactivity().element_id.clone().expect("fx needs an element id");
        let t = view.hover_amount(&id);
        let tracked = self.on_hover(cx.listener(move |view, hovered: &bool, _, cx| view.set_hover(id.clone(), *hovered, cx)));
        style(tracked, t)
    }
}

pub fn icon(name: &str, size_px: f32, color: Hsla) -> Svg {
    svg().path(format!("icons/{name}.svg")).size(px(size_px)).flex_none().text_color(color)
}

/// A clickable box with an icon in the middle. `colors` are the icon color at rest and hovered,
/// `bg` the background at rest (if any) and hovered. The caller sets the size and shape.
pub fn icon_button(
    view: &Krosshair,
    cx: &Context<Krosshair>,
    id: impl Into<ElementId>,
    name: &str,
    size_px: f32,
    colors: (u32, u32),
    bg: (Option<u32>, u32),
) -> Stateful<Div> {
    div().id(id).flex().flex_none().items_center().justify_center().cursor_pointer().fx(view, cx, |d, t| {
        let back = match bg.0 {
            Some(rest) => mix(rest, bg.1, t),
            None => fade(bg.1, t),
        };
        d.bg(back).child(icon(name, size_px, mix(colors.0, colors.1, t)))
    })
}

/// Small grey monospace caption.
pub fn caption(text: impl Into<SharedString>) -> Div {
    div().font_family(mono()).text_size(px(11.)).text_color(c(MUTED)).child(text.into())
}

/// Monospace value next to a caption.
pub fn value(text: impl Into<SharedString>) -> Div {
    div().font_family(mono()).text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(text.into())
}

/// The track and knob of an on/off switch. `width` x `height`, the knob is 4 px smaller.
/// `t` is how far the switch is into its current state: 1 when it has settled.
pub fn switch(on: bool, off_track: u32, width: f32, height: f32, t: f32) -> Div {
    let knob = height - 4.;
    let amount = if on { t } else { 1. - t };
    div()
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .rounded_full()
        .bg(mix(off_track, BLUE, amount))
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(2. + (width - knob - 4.) * amount))
                .size(px(knob))
                .rounded_full()
                .bg(c(TEXT)),
        )
}

struct Tip {
    text: SharedString,
    shown: Instant,
}

impl Render for Tip {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Fades in while sliding 4 px into place.
        let p = progress(self.shown);
        if p < 1. {
            window.request_animation_frame();
        }
        div().pl(px(4. * p)).pr(px(4. * (1. - p))).opacity(p).child(
            div()
                .px(px(10.))
                .py(px(6.))
                .rounded(px(4.))
                .bg(c(RAISED))
                .text_color(c(TEXT))
                .font_family(sans())
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .shadow_lg()
                .child(self.text.clone()),
        )
    }
}

/// Tooltip builder for `.tooltip(...)`.
pub fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_, cx| cx.new(|_| Tip { text: text.clone(), shown: Instant::now() }).into()
}
/// Paints the image pixel by pixel, each one as a `scale` sized square, so it stays sharp.
pub fn paint_bitmap(bmp: &Bitmap, origin: Point<Pixels>, scale: f32, opacity: f32, window: &mut Window) {
    for y in 0..bmp.h {
        let row = &bmp.px[(y * bmp.w) as usize..((y + 1) * bmp.w) as usize];
        let mut x = 0usize;
        while x < row.len() {
            let p = row[x];
            let mut run = 1;
            while x + run < row.len() && row[x + run] == p {
                run += 1;
            }
            if p[3] != 0 {
                let color = Rgba {
                    r: p[0] as f32 / 255.,
                    g: p[1] as f32 / 255.,
                    b: p[2] as f32 / 255.,
                    a: p[3] as f32 / 255. * opacity,
                };
                let bounds = Bounds::new(
                    point(origin.x + px(x as f32 * scale), origin.y + px(y as f32 * scale)),
                    size(px(run as f32 * scale), px(scale)),
                );
                window.paint_quad(fill(bounds, Hsla::from(color)));
            }
            x += run;
        }
    }
}

/// The image at `scale`, as an element of exactly that size.
pub fn bitmap_view(bmp: Arc<Bitmap>, scale: f32, opacity: f32) -> impl IntoElement {
    let (w, h) = (bmp.w as f32 * scale, bmp.h as f32 * scale);
    canvas(|_, _, _| (), move |bounds, _, window, _| paint_bitmap(&bmp, bounds.origin, scale, opacity, window))
        .w(px(w))
        .h(px(h))
        .flex_none()
}

/// A gallery crosshair fitted into a `side` x `side` box: enlarged by a whole factor up to
/// `max_scale` when it is small, scaled down when it is bigger than the box. Animations play.
pub fn thumb(view: &Krosshair, item: &GalleryItem, side: f32, max_scale: f32, opacity: f32) -> gpui::AnyElement {
    thumb_layer(view, item, (0, crate::model::WHITE), side, max_scale, opacity)
}

/// [`thumb`] as a layer shows it: tinted by its own mask and turned `rotation` degrees clockwise
/// around its middle.
pub fn thumb_layer(view: &Krosshair, item: &GalleryItem, (rotation, mask): (i32, u32), side: f32, max_scale: f32, opacity: f32) -> gpui::AnyElement {
    if rotation.rem_euclid(360) != 0 || mask & 0xffffff != crate::model::WHITE {
        let bmp = view.turned(&view.frame_of(item), rotation, mask);
        let largest = bmp.w.max(bmp.h).max(1) as f32;
        let scale = if largest > side { side / largest } else { (side / largest).floor().clamp(1., max_scale) };
        return bitmap_view(bmp, scale, opacity).into_any_element();
    }
    let largest = item.bmp.w.max(item.bmp.h).max(1) as f32;
    if largest > side {
        // gpui scales the file itself, and plays a GIF on its own.
        return div()
            .opacity(opacity)
            .child(img(item.path.clone()).size(px(side)).object_fit(ObjectFit::Contain))
            .into_any_element();
    }
    let scale = (side / largest).floor().clamp(1., max_scale);
    bitmap_view(view.frame_of(item), scale, opacity).into_any_element()
}

/// Small "gif · 12 frames" caption for animated gallery pictures.
pub fn anim_label(item: &GalleryItem) -> Option<Div> {
    let anim = item.anim.as_ref()?;
    Some(
        div()
            .font_family(mono())
            .text_size(px(10.5))
            .text_color(c(MUTED))
            .child(format!("gif · {} frames", anim.frames.len())),
    )
}

pub type SetValue = Rc<dyn Fn(&mut Krosshair, i32, &mut Context<Krosshair>)>;

const THUMB: f32 = 14.;

/// A horizontal slider. `set` receives the new value when it changes.
#[derive(Clone, Copy)]
pub struct SliderSpec {
    /// Tells the sliders apart while one is dragged.
    pub id: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
    pub current: i32,
    pub min: i32,
    pub max: i32,
    pub step: i32,
}

pub fn slider(spec: SliderSpec, view: WeakEntity<Krosshair>, dragging: bool, set: SetValue) -> impl IntoElement {
    let SliderSpec { id, current, min, max, step, .. } = spec;
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _| {
            let left = bounds.origin.x;
            let travel = bounds.size.width - px(THUMB);
            let frac = (current - min) as f32 / (max - min).max(1) as f32;
            let mid = bounds.origin.y + bounds.size.height / 2.;
            let knob_x = left + travel * frac;
            let bar = |x: Pixels, w: Pixels, color: u32| {
                quad(Bounds::new(point(x, mid - px(2.)), size(w, px(4.))), px(2.), c(color), px(0.), transparent_black(), BorderStyle::default())
            };
            window.paint_quad(bar(left, bounds.size.width, RAISED));
            window.paint_quad(bar(left, knob_x - left + px(THUMB / 2.), ACCENT));
            window.paint_quad(quad(
                Bounds::new(point(knob_x, mid - px(THUMB / 2.)), size(px(THUMB), px(THUMB))),
                px(THUMB / 2.),
                c(ACCENT),
                px(0.),
                transparent_black(),
                BorderStyle::default(),
            ));
            window.set_cursor_style(CursorStyle::PointingHand, &hitbox);

            let value_at = move |x: Pixels| -> i32 {
                let f = (f32::from(x - left - px(THUMB / 2.)) / f32::from(travel).max(1.)).clamp(0., 1.);
                let steps = ((max - min) as f32 * f / step as f32).round() as i32;
                (min + steps * step).clamp(min, max)
            };
            let apply = {
                let (view, set) = (view.clone(), set.clone());
                move |v: i32, start: bool, cx: &mut App| {
                    let _ = view.update(cx, |this, cx| {
                        if start {
                            this.slider_drag = Some(id);
                            cx.notify();
                        }
                        if v != current {
                            set(this, v, cx);
                        }
                    });
                }
            };

            window.on_mouse_event({
                let apply = apply.clone();
                move |e: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && e.button == MouseButton::Left && hitbox.is_hovered(window) {
                        apply(value_at(e.position.x), true, cx);
                    }
                }
            });
            if dragging {
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && e.pressed_button == Some(MouseButton::Left) {
                        apply(value_at(e.position.x), false, cx);
                    }
                });
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        let _ = view.update(cx, |this, cx| {
                            this.slider_drag = None;
                            cx.notify();
                        });
                    }
                });
            }
        },
    )
    .w_full()
    .h(px(20.))
}
