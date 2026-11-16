// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Granulizer editor: painting and hit regions (platform independent), in the Lemon "Pith" plugin look on a
//! 6 px grid: the header band (the lemon mark and "GRAN", the root key, the reverse switch, Load and the sample's
//! name), the well (the sample's waveform; a click plays it), and the cards of the grain cloud and the play knobs.
//!
// spec: plugins/lemon-granulizer.md §3, ui/lemon-design.md §11.1

use os_theme::design::lemon as brand;
use os_theme::Palette;
use os_ui::kit::lemon::{self, plug};
use os_ui::kit::{inst, State as Kst};
use os_ui::{Canvas, Color, Font, Rect};

use crate::params::{ATTACK_ID, GRAIN_ID, HOLD_ID, PAN_ID, PITCH_ID, RANDOM_ID, RELEASE_ID, SPACING_ID, State, REVERSE_ID};

/// Editor size in logical pixels.
pub const W: u32 = 640;
pub const H: u32 = 300;

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
/// The root key box (its caption to its left), the reverse switch, Load and the sample's name.
pub const ROOT_BOX: Rect = edges(180.0, 264.0, HEAD_Y - 11.0, HEAD_Y + 11.0);
pub const REV: Rect = edges(294.0, 342.0, HEAD_Y - 11.0, HEAD_Y + 11.0);
pub const LOAD: Rect = edges(372.0, 426.0, HEAD_Y - 11.0, HEAD_Y + 11.0);
pub const NAME: Rect = edges(456.0, RIGHT, HEAD_Y - 11.0, HEAD_Y + 11.0);
/// The well and the picture inside it.
pub const WELL: Rect = edges(GAP, RIGHT, HEAD.1 + GAP, 186.0);
pub const WAVE: Rect = edges(GAP + 6.0, RIGHT - 6.0, HEAD.1 + GAP + 6.0, 180.0);
/// The cards under the well.
const ROW: (f32, f32) = (192.0, BOTTOM);
pub const GRAIN_CARD: Rect = edges(GAP, 432.0, ROW.0, ROW.1);
pub const PLAY_CARD: Rect = edges(438.0, RIGHT, ROW.0, ROW.1);
/// A card's title band.
const BAND: f32 = 18.0;
/// The cards' control line and the captions' top (one line for every card).
const CTRL_Y: f32 = 244.0;
const CAPTION_Y: f32 = 270.0;
const GRAIN_X: [f32; 5] = [48.0, 132.0, 216.0, 300.0, 384.0];
const PLAY_X: [f32; 3] = [470.0, 534.0, 598.0];
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
    /// The root key (a menu).
    Root,
    Load,
    /// The well: a click plays the sample on the root key.
    Well,
    Logo,
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub r: Rect,
    pub t: Target,
}

/// The pointer and the pressed control.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    pub mouse: Option<(f32, f32)>,
    pub pressed: Option<Target>,
}

/// What a paint shows besides the parameters.
pub struct Look<'a> {
    pub state: &'a State,
    /// The sample's length in frames (0 = no sample).
    pub frames: usize,
    /// The waveform's low and high per column of `WAVE` (empty = no sample).
    pub peaks: &'a [(f32, f32)],
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

/// The frame of the sample at x in the well.
pub fn frame_at(x: f32, frames: usize) -> usize {
    (((x - WAVE.x) / WAVE.w).clamp(0.0, 1.0) as f64 * frames as f64) as usize
}

/// Paints the editor. `hint` (not hosted: the host shows it otherwise) goes in the well's top-left corner.
pub fn paint(c: &mut Canvas, t: &Palette, m: &dyn Model, st: &ViewState, look: &Look, hint: &str) -> Vec<Hit> {
    let mut ui = Ui { c, t, m, st, hits: Vec::new() };
    ui.c.begin_frame(None);
    ui.c.clear(plug::BG);
    header(&mut ui, look);
    well(&mut ui, look, hint);
    titled(&mut ui, GRAIN_CARD, "GRAIN CLOUD");
    for (i, (id, cap)) in [(GRAIN_ID, "GRAIN"), (SPACING_ID, "SPACING"), (HOLD_ID, "HOLD"), (RANDOM_ID, "RANDOM"), (PAN_ID, "PAN")].into_iter().enumerate() {
        knob(&mut ui, id, GRAIN_X[i], cap, false);
    }
    titled(&mut ui, PLAY_CARD, "PLAY");
    for (i, (id, cap, bipolar)) in [(PITCH_ID, "PITCH", true), (ATTACK_ID, "ATTACK", false), (RELEASE_ID, "RELEASE", false)].into_iter().enumerate() {
        knob(&mut ui, id, PLAY_X[i], cap, bipolar);
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
    // The emblem: the lemon mark and "GRAN" on one line.
    let f = Font::condensed(18, 0).bold();
    let s: f32 = 6.0;
    let (wl, gap) = (2.2 * s, (s * 1.3).round());
    let x = LOGO.x + 4.0;
    let lh = ui.c.line_height(f);
    lemon::lemon_mark(ui.c, x + wl / 2.0, HEAD_Y, s, brand::MAIN);
    lemon::spaced(ui.c, f, x + wl + gap, (HEAD_Y - lh / 2.0).round(), plug::WORDMARK, "GRAN", 3.0);
    ui.hit(LOGO, Target::Logo);
    // The root key, its caption to its left.
    let root_text = key_name(look.state.root);
    let cap = "ROOT";
    let cw = ui.c.text_width(SMALL.bold(), cap);
    let lh = ui.c.line_height(SMALL.bold());
    ui.c.text(SMALL.bold(), (ROOT_BOX.x - 6.0 - cw).round(), (HEAD_Y - lh / 2.0).round(), plug::DIM, cap);
    let t = Target::Root;
    let kst = ui.state(ROOT_BOX, &t);
    lemon::field(ui.c, ROOT_BOX, kst);
    ui.c.text_center(SMALL.bold(), ROOT_BOX, if kst.hover { brand::TEXT } else { brand::MAIN }, &root_text);
    ui.hit(ROOT_BOX, t);
    switch(ui, REVERSE_ID, "REV");
    button(ui, LOAD, Target::Load, "LOAD", false);
    // The sample's name.
    let has = look.frames > 0;
    let file = if has { file_name(&look.state.path).to_string() } else { String::new() };
    lemon::field(ui.c, NAME, Kst { disabled: !has, ..Kst::NORMAL });
    ui.c.text_clip(LABEL, NAME.x + 6.0, NAME.cy() - 7.0, NAME.w - 12.0, if has { brand::TEXT } else { plug::FAINT }, &file);
}

/// A header button with its name inside.
fn button(ui: &mut Ui, r: Rect, t: Target, label: &str, disabled: bool) {
    let mut st = ui.state(r, &t);
    st.disabled = disabled;
    let ink = lemon::plug_button(ui.c, r, 6.0, st);
    ui.c.text_center(PILL, r, ink, label);
    ui.hit(r, t);
}

/// The reverse switch in the header.
fn switch(ui: &mut Ui, id: &str, label: &str) {
    let target = Target::Switch(id.into());
    let mut st = ui.state(REV, &target);
    st.on = ui.m.norm(id) > 0.5;
    let ink = lemon::plug_button(ui.c, REV, 6.0, st);
    ui.c.text_center(PILL, REV, ink, label);
    ui.hit(REV, target);
}

/// The sample in its well: the waveform; a click plays it on the root key.
fn well(ui: &mut Ui, look: &Look, hint: &str) {
    lemon::plug_well(ui.c, WELL, 8.0);
    if look.frames == 0 || look.peaks.is_empty() {
        let msg = if look.state.path.is_empty() { "No sample: LOAD opens one".to_string() } else { format!("{} could not be read", look.state.path) };
        ui.c.text_center(LABEL, WAVE, plug::FAINT, &msg);
    } else {
        let mid = WAVE.cy();
        let half = WAVE.h / 2.0 - 2.0;
        for (i, &(lo, hi)) in look.peaks.iter().enumerate() {
            let x = WAVE.x + i as f32;
            let (y0, y1) = ((mid - hi.clamp(-1.0, 1.0) * half).floor(), (mid - lo.clamp(-1.0, 1.0) * half).ceil());
            ui.c.fill_rect(Rect::new(x, y0, 1.0, (y1 - y0).max(1.0)), plug::DIM.with_alpha(200));
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

/// The file name of a stored path.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every card and button starts on the 6 px grid, inside the editor, 6 px from its neighbours.
    #[test]
    fn the_layout_keeps_its_grid() {
        assert_eq!(H % 6, 0);
        for r in [LOGO, ROOT_BOX, REV, LOAD, NAME, WELL, GRAIN_CARD, PLAY_CARD] {
            assert!(r.x >= GAP && r.right() <= RIGHT && r.y >= GAP && r.bottom() <= BOTTOM, "{r:?}");
            assert_eq!(r.x % 6.0, 0.0, "{r:?}");
        }
        for (a, b) in [(REV, LOAD), (LOAD, NAME)] {
            let gap = b.x - a.right();
            assert!(gap >= GAP && gap <= 42.0, "the header keeps its own wider gaps: {gap}");
        }
        assert_eq!(PLAY_CARD.x - GRAIN_CARD.right(), GAP);
        assert_eq!(WELL.y - HEAD.1, GAP);
        assert_eq!(GRAIN_CARD.y - WELL.bottom(), GAP);
        // The knobs sit inside their cards with a margin.
        for x in GRAIN_X {
            assert!(x - KNOB_R - 3.0 >= GRAIN_CARD.x + 4.0 && x + KNOB_R + 3.0 <= GRAIN_CARD.right() - 4.0, "{x}");
        }
        for x in PLAY_X {
            assert!(x - KNOB_R - 3.0 >= PLAY_CARD.x + 4.0 && x + KNOB_R + 3.0 <= PLAY_CARD.right() - 4.0, "{x}");
        }
        assert_eq!((key_name(60), key_name(36), key_name(61)), ("C5".into(), "C3".into(), "C#5".into()));
    }
}
