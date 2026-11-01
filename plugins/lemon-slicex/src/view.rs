// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Slicex editor: painting and hit regions (platform independent), in the Lemon "Pith" plugin look on a 6 px
//! grid: the header band (the lemon mark and "SLICEX", the loop's BPM and BEATS boxes, the Load / Slicing / Dump
//! buttons), the slice well (the loop, its slices, the chosen one lit), and the cards of the chosen slice, the play
//! knobs and the switches.
//!
// spec: plugins/lemon-slicex.md §5, ui/lemon-design.md §11.1

use os_theme::design::lemon as brand;
use os_theme::Palette;
use os_ui::kit::lemon::{self, plug};
use os_ui::kit::{inst, State as Kst};
use os_ui::{Canvas, Color, Font, Rect};

use crate::ext::{Ext, Source, ALL_SOURCES, ALL_TARGETS, FILTER_MODES, LAYERS, LFO_SHAPES};
use crate::extctl::{self, Ctl, Sel};
use crate::params::{ATTACK_ID, DECAY_ID, DECLICK_ID, PITCH_ID, TO_END_ID};
use crate::state::State;

/// Editor size in logical pixels.
pub const W: u32 = 640;
pub const H: u32 = 504;

/// Every card 6 px from the editor's edges and from its neighbours.
const GAP: f32 = 6.0;
/// A rect from its left, right, top and bottom edges (right and bottom exclusive).
const fn edges(x0: f32, x1: f32, y0: f32, y1: f32) -> Rect {
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
const RIGHT: f32 = W as f32 - GAP;
const BOTTOM: f32 = H as f32 - GAP;

/// The header band, its centre line.
const HEAD: (f32, f32) = (GAP, 36.0);
const HEAD_Y: f32 = (HEAD.0 + HEAD.1) / 2.0;
pub const LOGO: Rect = edges(GAP, 132.0, HEAD.0, HEAD.1);
/// The loop's tempo and length boxes (their captions to their left).
pub const BPM_BOX: Rect = edges(180.0, 246.0, HEAD_Y - 9.0, HEAD_Y + 9.0);
pub const BEATS_BOX: Rect = edges(306.0, 360.0, HEAD_Y - 9.0, HEAD_Y + 9.0);
/// The header buttons.
pub const LOAD: Rect = edges(438.0, 492.0, HEAD_Y - 11.0, HEAD_Y + 11.0);
pub const SLICE: Rect = edges(498.0, 564.0, HEAD_Y - 11.0, HEAD_Y + 11.0);
pub const DUMP: Rect = edges(570.0, RIGHT, HEAD_Y - 11.0, HEAD_Y + 11.0);
/// The slice well and the picture inside it.
pub const WELL: Rect = edges(GAP, RIGHT, 270.0, 390.0);
pub const WAVE: Rect = edges(GAP + 6.0, RIGHT - 6.0, 276.0, 384.0);
/// The cards above the well: the modulation pad and the layering, then the articulator, its filter and the sources.
pub const MOD_CARD: Rect = edges(GAP, 180.0, 42.0, 126.0);
pub const LAYER_CARD: Rect = edges(186.0, RIGHT, 42.0, 126.0);
pub const ART_CARD: Rect = edges(GAP, 132.0, 132.0, 264.0);
pub const FILTER_CARD: Rect = edges(138.0, 318.0, 132.0, 264.0);
pub const ENV_CARD: Rect = edges(324.0, RIGHT, 132.0, 264.0);
/// The modulation pad.
pub const PAD: Rect = edges(14.0, 84.0, 62.0, 120.0);
/// The cards under the well.
const ROW: (f32, f32) = (396.0, BOTTOM);
pub const SLICE_CARD: Rect = edges(GAP, 216.0, ROW.0, ROW.1);
pub const PLAY_CARD: Rect = edges(222.0, 432.0, ROW.0, ROW.1);
pub const SWITCH_CARD: Rect = edges(438.0, RIGHT, ROW.0, ROW.1);
/// A card's title band.
const BAND: f32 = 18.0;
/// The cards' control line and the captions' top (one line for every card).
const CTRL_Y: f32 = 448.0;
const CAPTION_Y: f32 = 474.0;
/// The slice's name field.
pub const NAME: Rect = edges(GAP + 8.0, 216.0 - 8.0, ROW.0 + BAND + 6.0, ROW.0 + BAND + 24.0);
pub const KEY_BOX: Rect = edges(48.0, 108.0, CTRL_Y - 10.0, CTRL_Y + 10.0);
const REV_X: f32 = 164.0;
const KNOB_X: [f32; 3] = [270.0, 327.0, 384.0];
const SWITCH_X: [f32; 2] = [494.0, 578.0];
const KNOB_R: f32 = 17.0;

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
    /// The loop's length in frames (0 = no sample).
    pub frames: usize,
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
    t: &'a Palette,
    m: &'a dyn Model,
    st: &'a ViewState,
    hits: Vec<Hit>,
}

impl Ui<'_> {
    fn hit(&mut self, r: Rect, t: Target) {
        self.hits.push(Hit { r, t });
    }
    fn state(&self, r: Rect, t: &Target) -> Kst {
        let pressed = self.st.pressed.as_ref() == Some(t);
        let hover = !pressed && self.st.pressed.is_none() && self.st.mouse.is_some_and(|(x, y)| r.contains(x, y));
        Kst { hover, pressed, ..Kst::NORMAL }
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

/// Paints the editor. `hint` (not hosted: the host shows it otherwise) goes in the well's top-left corner.
pub fn paint(c: &mut Canvas, t: &Palette, m: &dyn Model, st: &ViewState, look: &Look, hint: &str) -> Vec<Hit> {
    let mut ui = Ui { c, t, m, st, hits: Vec::new() };
    ui.c.begin_frame(None);
    ui.c.clear(plug::BG);
    header(&mut ui, look);
    pad_card(&mut ui, look);
    layer_card(&mut ui, look);
    art_card(&mut ui, look);
    filter_card(&mut ui, look);
    env_card(&mut ui, look);
    well(&mut ui, look, hint);
    slice_card(&mut ui, look);
    titled(&mut ui, PLAY_CARD, "PLAY");
    for (i, (id, cap)) in [(PITCH_ID, "PITCH"), (ATTACK_ID, "ATTACK"), (DECAY_ID, "DECAY")].into_iter().enumerate() {
        knob(&mut ui, id, KNOB_X[i], cap, id == PITCH_ID);
    }
    titled(&mut ui, SWITCH_CARD, "OPTIONS");
    for (i, (id, cap)) in [(DECLICK_ID, "DECLICK"), (TO_END_ID, "PLAY TO END")].into_iter().enumerate() {
        let target = Target::Switch(id.into());
        let on = ui.m.norm(id) > 0.5;
        switch(&mut ui, target, SWITCH_X[i], on, cap, false);
    }
    ui.hits
}

/// A card with its title centred in a band and a line under the band.
fn titled(ui: &mut Ui, r: Rect, title: &str) {
    lemon::plug_panel(ui.c, r);
    ui.c.fill_rect(Rect::new(r.x + 1.0, r.y + BAND, r.w - 2.0, 1.0), plug::PANEL_LINE);
    ui.c.text_center(SMALL.bold(), Rect::new(r.x, r.y, r.w, BAND), plug::DIM, title);
}

fn header(ui: &mut Ui, look: &Look) {
    // The emblem: the lemon mark and "SLICEX" on one line.
    let f = Font::condensed(18, 0).bold();
    let s: f32 = 6.0;
    let (wl, gap) = (2.2 * s, (s * 1.3).round());
    let x = LOGO.x + 4.0;
    let lh = ui.c.line_height(f);
    lemon::lemon_mark(ui.c, x + wl / 2.0, HEAD_Y, s, brand::MAIN);
    lemon::spaced(ui.c, f, x + wl + gap, (HEAD_Y - lh / 2.0).round(), plug::WORDMARK, "SLICEX", 3.0);
    ui.hit(LOGO, Target::Logo);
    let st = look.state;
    let has = look.frames > 0;
    value_box(ui, BPM_BOX, Target::Bpm, "BPM", &format!("{:.3}", st.bpm()), has);
    value_box(ui, BEATS_BOX, Target::Beats, "BEATS", &format!("{:.2}", st.beats()), has);
    button(ui, LOAD, Target::Load, "LOAD", false);
    button(ui, SLICE, Target::Slicing, "SLICING", !has);
    button(ui, DUMP, Target::Dump, "DUMP", !look.can_dump);
}

/// A value box with its caption to its left.
fn value_box(ui: &mut Ui, r: Rect, t: Target, caption: &str, text: &str, enabled: bool) {
    let w = ui.c.text_width(SMALL.bold(), caption);
    let lh = ui.c.line_height(SMALL.bold());
    ui.c.text(SMALL.bold(), (r.x - 6.0 - w).round(), (r.cy() - lh / 2.0).round(), plug::DIM, caption);
    let mut st = ui.state(r, &t);
    st.disabled = !enabled;
    lemon::field(ui.c, r, st);
    ui.c.text_center(SMALL.bold(), r, if !enabled { plug::OFF } else if st.hover { brand::TEXT } else { brand::MAIN }, text);
    ui.hit(r, t);
}

/// A header button with its name inside and a small arrow when it opens a menu.
fn button(ui: &mut Ui, r: Rect, t: Target, label: &str, disabled: bool) {
    let mut st = ui.state(r, &t);
    st.disabled = disabled;
    let ink = lemon::plug_button(ui.c, r, 6.0, st);
    let menu = t != Target::Load;
    let tr = if menu { Rect::new(r.x, r.y, r.w - 8.0, r.h) } else { r };
    ui.c.text_center(PILL, tr, ink, label);
    if menu {
        let (x, y) = (r.right() - 10.0, r.cy());
        ui.c.fill_polygon(&[(x - 3.0, y - 1.5), (x + 3.0, y - 1.5), (x, y + 2.0)], ink);
    }
    ui.hit(r, t);
}

/// The loop in its well: the waveform, the slices' lines (the chosen slice lit), their numbers along the top.
fn well(ui: &mut Ui, look: &Look, hint: &str) {
    lemon::plug_well(ui.c, WELL, 8.0);
    let st = look.state;
    if look.frames == 0 || look.peaks.is_empty() {
        let msg = if st.path.is_empty() { "No loop: LOAD opens one".to_string() } else { format!("{} could not be read", st.path()) };
        ui.c.text_center(LABEL, WAVE, plug::FAINT, &msg);
    } else {
        let n = st.slices.len();
        let chosen = ui.st.chosen.min(n.saturating_sub(1));
        if let Some(s) = st.slices.get(chosen) {
            let x0 = x_of(s.start, look.frames);
            let x1 = st.slices.get(chosen + 1).map_or(WAVE.right(), |s| x_of(s.start, look.frames));
            ui.c.fill_rect(edges(x0, x1.max(x0 + 1.0), WAVE.y, WAVE.bottom()), brand::MAIN.with_alpha(36));
        }
        let mid = WAVE.cy();
        let half = WAVE.h / 2.0 - 2.0;
        for (i, &(lo, hi)) in look.peaks.iter().enumerate() {
            let x = WAVE.x + i as f32;
            let (y0, y1) = ((mid - hi.clamp(-1.0, 1.0) * half).floor(), (mid - lo.clamp(-1.0, 1.0) * half).ceil());
            ui.c.fill_rect(Rect::new(x, y0, 1.0, (y1 - y0).max(1.0)), plug::DIM.with_alpha(200));
        }
        let mut last_label = f32::NEG_INFINITY;
        for (i, s) in st.slices.iter().enumerate() {
            let x = x_of(s.start, look.frames).min(WAVE.right() - 1.0);
            let col = if i == chosen { brand::MAIN } else { plug::FAINT };
            ui.c.fill_rect(Rect::new(x, WAVE.y, 1.0, WAVE.h), col);
            let label = (i + 1).to_string();
            let w = ui.c.text_width(SMALL, &label);
            if x + 3.0 > last_label + 4.0 && x + 3.0 + w < WAVE.right() {
                ui.c.text(SMALL, x + 3.0, WAVE.y + 1.0, if i == chosen { brand::MAIN } else { plug::FAINT }, &label);
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
        ui.c.fill_rrect(Rect::new(WAVE.x + 1.0, y - 2.0, tw + 10.0, lh + 4.0), 4.0, Color::rgb(0x1a1a1a).with_alpha(170));
        ui.c.text_clip(SMALL, WAVE.x + 6.0, y, WAVE.w - 12.0, brand::TEXT, &s);
    }
    ui.hit(WAVE, Target::Well);
}

/// The chosen slice: its number and name, its key and its reverse switch.
fn slice_card(ui: &mut Ui, look: &Look) {
    let st = look.state;
    let n = st.slices.len();
    let chosen = ui.st.chosen.min(n.saturating_sub(1));
    let title = if n == 0 { "SLICE".to_string() } else { format!("SLICE {} OF {n}", chosen + 1) };
    titled(ui, SLICE_CARD, &title);
    let slice = st.slices.get(chosen);
    lemon::field(ui.c, NAME, Kst { disabled: slice.is_none(), ..Kst::NORMAL });
    let name = slice.map(|s| s.name()).filter(|s| !s.is_empty()).unwrap_or_else(|| if slice.is_some() { "(no name)".into() } else { String::new() });
    ui.c.text_center(LABEL, NAME, if slice.is_some_and(|s| !s.name.is_empty()) { brand::TEXT } else { plug::FAINT }, &name);
    // The key box: the key it plays on, lemon when set apart from its own.
    let mut kst = ui.state(KEY_BOX, &Target::Key);
    kst.disabled = slice.is_none();
    lemon::field(ui.c, KEY_BOX, kst);
    if let Some(s) = slice {
        let col = if s.root >= 0 { brand::MAIN } else if kst.hover { brand::TEXT } else { plug::DIM };
        ui.c.text_center(SMALL.bold(), KEY_BOX, col, &key_name(st.key_of(chosen)));
    }
    inst::caption(ui.c, ui.t, KEY_BOX.cx(), CAPTION_Y, "KEY", slice.is_none());
    ui.hit(KEY_BOX, Target::Key);
    switch(ui, Target::Rev, REV_X, slice.is_some_and(|s| s.reversed()), "REVERSE", slice.is_none());
}

/// A knob (hit area 40 px across), caption under it.
fn knob(ui: &mut Ui, id: &str, cx: f32, caption: &str, bipolar: bool) {
    let target = Target::Knob(id.into());
    let hr = Rect::new(cx - KNOB_R - 3.0, CTRL_Y - KNOB_R - 3.0, 2.0 * KNOB_R + 6.0, 2.0 * KNOB_R + 6.0);
    let st = ui.state(hr, &target);
    lemon::dial(ui.c, cx, CTRL_Y, KNOB_R, 2.8, ui.m.norm(id), bipolar, brand::MAIN, st);
    inst::caption(ui.c, ui.t, cx, CAPTION_Y, caption, false);
    ui.hit(hr, target);
}

/// A switch: a raised button lit lemon when on ("ON" / "OFF" inside), the caption under it.
fn switch(ui: &mut Ui, target: Target, cx: f32, on: bool, caption: &str, disabled: bool) {
    let r = Rect::new(cx - 20.0, CTRL_Y - 10.0, 40.0, 20.0);
    let hr = Rect::new(cx - 26.0, CTRL_Y - 14.0, 52.0, 28.0);
    let mut st = ui.state(hr, &target);
    st.on = on;
    st.disabled = disabled;
    let ink = lemon::plug_button(ui.c, r, 6.0, st);
    ui.c.text_center(PILL, r, ink, if on { "ON" } else { "OFF" });
    inst::caption(ui.c, ui.t, cx, CAPTION_Y, caption, disabled);
    ui.hit(hr, target);
}


// ---------------------------------------------------------------------------------------------------------------
// What Slicex adds: the modulation pad, the layering, the articulators, their filter and the sources.

/// A small button with its label, lit when on.
fn small(ui: &mut Ui, r: Rect, c: Ctl, label: &str, on: bool, disabled: bool) {
    let t = Target::Ext(c);
    let mut st = ui.state(r, &t);
    st.on = on;
    st.disabled = disabled;
    let ink = lemon::plug_button(ui.c, r, 4.0, st);
    ui.c.text_center(PILL, r, ink, label);
    ui.hit(r, t);
}

/// A knob of the added panels with its caption under it.
fn ext_knob(ui: &mut Ui, look: &Look, c: Ctl, cx: f32, cy: f32, r: f32, cap_y: f32, bipolar: bool) {
    let target = Target::Ext(c);
    let hr = Rect::new(cx - r - 3.0, cy - r - 3.0, 2.0 * r + 6.0, 2.0 * r + 6.0);
    let st = ui.state(hr, &target);
    lemon::dial(ui.c, cx, cy, r, 2.8, extctl::get(look.ext, &ui.st.sel, c), bipolar, brand::MAIN, st);
    inst::caption(ui.c, ui.t, cx, cap_y, c.caption(), false);
    ui.hit(hr, target);
}

fn pad_card(ui: &mut Ui, look: &Look) {
    titled(ui, MOD_CARD, "MODULATION");
    lemon::plug_well(ui.c, PAD, 4.0);
    ui.c.fill_rect(Rect::new(PAD.cx(), PAD.y + 3.0, 1.0, PAD.h - 6.0), plug::PANEL_LINE);
    ui.c.fill_rect(Rect::new(PAD.x + 3.0, PAD.cy(), PAD.w - 6.0, 1.0), plug::PANEL_LINE);
    let x = &look.ext;
    let (px, py) = (PAD.x + 4.0 + x.mod_x * (PAD.w - 8.0), PAD.bottom() - 4.0 - x.mod_y * (PAD.h - 8.0));
    ui.c.fill_circle(px, py, 4.0, brand::MAIN);
    ui.hit(PAD, Target::Ext(Ctl::Pad));
    ui.c.text(SMALL.bold(), 92.0, 64.0, plug::DIM, &format!("X {:.0}%", x.mod_x * 100.0));
    ui.c.text(SMALL.bold(), 92.0, 78.0, plug::DIM, &format!("Y {:.0}%", x.mod_y * 100.0));
    small(ui, edges(92.0, 170.0, 98.0, 116.0), Ctl::Smooth, "SMOOTH", x.smooth, false);
}

fn layer_card(ui: &mut Ui, look: &Look) {
    titled(ui, LAYER_CARD, "LAYERING");
    let x = look.ext;
    for (i, name) in LAYERS.iter().enumerate() {
        let r = edges(194.0 + 52.0 * i as f32, 240.0 + 52.0 * i as f32, 66.0, 84.0);
        small(ui, r, Ctl::Layer(i as u8), &name.to_uppercase(), x.layer == i as u8, false);
    }
    small(ui, edges(362.0, 442.0, 66.0, 84.0), Ctl::SameDeck, "SAME DECK", x.same_deck, false);
    small(ui, edges(448.0, 528.0, 66.0, 84.0), Ctl::Crossfade, "CROSSFADE", x.crossfade, false);
    for (i, d) in look.decks.iter().enumerate() {
        let name = if d.is_empty() { "(empty)" } else { d.as_str() };
        let y = 94.0 + 14.0 * i as f32;
        ui.c.text(SMALL.bold(), 194.0, y, plug::DIM, if i == 0 { "DECK A" } else { "DECK B" });
        ui.c.text_clip(SMALL, 244.0, y, LAYER_CARD.right() - 252.0, if d.is_empty() { plug::FAINT } else { brand::TEXT }, name);
    }
}

fn art_card(ui: &mut Ui, look: &Look) {
    titled(ui, ART_CARD, "ARTICULATOR");
    let sel = ui.st.sel;
    let st = look.state;
    let chosen = ui.st.chosen.min(st.slices.len().saturating_sub(1));
    let mine = (!st.slices.is_empty()).then(|| look.ext.art_of(chosen));
    for i in 0..8usize {
        let (col, row) = ((i % 4) as f32, (i / 4) as f32);
        let r = edges(12.0 + 28.0 * col, 36.0 + 28.0 * col, 156.0 + 30.0 * row, 180.0 + 30.0 * row);
        small(ui, r, Ctl::Art(i), &(i + 1).to_string(), sel.art == i, false);
        if mine == Some(i) {
            ui.c.fill_circle(r.right() - 4.0, r.y + 4.0, 2.5, brand::MAIN);
        }
    }
    let line = match mine {
        Some(a) => format!("Slice {} plays through {}", chosen + 1, a + 1),
        None => "Load a loop to assign slices".to_string(),
    };
    ui.c.text_clip(SMALL, ART_CARD.x + 8.0, 222.0, ART_CARD.w - 16.0, plug::DIM, &line);
    ui.c.text_clip(SMALL, ART_CARD.x + 8.0, 236.0, ART_CARD.w - 16.0, plug::FAINT, "Pick a number to assign it");
}

fn filter_card(ui: &mut Ui, look: &Look) {
    titled(ui, FILTER_CARD, "FILTER");
    let sel = ui.st.sel;
    let a = &look.ext.artic[sel.art];
    for (i, name) in FILTER_MODES.iter().enumerate() {
        let r = edges(144.0 + 34.0 * i as f32, 174.0 + 34.0 * i as f32, 154.0, 172.0);
        small(ui, r, Ctl::Mode(i as u8), name, a.mode == i as u8, false);
    }
    let off = a.mode >= crate::ext::FILTER_OFF;
    for i in 0..3u8 {
        let r = edges(144.0 + 34.0 * f32::from(i), 174.0 + 34.0 * f32::from(i), 178.0, 196.0);
        small(ui, r, Ctl::Slope(i + 1), &format!("x{}", i + 1), a.slope == i + 1 && !off, off);
    }
    for (i, c) in [Ctl::CutEnv, Ctl::Cut, Ctl::Res].into_iter().enumerate() {
        ext_knob(ui, look, c, 168.0 + 60.0 * i as f32, 228.0, 16.0, 250.0, c == Ctl::CutEnv);
    }
}

/// The envelope's curve for the panel's graph: time across, level up.
fn env_curve(ui: &mut Ui, g: Rect, e: &crate::ext::Env, amount: f32) {
    let (att, dec, rel) = e.seconds(0.5);
    let hold = (att + dec * 1.3).max(0.05);
    let total = hold + rel.max(0.02) + hold * 0.15;
    let at = |t: f64| g.x + 4.0 + (t / total) as f32 * (g.w - 8.0);
    let ay = |v: f32| g.bottom() - 4.0 - v.clamp(0.0, 1.0) * (g.h - 8.0);
    let col = if amount == 0.0 { plug::FAINT } else { brand::MAIN };
    let n = 48;
    let mut last = (at(0.0), ay(0.0));
    for i in 1..=n {
        let t = hold * f64::from(i) / f64::from(n);
        let p = (at(t), ay(e.level(t, 0.5)));
        ui.c.line(last.0, last.1, p.0, p.1, 1.6, col);
        last = p;
    }
    let end = (at(hold + rel.max(0.02)), ay(0.0));
    ui.c.line(last.0, last.1, end.0, end.1, 1.6, col);
}

fn env_card(ui: &mut Ui, look: &Look) {
    let sel = ui.st.sel;
    let a = &look.ext.artic[sel.art];
    titled(ui, ENV_CARD, &format!("ARTICULATOR {}  -  {} / {}", sel.art + 1, sel.tgt.name(), sel.src.name()));
    for (i, tg) in ALL_TARGETS.iter().enumerate() {
        let x = 330.0 + 49.0 * i as f32;
        small(ui, edges(x, x + 46.0, 152.0, 168.0), Ctl::Tgt(i), tg.name(), sel.tgt == *tg, false);
    }
    for (i, s) in ALL_SOURCES.iter().enumerate() {
        let x = 330.0 + 49.0 * i as f32;
        let used = a.amount(sel.tgt, *s) != 0.0;
        let r = edges(x, x + 46.0, 172.0, 188.0);
        small(ui, r, Ctl::Src(i), s.name(), sel.src == *s, false);
        if used && sel.src != *s {
            ui.c.fill_circle(r.right() - 4.0, r.y + 4.0, 2.0, brand::MAIN);
        }
    }
    let g = edges(330.0, 440.0, 194.0, 258.0);
    lemon::plug_well(ui.c, g, 4.0);
    let amount = a.amount(sel.tgt, sel.src);
    let t = sel.tgt.index();
    let xs = |k: usize| 458.0 + 34.0 * k as f32;
    match sel.src {
        Source::Env => {
            env_curve(ui, g, &a.env[t], amount);
            for (k, c) in [Ctl::Att, Ctl::Dec, Ctl::Sus, Ctl::Rel, Ctl::Amt].into_iter().enumerate() {
                ext_knob(ui, look, c, xs(k), 222.0, 11.0, 244.0, c == Ctl::Amt);
            }
            small(ui, edges(400.0, 438.0, 244.0, 256.0), Ctl::Tempo, "TEMPO", a.env[t].tempo, false);
        }
        Source::Lfo => {
            let l = a.lfo[t];
            let col = if amount == 0.0 { plug::FAINT } else { brand::MAIN };
            let probe = crate::ext::Lfo { shape: l.shape, rate: 1.0, tempo: false };
            let mut last = (g.x + 4.0, g.cy());
            for i in 0..=60 {
                let ph = f64::from(i) / 60.0 * 2.0;
                let v = probe.value(ph, 0.5, 0.37);
                let p = (g.x + 4.0 + (i as f32 / 60.0) * (g.w - 8.0), g.cy() - v * (g.h / 2.0 - 8.0));
                if i > 0 {
                    ui.c.line(last.0, last.1, p.0, p.1, 1.6, col);
                }
                last = p;
            }
            ui.c.text(SMALL.bold(), g.x + 6.0, g.y + 3.0, plug::DIM, LFO_SHAPES[usize::from(l.shape).min(4)]);
            ui.hit(g, Target::Ext(Ctl::LfoShape(0)));
            ext_knob(ui, look, Ctl::LfoRate, xs(0), 222.0, 11.0, 244.0, false);
            ext_knob(ui, look, Ctl::Amt, xs(1), 222.0, 11.0, 244.0, true);
            small(ui, edges(400.0, 438.0, 244.0, 256.0), Ctl::Tempo, "TEMPO", l.tempo, false);
        }
        other => {
            let what = match other {
                Source::Vel => "The note's velocity",
                Source::ModX => "The pad's X",
                Source::ModY => "The pad's Y",
                _ => "A number picked at every note",
            };
            ui.c.text_clip(SMALL, g.x + 6.0, g.y + 6.0, g.w - 12.0, plug::DIM, what);
            ui.c.text_clip(SMALL, g.x + 6.0, g.y + 22.0, g.w - 12.0, plug::DIM, &format!("moves {}", sel.tgt.name()));
            ui.c.text_clip(SMALL.bold(), g.x + 6.0, g.y + 40.0, g.w - 12.0, if amount == 0.0 { plug::FAINT } else { brand::MAIN }, &extctl::text(look.ext, &sel, Ctl::Amt));
            ext_knob(ui, look, Ctl::Amt, xs(0), 222.0, 11.0, 244.0, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every card and button starts on the 6 px grid, inside the editor, 6 px from its neighbours.
    #[test]
    fn the_layout_keeps_its_grid() {
        for r in [LOGO, WELL, SLICE_CARD, PLAY_CARD, SWITCH_CARD, LOAD, SLICE, DUMP, MOD_CARD, LAYER_CARD, ART_CARD, FILTER_CARD, ENV_CARD] {
            assert!(r.x >= GAP && r.right() <= RIGHT && r.y >= GAP && r.bottom() <= BOTTOM, "{r:?}");
            assert_eq!(r.x % 6.0, 0.0, "{r:?}");
        }
        for (a, b) in [(SLICE_CARD, PLAY_CARD), (PLAY_CARD, SWITCH_CARD), (LOAD, SLICE), (SLICE, DUMP)] {
            assert_eq!(b.x - a.right(), GAP, "{a:?} {b:?}");
        }
        assert_eq!(SLICE_CARD.y - WELL.bottom(), GAP);
        assert_eq!(MOD_CARD.y - HEAD.1, GAP);
        assert_eq!(WELL.y - ART_CARD.bottom(), GAP);
        for (a, b) in [(MOD_CARD, LAYER_CARD), (ART_CARD, FILTER_CARD), (FILTER_CARD, ENV_CARD)] {
            assert_eq!(b.x - a.right(), GAP, "{a:?} {b:?}");
        }
        assert_eq!((key_name(60), key_name(36), key_name(61)), ("C5".into(), "C3".into(), "C#5".into()));
    }
}
