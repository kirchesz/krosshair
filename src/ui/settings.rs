//! Settings in two columns: on the left the overlay (how the crosshair gets over games, then its
//! scale, rotation and color mask as in the rail), on the right how the program behaves (tray,
//! starting with Windows, updates).

use super::theme::*;
use super::widgets::*;
use super::{heading, Knob, Krosshair, SCREEN_TOP};
use crate::model::TrayMode;
use crate::{autorun, tray};
use gpui::{deferred, div, prelude::*, px, AnyElement, Context, Div, FontWeight, SharedString};

/// Ways to draw the crosshair over games. Only RTSS's on-screen display so far.
const METHODS: [&str; 1] = ["RTSS"];

/// What each tray choice does, under the setting's title and in the question on the first
/// minimize or close.
pub(super) fn tray_about(mode: TrayMode) -> &'static str {
    match mode {
        TrayMode::Never => "Minimize goes to the taskbar, close quits.",
        TrayMode::OnMinimize => "Minimize hides to the tray, the crosshair stays on.",
        TrayMode::OnClose => "Close hides to the tray, the crosshair stays on.",
    }
}

impl Krosshair {
    /// The tray choice, from the settings or the question: made for good, nothing is asked any
    /// more.
    pub(super) fn set_tray(&mut self, mode: TrayMode, cx: &mut Context<Self>) {
        self.cfg.tray = mode;
        self.cfg.tray_asked = true;
        tray::set_mode(mode);
        tray::set_ask(false);
        self.commit(cx);
    }

    fn set_autorun(&mut self, on: bool, cx: &mut Context<Self>) {
        self.error = autorun::set(on).err().map(|e| e.to_string().into());
        self.autorun = autorun::command();
        cx.notify();
    }

    /// Switched on: checks right away. Switched off: forgets what was found.
    fn set_check_updates(&mut self, on: bool, cx: &mut Context<Self>) {
        self.cfg.check_updates = on;
        self.commit(cx);
        if on {
            self.check_for_update(cx);
        } else if !matches!(self.update, super::updates::UpdateState::Installing(_)) {
            self.update = super::updates::UpdateState::Idle;
        }
    }

    /// One setting: a title and what it does on the left, its control on the right. A click
    /// anywhere on the row does what `click` says.
    fn setting_row(
        &self,
        id: &'static str,
        title: &'static str,
        about: impl Into<SharedString>,
        control: AnyElement,
        click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            .on_click(cx.listener(move |this, _, _, cx| click(this, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().font_weight(FontWeight::MEDIUM).child(title))
                    .child(div().text_size(px(13.)).line_height(px(18.)).text_color(c(MUTED)).child(about.into())),
            )
            .child(control)
            .into_any_element()
    }

    /// A setting whose control does everything itself (a tape between minus and plus): the row
    /// only holds it, so it neither lights up nor reacts to clicks.
    fn control_row(&self, id: &'static str, title: &'static str, about: &'static str, control: AnyElement) -> AnyElement {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(24.))
            .px(px(20.))
            .py(px(16.))
            .rounded(px(12.))
            .bg(c(SURFACE))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().font_weight(FontWeight::MEDIUM).child(title))
                    .child(div().text_size(px(13.)).line_height(px(18.)).text_color(c(MUTED)).child(about)),
            )
            .child(div().flex_none().child(control))
            .into_any_element()
    }

    /// A setting with a switch; the row flips it.
    fn switch_row(
        &self,
        id: &'static str,
        title: &'static str,
        about: impl Into<SharedString>,
        on: bool,
        set: fn(&mut Self, bool, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let control = switch(on, RAISED, 36., 20., self.flip_amount(id)).into_any_element();
        self.setting_row(
            id,
            title,
            about,
            control,
            move |this, cx| {
                this.flip(id);
                set(this, !on, cx);
            },
            cx,
        )
    }

    /// A setting the current method leaves no choice in: dimmed, with a small note after its
    /// title, showing what it is, not clickable; the tooltip says why and what there would be to
    /// choose from.
    fn disabled_row(&self, id: &'static str, title: &'static str, about: &'static str, value: String, why: String) -> AnyElement {
        let face = div()
            .h(px(32.))
            .px(px(12.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .bg(c(BG))
            .border_1()
            .border_color(border())
            .text_size(px(13.5))
            .font_weight(FontWeight::MEDIUM)
            .child(value)
            .child(icon("chevron", 12., c(MUTED)));
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(24.))
            .px(px(20.))
            .py(px(16.))
            .rounded(px(12.))
            .bg(c(SURFACE))
            .tooltip(tip(why))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    // The small note sits in the middle of the title's height.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().font_weight(FontWeight::MEDIUM).line_height(px(20.)).text_color(c(MUTED)).child(title))
                            .child(
                                div()
                                    .font_family(mono())
                                    .text_size(px(11.))
                                    .line_height(px(16.))
                                    // Small type sits low in its line: 1 px up, onto the
                                    // title's middle (measured).
                                    .mb(px(2.))
                                    .text_color(c(MUTED))
                                    .child("unsupported with current method"),
                            ),
                    )
                    .child(div().text_size(px(13.)).line_height(px(18.)).text_color(c(MUTED)).child(about)),
            )
            .child(face.opacity(0.4))
            .into_any_element()
    }

    /// A setting with a list of choices: the current one with an arrow; the row opens the list,
    /// a choice or a click elsewhere closes it.
    fn select_row<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        title: &'static str,
        about: impl Into<SharedString>,
        (current, choices, label): (T, &'static [T], fn(T) -> &'static str),
        pick: fn(&mut Self, T, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let face = div()
            .h(px(32.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .bg(c(BG))
            .border_1()
            .border_color(border())
            .text_size(px(13.5))
            .font_weight(FontWeight::MEDIUM)
            .child(label(current))
            .child(icon("chevron", 12., c(MUTED)));

        let options = choices.iter().enumerate().map(|(i, &choice)| {
            let on = choice == current;
            div()
                .id((id, i))
                .h(px(36.))
                .px(px(10.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .rounded(px(8.))
                .cursor_pointer()
                .text_size(px(14.))
                .fx(self, cx, |d, t| d.bg(fade(SURFACE, t)))
                .when(on, |d| d.text_color(c(ACCENT)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_select = None;
                    if choice != current {
                        pick(this, choice, cx);
                    }
                    cx.notify();
                }))
                .child(label(choice))
                .when(on, |d| d.child(icon("check", 14., c(ACCENT))))
        });
        let menu = div()
            .absolute()
            .top(px(40.))
            .right(px(0.))
            .w(px(180.))
            .p(px(6.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .rounded(px(12.))
            .bg(c(RAISED))
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.open_select = None;
                cx.notify();
            }))
            .children(options);

        let open = self.open_select == Some(id);
        let control =
            div().relative().flex_none().child(face).when(open, |d| d.child(deferred(menu).with_priority(1))).into_any_element();
        self.setting_row(
            id,
            title,
            about,
            control,
            move |this, cx| {
                this.open_select = if this.open_select == Some(id) { None } else { Some(id) };
                cx.notify();
            },
            cx,
        )
    }

    pub(super) fn settings_screen(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut autorun_about = String::from("Starts minimized when you sign in.");
        // On, but for another copy of the program (moved, or a different build).
        if let Some(command) = &self.autorun
            && autorun::this_command().is_ok_and(|this| !this.eq_ignore_ascii_case(command))
        {
            autorun_about.push_str(&format!(" Set for {command}; switch off and on for this copy."));
        }

        // A big heading over its settings.
        let section = |title: &'static str, rows: Vec<AnyElement>| -> Div {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(heading(title, String::new()))
                .child(div().flex().flex_col().gap(px(2.)).children(rows))
        };

        // The crosshair as a whole: scale, rotation and color mask, as at the bottom of the rail.
        let mask = self.cfg.mask;
        let mask_face = div()
            .h(px(32.))
            .px(px(12.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .bg(c(BG))
            .border_1()
            .border_color(border())
            .child(div().size(px(16.)).rounded_full().bg(c(mask)).border_1().border_color(alpha(TEXT, 0x59)))
            .child(div().font_family(mono()).text_size(px(13.)).child(format!("#{mask:06X}")))
            .into_any_element();

        // RTSS draws inside the game, so the crosshair is on the game's display: nothing to pick
        // until a method draws over the screen itself. The button shows the primary display; the
        // tooltip lists them all.
        let primary = self.displays.iter().find(|d| d.primary).or(self.displays.first());
        let mut why = String::from("RTSS draws on the game's display.");
        for (i, d) in self.displays.iter().enumerate() {
            why.push_str(&format!("\n{}. {}{}", i + 1, d.label(), if d.primary { " (primary)" } else { "" }));
        }
        let display = self.disabled_row(
            "set-display",
            "Display",
            "Which screen shows the crosshair.",
            primary.map_or_else(|| "Primary".to_string(), |d| d.name.clone()),
            why,
        );

        // How the crosshair gets into games, and how it looks there.
        let overlay = section(
            "Overlay",
            vec![
                self.select_row(
                    "set-method",
                    "Method",
                    "How the crosshair gets into games.",
                    ("RTSS", &METHODS, |m| m),
                    |_, _, _| {},
                    cx,
                ),
                display,
                self.control_row(
                    "set-scale",
                    "Scale",
                    "All layers at once. Whole values stay sharp.",
                    self.knob_field(Knob::Scale, cx).into_any_element(),
                ),
                self.control_row(
                    "set-rotation",
                    "Rotation",
                    "All layers at once. Quarter turns stay sharp.",
                    self.knob_field(Knob::Rotation, cx).into_any_element(),
                ),
                self.setting_row(
                    "set-mask",
                    "Color mask",
                    "Tints every crosshair. White leaves colors as they are.",
                    mask_face,
                    Self::pick_mask,
                    cx,
                ),
            ],
        );

        // How the program itself behaves.
        let app = section(
            "App",
            vec![
                self.select_row(
                    "set-tray",
                    "Hide to tray",
                    tray_about(self.cfg.tray),
                    (self.cfg.tray, &TrayMode::ALL, TrayMode::label),
                    Self::set_tray,
                    cx,
                ),
                self.switch_row("set-autorun", "Start with Windows", autorun_about, self.autorun.is_some(), Self::set_autorun, cx),
                self.switch_row(
                    "set-updates",
                    "Check for updates",
                    self.update_about(),
                    self.cfg.check_updates,
                    Self::set_check_updates,
                    cx,
                ),
            ],
        );

        div()
            .id("settings-screen")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(32.))
            .pt(px(SCREEN_TOP))
            .pb(px(40.))
            .flex()
            .flex_col()
            .child(div().max_w(px(1100.)).flex().items_start().gap(px(24.)).child(overlay).child(app))
    }
}
