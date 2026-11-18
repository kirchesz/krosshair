//! Main screen: the layers of the current profile.

use super::theme::*;
use super::widgets::*;
use super::{heading, Krosshair, LayerDraft, Screen, SCREEN_TOP};
use crate::raster::Bitmap;
use gpui::{div, prelude::*, px, Context, FontWeight, Pixels, Point, Window};
use std::sync::Arc;

/// Index of the layer card being dragged.
#[derive(Clone)]
struct DragLayer(usize);

/// What follows the cursor while a card is dragged.
struct DragGhost {
    bmp: Option<Arc<Bitmap>>,
    grab: Point<Pixels>,
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let side = px(96.);
        div().pl(self.grab.x - side / 2.).pt(self.grab.y - side / 2.).child(
            div()
                .size(side)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(12.))
                .bg(c(SURFACE))
                .border_2()
                .border_color(c(ACCENT))
                .opacity(0.9)
                .when_some(self.bmp.clone(), |d, bmp| {
                    let scale = (72. / bmp.w.max(bmp.h).max(1) as f32).floor().clamp(1., 3.);
                    d.child(bitmap_view(bmp, scale, 1.))
                }),
        )
    }
}

impl Krosshair {
    pub(super) fn open_layer(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        self.rescan();
        self.draft = match index.and_then(|i| self.cfg.profile().layers.get(i).map(|l| (i, l))) {
            Some((i, l)) => LayerDraft { index: Some(i), file: Some(l.file.clone()), anchor: l.anchor, ox: l.ox, oy: l.oy, on: l.on, rotation: l.rotation },
            None => LayerDraft { index: None, file: self.gallery.first().map(|g| g.file.clone()), anchor: 4, ox: 0, oy: 0, on: true, rotation: 0 },
        };
        self.go(Screen::LayerEdit, cx);
    }

    fn move_layer(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let layers = &mut self.cfg.profile_mut().layers;
        if from != to && from < layers.len() && to < layers.len() {
            let layer = layers.remove(from);
            layers.insert(to, layer);
            self.commit(cx);
        }
    }

    pub(super) fn layers_screen(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let layers = &self.cfg.profile().layers;
        let shown = layers.iter().filter(|l| l.on).count();
        // Next to the "Layers" heading, so the count goes without the word. Reordering needs two.
        let note = match layers.len() {
            0 => "none yet".to_string(),
            1 => format!("1 total · {shown} on"),
            n => format!("{n} total · {shown} on · drag to reorder"),
        };

        let cards = layers.iter().enumerate().map(|(i, layer)| {
            let item = self.item(&layer.file);
            let bmp = item.map(|g| g.bmp.clone());
            let preview = match item {
                Some(item) => thumb_turned(self, item, layer.rotation, 132., 4., if layer.on { 1. } else { 0.25 }),
                None => div()
                    .font_family(mono())
                    .text_size(px(11.))
                    .text_color(c(FAINT))
                    .child(format!("{} is missing", layer.file))
                    .into_any_element(),
            };
            div()
                .id(("layer", i))
                .w(px(200.))
                .p(px(6.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .rounded(px(16.))
                .bg(c(RAISED))
                .border_2()
                .border_color(c(RAISED))
                .on_drag(DragLayer(i), move |_, grab, _, cx| cx.new(|_| DragGhost { bmp: bmp.clone(), grab }))
                .drag_over::<DragLayer>(|style, _, _, _| style.border_color(c(ACCENT)))
                .on_drop(cx.listener(move |this, drag: &DragLayer, _, cx| this.move_layer(drag.0, i, cx)))
                .child(
                    div()
                        .h(px(156.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .overflow_hidden()
                        .rounded(px(10.))
                        .bg(c(SURFACE))
                        .child(preview),
                )
                .child(
                    div()
                        .p(px(4.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .child(div().w(px(28.)).h(px(40.)).flex().items_center().justify_center().cursor_move().child(icon("grip", 14., c(FAINT))))
                                .child(
                                    div()
                                        .id(("layer-on", i))
                                        .w(px(44.))
                                        .h(px(40.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .tooltip(tip("Show layer"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(l) = this.cfg.profile_mut().layers.get_mut(i) {
                                                l.on = !l.on;
                                            }
                                            this.flip(("layer-on", i));
                                            this.commit(cx);
                                        }))
                                        .child(switch(layer.on, SURFACE, 32., 18., self.flip_amount(("layer-on", i)))),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(4.))
                                .child(
                                    icon_button(self, cx, ("layer-edit", i), "pencil", 14., (MUTED, TEXT), (Some(SURFACE), BG))
                                        .size(px(40.))
                                        .rounded_full()
                                        .tooltip(tip("Edit layer"))
                                        .on_click(cx.listener(move |this, _, _, cx| this.open_layer(Some(i), cx))),
                                )
                                .child(
                                    icon_button(self, cx, ("layer-delete", i), "trash", 14., (MUTED, DANGER), (Some(SURFACE), BG))
                                        .size(px(40.))
                                        .rounded_full()
                                        .tooltip(tip("Delete layer"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let layers = &mut this.cfg.profile_mut().layers;
                                            if i < layers.len() {
                                                layers.remove(i);
                                            }
                                            this.commit(cx);
                                        })),
                                ),
                        ),
                )
        });

        let add = div()
            .id("layer-add")
            .group("add")
            .w(px(200.))
            .h(px(226.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .rounded(px(16.))
            .fx(self, cx, |d, t| d.bg(mix(SURFACE, RAISED, t)).text_color(mix(MUTED, TEXT, t)))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.open_layer(None, cx)))
            .child(div().size(px(48.)).flex().items_center().justify_center().rounded_full().bg(c(TEXT)).child(icon("plus", 18., c(BG))))
            .child(div().font_weight(FontWeight::MEDIUM).child("Add layer"));

        div()
            .id("layers-screen")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(32.))
            .pt(px(SCREEN_TOP))
            .pb(px(40.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(heading("Layers", note))
            .child(div().flex().flex_wrap().gap(px(16.)).children(cards).child(add))
    }
}
