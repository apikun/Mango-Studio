// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Slicex editor: painting and hit regions (platform independent), laid out as the original Slicex's window:
//! the top strip (four faders, the modulation pad, the layering and the emblem), the middle panel (the emblem plate,
//! the articulator buttons, the filter, the target and source tabs and the envelope), the toolbar, the info strip, the
//! wave with its slices, the chosen slice's strip and the keyboard.
//!
// spec: plugins/lemon-slicex.md §5

use os_theme::design::lemon as brand;
use os_theme::Palette;
use os_ui::kit::lemon;
use os_ui::kit::State as Kst;
use os_ui::{Canvas, Color, Font, Rect};

use crate::ext::{Ext, Source, ALL_SOURCES, ALL_TARGETS, FILTER_MODES, LAYERS, LFO_SHAPES};
use crate::extctl::{self, Ctl, Sel};
use crate::params::{ATTACK_ID, DECAY_ID, DECLICK_ID, PITCH_ID, TO_END_ID};
use crate::state::State;

/// Editor size in logical pixels.
pub const W: u32 = 660;
pub const H: u32 = 624;

/// A rect from its left, right, top and bottom edges (right and bottom exclusive).
const fn edges(x0: f32, x1: f32, y0: f32, y1: f32) -> Rect {
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

// The colours of the original's window, in Mango's orange.
const FRAME: Color = Color::rgb(0x2f3747);
const STRIP: Color = Color::rgb(0x1f2530);
const PLATE: Color = Color::rgb(0x6a7690);
const PLATE_DARK: Color = Color::rgb(0x56627b);
const TAB_OFF: Color = Color::rgb(0x76829b);
const TAB_ON: Color = Color::rgb(0xc4cddc);
const INK: Color = Color::rgb(0x1e2430);
const INK_DIM: Color = Color::rgb(0x39425a);
const WELL: Color = Color::rgb(0x273041);
const GRID: Color = Color::rgb(0x36425a);
const WAVE_INK: Color = Color::rgb(0xa5b8da);
const LIGHT: Color = Color::rgb(0xe3e7ef);
const SOFT: Color = Color::rgb(0xaab4c8);
const KEY_WHITE: Color = Color::rgb(0xe6e9ef);
const KEY_BLACK: Color = Color::rgb(0x242a35);
const SLOT: Color = Color::rgb(0x12161d);

/// The top strip: its faders, the pad, the layering and the emblem.
pub const FADER_X: [f32; 4] = [24.0, 54.0, 84.0, 114.0];
const FADER_TOP: f32 = 18.0;
const FADER_H: f32 = 62.0;
pub const PAD: Rect = edges(152.0, 232.0, 26.0, 100.0);
pub const MOD_PANEL: Rect = edges(144.0, 334.0, 8.0, 104.0);
pub const LAYER_PANEL: Rect = edges(342.0, 520.0, 8.0, 104.0);
const DROP: Rect = edges(350.0, 512.0, 26.0, 44.0);
pub const LOGO: Rect = edges(528.0, 654.0, 34.0, 104.0);
/// The view buttons: the keyboard, then the four layouts.
const KEYS_BTN: Rect = edges(528.0, 550.0, 10.0, 28.0);
/// The middle panel.
const PLATE_R: Rect = edges(6.0, 156.0, 112.0, 322.0);
const ART_PANEL: Rect = edges(162.0, 316.0, 112.0, 180.0);
const FILTER_PANEL: Rect = edges(162.0, 316.0, 186.0, 322.0);
const GRAPH: Rect = edges(324.0, 652.0, 148.0, 282.0);
const KNOB_BAR: Rect = edges(322.0, 654.0, 286.0, 322.0);
/// The toolbar and the info strip under the middle panel, and the wave.
pub const BPM_BOX: Rect = edges(130.0, 188.0, 381.0, 397.0);
pub const BEATS_BOX: Rect = edges(486.0, 540.0, 381.0, 397.0);
const DECK_A: Rect = edges(196.0, 214.0, 382.0, 398.0);
const DECK_B: Rect = edges(216.0, 234.0, 382.0, 398.0);
pub const WELL_R: Rect = edges(6.0, 654.0, 404.0, 516.0);
pub const WAVE: Rect = edges(12.0, 648.0, 408.0, 512.0);
/// The chosen slice's strip.
pub const NAME: Rect = edges(96.0, 262.0, 523.0, 547.0);
pub const KEY_BOX: Rect = edges(306.0, 366.0, 523.0, 547.0);
const REVERSE: Rect = edges(430.0, 510.0, 523.0, 547.0);
/// The keyboard: white keys 12 px wide from key `KEYS_FROM`.
const KEYS_Y: f32 = 554.0;
const KEYS_H: f32 = 62.0;
const KEY_W: f32 = 12.0;
const KEYS_FROM: i32 = 24;

/// The window's bands below the top strip (from, to), in the full layout: the middle panel, the audio editor and the
/// keyboard.
const BANDS: [(f32, f32); 3] = [(112.0, 326.0), (326.0, 554.0), (554.0, 624.0)];
const TOP_H: f32 = 112.0;

/// The bands a view shows with how far each moves up from the full layout, and the window's height: view 0 shows all,
/// 1 hides the audio editor, 2 hides the region settings (the middle panel), 3 shows the top strip alone.
pub fn arrange(view: u8, keys: bool) -> (Vec<(f32, f32, f32)>, f32) {
    let shown = [view == 0 || view == 1, view == 0 || view == 2, keys && view != 3];
    let mut y = TOP_H;
    let mut out = Vec::new();
    for (i, (a, b)) in BANDS.iter().enumerate() {
        if shown[i] {
            out.push((*a, *b, y - a));
            y += b - a;
        }
    }
    (out, y)
}

/// The window's size in logical pixels for a view.
pub fn window_size(view: u8, keys: bool) -> (u32, u32) {
    (W, arrange(view, keys).1 as u32)
}

const SMALL: Font = Font::new(10);
const LABEL: Font = Font::new(11);
const PILL: Font = Font::new(10).bold();

/// Read access to parameter values by id.
pub trait Model {
    fn norm(&self, id: &str) -> f32;
    fn text(&self, id: &str) -> String;
}

impl Model for os_plugkit::vals::Vals {
    fn norm(&self, id: &str) -> f32 {
        os_plugkit::vals::Vals::norm(self, id)
    }
    fn text(&self, id: &str) -> String {
        os_plugkit::vals::Vals::text(self, id)
    }
}

/// What a region of the editor does when clicked or dragged.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Vertical drag / wheel; menu on right click.
    Knob(String),
    /// A switch parameter (click toggles; menu on right click).
    Switch(String),
    Bpm,
    Beats,
    Load,
    Slicing,
    Dump,
    /// The well: a click chooses the slice under it and plays it.
    Well,
    /// The chosen slice's key (a menu) and reverse switch.
    Key,
    Rev,
    Logo,
    /// A control of what Slicex adds (`extctl`).
    Ext(Ctl),
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub r: Rect,
    pub t: Target,
}

/// The pointer, the pressed control and the chosen slice.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    pub mouse: Option<(f32, f32)>,
    pub pressed: Option<Target>,
    pub chosen: usize,
    /// The articulator and the tabs the panels show.
    pub sel: Sel,
}

/// What a paint shows besides the parameters.
pub struct Look<'a> {
    pub state: &'a State,
    /// The loop's length in frames (0 = no sample) and its sample rate.
    pub frames: usize,
    pub rate: u32,
    /// The waveform's low and high per column of `WAVE` (empty = no sample).
    pub peaks: &'a [(f32, f32)],
    /// The dump is offered (hosted by Lemon Studio, a loop with slices).
    pub can_dump: bool,
    /// The articulators, layering and pad.
    pub ext: &'a Ext,
    /// The loaded decks' file names (A, B).
    pub decks: [String; 2],
}

struct Ui<'a> {
    c: &'a mut Canvas,
    m: &'a dyn Model,
    st: &'a ViewState,
    hits: Vec<Hit>,
}

impl Ui<'_> {
    fn hit(&mut self, r: Rect, t: Target) {
        self.hits.push(Hit { r, t });
    }
    fn hover(&self, r: Rect, t: &Target) -> bool {
        self.st.pressed.as_ref() != Some(t) && self.st.pressed.is_none() && self.st.mouse.is_some_and(|(x, y)| r.contains(x, y))
    }
    fn state(&self, r: Rect, t: &Target) -> Kst {
        let pressed = self.st.pressed.as_ref() == Some(t);
        Kst { hover: self.hover(r, t), pressed, ..Kst::NORMAL }
    }
}

/// The name of key `k` (60 = C5).
pub fn key_name(k: i32) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[k.rem_euclid(12) as usize], k.div_euclid(12))
}

/// The frame of the loop at x in the well.
pub fn frame_at(x: f32, frames: usize) -> usize {
    (((x - WAVE.x) / WAVE.w).clamp(0.0, 1.0) as f64 * frames as f64) as usize
}

fn x_of(frame: u32, frames: usize) -> f32 {
    (WAVE.x + f64::from(frame) as f32 / frames.max(1) as f32 * WAVE.w).round()
}

/// Paints the editor. `hint` (not hosted: the host shows it otherwise) goes in the well's bottom-left corner.
pub fn paint(c: &mut Canvas, _t: &Palette, m: &dyn Model, st: &ViewState, look: &Look, hint: &str) -> Vec<Hit> {
    let mut ui = Ui { c, m, st, hits: Vec::new() };
    ui.c.begin_frame(None);
    ui.c.clear(FRAME);
    ui.c.fill_rect(edges(6.0, 654.0, 6.0, 106.0), STRIP);
    faders(&mut ui, look);
    pad_panel(&mut ui, look);
    layer_panel(&mut ui, look);
    emblem(&mut ui);
    plate(&mut ui, look);
    art_panel(&mut ui, look);
    filter_panel(&mut ui, look);
    tabs(&mut ui, look);
    graph(&mut ui, look);
    toolbar(&mut ui, look);
    info(&mut ui, look);
    well(&mut ui, look, hint);
    slice_strip(&mut ui, look);
    keyboard(&mut ui, look);
    view_buttons(&mut ui, look);
    arrange_bands(ui, look)
}

/// Moves the shown bands up under the top strip, and the hit regions with them (a hidden band's go).
fn arrange_bands(mut ui: Ui, look: &Look) -> Vec<Hit> {
    let (bands, _) = arrange(look.ext.view, look.ext.keys);
    let k = ui.c.scale;
    for &(from, to, dy) in &bands {
        if dy != 0.0 {
            ui.c.shift(Rect::new(0.0, from + dy, W as f32, to - from - dy), 0, (dy * k).round() as i32, &[]);
        }
    }
    let mut hits = Vec::new();
    for mut h in std::mem::take(&mut ui.hits) {
        let y = h.r.cy();
        if y < TOP_H {
            hits.push(h);
        } else if let Some(&(_, _, dy)) = bands.iter().find(|(a, b, _)| y >= *a && y < *b) {
            h.r.y += dy;
            hits.push(h);
        }
    }
    hits
}

/// The view buttons in the top-right corner: the keyboard and the four layouts, each showing which bands it keeps.
fn view_buttons(ui: &mut Ui, look: &Look) {
    let x = look.ext;
    let glyph = |ui: &mut Ui, r: Rect, rows: [bool; 3], ink: Color| {
        for (i, on) in rows.into_iter().enumerate() {
            let h = [3.0, 4.0, 3.0][i];
            let y = r.y + 4.0 + [0.0f32, 5.0, 11.0][i];
            ui.c.fill_rect(Rect::new(r.x + 5.0, y, r.w - 10.0, h), if on { ink } else { ink.with_alpha(60) });
        }
    };
    let keys = Target::Ext(Ctl::Keys);
    let kon = x.keys;
    let hov = ui.hover(KEYS_BTN, &keys);
    ui.c.fill_rect(KEYS_BTN, if kon { TAB_ON } else if hov { TAB_OFF.shade(0.12) } else { TAB_OFF });
    for i in 0..4 {
        let kx = KEYS_BTN.x + 4.0 + 4.0 * i as f32;
        ui.c.fill_rect(Rect::new(kx, KEYS_BTN.y + 4.0, 2.0, 10.0), INK);
    }
    ui.hit(KEYS_BTN, keys);
    for i in 0..4u8 {
        let r = edges(553.0 + 24.0 * f32::from(i), 575.0 + 24.0 * f32::from(i), 10.0, 28.0);
        let t = Target::Ext(Ctl::View(i));
        let on = x.view == i;
        let hov = ui.hover(r, &t);
        ui.c.fill_rect(r, if on { TAB_ON } else if hov { TAB_OFF.shade(0.12) } else { TAB_OFF });
        let rows = match i {
            0 => [true, true, true],
            1 => [true, true, false],
            2 => [true, false, true],
            _ => [true, false, false],
        };
        glyph(ui, r, rows, INK);
        ui.hit(r, t);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Small parts

/// A raised rounded panel of the middle.
fn plate_box(ui: &mut Ui, r: Rect) {
    ui.c.fill_rect_gradient(r, PLATE.shade(0.06), PLATE_DARK, 4.0);
    ui.c.stroke_rrect(r.inset(0.5), 4.0, 1.0, INK_DIM.with_alpha(160));
}

/// A title at the top-left of a panel.
fn title(ui: &mut Ui, r: Rect, text: &str, ink: Color) {
    ui.c.text(SMALL.bold(), r.x + 8.0, r.y + 4.0, ink, text);
}

/// A small button of the middle panel: light when on, with its label.
fn tab(ui: &mut Ui, r: Rect, t: Target, label: &str, on: bool, disabled: bool) {
    let hov = ui.hover(r, &t) && !disabled;
    let face = if on { TAB_ON } else if hov { TAB_OFF.shade(0.12) } else { TAB_OFF };
    ui.c.fill_rect(r, face);
    ui.c.stroke_rrect(r.inset(0.5), 0.0, 1.0, INK_DIM.with_alpha(120));
    ui.c.text_center(PILL, r, if disabled { INK_DIM } else { INK }, label);
    ui.hit(r, t);
}

/// An LED toggle with its label to the right.
fn led(ui: &mut Ui, cx: f32, cy: f32, label: &str, on: bool, t: Target, ink: Color) {
    let w = ui.c.text_width(SMALL.bold(), label);
    let hr = Rect::new(cx - 8.0, cy - 8.0, 16.0 + 6.0 + w + 2.0, 16.0);
    ui.c.fill_circle(cx, cy, 5.5, SLOT);
    ui.c.stroke_circle(cx, cy, 5.5, 1.0, INK_DIM);
    if on {
        ui.c.fill_circle(cx, cy, 3.6, brand::MAIN);
    }
    let lh = ui.c.line_height(SMALL.bold());
    ui.c.text(SMALL.bold(), cx + 10.0, (cy - lh / 2.0).round(), ink, label);
    ui.hit(hr, t);
}

/// A round knob with its caption under it.
#[allow(clippy::too_many_arguments)]
fn knob(ui: &mut Ui, t: Target, cx: f32, cy: f32, r: f32, norm: f32, cap: &str, cap_y: f32, ink: Color) {
    let hr = Rect::new(cx - r - 3.0, cy - r - 3.0, 2.0 * r + 6.0, 2.0 * r + 6.0);
    let hov = ui.hover(hr, &t) || ui.st.pressed.as_ref() == Some(&t);
    ui.c.fill_circle(cx, cy, r + 1.5, INK.with_alpha(150));
    ui.c.fill_circle(cx, cy, r, if hov { Color::rgb(0x2c3342) } else { Color::rgb(0x232a37) });
    ui.c.stroke_circle(cx, cy, r, 1.0, Color::rgb(0x3f495d));
    let a = (norm.clamp(0.0, 1.0) - 0.5) * 270f32.to_radians();
    let (sx, sy) = (a.sin(), -a.cos());
    ui.c.line(cx + sx * r * 0.25, cy + sy * r * 0.25, cx + sx * (r - 2.0), cy + sy * (r - 2.0), 1.8, LIGHT);
    if !cap.is_empty() {
        let w = ui.c.text_width(SMALL.bold(), cap);
        ui.c.text(SMALL.bold(), (cx - w / 2.0).round(), cap_y, ink, cap);
    }
    ui.hit(hr, t);
}

/// A vertical fader of the top strip, its name under it.
fn fader(ui: &mut Ui, t: Target, cx: f32, norm: f32, cap: &str) {
    let hr = Rect::new(cx - 13.0, FADER_TOP - 4.0, 26.0, FADER_H + 8.0);
    let slot = Rect::new(cx - 3.0, FADER_TOP, 6.0, FADER_H);
    ui.c.fill_rrect(slot, 3.0, SLOT);
    let y = FADER_TOP + FADER_H * (1.0 - norm.clamp(0.0, 1.0));
    ui.c.fill_rrect(Rect::new(slot.x + 1.0, y, slot.w - 2.0, slot.bottom() - y), 2.0, brand::MAIN);
    let hov = ui.hover(hr, &t) || ui.st.pressed.as_ref() == Some(&t);
    let handle = Rect::new(cx - 8.0, y - 4.0, 16.0, 8.0);
    ui.c.fill_rrect(handle, 2.0, if hov { LIGHT } else { SOFT });
    ui.c.fill_rect(Rect::new(handle.x + 3.0, handle.cy() - 0.5, handle.w - 6.0, 1.0), INK_DIM);
    let w = ui.c.text_width(SMALL.bold(), cap);
    ui.c.text(SMALL.bold(), (cx - w / 2.0).round(), 86.0, SOFT, cap);
    ui.hit(hr, t);
}

// ---------------------------------------------------------------------------------------------------------------
// The top strip

fn faders(ui: &mut Ui, look: &Look) {
    for (i, (id, cap)) in [(PITCH_ID, "PITCH"), (ATTACK_ID, "ATT"), (DECAY_ID, "DEC")].into_iter().enumerate() {
        let n = ui.m.norm(id);
        fader(ui, Target::Knob(id.into()), FADER_X[i], n, cap);
    }
    fader(ui, Target::Ext(Ctl::Master), FADER_X[3], look.ext.master / 1.25, "VOL");
}

fn pad_panel(ui: &mut Ui, look: &Look) {
    ui.c.fill_rrect(MOD_PANEL, 4.0, Color::rgb(0x262d3a));
    title(ui, MOD_PANEL, "MODULATION", brand::MAIN);
    ui.c.fill_rrect(PAD, 3.0, SLOT);
    ui.c.fill_rect(Rect::new(PAD.cx(), PAD.y + 3.0, 1.0, PAD.h - 6.0), GRID);
    ui.c.fill_rect(Rect::new(PAD.x + 3.0, PAD.cy(), PAD.w - 6.0, 1.0), GRID);
    let x = look.ext;
    let (px, py) = (PAD.x + 4.0 + x.mod_x * (PAD.w - 8.0), PAD.bottom() - 4.0 - x.mod_y * (PAD.h - 8.0));
    ui.c.fill_circle(px, py, 4.0, brand::MAIN);
    ui.c.stroke_circle(px, py, 6.0, 1.0, brand::MAIN.with_alpha(120));
    ui.hit(PAD, Target::Ext(Ctl::Pad));
    for (i, (c, cap)) in [(Ctl::PadX, "X"), (Ctl::PadY, "Y")].into_iter().enumerate() {
        let cy = 40.0 + 28.0 * i as f32;
        knob(ui, Target::Ext(c), 262.0, cy, 11.0, if c == Ctl::PadX { x.mod_x } else { x.mod_y }, "", 0.0, SOFT);
        ui.c.text(SMALL.bold(), 282.0, cy - 6.0, SOFT, cap);
    }
    led(ui, 252.0, 94.0, "SMOOTH", x.smooth, Target::Ext(Ctl::Smooth), SOFT);
}

fn layer_panel(ui: &mut Ui, look: &Look) {
    ui.c.fill_rrect(LAYER_PANEL, 4.0, Color::rgb(0x262d3a));
    title(ui, LAYER_PANEL, "LAYERING", brand::MAIN);
    let x = look.ext;
    // The list of layers: a click shows the next.
    ui.c.fill_rrect(DROP, 3.0, Color::rgb(0x8a95ab));
    ui.c.text(LABEL.bold(), DROP.x + 8.0, DROP.y + 2.0, INK, LAYERS[usize::from(x.layer).min(2)]);
    let (ax, ay) = (DROP.right() - 12.0, DROP.cy());
    ui.c.fill_polygon(&[(ax - 4.0, ay - 3.0), (ax + 4.0, ay - 3.0), (ax, ay + 3.0)], INK);
    ui.hit(DROP, Target::Ext(Ctl::Layer((x.layer + 1) % 3)));
    led(ui, 358.0, 58.0, "SAME DECK", x.same_deck, Target::Ext(Ctl::SameDeck), SOFT);
    led(ui, 358.0, 76.0, "CROSSFADE", x.crossfade, Target::Ext(Ctl::Crossfade), SOFT);
    let (d, e) = (ui.m.norm(DECLICK_ID) > 0.5, ui.m.norm(TO_END_ID) > 0.5);
    led(ui, 448.0, 58.0, "DECLICK", d, Target::Switch(DECLICK_ID.into()), SOFT);
    led(ui, 448.0, 76.0, "TO END", e, Target::Switch(TO_END_ID.into()), SOFT);
}

/// The Mango Studio logo (the mango, its stem and the three bars and leaf) fitted into a circle of radius `r`.
fn mango_mark(ui: &mut Ui, cx: f32, cy: f32, r: f32) {
    use crate::logo::{CENTER, RIND, SLICES, VIEW_H, VIEW_W};
    let k = 2.0 * r / VIEW_W.max(VIEW_H);
    let (ox, oy) = (cx - VIEW_W * k / 2.0, cy - VIEW_H * k / 2.0);
    let parts = [(CENTER, 0x6b3e1f), (RIND, 0xf2711c), (SLICES[0], 0xffb02e), (SLICES[1], 0xffc35c), (SLICES[2], 0xffd27a), (SLICES[3], 0x5cb85c)];
    for (d, c) in parts {
        let mut g = os_ui::Glyph::new();
        g.commands(d, ox, oy, k, None);
        ui.c.glyph(Color::rgb(c), &g);
    }
}

/// The emblem: the mango and "SLICEX".
fn emblem(ui: &mut Ui) {
    let f = Font::condensed(30, 0).bold();
    let lh = ui.c.line_height(f);
    mango_mark(ui, LOGO.x + 15.0, LOGO.cy() - 6.0, 15.0);
    lemon::spaced(ui.c, f, LOGO.x + 32.0, (LOGO.cy() - lh / 2.0 - 6.0).round(), brand::MAIN, "SLICEX", 2.0);
    ui.c.text(SMALL, LOGO.x + 32.0, LOGO.cy() + 14.0, SOFT, "Mango Studio");
    ui.hit(LOGO, Target::Logo);
}

// ---------------------------------------------------------------------------------------------------------------
// The middle panel

fn plate(ui: &mut Ui, look: &Look) {
    plate_box(ui, PLATE_R);
    // The emblem again, big and dim, and what is loaded.
    let f = Font::condensed(30, 0).bold();
    let lh = ui.c.line_height(f);
    mango_mark(ui, PLATE_R.cx(), PLATE_R.y + 76.0, 52.0);
    lemon::spaced(ui.c, f, PLATE_R.x + 18.0, (PLATE_R.cy() + 36.0 - lh / 2.0).round(), TAB_ON.with_alpha(150), "SLICEX", 2.0);
    let n = look.state.slices.len();
    let line = if n == 0 { "No loop yet".to_string() } else { format!("{n} slices") };
    ui.c.text_clip(SMALL.bold(), PLATE_R.x + 12.0, PLATE_R.bottom() - 22.0, PLATE_R.w - 24.0, INK_DIM, &line);
}

fn art_panel(ui: &mut Ui, look: &Look) {
    plate_box(ui, ART_PANEL);
    title(ui, ART_PANEL, "ARTICULATOR", INK);
    let sel = ui.st.sel;
    let st = look.state;
    let chosen = ui.st.chosen.min(st.slices.len().saturating_sub(1));
    let mine = (!st.slices.is_empty()).then(|| look.ext.art_of(chosen));
    for i in 0..8usize {
        let x = 168.0 + 18.5 * i as f32;
        let r = Rect::new(x.round(), 134.0, 16.0, 20.0);
        tab(ui, r, Target::Ext(Ctl::Art(i)), &(i + 1).to_string(), sel.art == i, false);
        if mine == Some(i) {
            ui.c.fill_circle(r.cx(), r.bottom() + 4.0, 1.8, brand::MAIN);
        }
    }
    let line = match mine {
        Some(a) => format!("Slice {} plays through {}", chosen + 1, a + 1),
        None => "Load a loop to assign slices".to_string(),
    };
    ui.c.text_clip(SMALL, ART_PANEL.x + 8.0, 162.0, ART_PANEL.w - 16.0, INK_DIM, &line);
}

fn filter_panel(ui: &mut Ui, look: &Look) {
    plate_box(ui, FILTER_PANEL);
    title(ui, FILTER_PANEL, "FILTER", INK);
    let sel = ui.st.sel;
    let a = &look.ext.artic[sel.art];
    // Modes: LP BP BS HP on the first row, OFF and the slopes on the second.
    for (i, name) in FILTER_MODES.iter().take(4).enumerate() {
        tab(ui, Rect::new(196.0 + 29.0 * i as f32, 204.0, 27.0, 17.0), Target::Ext(Ctl::Mode(i as u8)), name, a.mode == i as u8, false);
    }
    let off = a.mode >= crate::ext::FILTER_OFF;
    tab(ui, Rect::new(196.0, 224.0, 27.0, 17.0), Target::Ext(Ctl::Mode(crate::ext::FILTER_OFF)), "OFF", off, false);
    for i in 0..3u8 {
        tab(ui, Rect::new(225.0 + 29.0 * f32::from(i), 224.0, 27.0, 17.0), Target::Ext(Ctl::Slope(i + 1)), &format!("x{}", i + 1), a.slope == i + 1 && !off, off);
    }
    for (c, cx, r, cap) in [(Ctl::CutEnv, 192.0, 12.0, "ENV"), (Ctl::Cut, 242.0, 17.0, "CUT"), (Ctl::Res, 292.0, 12.0, "RES")] {
        knob(ui, Target::Ext(c), cx, 272.0, r, extctl::get(look.ext, &sel, c), cap, 300.0, INK);
    }
}

fn tabs(ui: &mut Ui, look: &Look) {
    let sel = ui.st.sel;
    let a = &look.ext.artic[sel.art];
    let w = 332.0 / 6.0;
    for (i, tg) in ALL_TARGETS.iter().enumerate() {
        let r = Rect::new((322.0 + w * i as f32).round(), 112.0, w.round(), 16.0);
        tab(ui, r, Target::Ext(Ctl::Tgt(i)), tg.name(), sel.tgt == *tg, false);
    }
    for (i, s) in ALL_SOURCES.iter().enumerate() {
        let r = Rect::new((322.0 + w * i as f32).round(), 129.0, w.round(), 16.0);
        tab(ui, r, Target::Ext(Ctl::Src(i)), s.name(), sel.src == *s, false);
        if a.amount(sel.tgt, *s) != 0.0 && sel.src != *s {
            ui.c.fill_circle(r.right() - 5.0, r.cy(), 2.0, brand::MAIN);
        }
    }
}

/// The envelope's curve: time across, level up.
fn env_curve(ui: &mut Ui, g: Rect, e: &crate::ext::Env, amount: f32) {
    let (att, dec, rel) = e.seconds(0.5);
    let hold = (att + dec * 1.3).max(0.05);
    let total = hold + rel.max(0.02) + hold * 0.15;
    let at = |t: f64| g.x + 6.0 + (t / total) as f32 * (g.w - 12.0);
    let ay = |v: f32| g.bottom() - 6.0 - v.clamp(0.0, 1.0) * (g.h - 34.0);
    let col = if amount == 0.0 { SOFT.with_alpha(110) } else { brand::MAIN };
    let n = 64;
    let mut pts = vec![(at(0.0), ay(0.0))];
    for i in 1..=n {
        let t = hold * f64::from(i) / f64::from(n);
        pts.push((at(t), ay(e.level(t, 0.5))));
    }
    pts.push((at(hold + rel.max(0.02)), ay(0.0)));
    ui.c.polyline(&pts, 1.8, col);
}

fn graph(ui: &mut Ui, look: &Look) {
    let sel = ui.st.sel;
    let a = &look.ext.artic[sel.art];
    ui.c.fill_rect(GRAPH, WELL);
    for i in 1..8 {
        let x = GRAPH.x + GRAPH.w * i as f32 / 8.0;
        ui.c.fill_rect(Rect::new(x.round(), GRAPH.y, 1.0, GRAPH.h), GRID);
    }
    for i in 1..4 {
        let y = GRAPH.y + GRAPH.h * i as f32 / 4.0;
        ui.c.fill_rect(Rect::new(GRAPH.x, y.round(), GRAPH.w, 1.0), GRID);
    }
    ui.c.fill_rect(KNOB_BAR, PLATE_DARK);
    ui.c.fill_rect(Rect::new(KNOB_BAR.x, KNOB_BAR.y, KNOB_BAR.w, 1.0), INK_DIM);
    let amount = a.amount(sel.tgt, sel.src);
    let t = sel.tgt.index();
    let xs = |k: usize| 344.0 + 36.0 * k as f32;
    let small_knob = |ui: &mut Ui, c: Ctl, k: usize| {
        knob(ui, Target::Ext(c), xs(k), 298.0, 8.5, extctl::get(look.ext, &sel, c), c.caption(), 309.0, INK);
    };
    ui.c.text(SMALL.bold(), GRAPH.x + 8.0, GRAPH.y + 4.0, SOFT, &format!("{} / {}", sel.tgt.name(), sel.src.name()));
    match sel.src {
        Source::Env => {
            env_curve(ui, GRAPH, &a.env[t], amount);
            for (k, c) in [Ctl::Att, Ctl::Dec, Ctl::Sus, Ctl::Rel, Ctl::Amt].into_iter().enumerate() {
                small_knob(ui, c, k);
            }
            led(ui, 540.0, 298.0, "TEMPO", a.env[t].tempo, Target::Ext(Ctl::Tempo), INK);
        }
        Source::Lfo => {
            let l = a.lfo[t];
            let col = if amount == 0.0 { SOFT.with_alpha(110) } else { brand::MAIN };
            let probe = crate::ext::Lfo { shape: l.shape, rate: 1.0, tempo: false };
            let pts: Vec<(f32, f32)> = (0..=96)
                .map(|i| {
                    let v = probe.value(f64::from(i) / 96.0 * 3.0, 0.5, 0.37);
                    (GRAPH.x + 6.0 + (i as f32 / 96.0) * (GRAPH.w - 12.0), GRAPH.cy() - v * (GRAPH.h / 2.0 - 14.0))
                })
                .collect();
            ui.c.polyline(&pts, 1.8, col);
            ui.c.text(SMALL.bold(), GRAPH.x + 8.0, GRAPH.y + 18.0, SOFT, &format!("{} (click for the next)", LFO_SHAPES[usize::from(l.shape).min(4)]));
            ui.hit(GRAPH, Target::Ext(Ctl::LfoShape(0)));
            small_knob(ui, Ctl::LfoRate, 0);
            small_knob(ui, Ctl::Amt, 1);
            led(ui, 540.0, 298.0, "TEMPO", l.tempo, Target::Ext(Ctl::Tempo), INK);
        }
        other => {
            let what = match other {
                Source::Vel => "The note's velocity",
                Source::ModX => "The pad's X",
                Source::ModY => "The pad's Y",
                _ => "A number picked at every note",
            };
            ui.c.text_clip(LABEL, GRAPH.x + 12.0, GRAPH.cy() - 22.0, GRAPH.w - 24.0, LIGHT, &format!("{what} moves {}", sel.tgt.name()));
            ui.c.text_clip(LABEL.bold(), GRAPH.x + 12.0, GRAPH.cy() - 4.0, GRAPH.w - 24.0, if amount == 0.0 { SOFT } else { brand::MAIN }, &format!("Amount {}", extctl::text(look.ext, &sel, Ctl::Amt)));
            small_knob(ui, Ctl::Amt, 0);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The toolbar, the info strip, the wave, the slice strip and the keyboard

/// A glyph of the toolbar and the info strip.
#[derive(Clone, Copy, PartialEq)]
enum Gl {
    Loop,
    Play,
    Stop,
    File,
    Format,
    Edit,
    Tools,
    Regions,
    View,
    Snap,
    Select,
    Zoom,
    Undo,
    Normalize,
    FadeIn,
    FadeOut,
    Run,
    Stretch,
    Marker,
    AutoSlice,
    Score,
    SaveAs,
    Drag,
    Scrub,
    Decks,
    WaveSpec,
    ClickFree,
    AutoScroll,
}

/// The menu buttons in the order of `MENUS`: their glyphs and names.
pub const MENUS: [&str; 9] = ["File", "Format", "Edit", "Tools", "Regions", "View", "Snap", "Select", "Zoom"];
const MENU_GLYPHS: [Gl; 9] = [Gl::File, Gl::Format, Gl::Edit, Gl::Tools, Gl::Regions, Gl::View, Gl::Snap, Gl::Select, Gl::Zoom];
/// The sample edit functions in order: what each does (its name in the hint).
pub const EDITS: [&str; 11] = [
    "Undo",
    "Normalize",
    "Fade in",
    "Fade out",
    "Run script",
    "Drum (loop) stretch",
    "Add / remove marker",
    "Auto-slice",
    "Dump score",
    "Save as",
    "Drag sample",
];
const EDIT_GLYPHS: [Gl; 11] = [Gl::Undo, Gl::Normalize, Gl::FadeIn, Gl::FadeOut, Gl::Run, Gl::Stretch, Gl::Marker, Gl::AutoSlice, Gl::Score, Gl::SaveAs, Gl::Drag];
/// The switches at the right of the info strip.
pub const SWITCHES: [&str; 5] = ["Scrub", "Deck display", "Wave / spectrum", "Click-free editing", "Auto-scroll"];
const SWITCH_GLYPHS: [Gl; 5] = [Gl::Scrub, Gl::Decks, Gl::WaveSpec, Gl::ClickFree, Gl::AutoScroll];

const TB_Y: f32 = 332.0;
const TB_H: f32 = 22.0;
const BTN_FACE: Color = Color::rgb(0x2a3141);
const BTN_EDGE: Color = Color::rgb(0x161a23);

/// The transport buttons (loop, play, stop), the menu buttons and the edit buttons.
const LOOP: Rect = edges(28.0, 52.0, TB_Y, TB_Y + TB_H);
const PLAY: Rect = edges(54.0, 78.0, TB_Y, TB_Y + TB_H);
const STOP: Rect = edges(80.0, 104.0, TB_Y, TB_Y + TB_H);
const fn menu_cell(i: usize) -> Rect {
    Rect::new(158.0 + 25.0 * i as f32, TB_Y, 22.0, TB_H)
}
const fn edit_cell(i: usize) -> Rect {
    Rect::new(404.0 + 22.5 * i as f32, TB_Y, 21.0, TB_H)
}
const fn switch_cell(i: usize) -> Rect {
    Rect::new(566.0 + 17.0 * i as f32, 380.0, 16.0, 16.0)
}

/// Draws a glyph in `r`.
fn glyph(ui: &mut Ui, r: Rect, g: Gl, ink: Color) {
    let (cx, cy) = (r.cx(), r.cy());
    let ic = |ui: &mut Ui, i: &os_ui::tabler::TablerIcon| ui.c.icon(i, Rect::new(cx - 8.0, cy - 8.0, 16.0, 16.0), ink);
    use os_ui::tabler as tb;
    match g {
        Gl::Loop => ic(ui, &tb::REPEAT),
        Gl::Play => ui.c.fill_polygon(&[(cx - 4.0, cy - 6.0), (cx - 4.0, cy + 6.0), (cx + 6.0, cy)], ink),
        Gl::Stop => ui.c.fill_rect(Rect::new(cx - 5.0, cy - 5.0, 10.0, 10.0), ink),
        Gl::File => ic(ui, &tb::DEVICE_FLOPPY),
        Gl::Format => ic(ui, &tb::FILE),
        Gl::Edit => ic(ui, &tb::CUT),
        Gl::Tools => ic(ui, &tb::TOOL),
        Gl::Regions => ic(ui, &tb::SLICE),
        Gl::View => {
            // An eye: the lids and the pupil.
            let pts: Vec<(f32, f32)> = (0..=16).map(|i| {
                let a = std::f32::consts::PI * i as f32 / 16.0;
                (cx - 7.0 + 14.0 * i as f32 / 16.0, cy - a.sin() * 4.5)
            }).chain((0..=16).rev().map(|i| {
                let a = std::f32::consts::PI * i as f32 / 16.0;
                (cx - 7.0 + 14.0 * i as f32 / 16.0, cy + a.sin() * 4.5)
            })).collect();
            ui.c.polyline(&pts, 1.4, ink);
            ui.c.fill_circle(cx, cy, 2.4, ink);
        }
        Gl::Snap => ic(ui, &tb::MAGNET),
        Gl::Select => ic(ui, &tb::MARQUEE_2),
        Gl::Zoom => ic(ui, &tb::SEARCH),
        Gl::Undo => ic(ui, &tb::ARROW_BACK_UP),
        Gl::Normalize => {
            ui.c.line(cx - 7.0, cy, cx + 7.0, cy, 1.4, ink);
            ui.c.line(cx, cy - 6.0, cx, cy + 6.0, 1.4, ink);
            ui.c.fill_polygon(&[(cx - 7.0, cy - 3.0), (cx - 7.0, cy + 3.0), (cx - 3.0, cy)], ink);
            ui.c.fill_polygon(&[(cx + 7.0, cy - 3.0), (cx + 7.0, cy + 3.0), (cx + 3.0, cy)], ink);
        }
        Gl::FadeIn => ui.c.fill_polygon(&[(cx - 7.0, cy + 6.0), (cx + 7.0, cy - 6.0), (cx + 7.0, cy + 6.0)], ink),
        Gl::FadeOut => ui.c.fill_polygon(&[(cx - 7.0, cy - 6.0), (cx + 7.0, cy + 6.0), (cx - 7.0, cy + 6.0)], ink),
        Gl::Run => {
            ui.c.text_center(Font::new(8).bold(), Rect::new(r.x, r.y, r.w, r.h - 8.0), ink, "RUN");
            ui.c.fill_polygon(&[(cx - 4.0, cy + 4.0), (cx + 4.0, cy + 4.0), (cx, cy + 8.0)], ink);
        }
        Gl::Stretch => {
            ui.c.fill_rect(Rect::new(cx - 7.0, cy - 5.0, 5.0, 10.0), ink);
            ui.c.fill_rect(Rect::new(cx + 2.0, cy - 5.0, 5.0, 10.0), ink.with_alpha(130));
            ui.c.line(cx - 7.0, cy + 7.0, cx + 7.0, cy + 7.0, 1.2, ink);
        }
        Gl::Marker => {
            ui.c.line(cx - 3.0, cy - 7.0, cx - 3.0, cy + 7.0, 1.4, ink);
            ui.c.fill_polygon(&[(cx - 3.0, cy - 7.0), (cx + 6.0, cy - 4.0), (cx - 3.0, cy - 1.0)], ink);
        }
        Gl::AutoSlice => {
            ui.c.fill_rect(Rect::new(cx - 7.0, cy - 3.0, 14.0, 6.0), ink.with_alpha(130));
            for dx in [-3.0, 1.0, 5.0] {
                ui.c.line(cx + dx, cy - 7.0, cx + dx, cy + 7.0, 1.4, ink);
            }
        }
        Gl::Score => {
            for (dx, dy) in [(-6.0, 1.0), (-1.0, -3.0), (3.0, 2.0)] {
                ui.c.fill_rect(Rect::new(cx + dx, cy + dy, 4.0, 2.0), ink);
            }
            ui.c.fill_rect(Rect::new(cx - 7.0, cy + 6.0, 14.0, 1.0), ink.with_alpha(130));
        }
        Gl::SaveAs => ic(ui, &tb::DEVICE_FLOPPY),
        Gl::Drag => ic(ui, &tb::FILE_EXPORT),
        Gl::Scrub => {
            ui.c.stroke_circle(cx, cy, 5.5, 1.2, ink);
            ui.c.fill_circle(cx, cy, 1.6, ink);
        }
        Gl::Decks => {
            ui.c.stroke_circle(cx, cy, 5.5, 1.2, ink);
            ui.c.line(cx, cy - 5.5, cx, cy + 5.5, 1.2, ink);
        }
        Gl::WaveSpec => {
            ui.c.stroke_circle(cx, cy, 5.5, 1.2, ink);
            ui.c.line(cx - 3.0, cy, cx + 3.0, cy, 1.2, ink);
            ui.c.line(cx, cy - 3.0, cx, cy + 3.0, 1.2, ink);
        }
        Gl::ClickFree => {
            ui.c.stroke_circle(cx, cy, 5.5, 1.2, ink);
            ui.c.line(cx - 3.0, cy + 2.0, cx + 3.0, cy - 2.0, 1.2, ink);
        }
        Gl::AutoScroll => {
            ui.c.stroke_circle(cx, cy, 5.5, 1.2, ink);
            ui.c.fill_polygon(&[(cx - 2.0, cy - 3.0), (cx - 2.0, cy + 3.0), (cx + 3.0, cy)], ink);
        }
    }
}

/// A toolbar button: a dark raised face with its glyph; `menu` adds the small arrow under it.
fn icon_button(ui: &mut Ui, r: Rect, t: Target, g: Gl, menu: bool, enabled: bool, on: bool) {
    let hov = enabled && ui.hover(r, &t);
    let pressed = ui.st.pressed.as_ref() == Some(&t);
    let face = if on { Color::rgb(0x4a5675) } else if pressed { BTN_FACE.shade(-0.1) } else if hov { BTN_FACE.shade(0.16) } else { BTN_FACE };
    ui.c.fill_rrect(r, 3.0, face);
    ui.c.stroke_rrect(r.inset(0.5), 3.0, 1.0, BTN_EDGE);
    let ink = if !enabled { Color::rgb(0x5d667c) } else if hov || on { Color::rgb(0xffffff) } else { Color::rgb(0xcfd6e6) };
    glyph(ui, r, g, ink);
    if menu {
        let (x, y) = (r.cx(), r.bottom() + 5.0);
        ui.c.fill_polygon(&[(x - 3.0, y - 1.5), (x + 3.0, y - 1.5), (x, y + 1.5)], if enabled { SOFT } else { Color::rgb(0x5d667c) });
    }
    ui.hit(if menu { Rect::new(r.x, r.y, r.w, r.h + 10.0) } else { r }, t);
}

fn toolbar(ui: &mut Ui, look: &Look) {
    ui.c.fill_rect(edges(6.0, 654.0, 326.0, 370.0), Color::rgb(0x394258));
    let has = look.frames > 0;
    icon_button(ui, LOOP, Target::Ext(Ctl::Loop), Gl::Loop, false, false, false);
    icon_button(ui, PLAY, Target::Ext(Ctl::PlaySlice), Gl::Play, false, has, false);
    icon_button(ui, STOP, Target::Ext(Ctl::Stop), Gl::Stop, false, has, false);
    // The menus; the ones that do something already are lit up, the rest wait for the sample editor.
    for (i, g) in MENU_GLYPHS.iter().enumerate() {
        icon_button(ui, menu_cell(i), Target::Ext(Ctl::Menu(i as u8)), *g, true, true, false);
    }
    // The sample edit functions: auto-slice and dump score work, the others are not built yet.
    for (i, g) in EDIT_GLYPHS.iter().enumerate() {
        let enabled = match i {
            7 => has,
            8 => look.can_dump,
            _ => false,
        };
        icon_button(ui, edit_cell(i), Target::Ext(Ctl::Edit(i as u8)), *g, i == 4, enabled, false);
    }
}

/// A caption over a value, on the info strip.
fn info_field(ui: &mut Ui, x: f32, cap: &str, text: &str, w: f32) {
    ui.c.text(Font::new(8).bold(), x, 372.0, SOFT.with_alpha(180), cap);
    ui.c.text_clip(LABEL, x, 383.0, w, LIGHT, text);
}

fn value_box(ui: &mut Ui, r: Rect, t: Target, text: &str, enabled: bool) {
    let mut st = ui.state(r, &t);
    st.disabled = !enabled;
    lemon::field(ui.c, r, st);
    ui.c.text_center(SMALL.bold(), r, if !enabled { INK_DIM } else if st.hover { brand::TEXT } else { brand::MAIN }, text);
    ui.hit(r, t);
}

fn info(ui: &mut Ui, look: &Look) {
    ui.c.fill_rect(edges(6.0, 654.0, 370.0, 402.0), Color::rgb(0x333b50));
    let has = look.frames > 0;
    info_field(ui, 12.0, "SAMPLERATE", &if has { format!("{} Hz", look.rate) } else { String::new() }, 70.0);
    ui.c.text(Font::new(8).bold(), 84.0, 372.0, SOFT.with_alpha(180), "FORMAT");
    if has {
        ui.c.text(LABEL, 84.0, 383.0, LIGHT, "32");
        // Stereo: two small cassettes.
        ui.c.stroke_rrect(Rect::new(104.0, 384.0, 10.0, 8.0), 1.5, 1.0, SOFT);
    }
    ui.c.text(Font::new(8).bold(), BPM_BOX.x, 372.0, SOFT.with_alpha(180), "TEMPO");
    value_box(ui, BPM_BOX, Target::Bpm, &format!("{:.3}", look.state.bpm()), has);
    ui.c.text(Font::new(8).bold(), 196.0, 372.0, SOFT.with_alpha(180), "DECK");
    let x = look.ext;
    let (a_on, b_on) = (x.layer != 2, x.layer != 1);
    tab(ui, DECK_A, Target::Ext(Ctl::DeckToggle(0)), "A", a_on, false);
    tab(ui, DECK_B, Target::Ext(Ctl::DeckToggle(1)), "B", b_on, look.decks[1].is_empty());
    let name = if look.decks[0].is_empty() { "(no loop)".to_string() } else { look.decks[0].clone() };
    info_field(ui, 250.0, "TITLE", &name, 226.0);
    ui.c.text(Font::new(8).bold(), BEATS_BOX.x, 372.0, SOFT.with_alpha(180), "LENGTH / SEL");
    value_box(ui, BEATS_BOX, Target::Beats, &format!("{:.2}", look.state.beats()), has);
    for (i, g) in SWITCH_GLYPHS.iter().enumerate() {
        let r = switch_cell(i);
        glyph(ui, r, *g, Color::rgb(0x707a92));
        ui.hit(r, Target::Ext(Ctl::Info(i as u8)));
    }
}

/// The loop in its well: the waveform, the slices' lines (the chosen slice lit), their numbers along the top.
fn well(ui: &mut Ui, look: &Look, hint: &str) {
    ui.c.fill_rect(WELL_R, WELL);
    for i in 1..8 {
        let x = WELL_R.x + WELL_R.w * i as f32 / 8.0;
        ui.c.fill_rect(Rect::new(x.round(), WELL_R.y, 1.0, WELL_R.h), GRID.with_alpha(110));
    }
    let st = look.state;
    if look.frames == 0 || look.peaks.is_empty() {
        let msg = if st.path.is_empty() { "No loop: File > Load sample opens one".to_string() } else { format!("{} could not be read", st.path()) };
        ui.c.text_center(LABEL, WAVE, SOFT, &msg);
    } else {
        let n = st.slices.len();
        let chosen = ui.st.chosen.min(n.saturating_sub(1));
        if let Some(s) = st.slices.get(chosen) {
            let x0 = x_of(s.start, look.frames);
            let x1 = st.slices.get(chosen + 1).map_or(WAVE.right(), |s| x_of(s.start, look.frames));
            ui.c.fill_rect(edges(x0, x1.max(x0 + 1.0), WAVE.y, WAVE.bottom()), brand::MAIN.with_alpha(46));
        }
        let mid = WAVE.cy();
        let half = WAVE.h / 2.0 - 2.0;
        for (i, &(lo, hi)) in look.peaks.iter().enumerate() {
            let x = WAVE.x + i as f32;
            let (y0, y1) = ((mid - hi.clamp(-1.0, 1.0) * half).floor(), (mid - lo.clamp(-1.0, 1.0) * half).ceil());
            ui.c.fill_rect(Rect::new(x, y0, 1.0, (y1 - y0).max(1.0)), WAVE_INK);
        }
        let mut last_label = f32::NEG_INFINITY;
        for (i, s) in st.slices.iter().enumerate() {
            let x = x_of(s.start, look.frames).min(WAVE.right() - 1.0);
            let col = if i == chosen { brand::MAIN } else { SOFT.with_alpha(150) };
            ui.c.fill_rect(Rect::new(x, WAVE.y, 1.0, WAVE.h), col);
            let label = (i + 1).to_string();
            let w = ui.c.text_width(SMALL, &label);
            if x + 3.0 > last_label + 4.0 && x + 3.0 + w < WAVE.right() {
                ui.c.text(SMALL, x + 3.0, WAVE.y + 1.0, if i == chosen { brand::MAIN } else { SOFT }, &label);
                last_label = x + 3.0 + w;
            }
        }
    }
    if !hint.is_empty() {
        let (left, right) = hint.split_once('\t').unwrap_or((hint, ""));
        let s = if right.is_empty() { left.to_string() } else { format!("{left}  ({right})") };
        let tw = ui.c.text_width(SMALL, &s).min(WAVE.w - 12.0);
        let lh = ui.c.line_height(SMALL);
        let y = WAVE.bottom() - lh - 5.0;
        ui.c.fill_rrect(Rect::new(WAVE.x + 1.0, y - 2.0, tw + 10.0, lh + 4.0), 4.0, Color::rgb(0x10141a).with_alpha(190));
        ui.c.text_clip(SMALL, WAVE.x + 6.0, y, WAVE.w - 12.0, LIGHT, &s);
    }
    ui.hit(WAVE, Target::Well);
}

/// The chosen slice: its number, name, key and reverse switch.
fn slice_strip(ui: &mut Ui, look: &Look) {
    ui.c.fill_rect(edges(6.0, 654.0, 518.0, 552.0), Color::rgb(0x333b50));
    let st = look.state;
    let n = st.slices.len();
    let chosen = ui.st.chosen.min(n.saturating_sub(1));
    let slice = st.slices.get(chosen);
    let head = if n == 0 { "SLICE".to_string() } else { format!("SLICE {} OF {n}", chosen + 1) };
    ui.c.text(SMALL.bold(), 12.0, 530.0, SOFT, &head);
    lemon::field(ui.c, NAME, Kst { disabled: slice.is_none(), ..Kst::NORMAL });
    let name = slice.map(|s| s.name()).filter(|s| !s.is_empty()).unwrap_or_else(|| if slice.is_some() { "(no name)".into() } else { String::new() });
    ui.c.text_center(LABEL, NAME, if slice.is_some_and(|s| !s.name.is_empty()) { brand::TEXT } else { SOFT }, &name);
    ui.c.text(SMALL.bold(), 272.0, 530.0, SOFT, "KEY");
    let mut kst = ui.state(KEY_BOX, &Target::Key);
    kst.disabled = slice.is_none();
    lemon::field(ui.c, KEY_BOX, kst);
    if let Some(s) = slice {
        let col = if s.root >= 0 { brand::MAIN } else if kst.hover { brand::TEXT } else { SOFT };
        ui.c.text_center(SMALL.bold(), KEY_BOX, col, &key_name(st.key_of(chosen)));
    }
    ui.hit(KEY_BOX, Target::Key);
    let mut rst = ui.state(REVERSE, &Target::Rev);
    rst.on = slice.is_some_and(|s| s.reversed());
    rst.disabled = slice.is_none();
    let ink = lemon::plug_button(ui.c, REVERSE, 4.0, rst);
    ui.c.text_center(PILL, REVERSE, ink, "REVERSE");
    ui.hit(REVERSE, Target::Rev);
}

/// A key's rect: white keys in a row, black keys over the gaps. `None` outside the keyboard.
fn key_rect(k: i32) -> Option<(Rect, bool)> {
    // The white key's number within the octave for each pitch class; black keys sit after the white one before them.
    const WHITE: [i32; 12] = [0, -1, 1, -1, 2, 3, -1, 4, -1, 5, -1, 6];
    let oct = k.div_euclid(12) - KEYS_FROM.div_euclid(12);
    let n = k.rem_euclid(12) as usize;
    if oct < 0 {
        return None;
    }
    if WHITE[n] >= 0 {
        let x = 6.0 + KEY_W * (oct * 7 + WHITE[n]) as f32;
        (x + KEY_W <= 654.5).then(|| (Rect::new(x, KEYS_Y, KEY_W, KEYS_H), false))
    } else {
        let x = 6.0 + KEY_W * (oct * 7 + WHITE[n - 1] + 1) as f32 - 4.0;
        (x + 8.0 <= 654.5).then(|| (Rect::new(x, KEYS_Y, 8.0, KEYS_H * 0.62), true))
    }
}

fn keyboard(ui: &mut Ui, look: &Look) {
    ui.c.fill_rect(edges(6.0, 654.0, KEYS_Y - 2.0, KEYS_Y + KEYS_H + 2.0), Color::rgb(0x161a22));
    let st = look.state;
    let chosen = ui.st.chosen.min(st.slices.len().saturating_sub(1));
    for black in [false, true] {
        for k in KEYS_FROM..KEYS_FROM + 12 * 8 {
            let Some((r, is_black)) = key_rect(k) else { continue };
            if is_black != black {
                continue;
            }
            let t = Target::Ext(Ctl::Key(k as u8));
            let hov = ui.hover(r, &t) || ui.st.pressed.as_ref() == Some(&t);
            let face = match st.slice_for(k) {
                Some(i) if i == chosen => brand::MAIN,
                Some(_) if black => Color::rgb(0x7f8aa6),
                Some(_) => Color::rgb(0xb7c4e4),
                None if black => KEY_BLACK,
                None => KEY_WHITE,
            };
            ui.c.fill_rect(r, if hov { face.shade(0.12) } else { face });
            if !black {
                ui.c.fill_rect(Rect::new(r.right() - 1.0, r.y, 1.0, r.h), Color::rgb(0x8b94a8));
                if k % 12 == 0 {
                    ui.c.text(Font::new(8), r.x + 1.0, r.bottom() - 11.0, INK_DIM, &key_name(k));
                }
            }
            ui.hit(r, t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every region sits inside the window and the panels do not overlap each other.
    #[test]
    fn the_layout_fits_the_window() {
        let win = edges(0.0, W as f32, 0.0, H as f32);
        for r in [KEYS_BTN, PAD, MOD_PANEL, LAYER_PANEL, LOGO, PLATE_R, ART_PANEL, FILTER_PANEL, GRAPH, KNOB_BAR, LOOP, PLAY, STOP, BPM_BOX, BEATS_BOX, WELL_R, WAVE, NAME, KEY_BOX, REVERSE] {
            assert!(r.x >= 0.0 && r.right() <= win.right() && r.y >= 0.0 && r.bottom() <= win.bottom(), "{r:?}");
        }
        for (a, b) in [(MOD_PANEL, LAYER_PANEL), (LAYER_PANEL, LOGO), (PLATE_R, ART_PANEL), (ART_PANEL, FILTER_PANEL), (LOOP, PLAY), (PLAY, STOP)] {
            let apart = a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y;
            assert!(apart, "{a:?} {b:?}");
        }
        assert_eq!((key_name(60), key_name(36), key_name(61)), ("C5".into(), "C3".into(), "C#5".into()));
        // The keyboard covers the slices' keys (C5 up) inside the window.
        assert!(key_rect(60).is_some_and(|k| !k.1) && key_rect(61).is_some_and(|k| k.1));
    }

    /// The views keep their bands under the top strip: everything is the full window, no audio editor drops the
    /// editor's 218 px, no region settings drops the middle's 214, the top alone is 112, and the keyboard is 68.
    #[test]
    fn views_stack_their_bands() {
        assert_eq!(window_size(0, true), (W, H));
        assert_eq!(window_size(1, true).1, H - 228);
        assert_eq!(window_size(2, true).1, H - 214);
        assert_eq!(window_size(0, false).1, H - 70);
        assert_eq!(window_size(3, true).1, 112);
        let (bands, _) = arrange(2, true);
        assert_eq!(bands, vec![(326.0, 554.0, -214.0), (554.0, 624.0, -214.0)]);
    }
}
