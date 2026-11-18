//! About: what krosshair is, what inspired it and what it is made with.

use super::theme::*;
use super::widgets::*;
use super::{Krosshair, SCREEN_TOP};
use gpui::{div, prelude::*, px, Context, Div, ElementId, FontWeight};

/// A credit: name, a note (who, what for, license), and where to read more.
type Credit = (&'static str, &'static str, &'static str);

/// What krosshair took its ideas and look from.
const INSPIRED_BY: [Credit; 2] = [
    ("RTSS_Crosshair", "by LazySlavik · idea · MIT License", "https://github.com/LazySlavik/RTSS_Crosshair"),
    ("x76.gg", "many elements were inspired by the impression this site made", "https://x76.gg"),
];

/// What krosshair is built on.
const MADE_WITH: [Credit; 5] = [
    ("RivaTuner Statistics Server", "by Unwinder · overlay", "https://www.guru3d.com/page/rivatuner-rtss-homepage/"),
    ("gpui", "by Zed Industries · window & controls · Apache License 2.0", "https://www.gpui.rs/"),
    ("Archivo", "by Omnibus-Type · interface font · SIL Open Font License 1.1", "https://github.com/Omnibus-Type/Archivo"),
    ("IBM Plex Mono", "by IBM · monospace font · SIL Open Font License 1.1", "https://github.com/IBM/plex"),
    ("Lucide", "some of the icons · ISC License", "https://lucide.dev"),
];

impl Krosshair {
    /// One credit: its name and a note on the left, an arrow on the right. A click opens its page.
    fn credit_row(&self, id: ElementId, (name, about, url): Credit, cx: &mut Context<Self>) -> Div {
        div().child(
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(24.))
                .px(px(20.))
                .py(px(16.))
                .rounded(px(12.))
                .cursor_pointer()
                .fx(self, cx, |d, t| d.bg(mix(SURFACE, RAISED, t)))
                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(div().font_weight(FontWeight::MEDIUM).child(name))
                        .child(div().text_size(px(13.)).line_height(px(18.)).text_color(c(MUTED)).child(about)),
                )
                .child(icon("external", 16., c(MUTED))),
        )
    }

    /// The project's repository (from Cargo.toml), without the scheme, and an arrow; a click
    /// opens it.
    fn repository_link(&self, cx: &mut Context<Self>) -> impl IntoElement {
        const URL: &str = env!("CARGO_PKG_REPOSITORY");
        let lit = self.hover_amount(&gpui::ElementId::from("about-repository"));
        div()
            .id("about-repository")
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .font_family(mono())
            .text_size(px(13.))
            .line_height(px(18.))
            .fx(self, cx, |d, t| d.text_color(mix(ACCENT, TEXT, t)))
            .tooltip(tip(URL))
            .on_click(cx.listener(|_, _, _, cx| cx.open_url(URL)))
            .child(URL.trim_start_matches("https://"))
            .child(icon("external", 14., mix(ACCENT, TEXT, lit)))
    }

    pub(super) fn about_screen(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hero = div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(20.))
            .child(
                div()
                    .size(px(72.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(16.))
                    .bg(c(RAISED))
                    .child(icon("logo", 40., c(TEXT))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(12.))
                            .child(div().text_size(px(28.)).font_weight(FontWeight::MEDIUM).line_height(px(30.)).child("krosshair"))
                            .child(div().font_family(mono()).text_size(px(13.)).text_color(c(MUTED)).child(format!("version {}", env!("CARGO_PKG_VERSION")))),
                    )
                    .child(div().text_color(c(MUTED)).line_height(px(20.)).child("Open-source crosshair overlay made by kirchesz."))
                    .child(div().flex().child(self.repository_link(cx))),
            );

        // A caption over its credits.
        let group = |title: &'static str, credits: &[Credit], cx: &mut Context<Self>| {
            let rows: Vec<Div> =
                credits.iter().enumerate().map(|(i, &credit)| self.credit_row((title, i).into(), credit, cx)).collect();
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(caption(title))
                .child(div().flex().flex_col().gap(px(2.)).children(rows))
        };
        let inspired_by = group("inspired by", &INSPIRED_BY, cx);
        let made_with = group("made with", &MADE_WITH, cx);

        div()
            .id("about-screen")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(32.))
            .pt(px(SCREEN_TOP))
            .pb(px(40.))
            .flex()
            .flex_col()
            .child(
                div()
                    .max_w(px(1100.))
                    .flex()
                    .flex_col()
                    .gap(px(28.))
                    .child(hero)
                    // Two columns: what krosshair is made with, and on the right what inspired it.
                    .child(div().flex().items_start().gap(px(24.)).child(made_with).child(inspired_by)),
            )
    }
}
