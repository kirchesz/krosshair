//! The question on the first minimize or close: what they should do, now that krosshair can keep
//! the crosshair on from the tray. The answer is the tray setting, and the held minimize or close
//! goes on with it. Escape, Not now or a click beside the card leave the window as it is; the
//! question comes again next time.

use super::settings::tray_about;
use super::theme::*;
use super::widgets::*;
use super::Krosshair;
use crate::model::TrayMode;
use crate::tray::{self, Held};
use gpui::{div, prelude::*, px, Context, FontWeight, KeyDownEvent, MouseButton, Window};

impl Krosshair {
    /// The choice made: kept as the setting, then the held minimize or close happens with it.
    fn answer_tray(&mut self, mode: TrayMode, cx: &mut Context<Self>) {
        let Some(held) = self.held.take() else { return };
        self.set_tray(mode, cx);
        match held {
            Held::Minimize => tray::minimize(),
            Held::Close => tray::close(),
        }
    }

    fn not_now(&mut self, cx: &mut Context<Self>) {
        self.held = None;
        cx.notify();
    }

    fn on_question_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if e.keystroke.key == "escape" {
            self.not_now(cx);
        }
    }

    pub(super) fn tray_question(&self, held: Held, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match held {
            Held::Minimize => "What should minimize do?",
            Held::Close => "What should close do?",
        };
        let choices = TrayMode::ALL.iter().enumerate().map(|(i, &mode)| {
            div()
                .id(("tray-choice", i))
                .px(px(16.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .rounded(px(10.))
                .cursor_pointer()
                .border_1()
                .border_color(border())
                .fx(self, cx, |d, t| d.bg(fade(RAISED, t)))
                .on_click(cx.listener(move |this, _, _, cx| this.answer_tray(mode, cx)))
                .child(div().font_weight(FontWeight::MEDIUM).child(mode.label()))
                .child(div().text_size(px(13.)).line_height(px(18.)).text_color(c(MUTED)).child(tray_about(mode)))
        });
        let not_now = div()
            .id("tray-not-now")
            .h(px(32.))
            .px(px(14.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .cursor_pointer()
            .text_size(px(13.5))
            .font_weight(FontWeight::MEDIUM)
            .fx(self, cx, |d, t| d.bg(fade(RAISED, t)).text_color(mix(MUTED, TEXT, t)))
            .on_click(cx.listener(|this, _, _, cx| this.not_now(cx)))
            .child("Not now");

        let card = div()
            .id("tray-question")
            .w(px(440.))
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .rounded(px(16.))
            .bg(c(SURFACE))
            .border_1()
            .border_color(border())
            .shadow_lg()
            .occlude()
            .track_focus(&self.ask_focus)
            .on_key_down(cx.listener(Self::on_question_key))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().text_size(px(18.)).font_weight(FontWeight::MEDIUM).child(title))
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(c(MUTED))
                            .child("In the tray the crosshair stays on. You can change this in Settings."),
                    ),
            )
            .child(div().flex().flex_col().gap(px(8.)).children(choices))
            .child(div().flex().justify_end().child(not_now));

        // Over the whole window, header included; a click beside the card is "not now".
        div()
            .id("tray-question-backdrop")
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(alpha(0x000000, 0x99))
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.not_now(cx)))
            .child(card)
    }
}
