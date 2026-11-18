//! Updates in the window: checking GitHub soon after the start and then twice a day (when the
//! setting is on), the news in the header with an Update button, and the state of the last check
//! under the setting.

use super::theme::*;
use super::widgets::*;
use super::Krosshair;
use crate::update::{self, Release};
use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, SharedString};
use std::time::Duration;

/// The first check waits for the window to settle; then one every twelve hours.
const FIRST_CHECK: Duration = Duration::from_secs(5);
const CHECK_EVERY: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Clone, Debug, Default)]
pub enum UpdateState {
    /// Not checked yet, or checking is off.
    #[default]
    Idle,
    Checking,
    /// This is the newest version.
    Latest,
    /// The check didn't get through (offline, say). Shown only under the setting.
    CheckFailed(SharedString),
    Available(Release),
    /// Downloading and putting the new version in place.
    Installing(String),
    /// The update didn't go through; the release's page is where to get it by hand.
    InstallFailed(SharedString, String),
}

impl Krosshair {
    /// Checks now and then every [`CHECK_EVERY`], while the setting is on. Call once.
    pub(super) fn start_update_checks(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_CHECK).await;
            loop {
                let alive = this.update(cx, |this, cx| {
                    if this.cfg.check_updates {
                        this.check_for_update(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
                cx.background_executor().timer(CHECK_EVERY).await;
            }
        })
        .detach();
    }

    /// Asks GitHub for the newest release, unless a check or an update is under way or one was
    /// found already.
    pub(super) fn check_for_update(&mut self, cx: &mut Context<Self>) {
        if matches!(self.update, UpdateState::Checking | UpdateState::Installing(_) | UpdateState::Available(_)) {
            return;
        }
        self.update = UpdateState::Checking;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let found = cx.background_spawn(async { update::check() }).await;
            let _ = this.update(cx, |this, cx| {
                // Switched off while asking: forget the answer.
                if !this.cfg.check_updates {
                    this.update = UpdateState::Idle;
                } else {
                    this.update = match found {
                        Ok(Some(release)) => UpdateState::Available(release),
                        Ok(None) => UpdateState::Latest,
                        Err(e) => UpdateState::CheckFailed(format!("{e:#}").into()),
                    };
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Downloads the new version, puts it in place and restarts into it.
    fn install_update(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Available(release) = self.update.clone() else { return };
        self.update = UpdateState::Installing(release.version.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let installed = cx.background_spawn({
                let release = release.clone();
                async move {
                    let exe = update::install(&release)?;
                    update::start_new(&exe)
                }
            })
            .await;
            let _ = this.update(cx, |this, cx| match installed {
                // The new copy waits for this one to quit.
                Ok(()) => crate::tray::quit(),
                Err(e) => {
                    this.update = UpdateState::InstallFailed(format!("{e:#}").into(), release.page.clone());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Under the "Check for updates" setting: what the last check found.
    pub(super) fn update_about(&self) -> String {
        if !self.cfg.check_updates {
            return "Looks for new versions on GitHub.".into();
        }
        match &self.update {
            UpdateState::Idle => "Looks for new versions on GitHub.".into(),
            UpdateState::Checking => "Checking…".into(),
            UpdateState::Latest => format!("Up to date: {} is the newest.", update::current()),
            UpdateState::CheckFailed(e) => format!("Couldn't check: {e}"),
            UpdateState::Available(r) => format!("Version {} is out; update from the header.", r.version),
            UpdateState::Installing(v) => format!("Updating to {v}…"),
            UpdateState::InstallFailed(e, _) => format!("The update failed: {e}"),
        }
    }

    /// In the header: a new version with its Update button, the update under way, or why it
    /// failed. Nothing otherwise.
    pub(super) fn update_banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let link = |id: &'static str, label: &'static str, url: String, cx: &mut Context<Self>| {
            div()
                .id(id)
                .flex_none()
                .cursor_pointer()
                .font_family(mono())
                .text_size(px(11.))
                .fx(self, cx, |d, t| d.text_color(mix(ACCENT, TEXT, t)))
                .tooltip(tip(url.clone()))
                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
                .child(label)
        };
        let row = || div().flex().flex_none().items_center().gap(px(12.)).pr(px(8.)).text_size(px(13.));
        match &self.update {
            UpdateState::Available(release) => Some(
                row()
                    .child(div().text_color(c(MUTED)).child(format!("krosshair {} is out", release.version)))
                    .child(link("update-notes", "what's new", release.page.clone(), cx))
                    .child(
                        div()
                            .id("update-install")
                            .h(px(28.))
                            .px(px(14.))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .cursor_pointer()
                            .text_color(c(BG))
                            .font_weight(FontWeight::MEDIUM)
                            .fx(self, cx, |d, t| d.bg(mix(TEXT, 0xffffff, t)))
                            .on_click(cx.listener(|this, _, _, cx| this.install_update(cx)))
                            .child("Update"),
                    )
                    .into_any_element(),
            ),
            UpdateState::Installing(version) => {
                Some(row().child(div().text_color(c(MUTED)).child(format!("Updating to {version}…"))).into_any_element())
            }
            UpdateState::InstallFailed(e, page) => Some(
                row()
                    .child(div().max_w(px(360.)).truncate().text_color(c(DANGER)).child(format!("Update failed: {e}")))
                    .child(link("update-page", "download", page.clone(), cx))
                    .into_any_element(),
            ),
            _ => None,
        }
    }
}
