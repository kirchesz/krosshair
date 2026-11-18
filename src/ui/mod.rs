//! The window: one view that owns all state, a navigation rail, a header and the screens.

mod about;
mod editor;
mod gallery;
mod layer_edit;
mod layers;
mod settings;
pub mod theme;
mod tray_question;
mod updates;
mod widgets;

use crate::model::{
    clean_rotation, clean_scale, rotation_is_sharp, scale_is_sharp, scale_label, Config, EditLayer, LayerRef, Profile, Shape,
    MAX_OUTLINE, SCALE_MAX, SCALE_MIN, SCALE_STEPS_PER_UNIT,
};
use crate::overlay::{self, Overlay, Status};
use crate::raster::Bitmap;
use crate::store::{self, GalleryItem};
use crate::dialogs;
use gpui::{
    canvas, deferred, div, fill, point, prelude::*, px, quad, size, transparent_black, BorderStyle, Bounds, Context,
    CursorStyle, DispatchPhase, Div, ElementId, FocusHandle, FontWeight, HitboxBehavior, Hsla, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, SharedString, Task, Window,
    WindowControlArea,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use theme::*;
use widgets::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Layers,
    LayerEdit,
    Gallery,
    Editor,
    Settings,
    About,
}

/// The layer being added or changed on the "edit layer" screen.
pub struct LayerDraft {
    /// Position in the profile, `None` for a new layer.
    pub index: Option<usize>,
    pub file: Option<String>,
    pub anchor: u8,
    pub ox: i32,
    pub oy: i32,
    pub on: bool,
    /// Degrees clockwise around the layer's own middle.
    pub rotation: i32,
}

/// The crosshair being drawn in the editor.
pub struct EditorState {
    pub layers: Vec<EditLayer>,
    pub sel: usize,
    pub picker: bool,
    pub backdrop: usize,
    /// While the mouse paints on the custom grid: the value it paints.
    pub paint: Option<bool>,
}

impl EditorState {
    fn new() -> Self {
        Self {
            layers: vec![EditLayer::new(Shape::Cross), EditLayer::new(Shape::Dot)],
            sel: 0,
            picker: false,
            backdrop: 0,
            paint: None,
        }
    }
}

pub struct Krosshair {
    pub cfg: Config,
    pub gallery: Vec<GalleryItem>,
    pub overlay: Arc<Overlay>,
    pub status: Status,
    pub screen: Screen,
    pub draft: LayerDraft,
    pub ed: EditorState,
    pub profile_menu: bool,
    /// The settings dropdown whose list is open, by its id.
    pub open_select: Option<&'static str>,
    /// The connected displays, read again whenever the settings open.
    pub displays: Vec<crate::displays::Display>,
    /// While a knob is being dragged: which one and where it is drawn, where the drag started and
    /// its value then.
    pub drag: Option<((Knob, Place), f32, f32)>,
    /// Profile being renamed and the text typed so far.
    pub rename: Option<(usize, String)>,
    pub slider_drag: Option<&'static str>,
    pub focus: FocusHandle,
    /// Last thing that went wrong, shown in the header until the next action.
    pub error: Option<SharedString>,
    /// What the last action did (files added to the gallery), shown in the header until the next.
    pub info: Option<SharedString>,
    /// Gallery files added by the last drop, highlighted until the screen changes.
    pub fresh: HashSet<String>,
    /// What Windows runs at sign-in, when starting with Windows is on (see [`crate::autorun`]).
    pub autorun: Option<String>,
    /// A minimize or close held until the tray choice is made; the question is on screen.
    pub held: Option<crate::tray::Held>,
    /// The question takes the keyboard, for Escape.
    pub ask_focus: FocusHandle,
    /// The last update check, or the update under way.
    pub update: updates::UpdateState,
    /// Hover transitions by element, see [`widgets::Fx`].
    hovers: HashMap<ElementId, Hover>,
    /// Elements that asked for their hover amount during the last render. A transition whose
    /// element is no longer drawn is dropped, so it can't come back half finished.
    drawn: RefCell<HashSet<ElementId>>,
    /// Switches that were flipped a moment ago, for sliding their knobs.
    flips: HashMap<ElementId, Instant>,
    /// Animated previews all play from this moment.
    anim_epoch: Instant,
    /// Milliseconds until the soonest frame change among the previews drawn in this render.
    anim_next: Cell<Option<u32>>,
    /// Redraws the window when that frame change comes.
    anim_wake: Option<Task<()>>,
    /// What the overlay had to leave out (a GIF too big to animate), shown in the header.
    note: RefCell<Option<SharedString>>,
    /// Turned previews of layers, by picture and angle. Emptied when the gallery is read again.
    turned: RefCell<HashMap<(usize, i32), Arc<Bitmap>>>,
}

/// One element's hover transition: where it is heading, since when, and the amount it started at.
struct Hover {
    on: bool,
    since: Instant,
    from: f32,
}

impl Hover {
    fn amount(&self) -> f32 {
        let target = if self.on { 1. } else { 0. };
        self.from + (target - self.from) * progress(self.since)
    }
}

impl Krosshair {
    /// Whether the pointer is on the element (one tracked with [`widgets::Fx`]).
    pub fn is_hovered(&self, id: &ElementId) -> bool {
        self.hovers.get(id).is_some_and(|h| h.on)
    }

    /// How far the element is into its hover state: 0 at rest, 1 hovered.
    pub fn hover_amount(&self, id: &ElementId) -> f32 {
        self.drawn.borrow_mut().insert(id.clone());
        self.hovers.get(id).map_or(0., Hover::amount)
    }

    pub fn set_hover(&mut self, id: ElementId, on: bool, cx: &mut Context<Self>) {
        let from = self.hovers.get(&id).map_or(0., Hover::amount);
        self.hovers.insert(id, Hover { on, since: Instant::now(), from });
        cx.notify();
    }

    /// Call when a switch changes, so its knob slides instead of jumping.
    pub fn flip(&mut self, id: impl Into<ElementId>) {
        self.flips.insert(id.into(), Instant::now());
    }

    /// How far a switch is into its current state: 1 when it has settled.
    pub fn flip_amount(&self, id: impl Into<ElementId>) -> f32 {
        self.flips.get(&id.into()).map_or(1., |since| progress(*since))
    }

    /// Housekeeping at the start of a render. Returns true while a transition is running.
    fn tick_transitions(&mut self) -> bool {
        let drawn = std::mem::take(&mut *self.drawn.borrow_mut());
        self.hovers.retain(|id, h| drawn.contains(id) && (h.on || h.since.elapsed() < ANIM));
        self.flips.retain(|_, since| since.elapsed() < ANIM);
        !self.flips.is_empty() || self.hovers.values().any(|h| h.since.elapsed() < ANIM)
    }

    pub fn new(overlay: Arc<Overlay>, cx: &mut Context<Self>) -> Self {
        let cfg = store::load();
        crate::tray::set_mode(cfg.tray);
        crate::tray::set_ask(!cfg.tray_asked);
        let gallery = store::scan_gallery(&cfg.gallery_dir);
        let this = Self {
            cfg,
            gallery,
            overlay,
            status: Status::Off,
            screen: Screen::Layers,
            draft: LayerDraft { index: None, file: None, anchor: 4, ox: 0, oy: 0, on: true, rotation: 0 },
            ed: EditorState::new(),
            profile_menu: false,
            open_select: None,
            displays: crate::displays::list(),
            drag: None,
            rename: None,
            slider_drag: None,
            focus: cx.focus_handle(),
            error: None,
            info: None,
            fresh: HashSet::new(),
            autorun: crate::autorun::command(),
            held: None,
            ask_focus: cx.focus_handle(),
            update: updates::UpdateState::Idle,
            hovers: HashMap::new(),
            drawn: RefCell::new(HashSet::new()),
            flips: HashMap::new(),
            anim_epoch: Instant::now(),
            anim_next: Cell::new(None),
            anim_wake: None,
            note: RefCell::new(None),
            turned: RefCell::new(HashMap::new()),
        };
        this.sync_overlay();

        // The first minimize or close, held by the window until the tray choice is made: ask.
        let held = crate::tray::held();
        cx.spawn(async move |this, cx| {
            while let Ok(what) = held.recv().await {
                let alive = this.update(cx, |this, cx| {
                    this.held = Some(what);
                    this.open_select = None;
                    this.profile_menu = false;
                    this.rename = None;
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();

        Self::start_update_checks(cx);

        // The overlay thread reports whether RTSS took the crosshair; show changes of that.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                let alive = this.update(cx, |this, cx| {
                    let status = this.overlay.status();
                    if status != this.status {
                        this.status = status;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        this
    }

    /// The frame of a gallery picture to show right now: the picture itself, or the current frame
    /// of its animation.
    pub fn frame_of(&self, item: &GalleryItem) -> Arc<Bitmap> {
        let Some(anim) = &item.anim else { return item.bmp.clone() };
        let (i, left) = anim.at(self.anim_epoch.elapsed().as_millis() as u64);
        self.anim_next.set(Some(self.anim_next.get().map_or(left, |n| n.min(left))));
        anim.frames[i].clone()
    }

    pub fn item(&self, file: &str) -> Option<&GalleryItem> {
        self.gallery.iter().find(|i| i.file == file)
    }

    /// The layers to show right now: the profile, with the layer being edited swapped in so its
    /// position can be checked in game before saving.
    fn shown_layers(&self) -> Vec<LayerRef> {
        let mut layers = self.cfg.profile().layers.clone();
        if self.screen == Screen::LayerEdit
            && let Some(file) = &self.draft.file
        {
            let d = &self.draft;
            let layer = LayerRef { file: file.clone(), anchor: d.anchor, ox: d.ox, oy: d.oy, on: d.on, rotation: d.rotation };
            match d.index {
                Some(i) if i < layers.len() => layers[i] = layer,
                _ => layers.push(layer),
            }
        }
        layers
    }

    pub fn sync_overlay(&self) {
        let composed = if self.cfg.power {
            overlay::compose(&self.shown_layers(), |file| self.item(file).map(GalleryItem::picture), self.cfg.mask, self.cfg.scale, self.cfg.rotation)
        } else {
            overlay::Composed::default()
        };
        *self.note.borrow_mut() = composed.note.map(Into::into);
        self.overlay.set(composed.frame);
    }

    /// After a change to the configuration: save it, update the overlay, redraw.
    pub fn commit(&mut self, cx: &mut Context<Self>) {
        self.error = store::save(&self.cfg).err().map(|e| e.to_string().into());
        self.sync_overlay();
        cx.notify();
    }

    pub fn rescan(&mut self) {
        self.gallery = store::scan_gallery(&self.cfg.gallery_dir);
        // The cache is keyed by picture address; new pictures may reuse old addresses.
        self.turned.borrow_mut().clear();
    }

    /// A picture turned `rotation` degrees clockwise around its middle, for previews.
    pub fn turned(&self, bmp: &Arc<Bitmap>, rotation: i32) -> Arc<Bitmap> {
        let key = (Arc::as_ptr(bmp) as usize, rotation.rem_euclid(360));
        if let Some(hit) = self.turned.borrow().get(&key) {
            return hit.clone();
        }
        let middle = (bmp.w as f32 / 2., bmp.h as f32 / 2.);
        let out = Arc::new(bmp.rotated((0., 0.), middle, key.1).0);
        let mut cache = self.turned.borrow_mut();
        // Dragging the angle makes a new entry per degree: keep it from growing without end.
        if cache.len() > 512 {
            cache.clear();
        }
        cache.insert(key, out.clone());
        out
    }

    pub fn go(&mut self, screen: Screen, cx: &mut Context<Self>) {
        self.screen = screen;
        self.profile_menu = false;
        self.open_select = None;
        if screen == Screen::Settings {
            self.displays = crate::displays::list();
        }
        self.rename = None;
        self.ed.picker = false;
        self.error = None;
        self.info = None;
        self.fresh.clear();
        if screen == Screen::Gallery || screen == Screen::LayerEdit {
            // Pick up files added to the folder by hand.
            self.rescan();
        }
        self.sync_overlay();
        cx.notify();
    }

    /// Opens a screen by name: "gallery", "editor", "draw" (the editor with a custom layer),
    /// "layer" (a new layer); anything else is the main screen.
    pub fn open_screen(&mut self, name: &str, cx: &mut Context<Self>) {
        match name {
            "gallery" => self.go(Screen::Gallery, cx),
            "editor" => self.go(Screen::Editor, cx),
            "settings" => self.go(Screen::Settings, cx),
            "about" => self.go(Screen::About, cx),
            "draw" => {
                self.ed.layers.push(EditLayer::new(Shape::Custom));
                self.ed.sel = self.ed.layers.len() - 1;
                self.go(Screen::Editor, cx)
            }
            "layer" => self.open_layer(None, cx),
            _ => self.go(Screen::Layers, cx),
        }
    }

    fn pick_mask(&mut self, cx: &mut Context<Self>) {
        let initial = self.cfg.mask;
        cx.spawn(async move |this, cx| {
            let picked = cx.background_spawn(async move { dialogs::choose_color(initial) }).await;
            if let Some(color) = picked {
                let _ = this.update(cx, |this, cx| {
                    this.cfg.mask = color;
                    this.commit(cx);
                });
            }
        })
        .detach();
    }

    fn on_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some((index, text)) = &mut self.rename else { return };
        let key = &e.keystroke;
        match key.key.as_str() {
            "enter" => {
                let name = text.trim().to_owned();
                let index = *index;
                self.rename = None;
                if !name.is_empty() && index < self.cfg.profiles.len() {
                    self.cfg.profiles[index].name = name;
                    self.commit(cx);
                }
            }
            "escape" => self.rename = None,
            "backspace" => {
                text.pop();
            }
            "space" => text.push(' '),
            _ => {
                if let Some(typed) = &key.key_char
                    && !key.modifiers.control
                    && !key.modifiers.alt
                    && text.chars().count() < 28
                {
                    text.push_str(typed);
                }
            }
        }
        cx.notify();
    }

    // ---- navigation rail ----

    fn nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let nav_button = |id: &'static str, icon_name: &'static str, label: &'static str, target: Screen, cx: &mut Context<Self>| {
            let active = self.screen == target;
            let colors = if active { (ACCENT, ACCENT) } else { (MUTED, TEXT) };
            icon_button(self, cx, id, icon_name, 20., colors, (active.then_some(RAISED), RAISED))
                .size(px(44.))
                .rounded(px(8.))
                .tooltip(tip(label))
                .on_click(cx.listener(move |this, _, _, cx| this.go(target, cx)))
        };

        let top: Div = if self.screen == Screen::LayerEdit {
            div().flex().flex_col().items_center().child(
                icon_button(self, cx, "nav-back", "back", 20., (MUTED, TEXT), (None, RAISED))
                    .size(px(48.))
                    .rounded(px(8.))
                    .tooltip(tip("Back to Layers"))
                    .on_click(cx.listener(|this, _, _, cx| this.go(Screen::Layers, cx))),
            )
        } else {
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .size(px(48.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .window_control_area(WindowControlArea::Drag)
                        .child(icon("logo", 26., c(TEXT))),
                )
                // A hairline between the logo and the screens across the whole rail, level with
                // the line under the header so the two read as one.
                .child(div().w_full().h(px(1.)).my(px(4.)).bg(border()))
                .child(nav_button("nav-layers", "layers", "Layers", Screen::Layers, cx))
                .child(nav_button("nav-gallery", "gallery", "Gallery", Screen::Gallery, cx))
                .child(nav_button("nav-editor", "editor", "Editor", Screen::Editor, cx))
                .child(nav_button("nav-settings", "settings", "Settings", Screen::Settings, cx))
                .child(nav_button("nav-about", "info", "About", Screen::About, cx))
        };

        let mask = self.cfg.mask;
        let power_tip = match (self.cfg.power, self.status) {
            (false, _) => "Overlay off",
            (true, Status::NoRtss) => "Overlay on · RTSS is not running",
            (true, _) => "Overlay on",
        };

        div()
            .w(px(72.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(12.))
            .pb(px(16.))
            .border_r_1()
            .border_color(border())
            .child(top)
            // The crosshair's own settings at the bottom; not while a layer is being edited, where
            // the rail only leads back.
            .when(self.screen != Screen::LayerEdit, |rail| rail.child(
                div()
                    .mt_auto()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("mask")
                            .size(px(44.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .fx(self, cx, |d, t| d.bg(fade(RAISED, t)))
                            .tooltip(tip(format!("Color mask · #{mask:06X}")))
                            .on_click(cx.listener(|this, _, _, cx| this.pick_mask(cx)))
                            .child(div().size(px(20.)).rounded_full().bg(c(mask)).border_1().border_color(alpha(TEXT, 0x59))),
                    )
                    .child(self.knob_control(Knob::Scale, cx))
                    .child(self.knob_control(Knob::Rotation, cx))
                    .child(
                        div()
                            .id("power")
                            .size(px(44.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .tooltip(tip(power_tip))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cfg.power = !this.cfg.power;
                                this.flip("power");
                                this.commit(cx);
                            }))
                            .child(switch(self.cfg.power, RAISED, 32., 18., self.flip_amount("power"))),
                    ),
            ))
    }

    /// The dragging part of a knob: an overlay on its element that paints the tape in `area` and
    /// turns drags into value changes; double-click resets.
    fn knob_input(&self, knob: Knob, place: Place, area: TapeArea, cx: &Context<Self>) -> impl IntoElement {
        let value = knob.get(self);
        let drag = self.drag.filter(|d| d.0 == (knob, place));
        // How lit the tape is: by the hover transition, fully while dragging.
        let lit = if drag.is_some() { 1. } else { self.hover_amount(&knob.element_id(place)) };
        let view = cx.weak_entity();
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                paint_tape(bounds, area, knob, value, lit, window);
                window.set_cursor_style(CursorStyle::ResizeLeftRight, &hitbox);
                window.on_mouse_event({
                    let view = view.clone();
                    move |e: &MouseDownEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble || e.button != MouseButton::Left || !hitbox.is_hovered(window) {
                            return;
                        }
                        let _ = view.update(cx, |this, cx| {
                            if e.click_count == 2 {
                                this.drag = None;
                                knob.set(this, knob.reset());
                                this.knob_changed(knob, true, cx);
                            } else {
                                this.drag = Some(((knob, place), f32::from(e.position.x), knob.get(this)));
                                cx.notify();
                            }
                        });
                    }
                });
                let Some((_, x0, from)) = drag else { return };
                window.on_mouse_event({
                    let view = view.clone();
                    move |e: &MouseMoveEvent, phase, _, cx| {
                        // Only moves with the button held count. The drag ends on release, which
                        // arrives even outside the window because the window holds the mouse
                        // capture while a button is down.
                        if phase != DispatchPhase::Bubble || e.pressed_button != Some(MouseButton::Left) {
                            return;
                        }
                        let _ = view.update(cx, |this, cx| {
                            let value = knob.dragged(from, f32::from(e.position.x) - x0);
                            if value != knob.get(this) {
                                knob.set(this, value);
                                this.knob_changed(knob, false, cx);
                            }
                        });
                    }
                });
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        let _ = view.update(cx, |this, cx| {
                            this.drag = None;
                            this.knob_changed(knob, true, cx);
                        });
                    }
                });
            },
        )
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .size_full()
    }

    /// After a knob changed: the game shows it right away. Settings of the crosshair are written
    /// to the file once the drag ends (`done`); a layer being edited or an editor shape are saved
    /// by their own buttons.
    fn knob_changed(&mut self, knob: Knob, done: bool, cx: &mut Context<Self>) {
        if done && knob.saved() {
            self.commit(cx);
        } else {
            self.sync_overlay();
            cx.notify();
        }
    }

    /// A knob in the navigation rail: the value above a tape, dragged sideways. While the pointer
    /// is on it, or it is being dragged, a hint beside it shows the value, whether the crosshair
    /// stays sharp or gets blended, and where the value is on its range.
    fn knob_control(&self, knob: Knob, cx: &mut Context<Self>) -> impl IntoElement {
        let value = knob.get(self);
        let dragging = self.drag.is_some_and(|d| d.0 == (knob, Place::Rail));
        let active = dragging || self.is_hovered(&knob.element_id(Place::Rail));
        let button = div()
            .id(knob.element_id(Place::Rail))
            .relative()
            .w(px(KNOB_W))
            .h(px(44.))
            .rounded(px(8.))
            .flex()
            .flex_col()
            .items_center()
            .pt(px(4.))
            .font_family(mono())
            .text_size(px(11.))
            .line_height(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .fx(self, cx, |d, t| {
                let t = if dragging { 1. } else { t };
                d.bg(fade(RAISED, t)).text_color(mix(MUTED, TEXT, t))
            })
            .child(knob.label(value))
            .child(self.knob_input(knob, Place::Rail, TapeArea { inset: 5., top: 21., height: 17. }, cx));
        div().relative().child(button).when(active, |d| d.child(deferred(knob_hint(knob, value, KNOB_W)).with_priority(1)))
    }

    /// A knob as a field: minus and plus buttons around the value and its tape, which can be
    /// dragged too. Shift on the buttons takes bigger steps.
    pub fn knob_field(&self, knob: Knob, cx: &mut Context<Self>) -> impl IntoElement {
        let value = knob.get(self);
        let dragging = self.drag.is_some_and(|d| d.0 == (knob, Place::Field));
        let active = dragging || self.is_hovered(&knob.element_id(Place::Field));
        let step =|sign: f32, icon_name: &'static str, cx: &mut Context<Self>| {
            let id = ElementId::Name(format!("{}-{}", knob.id(), if sign < 0. { "dec" } else { "inc" }).into());
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .fx(self, cx, |d, t| d.bg(fade(BG, t * FIELD_HOVER)).child(icon(icon_name, 12., mix(MUTED, TEXT, t))))
                .size(px(40.))
                .rounded(px(6.))
                .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                    let steps = if e.modifiers().shift { knob.big_step() } else { 1. };
                    knob.set(this, knob.get(this) + sign * steps * knob.step());
                    this.knob_changed(knob, true, cx);
                }))
        };
        let middle = div()
            .id(knob.element_id(Place::Field))
            .relative()
            .w(px(FIELD_MID_W))
            .h(px(40.))
            .rounded(px(6.))
            .flex()
            .flex_col()
            .items_center()
            .pt(px(3.))
            .font_family(mono())
            .text_size(px(13.))
            .line_height(px(16.))
            .font_weight(FontWeight::MEDIUM)
            .fx(self, cx, |d, t| d.bg(fade(BG, if dragging { 1. } else { t } * FIELD_HOVER)))
            .child(knob.label(value))
            .child(self.knob_input(knob, Place::Field, TapeArea { inset: 4., top: 22., height: 15. }, cx));
        let field = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(2.))
            .rounded(px(8.))
            .bg(c(RAISED))
            .child(step(-1., "minus", cx))
            .child(middle)
            .child(step(1., "plus", cx));
        div().relative().child(field).when(active, |d| d.child(deferred(knob_hint(knob, value, FIELD_W)).with_priority(1)))
    }
    // ---- header ----

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The title, centered in the header; with a small line above it the two are centered
        // together.
        let titled = |eyebrow: String, title: &'static str| {
            div()
                .h(px(48.))
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(2.))
                .when(!eyebrow.is_empty(), |d| {
                    d.child(div().font_family(mono()).text_size(px(10.5)).line_height(px(14.)).text_color(c(MUTED)).child(eyebrow))
                })
                .child(div().text_size(px(20.)).font_weight(FontWeight::MEDIUM).line_height(px(22.)).child(title))
        };
        let left = match self.screen {
            Screen::Layers => self.profile_button(cx).into_any_element(),
            Screen::LayerEdit => titled(
                format!("layers · {}", self.cfg.profile().name.to_lowercase()),
                if self.draft.index.is_some() { "Edit layer" } else { "New layer" },
            )
            .into_any_element(),
            // The title is the screen's name, as in the navigation rail; the line above it, where
            // there is one, says something about the screen, never the name again.
            Screen::Gallery => titled(String::new(), "Gallery").into_any_element(),
            Screen::Editor => titled(String::new(), "Editor").into_any_element(),
            Screen::Settings => titled(String::new(), "Settings").into_any_element(),
            Screen::About => titled(String::new(), "About").into_any_element(),
        };

        let window_button = |id: &'static str, name: &'static str, area: WindowControlArea, cx: &mut Context<Self>| {
            let close = area == WindowControlArea::Close;
            let (hover_icon, hover_bg) = if close { (BG, DANGER) } else { (TEXT, RAISED) };
            icon_button(self, cx, id, name, 14., (MUTED, hover_icon), (None, hover_bg))
                .w(px(48.))
                .h(px(40.))
                .window_control_area(area)
        };

        div()
            .flex()
            .flex_none()
            .items_start()
            .border_b_1()
            .border_color(border())
            .child(div().pl(px(24.)).py(px(12.)).child(left))
            .child(
                div()
                    .flex_1()
                    .h(px(72.))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .window_control_area(WindowControlArea::Drag)
                    .map(|d| match (self.error.clone().or_else(|| self.note.borrow().clone()), self.info.clone()) {
                        (Some(e), _) => d.child(div().text_size(px(13.)).text_color(c(DANGER)).truncate().child(e)),
                        (None, Some(i)) => d.child(div().text_size(px(13.)).text_color(c(MUTED)).truncate().child(i)),
                        _ => d,
                    }),
            )
            // Outside the drag area, so its button and link take clicks.
            .children(self.update_banner(cx).map(|b| div().h(px(72.)).flex().items_center().child(b)))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .child(window_button("win-min", "minimize", WindowControlArea::Min, cx))
                    .child(window_button("win-max", "maximize", WindowControlArea::Max, cx))
                    .child(window_button("win-close", "close", WindowControlArea::Close, cx)),
            )
    }

    fn profile_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .child(
                div()
                    .id("profile")
                    .h(px(48.))
                    .min_w(px(240.))
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .rounded(px(8.))
                    .fx(self, cx, |d, t| d.bg(mix(RAISED, HOVER, t)))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.profile_menu = !this.profile_menu;
                        this.rename = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            // The browser's "normal" line height of the design, not gpui's 1.618.
                            .child(div().font_family(mono()).text_size(px(10.5)).line_height(px(14.)).text_color(c(MUTED)).child("current profile"))
                            .child(
                                div()
                                    .text_size(px(14.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .line_height(px(16.))
                                    .child(self.cfg.profile().name.clone()),
                            ),
                    )
                    .child(icon("chevron", 12., c(MUTED))),
            )
            .when(self.profile_menu, |d| d.child(deferred(self.profile_menu(cx)).with_priority(1)))
    }

    fn profile_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let many = self.cfg.profiles.len() > 1;
        let rows = self.cfg.profiles.iter().enumerate().map(|(i, profile)| {
            let current = i == self.cfg.current;
            let renaming = self.rename.as_ref().filter(|r| r.0 == i).map(|r| r.1.clone());
            let row = div().id(("profile-row", i)).h(px(40.)).px(px(8.)).flex().items_center().gap(px(4.)).rounded(px(8.));
            if let Some(text) = renaming {
                return row
                    .bg(c(SURFACE))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .px(px(6.))
                            .text_size(px(14.5))
                            .child(text)
                            .child(div().w(px(1.)).h(px(16.)).ml(px(1.)).bg(c(ACCENT))),
                    )
                    .child(
                        icon_button(self, cx, ("profile-ok", i), "check", 14., (MUTED, TEXT), (None, BG))
                            .size(px(32.))
                            .rounded_full()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some((index, text)) = this.rename.take() {
                                    let name = text.trim().to_owned();
                                    if !name.is_empty() && index < this.cfg.profiles.len() {
                                        this.cfg.profiles[index].name = name;
                                    }
                                }
                                this.commit(cx);
                            })),
                    );
            }
            row.fx(self, cx, |d, t| d.bg(fade(SURFACE, t)))
                .child(
                    div()
                        .id(("profile-pick", i))
                        .flex_1()
                        .h_full()
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_size(px(14.5))
                        .when(current, |d| d.text_color(c(ACCENT)))
                        .child(profile.name.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cfg.current = i;
                            this.profile_menu = false;
                            this.commit(cx);
                        })),
                )
                .child(
                    icon_button(self, cx, ("profile-rename", i), "pencil", 14., (MUTED, TEXT), (None, BG))
                        .size(px(32.))
                        .rounded_full()
                        .tooltip(tip("Rename"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.rename = Some((i, this.cfg.profiles[i].name.clone()));
                            window.focus(&this.focus);
                            cx.notify();
                        })),
                )
                .when(many, |d| {
                    d.child(
                        icon_button(self, cx, ("profile-delete", i), "trash", 14., (MUTED, DANGER), (None, BG))
                            .size(px(32.))
                            .rounded_full()
                            .tooltip(tip("Delete profile"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.cfg.profiles.len() > 1 && i < this.cfg.profiles.len() {
                                    this.cfg.profiles.remove(i);
                                    if this.cfg.current > i || this.cfg.current >= this.cfg.profiles.len() {
                                        this.cfg.current = this.cfg.current.saturating_sub(1);
                                    }
                                    this.commit(cx);
                                }
                            })),
                    )
                })
        });

        div()
            .id("profile-menu")
            .absolute()
            .top(px(56.))
            .left(px(0.))
            .w(px(280.))
            .p(px(6.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .rounded(px(12.))
            .bg(c(RAISED))
            .shadow_lg()
            .occlude()
            // Typing a profile name. The focus must not sit on an element that covers the header:
            // a focusable element takes every mouse press, and the title bar would stop working.
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.profile_menu = false;
                this.rename = None;
                cx.notify();
            }))
            .children(rows)
            .child(
                div()
                    .id("profile-new")
                    .h(px(40.))
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_size(px(14.5))
                    .text_color(c(MUTED))
                    .fx(self, cx, |d, t| d.bg(fade(SURFACE, t)))
                    .child(icon("plus", 14., c(MUTED)))
                    .child("New profile")
                    .on_click(cx.listener(|this, _, window, cx| {
                        let name = (1..)
                            .map(|n| format!("Profile {n}"))
                            .find(|name| this.cfg.profiles.iter().all(|p| &p.name != name))
                            .unwrap();
                        this.cfg.profiles.push(Profile { name: name.clone(), layers: Vec::new() });
                        this.cfg.current = this.cfg.profiles.len() - 1;
                        this.rename = Some((this.cfg.current, name));
                        window.focus(&this.focus);
                        this.commit(cx);
                    })),
            )
    }
}

impl Render for Krosshair {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.tick_transitions() {
            window.request_animation_frame();
        }
        self.anim_next.set(None);
        let screen = match self.screen {
            Screen::Layers => self.layers_screen(cx).into_any_element(),
            Screen::LayerEdit => self.layer_edit_screen(cx).into_any_element(),
            Screen::Gallery => self.gallery_screen(cx).into_any_element(),
            Screen::Editor => self.editor_screen(cx).into_any_element(),
            Screen::Settings => self.settings_screen(cx).into_any_element(),
            Screen::About => self.about_screen(cx).into_any_element(),
        };
        // Animated previews were drawn: redraw when the next of their frames is due. Replacing
        // the task cancels the one from the previous render.
        self.anim_wake = self.anim_next.get().map(|ms| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(ms as u64 + 1)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
        });
        div()
            .id("root")
            .relative()
            .size_full()
            .flex()
            .bg(c(BG))
            .text_color(c(TEXT))
            .font_family(sans())
            .text_size(px(14.5))
            // A slider or the pixel grid lost the button outside the window: stop dragging.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.drag.take().is_some() {
                        this.commit(cx);
                    }
                    if this.slider_drag.is_some() || this.ed.paint.is_some() {
                        this.slider_drag = None;
                        this.ed.paint = None;
                        cx.notify();
                    }
                }),
            )
            .child(self.nav(cx))
            .child(div().flex_1().min_w_0().h_full().flex().flex_col().child(self.header(cx)).child(screen))
            .when_some(self.held, |d, held| {
                if !self.ask_focus.is_focused(window) {
                    window.focus(&self.ask_focus);
                }
                d.child(self.tray_question(held, cx))
            })
    }
}

/// A setting changed by dragging a tape: in the navigation rail, or as a field in a side panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Knob {
    Scale,
    Rotation,
    /// The rotation of the layer being edited.
    LayerRotation,
    /// The rotation of the selected shape in the editor.
    EditorRotation,
    /// A size of the selected shape in the editor.
    Size(Dim),
    /// How far the layer being edited sits from its anchor, sideways and down.
    OffsetX,
    OffsetY,
}

/// Where a knob is drawn. Scale and rotation are in the rail and on the settings screen at once,
/// so hovering or dragging one of them must not light up the other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    /// A button of its own in the navigation rail.
    Rail,
    /// Between minus and plus buttons, in a side panel or a settings row.
    Field,
}

/// The farthest a layer may sit from its anchor, either way.
const MAX_OFFSET: i32 = 4000;

/// An offset with its sign: "+12 px", "−3 px", "0 px".
fn signed_px(v: i32) -> String {
    match v {
        0 => "0 px".into(),
        v if v > 0 => format!("+{v} px"),
        v => format!("\u{2212}{} px", -v),
    }
}

/// A size of an editor shape, in whole screen pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    Length,
    Thickness,
    Gap,
    Radius,
    Dot,
    Outline,
}

impl Dim {
    fn id(self) -> &'static str {
        match self {
            Dim::Length => "ed-len",
            Dim::Thickness => "ed-thick",
            Dim::Gap => "ed-gap",
            Dim::Radius => "ed-radius",
            Dim::Dot => "ed-dot",
            Dim::Outline => "ed-outline",
        }
    }

    fn range(self) -> (i32, i32) {
        match self {
            Dim::Length => (1, 20),
            Dim::Thickness => (1, 6),
            Dim::Gap => (0, 10),
            Dim::Radius => (2, 20),
            Dim::Dot => (1, 8),
            Dim::Outline => (1, MAX_OUTLINE),
        }
    }

    fn of(self, l: &mut EditLayer) -> &mut i32 {
        match self {
            Dim::Length => &mut l.len,
            Dim::Thickness => &mut l.thickness,
            Dim::Gap => &mut l.gap,
            Dim::Radius => &mut l.radius,
            Dim::Dot => &mut l.dot,
            Dim::Outline => &mut l.outline_width,
        }
    }

    fn value(self, l: &EditLayer) -> i32 {
        match self {
            Dim::Length => l.len,
            Dim::Thickness => l.thickness,
            Dim::Gap => l.gap,
            Dim::Radius => l.radius,
            Dim::Dot => l.dot,
            Dim::Outline => l.outline_width,
        }
    }

    /// What a new shape starts with.
    fn default_value(self) -> i32 {
        self.value(&EditLayer::new(Shape::Cross))
    }

    fn clean(self, value: f32) -> i32 {
        let (lo, hi) = self.range();
        (value.round() as i32).clamp(lo, hi)
    }
}

impl Knob {
    fn id(self) -> &'static str {
        match self {
            Knob::Scale => "scale",
            Knob::Rotation => "rotation",
            Knob::LayerRotation => "layer-rotation",
            Knob::EditorRotation => "editor-rotation",
            Knob::Size(dim) => dim.id(),
            Knob::OffsetX => "offset-x",
            Knob::OffsetY => "offset-y",
        }
    }

    /// The id of the knob's element where it is drawn, for its hover state.
    fn element_id(self, place: Place) -> ElementId {
        match place {
            Place::Rail => self.id().into(),
            Place::Field => ElementId::Name(format!("{}-field", self.id()).into()),
        }
    }

    /// Settings of the whole crosshair, written to the configuration file.
    fn saved(self) -> bool {
        matches!(self, Knob::Scale | Knob::Rotation)
    }

    /// One step of the plus and minus buttons, and how many steps Shift takes.
    fn step(self) -> f32 {
        match self {
            Knob::Scale => 1. / SCALE_STEPS_PER_UNIT,
            _ => 1.,
        }
    }

    fn big_step(self) -> f32 {
        match self {
            Knob::Scale => 10.,
            Knob::Size(_) => 5.,
            Knob::OffsetX | Knob::OffsetY => 10.,
            _ => 15.,
        }
    }

    fn get(self, view: &Krosshair) -> f32 {
        let shape = || &view.ed.layers[view.ed.sel.min(view.ed.layers.len() - 1)];
        match self {
            Knob::Scale => clean_scale(view.cfg.scale),
            Knob::Rotation => clean_rotation(view.cfg.rotation) as f32,
            Knob::LayerRotation => clean_rotation(view.draft.rotation) as f32,
            Knob::EditorRotation => clean_rotation(shape().rotation) as f32,
            Knob::Size(dim) => dim.clean(dim.value(shape()) as f32) as f32,
            Knob::OffsetX => clean_offset(view.draft.ox as f32) as f32,
            Knob::OffsetY => clean_offset(view.draft.oy as f32) as f32,
        }
    }

    fn set(self, view: &mut Krosshair, value: f32) {
        let sel = view.ed.sel.min(view.ed.layers.len() - 1);
        match self {
            Knob::Scale => view.cfg.scale = clean_scale(value),
            Knob::Rotation => view.cfg.rotation = clean_rotation(value.round() as i32),
            Knob::LayerRotation => view.draft.rotation = clean_rotation(value.round() as i32),
            Knob::EditorRotation => view.ed.layers[sel].rotation = clean_rotation(value.round() as i32),
            Knob::Size(dim) => *dim.of(&mut view.ed.layers[sel]) = dim.clean(value),
            Knob::OffsetX => view.draft.ox = clean_offset(value),
            Knob::OffsetY => view.draft.oy = clean_offset(value),
        }
    }

    /// The hint opens on the left where there is no room on the right (the side panels).
    fn hint_on_left(self) -> bool {
        matches!(self, Knob::LayerRotation | Knob::EditorRotation | Knob::Size(_) | Knob::OffsetX | Knob::OffsetY)
    }

    /// The value after dragging `dx` pixels from `from`.
    fn dragged(self, from: f32, dx: f32) -> f32 {
        match self {
            // 0.05 per 3 px: 1x to 2x is 60 px.
            Knob::Scale => clean_scale(from + (dx / 3.).round() / SCALE_STEPS_PER_UNIT),
            // 1 degree per 2 px; past 359 it goes on from 0.
            Knob::Rotation | Knob::LayerRotation | Knob::EditorRotation => clean_rotation(from as i32 + (dx / 2.).round() as i32) as f32,
            // Stops at the ends of the range.
            Knob::Size(dim) => dim.clean(from + dx / self.px_per_unit()) as f32,
            Knob::OffsetX | Knob::OffsetY => clean_offset(from + dx / self.px_per_unit()) as f32,
        }
    }

    fn reset(self) -> f32 {
        match self {
            Knob::Scale => 1.,
            Knob::Rotation | Knob::LayerRotation | Knob::EditorRotation => 0.,
            Knob::Size(dim) => dim.default_value() as f32,
            Knob::OffsetX | Knob::OffsetY => 0.,
        }
    }

    fn label(self, value: f32) -> String {
        match self {
            Knob::Scale => scale_label(value),
            Knob::Rotation | Knob::LayerRotation | Knob::EditorRotation => format!("{}°", value as i32),
            Knob::Size(_) => format!("{} px", value as i32),
            Knob::OffsetX | Knob::OffsetY => signed_px(value as i32),
        }
    }
}

fn clean_offset(value: f32) -> i32 {
    (value.round() as i32).clamp(-MAX_OFFSET, MAX_OFFSET)
}

/// Width of the track in a knob's hint.
const TRACK_W: f32 = 220.;
/// Width of a knob in the navigation rail.
const KNOB_W: f32 = 60.;
/// Width of the value and tape in a knob field, and of the whole field with its buttons.
const FIELD_MID_W: f32 = 64.;
/// How far a part of a tape field (minus, the tape, plus) darkens toward the background when
/// hovered: all three the same.
const FIELD_HOVER: f32 = 0.6;
const FIELD_W: f32 = 2. + 40. + 2. + FIELD_MID_W + 2. + 40. + 2.;

/// Where a knob's tape goes inside its element: inset from the sides, top, height.
#[derive(Clone, Copy)]
pub struct TapeArea {
    inset: f32,
    top: f32,
    height: f32,
}

impl Knob {
    /// Drag distance for one unit of the value. The tape moves at the same rate, so a mark under
    /// the pointer stays under it while dragging.
    fn px_per_unit(self) -> f32 {
        match self {
            Knob::Scale => 60.,
            Knob::Rotation | Knob::LayerRotation | Knob::EditorRotation => 2.,
            Knob::Size(_) => 8.,
            Knob::OffsetX | Knob::OffsetY => 4.,
        }
    }

    /// Marks on the tape, finest first: every so many units, and how tall. A mark takes the
    /// height of the coarsest spacing it falls on.
    fn marks(self) -> &'static [(f32, f32)] {
        match self {
            Knob::Scale => &[(0.1, 4.), (0.5, 7.), (1., 11.)],
            Knob::Rotation | Knob::LayerRotation | Knob::EditorRotation => &[(5., 4.), (15., 7.), (45., 9.), (90., 11.)],
            Knob::Size(_) => &[(1., 4.), (5., 11.)],
            Knob::OffsetX | Knob::OffsetY => &[(1., 4.), (5., 7.), (10., 11.)],
        }
    }

    /// Where the tape ends; the angle goes round and round.
    fn range(self) -> Option<(f32, f32)> {
        match self {
            Knob::Scale => Some((SCALE_MIN, SCALE_MAX)),
            Knob::Rotation | Knob::LayerRotation | Knob::EditorRotation => None,
            Knob::Size(dim) => Some((dim.range().0 as f32, dim.range().1 as f32)),
            Knob::OffsetX | Knob::OffsetY => Some((-MAX_OFFSET as f32, MAX_OFFSET as f32)),
        }
    }
}

/// The tape of a knob: a flat ruler under a fixed index line in the middle, sliding as the value
/// changes, its marks evenly spaced and fading out at the ends, on a thin base line. `lit` (0 to
/// 1) brightens it on hover and while dragging.
fn paint_tape(bounds: Bounds<Pixels>, area: TapeArea, knob: Knob, value: f32, lit: f32, window: &mut Window) {
    let left = f32::from(bounds.origin.x) + area.inset;
    let top = f32::from(bounds.origin.y) + area.top;
    let (w, h) = (f32::from(bounds.size.width) - 2. * area.inset, area.height);
    let mid = left + w / 2.;
    let rect = |x: f32, y: f32, rw: f32, rh: f32| Bounds::new(point(px(x), px(y)), size(px(rw), px(rh)));
    const FADE: f32 = 10.;
    let fade_at = |x: f32| ((x - left) / FADE).min((left + w - x) / FADE).clamp(0., 1.);

    // The base line, fading at the ends like the marks.
    for i in 0..(w as i32) {
        let x = left + i as f32 + 0.5;
        let color = Hsla { a: fade_at(x) * 0.5, ..mix(FAINT, MUTED, lit) };
        window.paint_quad(fill(rect(x - 0.5, top + h - 1., 1., 1.), color));
    }

    let ppu = knob.px_per_unit();
    let marks = knob.marks();
    let finest = marks[0].0;
    let reach = w / 2. / ppu;
    let first = ((value - reach) / finest).ceil() as i64;
    let last = ((value + reach) / finest).floor() as i64;
    for n in first..=last {
        let at = n as f32 * finest;
        if knob.range().is_some_and(|(lo, hi)| at < lo - 1e-3 || at > hi + 1e-3) {
            continue;
        }
        let height = marks
            .iter()
            .rev()
            .find(|(every, _)| {
                let k = at / every;
                (k - k.round()).abs() < 1e-3
            })
            .map_or(marks[0].1, |m| m.1);
        let x = mid + (at - value) * ppu;
        let alpha = fade_at(x);
        if alpha <= 0. {
            continue;
        }
        let major = height >= marks[marks.len() - 1].1;
        let color = if major { c(TEXT) } else { mix(MUTED, TEXT, lit * 0.6) };
        let mw = if major { 1.5 } else { 1. };
        window.paint_quad(fill(rect(x - mw / 2., top + h - 1. - height, mw, height), Hsla { a: alpha, ..color }));
    }

    // The index line.
    window.paint_quad(quad(rect(mid - 1., top - 2., 2., h + 2.), px(1.), c(ACCENT), px(0.), transparent_black(), BorderStyle::default()));
}
/// What a knob's hint says.
struct HintParts {
    /// Whether the crosshair stays sharp, and why; sizes have nothing to say about that.
    sharp: Option<(bool, &'static str)>,
    /// Labels for the two ends of the track.
    ends: (String, String),
    /// Evenly spaced ticks along the track; true for the tall ones.
    ticks: Vec<bool>,
    /// Where the value is on the track, 0 to 1.
    at: f32,
    /// What a double-click sets.
    reset: String,
}

impl Knob {
    fn hint_parts(self, value: f32) -> HintParts {
        match self {
            Knob::Scale => {
                let sharp = scale_is_sharp(value);
                // A tick every 0.5, tall on whole values.
                let n = ((SCALE_MAX - SCALE_MIN) * 2.).round() as usize;
                HintParts {
                    sharp: Some((sharp, if sharp { "whole number: pixels repeat" } else { "not whole: pixels are averaged" })),
                    ends: (scale_label(SCALE_MIN), scale_label(SCALE_MAX)),
                    ticks: (0..=n).map(|i| (SCALE_MIN + i as f32 * 0.5).fract() == 0.).collect(),
                    at: (value - SCALE_MIN) / (SCALE_MAX - SCALE_MIN),
                    reset: "1×".into(),
                }
            }
            Knob::Rotation | Knob::LayerRotation | Knob::EditorRotation => {
                let sharp = rotation_is_sharp(value as i32);
                HintParts {
                    sharp: Some((sharp, if sharp { "quarter turn: pixels move exactly" } else { "slanted: edges are smoothed" })),
                    ends: ("0°".into(), "360°".into()),
                    // A tick every 45 degrees, tall on quarter turns.
                    ticks: (0..=8).map(|i| i % 2 == 0).collect(),
                    at: value / 360.,
                    reset: "0°".into(),
                }
            }
            Knob::Size(dim) => {
                let (lo, hi) = dim.range();
                HintParts {
                    sharp: None,
                    ends: (self.label(lo as f32), self.label(hi as f32)),
                    // A tick every pixel, tall every 5.
                    ticks: (lo..=hi).map(|v| v % 5 == 0).collect(),
                    at: (value - lo as f32) / (hi - lo).max(1) as f32,
                    reset: self.label(dim.default_value() as f32),
                }
            }
            Knob::OffsetX | Knob::OffsetY => HintParts {
                sharp: None,
                ends: (signed_px(-MAX_OFFSET), signed_px(MAX_OFFSET)),
                // A tick every 1000 px, tall at the anchor.
                ticks: (-4..=4).map(|i| i == 0).collect(),
                at: (value + MAX_OFFSET as f32) / (2 * MAX_OFFSET) as f32,
                reset: signed_px(0),
            },
        }
    }
}

/// The hint beside a knob whose element is `beside` wide: the value, sharp or blended, and where
/// the value is on its range.
fn knob_hint(knob: Knob, value: f32, beside: f32) -> impl IntoElement {
    let HintParts { sharp, ends, ticks, at, reset } = knob.hint_parts(value);
    let ticks = ticks.into_iter().map(|tall| {
        div().w(px(2.)).h(px(if tall { 10. } else { 6. })).rounded_full().bg(c(if tall { MUTED } else { FAINT }))
    });
    let marker = div()
        .absolute()
        .bottom(px(0.))
        .left(px(at * (TRACK_W - 2.) - 1.))
        .w(px(4.))
        .h(px(16.))
        .rounded_full()
        .bg(c(ACCENT));
    let track = div().relative().w(px(TRACK_W)).h(px(16.)).flex().items_end().justify_between().children(ticks).child(marker);
    let badge = sharp.map(|(sharp, _)| {
        div()
            .h(px(22.))
            .px(px(8.))
            .flex()
            .items_center()
            .rounded_full()
            .font_family(mono())
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .map(|d| if sharp { d.bg(c(ACCENT)).text_color(c(BG)) } else { d.bg(c(SURFACE)).text_color(c(TEXT)) })
            .child(if sharp { "sharp" } else { "blended" })
    });
    div()
        .absolute()
        .map(|d| if knob.hint_on_left() { d.right(px(beside + 8.)) } else { d.left(px(beside + 8.)) })
        .bottom(px(0.))
        .w(px(244.))
        .p(px(12.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .rounded(px(12.))
        .bg(c(RAISED))
        .shadow_lg()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .h(px(22.))
                .child(div().font_family(mono()).text_size(px(15.)).font_weight(FontWeight::MEDIUM).child(knob.label(value)))
                .children(badge),
        )
        .children(sharp.map(|(_, why)| caption(why)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(track)
                .child(div().flex().justify_between().child(caption(ends.0)).child(caption(ends.1))),
        )
        .child(caption(format!("drag · double-click: {reset}")))
}
/// Space above the content of every screen, below the header.
const SCREEN_TOP: f32 = 24.;

/// Screen title with a monospace note next to it. It always takes the height of the controls
/// that may stand beside it (44), centered, so it sits at the same place on every screen.
fn heading(title: &'static str, note: String) -> Div {
    div().h(px(44.)).flex_none().flex().items_center().child(
        div()
            .flex()
            .items_baseline()
            .gap(px(12.))
            .child(div().text_size(px(28.)).font_weight(FontWeight::MEDIUM).line_height(px(30.)).child(title))
            .child(div().font_family(mono()).text_size(px(13.)).text_color(c(MUTED)).child(note)),
    )
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
