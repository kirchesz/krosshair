//! Adding or changing a layer: pick a crosshair from the gallery, place it on the screen, and
//! give it a turn, a size and a tint of its own.

use super::theme::*;
use super::widgets::*;
use super::{heading, Knob, Krosshair, Screen, FIELD_W, SCREEN_TOP};
use crate::model::ANCHOR_NAMES;
use gpui::{div, prelude::*, px, Context, FontWeight};

/// Width of the side panel content: the panel is 300 wide with 24 padding.
const PANEL_INNER: f32 = 252.;
const PREVIEW_H: f32 = 142.;

impl Krosshair {
    fn save_layer(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.draft.file.clone() else { return };
        let layer = self.draft.layer(file);
        let index = self.draft.index;
        let layers = &mut self.cfg.profile_mut().layers;
        match index {
            Some(i) if i < layers.len() => layers[i] = layer,
            _ => layers.push(layer),
        }
        self.screen = Screen::Layers;
        self.commit(cx);
    }

    /// The layer's own color mask: a swatch and its hex code, as wide as a tape field. A click
    /// opens the system color picker; white changes nothing.
    fn mask_field(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mask = self.draft.mask;
        let button = div()
            .id("layer-mask")
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
            .tooltip(tip("Tints this layer · white changes nothing"))
            .on_click(cx.listener(|this, _, _, cx| this.pick_layer_mask(cx)))
            .child(div().size(px(22.)).flex_none().rounded_full().bg(c(mask)).border_1().border_color(alpha(TEXT, 0x33)))
            .child(format!("#{mask:06X}"));
        div().w(px(FIELD_W)).flex().p(px(2.)).rounded(px(8.)).bg(c(RAISED)).child(button)
    }

    fn pick_layer_mask(&mut self, cx: &mut Context<Self>) {
        let initial = self.draft.mask;
        cx.spawn(async move |this, cx| {
            let picked = cx.background_spawn(async move { crate::dialogs::choose_color(initial) }).await;
            if let Some(color) = picked {
                let _ = this.update(cx, |this, cx| {
                    // Still the same layer on screen: the picker doesn't stop navigation.
                    if this.screen == Screen::LayerEdit {
                        this.draft.mask = color;
                        this.sync_overlay();
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    pub(super) fn layer_edit_screen(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let d = &self.draft;
        let selected = d.file.as_deref();

        let tiles = self.gallery.iter().enumerate().map(|(i, item)| {
            let on = selected == Some(item.file.as_str());
            let file = item.file.clone();
            div()
                .id(("pick", i))
                .size(px(104.))
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .rounded(px(12.))
                .border_2()
                .cursor_pointer()
                .fx(self, cx, |d, t| {
                    if on {
                        d.bg(c(SURFACE)).border_color(c(ACCENT))
                    } else {
                        d.bg(mix(RAISED, HOVER, t)).border_color(mix(RAISED, HOVER, t))
                    }
                })
                .tooltip(tip(item.file.clone()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.draft.file = Some(file.clone());
                    this.sync_overlay();
                    cx.notify();
                }))
                .child(thumb(self, item, 80., 3., 1.))
        });

        let picker = div()
            .id("layer-edit-gallery")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .px(px(32.))
            .pt(px(SCREEN_TOP))
            .pb(px(32.))
            .flex()
            .flex_col()
            .gap(px(18.))
            // The crosshair for this layer is picked from the gallery.
            .child(heading("Crosshair", format!("{} in the gallery", self.gallery.len())))
            .child(div().flex().flex_wrap().gap(px(12.)).children(tiles))
            .when(self.gallery.is_empty(), |s| {
                s.child(div().text_color(c(MUTED)).child("The gallery is empty. Draw a crosshair in the editor, or put PNG files into the gallery folder."))
            });

        // A small map of the screen with the crosshair where it will be.
        let (col, row) = ((d.anchor % 3) as usize, (d.anchor / 3) as usize);
        let left = ([0.10, 0.50, 0.90][col] + d.ox as f32 / 2560. * 8.).clamp(0.04, 0.96) * PANEL_INNER;
        let top = ([0.14, 0.50, 0.86][row] + d.oy as f32 / 1440. * 8.).clamp(0.06, 0.94) * PREVIEW_H;
        let marker = selected.and_then(|f| self.item(f)).map(|item| {
            let side = 28.;
            div()
                .absolute()
                .left(px(left - side / 2.))
                .top(px(top - side / 2.))
                .size(px(side))
                .flex()
                .items_center()
                .justify_center()
                .child(thumb_layer(self, item, (d.rotation, d.mask), side, 1., 1.))
        });
        let preview = div()
            .relative()
            .w_full()
            .h(px(PREVIEW_H))
            .flex_none()
            .overflow_hidden()
            .rounded(px(8.))
            .bg(c(SURFACE))
            .child(div().absolute().left(px(PANEL_INNER / 2.)).top(px(0.)).w(px(1.)).h_full().bg(alpha(TEXT, 0x14)))
            .child(div().absolute().top(px(PREVIEW_H / 2.)).left(px(0.)).h(px(1.)).w_full().bg(alpha(TEXT, 0x14)))
            .children(marker)
            .child(
                div()
                    .absolute()
                    .left(px(8.))
                    .bottom(px(6.))
                    .font_family(mono())
                    .text_size(px(10.5))
                    .line_height(px(14.))
                    .text_color(c(MUTED))
                    .child(selected.map(|f| f.trim_end_matches(".png").to_owned()).unwrap_or_else(|| "nothing selected".into())),
            );

        let anchors = (0..9u8).map(|i| {
            let on = d.anchor == i;
            div()
                .id(("anchor", i as usize))
                .h(px(40.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .fx(self, cx, |d, t| d.bg(fade(RAISED, t)))
                .tooltip(tip(ANCHOR_NAMES[i as usize]))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.draft.anchor = i;
                    this.sync_overlay();
                    cx.notify();
                }))
                .child(div().size(px(if on { 12. } else { 6. })).rounded_full().bg(c(if on { ACCENT } else { FAINT })))
        });

        // A caption on the left, a tape field on the right.
        let knob_row = |label: &'static str, knob: Knob, cx: &mut Context<Self>| {
            div().flex().items_center().justify_between().child(caption(label)).child(self.knob_field(knob, cx))
        };

        let can_save = selected.is_some();
        let panel = div()
            .id("layer-edit-panel")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(div().flex_none().text_size(px(20.)).font_weight(FontWeight::MEDIUM).line_height(px(22.)).child("Position"))
            .child(preview)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .flex_col()
                    .gap(px(8.))
                    .child(caption(format!("anchor · {}", ANCHOR_NAMES[d.anchor as usize])))
                    .child(div().grid().grid_cols(3).gap(px(4.)).p(px(4.)).rounded(px(8.)).bg(c(SURFACE)).children(anchors)),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .flex_col()
                    .gap(px(10.))
                    .child(knob_row("offset x", Knob::OffsetX, cx))
                    .child(knob_row("offset y", Knob::OffsetY, cx))
                    .child(knob_row("rotation", Knob::LayerRotation, cx))
                    .child(knob_row("scale", Knob::LayerScale, cx))
                    .child(div().flex().items_center().justify_between().child(caption("color")).child(self.mask_field(cx)))
                    .child(
                        div()
                            .id("reset-position")
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .font_family(mono())
                            .text_size(px(11.))
                            .fx(self, cx, |d, t| d.text_color(mix(ACCENT, TEXT, t)))
                            .child("reset to center")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.draft.anchor = 4;
                                this.draft.ox = 0;
                                this.draft.oy = 0;
                                this.sync_overlay();
                                cx.notify();
                            })),
                    ),
            );
        // The buttons stay at the bottom of the panel while the rest scrolls.
        let footer = div().flex_none().px(px(24.)).pb(px(24.)).child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("layer-cancel")
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
                            .child("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.go(Screen::Layers, cx))),
                    )
                    .child(
                        div()
                            .id("layer-save")
                            .flex_1()
                            .h(px(44.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .fx(self, cx, |d, t| d.bg(mix(TEXT, 0xffffff, if can_save { t } else { 0. })))
                            .text_color(c(BG))
                            .text_size(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Save layer")
                            .map(|b| {
                                if can_save {
                                    b.cursor_pointer().on_click(cx.listener(|this, _, _, cx| this.save_layer(cx)))
                                } else {
                                    b.opacity(0.4)
                                }
                            }),
                    ),
            );
        let side = div().w(px(300.)).h_full().flex_none().flex().flex_col().border_l_1().border_color(border()).child(panel).child(footer);

        div().flex_1().min_h_0().flex().child(picker).child(side)
    }
}
