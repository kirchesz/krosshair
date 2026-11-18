//! The crosshair editor: stack simple shapes or paint pixels, then save the result to the gallery.

use super::theme::*;
use super::widgets::*;
use super::{plural, Dim, EditorState, Knob, Krosshair, Screen, FIELD_W, SCREEN_TOP};
use crate::model::{EditLayer, Shape, GRID_HALF};
use crate::raster::{self, Bitmap};
use crate::store;
use gpui::{
    canvas, deferred, div, fill, point, prelude::*, px, size, Bounds, ContentMask, Context, CursorStyle, DispatchPhase,
    Div, FontWeight, HitboxBehavior, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    Window,
};
use std::rc::Rc;
use std::sync::Arc;

/// Carbon grays 90, 60 and 20.
const BACKDROPS: [(u32, &str); 3] = [(0x262626, "Dark backdrop"), (0x6f6f6f, "Grey backdrop"), (0xe0e0e0, "Light backdrop")];
/// Size of one cell of the drawing grid on screen.
const CELL: f32 = 14.;
const CELLS: i32 = GRID_HALF * 2 + 1;

fn shape_icon(shape: Shape) -> &'static str {
    match shape {
        Shape::Cross => "shape-cross",
        Shape::T => "shape-t",
        Shape::Circle => "shape-circle",
        Shape::Dot => "shape-dot",
        Shape::Custom => "shape-custom",
    }
}

/// Index of the editor layer being dragged.
#[derive(Clone)]
struct DragShape(usize);

struct ShapeGhost {
    bmp: Arc<Bitmap>,
    grab: Point<Pixels>,
}

impl Render for ShapeGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let side = px(44.);
        div().pl(self.grab.x - side / 2.).pt(self.grab.y - side / 2.).child(
            div()
                .size(side)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .bg(c(SURFACE))
                .border_2()
                .border_color(c(ACCENT))
                .child(fitted(self.bmp.clone(), 32.)),
        )
    }
}

/// The image at 1:1, or scaled down when it is larger than `side`.
fn fitted(bmp: Arc<Bitmap>, side: f32) -> impl IntoElement {
    let scale = (side / bmp.w.max(bmp.h).max(1) as f32).min(1.);
    bitmap_view(bmp, scale, 1.)
}

impl Krosshair {
    fn selected(&self) -> &EditLayer {
        &self.ed.layers[self.ed.sel.min(self.ed.layers.len() - 1)]
    }

    fn edit(&mut self, change: impl FnOnce(&mut EditLayer), cx: &mut Context<Self>) {
        let sel = self.ed.sel.min(self.ed.layers.len() - 1);
        change(&mut self.ed.layers[sel]);
        cx.notify();
    }

    /// A color of the selected layer: a swatch and its hex code, the size of the rotation field.
    /// A click opens the system color picker.
    fn color_field(&self, id: &'static str, color: u32, set: fn(&mut EditLayer, u32), cx: &mut Context<Self>) -> Div {
        let button = div()
            .id(id)
            .flex_1()
            .h(px(40.))
            .px(px(10.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .font_family(mono())
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .fx(self, cx, |d, t| d.bg(fade(BG, t * 0.6)))
            .tooltip(tip("Pick a color"))
            .on_click(cx.listener(move |this, _, _, cx| this.pick_layer_color(color, set, cx)))
            .child(div().size(px(22.)).flex_none().rounded_full().bg(c(color)).border_1().border_color(alpha(TEXT, 0x33)))
            .child(format!("#{color:06X}"));
        div().w(px(FIELD_W)).flex().p(px(2.)).rounded(px(8.)).bg(c(RAISED)).child(button)
    }

    fn pick_layer_color(&mut self, initial: u32, set: fn(&mut EditLayer, u32), cx: &mut Context<Self>) {
        // The layer that was selected when the picker opened, even if another one is selected by
        // the time it closes.
        let layer = self.ed.sel.min(self.ed.layers.len() - 1);
        cx.spawn(async move |this, cx| {
            let picked = cx.background_spawn(async move { crate::dialogs::choose_color(initial) }).await;
            if let Some(color) = picked {
                let _ = this.update(cx, |this, cx| {
                    if let Some(l) = this.ed.layers.get_mut(layer) {
                        set(l, color);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn move_shape(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let n = self.ed.layers.len();
        if from == to || from >= n || to >= n {
            return;
        }
        // Follow the selected layer to its new place.
        let mut order: Vec<usize> = (0..n).collect();
        let moved = order.remove(from);
        order.insert(to, moved);
        let layer = self.ed.layers.remove(from);
        self.ed.layers.insert(to, layer);
        self.ed.sel = order.iter().position(|&i| i == self.ed.sel).unwrap_or(0);
        cx.notify();
    }

    fn save_to_gallery(&mut self, cx: &mut Context<Self>) {
        let bmp = raster::render(&self.ed.layers);
        if bmp.is_blank() {
            self.error = Some("Nothing to save: the crosshair is empty".into());
            cx.notify();
            return;
        }
        match store::save_crosshair(&self.cfg.gallery_dir, &bmp) {
            Ok(_) => self.go(Screen::Gallery, cx),
            Err(e) => {
                self.error = Some(e.to_string().into());
                cx.notify();
            }
        }
    }

    fn editor_slider(&self, spec: SliderSpec, set: fn(&mut EditLayer, i32), cx: &mut Context<Self>) -> Div {
        let setter: SetValue = Rc::new(move |this, v, cx| this.edit(|layer| set(layer, v), cx));
        div()
            .flex()
            .flex_none()
            .flex_col()
            .gap(px(6.))
            .child(div().flex().items_baseline().justify_between().child(caption(spec.label)).child(value(format!("{}{}", spec.current, spec.unit))))
            .child(slider(spec, cx.weak_entity(), self.slider_drag == Some(spec.id), setter))
    }

    /// The 21x21 grid of a custom layer. Click or drag to paint; the other layers show through
    /// faintly for reference.
    fn pixel_grid(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let layer = self.selected().clone();
        let others: Vec<EditLayer> =
            self.ed.layers.iter().enumerate().filter(|(i, _)| *i != self.ed.sel).map(|(_, l)| l.clone()).collect();
        let reference = raster::render(&others);
        let painting = self.ed.paint;
        let view = cx.weak_entity();
        let side = CELL * CELLS as f32;

        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                let o = bounds.origin;
                let cell_bounds = |x: i32, y: i32| {
                    Bounds::new(
                        point(o.x + px((x + GRID_HALF) as f32 * CELL), o.y + px((y + GRID_HALF) as f32 * CELL)),
                        size(px(CELL), px(CELL)),
                    )
                };
                // Pixel 0,0 of a crosshair is the middle cell, so its center corner is that cell's
                // top left corner.
                if !reference.is_blank() {
                    let origin = point(
                        o.x + px((GRID_HALF as f32 - reference.w as f32 / 2.) * CELL),
                        o.y + px((GRID_HALF as f32 - reference.h as f32 / 2.) * CELL),
                    );
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        paint_bitmap(&reference, origin, CELL, 0.3, window)
                    });
                }
                let axis: Hsla = alpha(ACCENT, 0x1a).into();
                for y in -GRID_HALF..=GRID_HALF {
                    for x in -GRID_HALF..=GRID_HALF {
                        if layer.pixels.contains(&(x, y)) {
                            window.paint_quad(fill(cell_bounds(x, y), c(layer.color)));
                        } else if x == 0 || y == 0 {
                            window.paint_quad(fill(cell_bounds(x, y), axis));
                        }
                    }
                }
                let line: Hsla = alpha(TEXT, 0x24).into();
                for i in 0..=CELLS {
                    let d = px(i as f32 * CELL);
                    window.paint_quad(fill(Bounds::new(point(o.x + d, o.y), size(px(1.), bounds.size.height)), line));
                    window.paint_quad(fill(Bounds::new(point(o.x, o.y + d), size(bounds.size.width, px(1.))), line));
                }
                window.set_cursor_style(CursorStyle::Crosshair, &hitbox);

                let cell_at = move |p: Point<Pixels>| -> Option<(i32, i32)> {
                    let x = (f32::from(p.x - o.x) / CELL).floor() as i32 - GRID_HALF;
                    let y = (f32::from(p.y - o.y) / CELL).floor() as i32 - GRID_HALF;
                    (x.abs() <= GRID_HALF && y.abs() <= GRID_HALF).then_some((x, y))
                };
                let pixels = Rc::new(layer.pixels.clone());

                window.on_mouse_event({
                    let (view, pixels) = (view.clone(), pixels.clone());
                    move |e: &MouseDownEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble || e.button != MouseButton::Left || !hitbox.is_hovered(window) {
                            return;
                        }
                        let Some(cell) = cell_at(e.position) else { return };
                        let on = !pixels.contains(&cell);
                        let _ = view.update(cx, |this, cx| {
                            this.ed.paint = Some(on);
                            this.edit(|l| set_pixel(l, cell, on), cx);
                        });
                    }
                });
                if let Some(on) = painting {
                    window.on_mouse_event({
                        let view = view.clone();
                        move |e: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble || e.pressed_button != Some(MouseButton::Left) {
                                return;
                            }
                            let Some(cell) = cell_at(e.position) else { return };
                            if pixels.contains(&cell) != on {
                                let _ = view.update(cx, |this, cx| this.edit(|l| set_pixel(l, cell, on), cx));
                            }
                        }
                    });
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            let _ = view.update(cx, |this, cx| {
                                this.ed.paint = None;
                                cx.notify();
                            });
                        }
                    });
                }
            },
        )
        .size(px(side))
        .flex_none()
    }

    pub(super) fn editor_screen(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sel = self.selected();
        let draw_mode = sel.shape == Shape::Custom;
        let backdrop = BACKDROPS[self.ed.backdrop.min(BACKDROPS.len() - 1)].0;
        let whole = Arc::new(raster::render(&self.ed.layers));

        let backdrops = BACKDROPS.iter().enumerate().map(|(i, &(color, label))| {
            div()
                .id(("backdrop", i))
                .size(px(36.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .when(self.ed.backdrop == i, |d| d.bg(c(SURFACE)))
                .tooltip(tip(label))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.ed.backdrop = i;
                    cx.notify();
                }))
                .child(div().size(px(18.)).rounded_full().bg(c(color)).border_1().border_color(border()))
        });
        let backdrops: Vec<_> = backdrops.collect();

        let stage = div()
            .flex_1()
            .min_h(px(380.))
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .rounded(px(16.))
            .bg(c(backdrop))
            .map(|d| if draw_mode { d.child(self.pixel_grid(cx)) } else { d.child(bitmap_view(whole.clone(), 4., 1.)) });

        let left = div()
            .id("editor-stage")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .px(px(32.))
            .pt(px(SCREEN_TOP))
            .pb(px(32.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(super::heading(
                        if draw_mode { "Draw" } else { "New crosshair" },
                        if draw_mode { "×14 · click or drag to paint pixels".into() } else { "×4 zoom".into() },
                    ))
                    .child(div().flex().flex_none().gap(px(4.)).p(px(4.)).rounded_full().bg(c(RAISED)).children(backdrops)),
            )
            .child(stage)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .size(px(72.))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .rounded(px(10.))
                            .bg(c(backdrop))
                            .child(fitted(whole.clone(), 72.)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(div().font_weight(FontWeight::MEDIUM).child("Actual size"))
                            .child(div().font_family(mono()).text_size(px(13.)).text_color(c(MUTED)).child(format!(
                                "1:1 · {} × {} px · {}",
                                whole.w,
                                whole.h,
                                plural(self.ed.layers.len(), "layer", "layers")
                            ))),
                    ),
            );

        // ---- side panel ----

        let thumbs = self.ed.layers.iter().enumerate().map(|(i, layer)| {
            let on = i == self.ed.sel;
            let bmp = Arc::new(raster::render(std::slice::from_ref(layer)));
            let ghost = bmp.clone();
            div()
                .id(("shape", i))
                .size(px(44.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .rounded(px(8.))
                .bg(c(SURFACE))
                .border_2()
                .border_color(c(if on { ACCENT } else { SURFACE }))
                .cursor_pointer()
                .tooltip(tip(format!("Layer {} · {}", i + 1, layer.shape.name())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.ed.sel = i;
                    this.ed.picker = false;
                    cx.notify();
                }))
                .on_drag(DragShape(i), move |_, grab, _, cx| cx.new(|_| ShapeGhost { bmp: ghost.clone(), grab }))
                .drag_over::<DragShape>(|style, _, _, _| style.border_color(c(FAINT)))
                .on_drop(cx.listener(move |this, drag: &DragShape, _, cx| this.move_shape(drag.0, i, cx)))
                .map(|d| if bmp.is_blank() { d.child(icon("pencil", 16., c(FAINT))) } else { d.child(fitted(bmp, 32.)) })
        });

        let picker = div()
            .id("shape-picker")
            .absolute()
            .left(px(0.))
            .top(px(52.))
            .p(px(6.))
            .flex()
            .gap(px(2.))
            .rounded(px(12.))
            .bg(c(RAISED))
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.ed.picker = false;
                cx.notify();
            }))
            .children(Shape::ALL.iter().enumerate().map(|(i, &shape)| {
                div()
                    .id(("add-shape", i))
                    .w(px(50.))
                    .h(px(64.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .fx(self, cx, |d, t| d.bg(fade(SURFACE, t)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.ed.layers.push(EditLayer::new(shape));
                        this.ed.sel = this.ed.layers.len() - 1;
                        this.ed.picker = false;
                        cx.notify();
                    }))
                    .child(icon(shape_icon(shape), 22., c(TEXT)))
                    .child(div().font_family(mono()).text_size(px(10.5)).line_height(px(14.)).text_color(c(MUTED)).child(shape.name().to_lowercase()))
            }));

        let layer_row = div()
            .relative()
            .child(
                div().flex().flex_wrap().gap(px(6.)).children(thumbs).child(
                    icon_button(self, cx, "shape-add", "plus", 16., (MUTED, TEXT), (Some(RAISED), HOVER))
                        .size(px(44.))
                        .rounded(px(8.))
                        .tooltip(tip("Add layer"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.ed.picker = !this.ed.picker;
                            cx.notify();
                        })),
                ),
            )
            .when(self.ed.picker, |d| d.child(deferred(picker).with_priority(1)));

        let can_delete = self.ed.layers.len() > 1;
        let title = div()
            .pt(px(14.))
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(border())
            .child(div().font_weight(FontWeight::MEDIUM).child(format!("Layer {} · {}", self.ed.sel + 1, sel.shape.name())))
            .child(
                icon_button(self, cx, "shape-delete", "trash", 14., (MUTED, if can_delete { DANGER } else { MUTED }), (None, if can_delete { RAISED } else { BG }))
                    .size(px(40.))
                    .rounded_full()
                    .map(|b| {
                        if can_delete {
                            b.tooltip(tip("Delete layer")).on_click(cx.listener(|this, _, _, cx| {
                                if this.ed.layers.len() > 1 {
                                    let sel = this.ed.sel.min(this.ed.layers.len() - 1);
                                    this.ed.layers.remove(sel);
                                    this.ed.sel = sel.saturating_sub(1);
                                    cx.notify();
                                }
                            }))
                        } else {
                            b.opacity(0.4)
                        }
                    }),
            );

        // A caption on the left, a field on the right: sizes, rotation and colors.
        let field_row = |label: &'static str, field: gpui::AnyElement| {
            div().flex().flex_none().items_center().justify_between().child(caption(label)).child(field)
        };
        let size_row = |label: &'static str, dim: Dim, cx: &mut Context<Self>| {
            field_row(label, self.knob_field(Knob::Size(dim), cx).into_any_element())
        };
        let sizes: Vec<Div> = match sel.shape {
            Shape::Cross | Shape::T => vec![
                size_row("length", Dim::Length, cx),
                size_row("thickness", Dim::Thickness, cx),
                size_row("gap", Dim::Gap, cx),
            ],
            Shape::Circle => vec![size_row("radius", Dim::Radius, cx), size_row("thickness", Dim::Thickness, cx)],
            Shape::Dot => vec![size_row("size", Dim::Dot, cx)],
            Shape::Custom => vec![],
        };

        let drawing = draw_mode.then(|| {
            div()
                .px(px(12.))
                .py(px(10.))
                .flex()
                .flex_none()
                .items_center()
                .justify_between()
                .rounded(px(8.))
                .bg(c(RAISED))
                .child(
                    div()
                        .font_family(mono())
                        .text_size(px(11.))
                        .child(format!("drawing · {}", plural(sel.pixels.len(), "pixel", "pixels"))),
                )
                .child(
                    div()
                        .id("clear-pixels")
                        .h(px(32.))
                        .px(px(4.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .font_family(mono())
                        .text_size(px(11.))
                        .fx(self, cx, |d, t| d.text_color(mix(ACCENT, TEXT, t)))
                        .child("clear")
                        .on_click(cx.listener(|this, _, _, cx| this.edit(|l| l.pixels.clear(), cx))),
                )
        });

        let outline = div()
            .id("outline")
            .h(px(40.))
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.flip("outline");
                this.edit(|l| l.outline = !l.outline, cx)
            }))
            .child(caption(if sel.outline { format!("outline · {} px", sel.outline_width) } else { "outline · off".to_string() }))
            .child(switch(sel.outline, RAISED, 36., 20., self.flip_amount("outline")));

        let color = field_row("color", self.color_field("ed-color", sel.color, |l, v| l.color = v, cx).into_any_element());

        // How the outline looks, while it is on.
        let outline_settings = sel.outline.then(|| {
            div()
                .flex()
                .flex_none()
                .flex_col()
                .gap(px(18.))
                .child(size_row("outline width", Dim::Outline, cx))
                .child(field_row(
                    "outline color",
                    self.color_field("ed-outline-color", sel.outline_color, |l, v| l.outline_color = v, cx).into_any_element(),
                ))
                .child(self.editor_slider(
                    SliderSpec { id: "ed-outline-opacity", label: "outline opacity", unit: "%", current: sel.outline_opacity as i32, min: 10, max: 100, step: 5 },
                    |l, v| l.outline_opacity = v as u8,
                    cx,
                ))
        });

        let rotation = field_row("rotation", self.knob_field(Knob::EditorRotation, cx).into_any_element());

        let panel = div()
            .id("editor-panel")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(div().flex().flex_none().flex_col().gap(px(8.)).child(caption("layers · bottom to top · drag to reorder")).child(layer_row))
            .child(title)
            .children(sizes)
            .child(rotation)
            .children(drawing)
            .child(outline)
            .children(outline_settings)
            .child(color)
            .child(self.editor_slider(SliderSpec { id: "ed-opacity", label: "opacity", unit: "%", current: sel.opacity as i32, min: 20, max: 100, step: 5 }, |l, v| l.opacity = v as u8, cx));
        // The buttons stay at the bottom of the panel while the rest scrolls.
        let footer = div().flex_none().px(px(24.)).pb(px(24.)).child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("editor-reset")
                            .flex_1()
                            .h(px(44.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .fx(self, cx, |d, t| d.bg(mix(RAISED, HOVER, t)))
                            .cursor_pointer()
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Reset")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.ed = EditorState { backdrop: this.ed.backdrop, ..EditorState::new() };
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("editor-save")
                            .flex_grow()
                            .flex_shrink()
                            .flex_basis(px(40.))
                            .h(px(44.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .fx(self, cx, |d, t| d.bg(mix(TEXT, 0xffffff, t)))
                            .cursor_pointer()
                            .text_color(c(BG))
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Save to gallery")
                            .on_click(cx.listener(|this, _, _, cx| this.save_to_gallery(cx))),
                    ),
            );
        let side = div().w(px(320.)).h_full().flex_none().flex().flex_col().border_l_1().border_color(border()).child(panel).child(footer);

        div().flex_1().min_h_0().flex().child(left).child(side)
    }
}

fn set_pixel(layer: &mut EditLayer, cell: (i32, i32), on: bool) {
    if on {
        layer.pixels.insert(cell);
    } else {
        layer.pixels.remove(&cell);
    }
}
