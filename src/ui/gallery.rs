//! The gallery: every PNG and GIF in the crosshairs folder. Files dropped on it from Explorer are
//! copied there.

use super::theme::*;
use super::widgets::*;
use super::{heading, plural, Krosshair, Screen, SCREEN_TOP};
use crate::store;
use gpui::{div, prelude::*, px, Context, ExternalPaths, FontWeight, PathPromptOptions};
use std::path::Path;

/// The folder the way a person would say it: without the user profile part when it is inside.
fn short_path(path: &Path) -> String {
    let home = std::env::var_os("USERPROFILE");
    let rel = home.as_ref().and_then(|home| path.strip_prefix(home).ok());
    rel.unwrap_or(path).display().to_string()
}

impl Krosshair {
    fn change_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Use this folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            let _ = this.update(cx, |this, cx| {
                this.cfg.gallery_dir = dir;
                this.rescan();
                this.commit(cx);
            });
        })
        .detach();
    }

    fn delete_crosshair(&mut self, file: &str, cx: &mut Context<Self>) {
        let result = store::delete_crosshair(&self.cfg.gallery_dir, file);
        self.rescan();
        self.commit(cx);
        if let Err(e) = result {
            self.error = Some(e.to_string().into());
        }
    }

    /// Files dropped on the gallery: copy the pictures in, and say what happened.
    fn drop_files(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let r = store::import(&self.cfg.gallery_dir, paths.paths());
        self.rescan();
        self.fresh = r.added.iter().cloned().collect();
        let mut said = Vec::new();
        if !r.added.is_empty() {
            said.push(format!("added {}", plural(r.added.len(), "crosshair", "crosshairs")));
        }
        if !r.present.is_empty() {
            said.push(format!("{} already in the gallery", r.present.len()));
        }
        let skipped: Vec<String> = r.skipped.iter().map(|(name, why)| format!("{name}: {why}")).collect();
        let mut text = said.join(", ");
        if !skipped.is_empty() {
            if !text.is_empty() {
                text += "; ";
            }
            text += &format!("left out {}", skipped.join(", "));
        }
        let text = capitalize(&text);
        if r.added.is_empty() && !r.skipped.is_empty() {
            self.error = Some(text.into());
            self.info = None;
        } else {
            self.error = None;
            self.info = Some(text.into());
        }
        // The overlay may show something new now, and the new files may be in use by layers.
        self.sync_overlay();
        cx.notify();
    }

    pub(super) fn gallery_screen(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let folder = div()
            .id("gallery-folder")
            .h(px(44.))
            .max_w(px(420.))
            .min_w_0()
            .pl(px(14.))
            .pr(px(8.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(8.))
            .fx(self, cx, |d, t| d.bg(mix(RAISED, HOVER, t)))
            .cursor_pointer()
            .tooltip(tip(self.cfg.gallery_dir.display().to_string()))
            .on_click(cx.listener(|this, _, _, cx| this.change_folder(cx)))
            .child(icon("folder", 16., c(MUTED)))
            .child(div().min_w_0().truncate().font_family(mono()).text_size(px(13.)).child(short_path(&self.cfg.gallery_dir)))
            .child(
                div()
                    .h(px(32.))
                    .px(px(12.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .rounded_full()
                    .bg(c(SURFACE))
                    .text_size(px(13.5))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Change"),
            );

        let add = div()
            .id("gallery-new")
            .w(px(160.))
            .h(px(182.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .rounded(px(16.))
            .fx(self, cx, |d, t| d.bg(mix(SURFACE, RAISED, t)).text_color(mix(MUTED, TEXT, t)))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.go(Screen::Editor, cx)))
            .child(div().size(px(48.)).flex().items_center().justify_center().rounded_full().bg(c(TEXT)).child(icon("plus", 18., c(BG))))
            .child(div().font_weight(FontWeight::MEDIUM).child("New crosshair"));

        let cards = self.gallery.iter().enumerate().map(|(i, item)| {
            let file = item.file.clone();
            let fresh = self.fresh.contains(&item.file);
            div()
                .id(("crosshair", i))
                .w(px(160.))
                .p(px(8.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .rounded(px(16.))
                .bg(c(RAISED))
                .border_2()
                .border_color(c(if fresh { ACCENT } else { RAISED }))
                .tooltip(tip(item.file.clone()))
                .child(
                    div()
                        .h(px(112.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .overflow_hidden()
                        .rounded(px(10.))
                        .bg(c(SURFACE))
                        .child(thumb(self, item, 96., 3., 1.)),
                )
                .child(
                    div().p(px(4.)).pl(px(8.)).flex().items_center().justify_between().child(anim_label(item).unwrap_or_else(div)).child(
                        icon_button(self, cx, ("crosshair-delete", i), "trash", 14., (MUTED, DANGER), (Some(SURFACE), BG))
                            .size(px(40.))
                            .rounded_full()
                            .tooltip(tip("Move to the Recycle Bin"))
                            .on_click(cx.listener(move |this, _, _, cx| this.delete_crosshair(&file, cx))),
                    ),
                )
        });

        let content = div()
            .id("gallery-screen")
            .size_full()
            .overflow_y_scroll()
            .px(px(32.))
            .pt(px(SCREEN_TOP))
            .pb(px(40.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(heading("Crosshairs", format!("{} total", self.gallery.len())))
                    .child(folder),
            )
            .child(div().flex().flex_wrap().gap(px(16.)).child(add).children(cards));

        // Over the whole screen: invisible until files from Explorer are dragged over it, then a
        // frame with a note; dropping them adds them to the gallery.
        let drop_zone = div()
            .id("gallery-drop")
            .absolute()
            .top(px(12.))
            .left(px(16.))
            .right(px(16.))
            .bottom(px(12.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(16.))
            .border_2()
            .border_color(gpui::transparent_black())
            .text_color(gpui::transparent_black())
            .drag_over::<ExternalPaths>(|style, _, _, _| {
                style.bg(fade(BG, 0.85)).border_color(c(ACCENT)).text_color(c(TEXT))
            })
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.drop_files(paths, cx)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(div().text_size(px(20.)).font_weight(FontWeight::MEDIUM).child("Drop to add to the gallery"))
                    .child(div().font_family(mono()).text_size(px(13.)).child("PNG and GIF files are copied to the gallery folder")),
            );

        div().relative().flex_1().min_h_0().child(content).child(drop_zone)
    }
}

/// The text with its first letter in capitals.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
