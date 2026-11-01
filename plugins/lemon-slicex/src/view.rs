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

use crate::ext::{Ext, Source, ALL_SOURCES, ALL_TARGETS, FILTER_MODES, LAYERS};
use crate::extctl::{self, Ctl, Sel};
use crate::params::PITCH_ID;
use crate::state::State;

/// Editor size in logical pixels.
pub const W: u32 = 660;
pub const H: u32 = 628;

/// A rect from its left, right, top and bottom edges (right and bottom exclusive).
const fn edges(x0: f32, x1: f32, y0: f32, y1: f32) -> Rect {
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

// The colours of the original's window, in Mango's orange.
const FRAME: Color = Color::rgb(0x2f3747);
const STRIP: Color = Color::rgb(0x2a2d33);
const STRIP_PANEL: Color = Color::rgb(0x33363d);
const PLATE: Color = Color::rgb(0x8a909a);
const PLATE_DARK: Color = Color::rgb(0x777e8a);
const TAB_OFF: Color = Color::rgb(0x7a8596);
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
const SLOT: Color = Color::rgb(0x17191d);

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
pub const GRAPH: Rect = edges(324.0, 652.0, 148.0, 282.0);
const KNOB_BAR: Rect = edges(322.0, 654.0, 286.0, 322.0);
/// The toolbar and the info strip under the middle panel, and the wave.
pub const BPM_BOX: Rect = edges(130.0, 188.0, 372.0, 397.0);
pub const BEATS_BOX: Rect = edges(486.0, 540.0, 372.0, 397.0);
const DECK_A: Rect = edges(196.0, 214.0, 382.0, 398.0);
const DECK_B: Rect = edges(216.0, 234.0, 382.0, 398.0);
/// The wave panel: the scroll bar row, the wave, the row of switches (with the chosen slice's name, key and reverse
/// switch between them) and the marker lane.
const SCROLL_R: Rect = edges(6.0, 654.0, 402.0, 416.0);
pub const WELL_R: Rect = edges(6.0, 654.0, 416.0, 512.0);
pub const WAVE: Rect = edges(12.0, 648.0, 420.0, 508.0);
const ROW_R: Rect = edges(6.0, 654.0, 512.0, 530.0);
const LANE: Rect = edges(6.0, 654.0, 530.0, 562.0);
/// The chosen slice's name, key and reverse switch sit in the logo plate.
pub const NAME: Rect = edges(14.0, 148.0, 274.0, 290.0);
pub const KEY_BOX: Rect = edges(40.0, 84.0, 296.0, 314.0);
const REVERSE: Rect = edges(92.0, 148.0, 296.0, 314.0);
/// The keyboard: every key from 0 to 127 as small rounded squares, the black keys in the upper row, the white in the lower.
const KEYS_Y: f32 = 570.0;
const KEY_X0: f32 = 34.0;
const KEY_W: f32 = 8.15;
const BLACK_H: f32 = 18.0;
const WHITE_Y: f32 = 598.0;
const WHITE_H: f32 = 22.0;

/// The window's bands below the top strip (from, to), in the full layout: the middle panel, the audio editor and the
/// keyboard.
const BANDS: [(f32, f32); 3] = [(112.0, 326.0), (326.0, 562.0), (562.0, 628.0)];
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
    middle_frame(&mut ui);
    plate(&mut ui, look);
    art_panel(&mut ui, look);
    filter_panel(&mut ui, look);
    tabs(&mut ui, look);
    graph(&mut ui, look);
    toolbar(&mut ui, look);
    info(&mut ui, look);
    well(&mut ui, look, hint);
    scroll_bar(&mut ui);
    switches_row(&mut ui, look);
    lane(&mut ui, look);
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
    ui.c.fill_rrect(edges(526.0, 652.0, 8.0, 30.0), 4.0, Color::rgb(0x1f2227));
    let glyph = |ui: &mut Ui, r: Rect, rows: [bool; 3], ink: Color| {
        for (i, on) in rows.into_iter().enumerate() {
            let h = [3.0, 4.0, 3.0][i];
            let y = r.y + 4.0 + [0.0f32, 5.0, 11.0][i];
            ui.c.fill_rect(Rect::new(r.x + 5.0, y, r.w - 10.0, h), if on { ink } else { ink.with_alpha(60) });
        }
    };
    let keys = Target::Ext(Ctl::Keys);
    let hov = ui.hover(KEYS_BTN, &keys);
    let face = if x.keys { TAB_ON } else if hov { Color::rgb(0x42474f) } else { Color::rgb(0x32363c) };
    ui.c.fill_rrect(KEYS_BTN, 3.0, face);
    for i in 0..4 {
        let kx = KEYS_BTN.x + 4.0 + 4.0 * i as f32;
        ui.c.fill_rect(Rect::new(kx, KEYS_BTN.y + 4.0, 2.0, 10.0), if x.keys { INK } else { SOFT });
    }
    ui.hit(KEYS_BTN, keys);
    for i in 0..4u8 {
        let r = edges(553.0 + 24.0 * f32::from(i), 575.0 + 24.0 * f32::from(i), 10.0, 28.0);
        let t = Target::Ext(Ctl::View(i));
        let on = x.view == i;
        let hov = ui.hover(r, &t);
        ui.c.fill_rrect(r, 3.0, if on { TAB_ON } else if hov { Color::rgb(0x42474f) } else { Color::rgb(0x32363c) });
        let rows = match i {
            0 => [true, true, true],
            1 => [true, true, false],
            2 => [true, false, true],
            _ => [true, false, false],
        };
        glyph(ui, r, rows, if on { INK } else { SOFT });
        ui.hit(r, t);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Small parts

/// A raised rounded panel of the middle.
fn plate_box(ui: &mut Ui, r: Rect) {
    // A recessed box inside the middle's frame: the articulator's and the filter's.
    ui.c.fill_rrect(r, 3.0, Color::rgb(0x6b7077));
    ui.c.stroke_rrect(r.inset(0.5), 3.0, 1.0, Color::rgb(0x565a61));
    ui.c.fill_rect(Rect::new(r.x + 2.0, r.y + 1.0, r.w - 4.0, 1.0), Color::rgb(0x7d8289));
}

/// The middle band as one raised piece, as the original has it: the plate at the left, the column of the articulator and
/// the filter beside it, the envelope at the right, no gaps between them.
fn middle_frame(ui: &mut Ui) {
    let band = edges(6.0, 654.0, 112.0, 322.0);
    ui.c.fill_rect_gradient(band, Color::rgb(0x91969c), Color::rgb(0x7d8288), 5.0);
    ui.c.stroke_rrect(band.inset(0.5), 5.0, 1.0, Color::rgb(0x4a4e55));
    ui.c.fill_rect(edges(157.0, 320.0, 114.0, 321.0), Color::rgb(0x777c83));
    for x in [156.0, 320.0] {
        ui.c.fill_rect(Rect::new(x, 114.0, 1.0, 207.0), Color::rgb(0x5d6168));
    }
}

/// A title at the top-left of a panel.
fn title(ui: &mut Ui, r: Rect, text: &str, ink: Color) {
    ui.c.text(SMALL.bold(), r.x + 8.0, r.y + 4.0, ink, text);
}

/// An italic title with a small curve icon before it, as the articulator's and the filter's have.
fn italic_title(ui: &mut Ui, r: Rect, text: &str, ink: Color) {
    let (x, y) = (r.x + 8.0, r.y + 7.0);
    ui.c.polyline(&[(x, y + 4.0), (x + 4.0, y + 4.0), (x + 7.0, y), (x + 10.0, y + 4.0)], 1.2, ink);
    ui.c.text(Font::new(11).bold().italic(), x + 14.0, r.y + 3.0, ink, text);
}

/// A small button of the middle panel: light when on, with its label.
fn tab(ui: &mut Ui, r: Rect, t: Target, label: &str, on: bool, disabled: bool) {
    let hov = ui.hover(r, &t) && !disabled;
    let face = if on { TAB_ON } else if hov { TAB_OFF.shade(0.12) } else { TAB_OFF };
    ui.c.fill_rect(r, face);
    ui.c.stroke_rrect(r.inset(0.5), 0.0, 1.0, INK_DIM.with_alpha(90));
    ui.c.text_center(Font::new(10), r, if disabled { INK_DIM } else { INK }, label);
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

/// A square check box with its label to the right.
fn check(ui: &mut Ui, x: f32, cy: f32, label: &str, on: bool, t: Target) {
    let w = ui.c.text_width(Font::new(8), label);
    let hr = Rect::new(x - 3.0, cy - 8.0, 18.0 + w, 16.0);
    let (bx, by) = (x + 4.5, cy);
    ui.c.fill_circle(bx, by, 4.5, Color::rgb(0x1f2531));
    ui.c.stroke_circle(bx, by, 4.5, 1.1, Color::rgb(0xd5dbe8));
    if on {
        ui.c.fill_circle(bx, by, 2.4, brand::MAIN);
    }
    let lh = ui.c.line_height(Font::new(8));
    ui.c.text(Font::new(8), x + 13.0, (cy - lh / 2.0).round(), SOFT, label);
    ui.hit(hr, t);
}

/// A small knob with its tiny caption under it, as the envelope bar has them.
#[allow(clippy::too_many_arguments)]
fn knob_small(ui: &mut Ui, t: Target, cx: f32, cy: f32, r: f32, norm: f32, cap: &str, cap_y: f32) {
    let hr = Rect::new(cx - r - 3.0, cy - r - 3.0, 2.0 * r + 6.0, 2.0 * r + 6.0);
    let hov = ui.hover(hr, &t) || ui.st.pressed.as_ref() == Some(&t);
    ui.c.fill_circle(cx, cy, r, if hov { Color::rgb(0x2c3342) } else { Color::rgb(0x1f2531) });
    ui.c.stroke_circle(cx, cy, r, 1.2, Color::rgb(0xd5dbe8));
    let a = (norm.clamp(0.0, 1.0) - 0.5) * 270f32.to_radians();
    let (sx, sy) = (a.sin(), -a.cos());
    ui.c.line(cx + sx * r * 0.2, cy + sy * r * 0.2, cx + sx * (r - 1.5), cy + sy * (r - 1.5), 1.4, LIGHT);
    let f = Font::new(7);
    let w = ui.c.text_width(f, cap);
    ui.c.text(f, (cx - w / 2.0).round(), cap_y, SOFT, cap);
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
fn fader(ui: &mut Ui, t: Target, cx: f32, norm: f32) {
    let hr = Rect::new(cx - 13.0, FADER_TOP - 4.0, 26.0, FADER_H + 8.0);
    let slot = Rect::new(cx - 3.0, FADER_TOP, 6.0, FADER_H);
    ui.c.fill_rrect(slot, 3.0, SLOT);
    let y = FADER_TOP + FADER_H * (1.0 - norm.clamp(0.0, 1.0));
    ui.c.fill_rrect(Rect::new(slot.x + 1.0, y, slot.w - 2.0, slot.bottom() - y), 2.0, brand::MAIN);
    let hov = ui.hover(hr, &t) || ui.st.pressed.as_ref() == Some(&t);
    let handle = Rect::new(cx - 8.0, y - 4.0, 16.0, 8.0);
    ui.c.fill_rrect(handle, 2.0, if hov { LIGHT } else { SOFT });
    ui.c.fill_rect(Rect::new(handle.x + 3.0, handle.cy() - 0.5, handle.w - 6.0, 1.0), INK_DIM);
    ui.hit(hr, t);
}

// ---------------------------------------------------------------------------------------------------------------
// The top strip

fn faders(ui: &mut Ui, look: &Look) {
    // The master levels, as the original has them: level, randomness, LFO, pitch; each with its small tile.
    let x = look.ext;
    fader(ui, Target::Ext(Ctl::Master), FADER_X[0], x.master / 1.25);
    fader(ui, Target::Ext(Ctl::RandLvl), FADER_X[1], x.rand_level);
    fader(ui, Target::Ext(Ctl::LfoLvl), FADER_X[2], x.lfo_level);
    let n = ui.m.norm(PITCH_ID);
    fader(ui, Target::Knob(PITCH_ID.into()), FADER_X[3], n);
    for (i, g) in [Gl::Level, Gl::Dice, Gl::Wave, Gl::Keys].into_iter().enumerate() {
        let r = Rect::new(FADER_X[i] - 9.0, 82.0, 18.0, 14.0);
        ui.c.fill_rrect(r, 2.0, Color::rgb(0x2d3544));
        ui.c.stroke_rrect(r.inset(0.5), 2.0, 1.0, Color::rgb(0x434d63));
        glyph(ui, r, g, if i == 1 { brand::MAIN } else { LIGHT });
    }
}

fn pad_panel(ui: &mut Ui, look: &Look) {
    ui.c.fill_rrect(MOD_PANEL, 4.0, STRIP_PANEL);
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
    ui.c.fill_rrect(LAYER_PANEL, 4.0, STRIP_PANEL);
    title(ui, LAYER_PANEL, "LAYERING", brand::MAIN);
    let x = look.ext;
    // The list of layering modes: a click shows the next.
    ui.c.fill_rrect(DROP, 3.0, Color::rgb(0x8a95ab));
    ui.c.text_clip(LABEL.bold(), DROP.x + 8.0, DROP.y + 2.0, DROP.w - 28.0, INK, LAYERS[usize::from(x.layer).min(6)]);
    let (ax, ay) = (DROP.right() - 12.0, DROP.cy());
    ui.c.fill_polygon(&[(ax - 4.0, ay - 3.0), (ax + 4.0, ay - 3.0), (ax, ay + 3.0)], INK);
    ui.hit(DROP, Target::Ext(Ctl::Layer((x.layer + 1) % 7)));
    led(ui, 358.0, 58.0, "SAME DECK", x.same_deck, Target::Ext(Ctl::SameDeck), SOFT);
    // The crossfade curves: none, linear, balanced, unbalanced, adding.
    ui.c.text(SMALL.bold(), 350.0, 74.0, SOFT, "CROSSFADE");
    for i in 0..5u8 {
        let r = edges(418.0 + 19.0 * f32::from(i), 435.0 + 19.0 * f32::from(i), 70.0, 84.0);
        let t = Target::Ext(Ctl::Crossfade(i));
        let on = x.fade == i;
        let hov = ui.hover(r, &t);
        ui.c.fill_rect(r, if on { TAB_ON } else if hov { TAB_OFF.shade(0.12) } else { Color::rgb(0x333c4c) });
        let ink = if on { INK } else { SOFT };
        let (lx, ty, w, h) = (r.x + 3.0, r.y + 3.0, r.w - 6.0, r.h - 6.0);
        let pts: Vec<(f32, f32)> = (0..=8)
            .map(|k| {
                let u = k as f32 / 8.0;
                let v = match i {
                    0 => if u < 0.5 { 0.0 } else { 1.0 },
                    1 => u,
                    2 => (u * std::f32::consts::FRAC_PI_2).sin(),
                    3 => u.powi(3),
                    _ => (2.0 * u).min(1.0),
                };
                (lx + w * u, ty + h * (1.0 - v))
            })
            .collect();
        ui.c.polyline(&pts, 1.2, ink);
        ui.hit(r, t);
    }
    led(ui, 358.0, 94.0, "AUTO-DUMP", x.auto_dump, Target::Ext(Ctl::AutoDump), SOFT);
    // The options menu: a round button.
    let (ox, oy) = (500.0, 95.0);
    let ot = Target::Ext(Ctl::Options);
    let hr = Rect::new(ox - 9.0, oy - 9.0, 18.0, 18.0);
    ui.c.fill_circle(ox, oy, 7.5, if ui.hover(hr, &ot) { Color::rgb(0x9aa5b8) } else { Color::rgb(0x7f8aa0) });
    ui.c.fill_polygon(&[(ox - 3.5, oy - 1.5), (ox + 3.5, oy - 1.5), (ox, oy + 2.5)], INK);
    ui.hit(hr, ot);
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
    ui.hit(LOGO, Target::Logo);
}

// ---------------------------------------------------------------------------------------------------------------
// The middle panel

fn plate(ui: &mut Ui, _look: &Look) {
    // Two small tabs on the top edge, as the original's plate has.
    for x in [PLATE_R.x + 26.0, PLATE_R.x + 70.0] {
        ui.c.fill_rrect(Rect::new(x, PLATE_R.y - 2.0, 20.0, 5.0), 2.0, PLATE_DARK.shade(-0.08));
    }
    // The emblem alone, as the original's plate has its own: the mango, and "SLICEX" with its last letters lit.
    mango_mark(ui, PLATE_R.cx(), PLATE_R.y + 84.0, 56.0);
    let f = Font::condensed(30, 0).bold();
    let (lh, a) = (ui.c.line_height(f), 2.0);
    let w1 = lemon::spaced_width(ui.c, f, "SLIC", a);
    let w2 = lemon::spaced_width(ui.c, f, "EX", a);
    let x = (PLATE_R.cx() - (w1 + w2 + 4.0) / 2.0).round();
    let y = (PLATE_R.y + 168.0 - lh / 2.0).round();
    lemon::spaced(ui.c, f, x, y, Color::rgb(0xf4f6fa), "SLIC", a);
    lemon::spaced(ui.c, f, x + w1 + 4.0, y, brand::MAIN, "EX", a);
}

fn art_panel(ui: &mut Ui, look: &Look) {
    plate_box(ui, ART_PANEL);
    italic_title(ui, ART_PANEL, "ARTICULATOR", Color::rgb(0xd8dde6));
    let sel = ui.st.sel;
    let st = look.state;
    let chosen = ui.st.chosen.min(st.slices.len().saturating_sub(1));
    let mine = (!st.slices.is_empty()).then(|| look.ext.art_of(chosen));
    for i in 0..8usize {
        let x = 168.0 + 18.5 * i as f32;
        let r = Rect::new(x.round(), 134.0, 16.0, 20.0);
        let on = sel.art == i;
        ui.c.fill_rect(r, if on { INK } else { TAB_OFF.shade(0.06) });
        ui.c.text_center(PILL, r, if on { LIGHT } else { Color::rgb(0xc8cfdc) }, &(i + 1).to_string());
        ui.hit(r, Target::Ext(Ctl::Art(i)));
        if mine == Some(i) {
            ui.c.fill_circle(r.cx(), r.bottom() + 4.0, 1.8, brand::MAIN);
        }
    }
}

fn filter_panel(ui: &mut Ui, look: &Look) {
    plate_box(ui, FILTER_PANEL);
    italic_title(ui, FILTER_PANEL, "FILTER", Color::rgb(0xd8dde6));
    let sel = ui.st.sel;
    let a = &look.ext.artic[sel.art];
    // Modes: LP BP BS HP on the first row, OFF and the slopes on the second.
    for (i, name) in FILTER_MODES.iter().take(4).enumerate() {
        tab(ui, Rect::new(196.0 + 29.0 * i as f32, 204.0, 27.0, 17.0), Target::Ext(Ctl::Mode(i as u8)), name, a.mode == i as u8, false);
    }
    let off = a.mode >= crate::ext::FILTER_OFF;
    tab(ui, Rect::new(196.0, 224.0, 27.0, 17.0), Target::Ext(Ctl::Mode(crate::ext::FILTER_OFF)), "OF", off, false);
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
    let geo = env_geometry(e, g);
    let at = |t: f64| g.x + (t / ENV_AXIS_S).min(1.0) as f32 * g.w;
    let ay = |v: f32| g.bottom() - 17.0 - v.clamp(0.0, 1.0) * (g.h - 21.0);
    let col = if amount == 0.0 { SOFT.with_alpha(110) } else { brand::MAIN };
    let hold = att + dec;
    let n = 64;
    let mut pts = vec![(at(0.0), ay(e.level(0.0, 0.5)))];
    for i in 1..=n {
        let t = hold * f64::from(i) / f64::from(n);
        pts.push((at(t), ay(e.level(t, 0.5))));
    }
    pts.push((at(hold + rel.max(0.02)), ay(0.0)));
    ui.c.polyline(&pts, 1.4, col);
    // The sustain line with its tag, and the points that can be dragged: the peak (the attack), the middle of the decay
    // (its bend) and the end (the sustain's time and level).
    ui.c.fill_rect(Rect::new(geo.end.0, g.y + 2.0, 1.0, g.h - 4.0), SOFT.with_alpha(110));
    ui.c.fill_rrect(geo.tag, 2.0, Color::rgb(0x8d97ae));
    ui.c.text_center(Font::new(7).bold(), geo.tag, INK, "S");
    if att > 0.0 {
        ui.c.fill_circle(geo.peak.0, geo.peak.1, 2.2, col);
    }
    ui.c.fill_circle(geo.mid.0, geo.mid.1, 2.2, col);
    ui.c.fill_circle(geo.end.0, geo.end.1, 3.0, Color::rgb(0x6fd35a));
    ui.c.stroke_circle(geo.end.0, geo.end.1, 3.0, 1.0, Color::rgb(0x2b3245));
}

/// The seconds the envelope graph shows across its width.
pub const ENV_AXIS_S: f64 = 3.0;

/// Where the envelope's draggable points are in the graph `g`.
pub struct EnvGeom {
    pub peak: (f32, f32),
    pub end: (f32, f32),
    pub mid: (f32, f32),
    pub tag: Rect,
}

pub fn env_geometry(e: &crate::ext::Env, g: Rect) -> EnvGeom {
    let (att, dec, _) = e.seconds(0.5);
    let at = |t: f64| g.x + (t / ENV_AXIS_S).min(1.0) as f32 * g.w;
    let ay = |v: f32| g.bottom() - 17.0 - v.clamp(0.0, 1.0) * (g.h - 21.0);
    let end = (at(att + dec), ay(e.sus));
    EnvGeom { peak: (at(att), ay(1.0)), end, mid: (at(att + dec * 0.5), ay(e.level(att + dec * 0.5, 0.5))), tag: Rect::new(end.0 - 4.5, g.bottom() - 31.0, 9.0, 11.0) }
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
    ui.hit(GRAPH, Target::Ext(Ctl::Amt));
    ui.c.fill_rect(KNOB_BAR, Color::rgb(0x2b3245));
    // The bar's small icons at the right and the arrow at its left, as the original's.
    for (i, g) in [Gl::Freeze, Gl::Marker, Gl::Snap, Gl::Slide].into_iter().enumerate() {
        let r = Rect::new(560.0 + 24.0 * i as f32, 290.0, 22.0, 20.0);
        glyph(ui, r, g, if g == Gl::Slide { Color::rgb(0xf0b030) } else { Color::rgb(0x8d97ae) });
        ui.hit(r, Target::Ext(Ctl::Row(14 + i as u8)));
    }
    // The scroll row at the bottom of the graph, with its arrow.
    let row = Rect::new(GRAPH.x, GRAPH.bottom() - 13.0, GRAPH.w, 13.0);
    ui.c.fill_rect(row, Color::rgb(0x333c52));
    ui.c.fill_rect(Rect::new(row.x + 18.0, row.y + 3.0, row.w - 36.0, row.h - 6.0), Color::rgb(0x2b3245));
    ui.c.fill_polygon(&[(row.x + 5.0, row.y + 4.0), (row.x + 12.0, row.y + 4.0), (row.x + 8.5, row.y + 9.0)], Color::rgb(0x8d97ae));
    ui.c.fill_rect(Rect::new(KNOB_BAR.x, KNOB_BAR.y, KNOB_BAR.w, 1.0), INK_DIM);
    let amount = a.amount(sel.tgt, sel.src);
    let t = sel.tgt.index();
    let xs = |k: usize| 340.0 + 30.0 * k as f32;
    let small_knob = |ui: &mut Ui, c: Ctl, k: usize| {
        let cap = match c {
            Ctl::Att => "ATT",
            Ctl::Dec => "DEC",
            Ctl::Sus => "SUS",
            Ctl::Rel => "REL",
            _ => "SPEED",
        };
        knob_small(ui, Target::Ext(c), xs(k), 298.0, 7.5, extctl::get(look.ext, &sel, c), cap, 307.0);
    };
    match sel.src {
        Source::Env => {
            env_curve(ui, GRAPH, &a.env[t], amount);
            for (k, c) in [Ctl::Att, Ctl::Dec, Ctl::Sus, Ctl::Rel].into_iter().enumerate() {
                small_knob(ui, c, k);
            }
            check(ui, 478.0, 298.0, "TEMPO", a.env[t].tempo, Target::Ext(Ctl::Tempo));
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
            let pill = Rect::new(GRAPH.x + 6.0, GRAPH.y + 5.0, 54.0, 14.0);
            ui.c.fill_rrect(pill, 3.0, Color::rgb(0x333d56));
            ui.c.text_center(SMALL.bold(), pill, SOFT, crate::ext::LFO_SHAPES[usize::from(l.shape).min(4)]);
            ui.hit(pill, Target::Ext(Ctl::LfoShape(0)));
            small_knob(ui, Ctl::LfoRate, 0);
            check(ui, 478.0, 298.0, "TEMPO", l.tempo, Target::Ext(Ctl::Tempo));
        }
        other => {
            let _ = other;
            // The source's amount as a line across the graph: the middle is none.
            let y = GRAPH.cy() - amount * (GRAPH.h / 2.0 - 14.0);
            ui.c.line(GRAPH.x + 6.0, y, GRAPH.right() - 6.0, y, 1.4, if amount == 0.0 { SOFT.with_alpha(110) } else { brand::MAIN });
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
    Menu,
    Refresh,
    Marks,
    Prev,
    Next,
    Link,
    Level,
    Dice,
    Wave,
    Keys,
    Slope,
    Regions2,
    NoiseT,
    Flag,
    Freeze,
    Step,
    Slide,
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
    let hs = (r.h * 0.4).min(8.0);
    let ic = |ui: &mut Ui, i: &os_ui::tabler::TablerIcon| ui.c.icon(i, Rect::new(cx - hs, cy - hs, 2.0 * hs, 2.0 * hs), ink);
    use os_ui::tabler as tb;
    match g {
        Gl::Loop => {
            // An infinity symbol: a lemniscate.
            let pts: Vec<(f32, f32)> = (0..=40)
                .map(|i| {
                    let a = i as f32 / 40.0 * std::f32::consts::TAU;
                    let d = 1.0 + a.sin() * a.sin();
                    (cx + 8.0 * a.cos() / d, cy + 8.0 * a.sin() * a.cos() / d)
                })
                .collect();
            ui.c.polyline(&pts, 1.8, ink);
        }
        Gl::Play => ui.c.fill_polygon(&[(cx - 3.0, cy - 4.5), (cx - 3.0, cy + 4.5), (cx + 4.5, cy)], ink),
        Gl::Stop => ui.c.fill_rect(Rect::new(cx - 3.5, cy - 3.5, 7.0, 7.0), ink),
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
        Gl::Menu => ui.c.fill_polygon(&[(cx - 3.5, cy - 2.0), (cx + 3.5, cy - 2.0), (cx, cy + 2.5)], ink),
        Gl::Refresh => ic(ui, &tb::ROTATE_CLOCKWISE_2),
        Gl::Marks => {
            for dx in [-5.0, 0.0, 5.0] {
                ui.c.line(cx + dx, cy - 6.0, cx + dx, cy + 6.0, 1.4, ink);
            }
            ui.c.line(cx - 7.0, cy, cx + 7.0, cy, 1.4, ink);
        }
        Gl::Prev => {
            ui.c.fill_polygon(&[(cx + 3.0, cy - 5.0), (cx + 3.0, cy + 5.0), (cx - 3.0, cy)], ink);
            ui.c.fill_rect(Rect::new(cx - 6.0, cy - 5.0, 1.6, 10.0), ink);
        }
        Gl::Next => {
            ui.c.fill_polygon(&[(cx - 3.0, cy - 5.0), (cx - 3.0, cy + 5.0), (cx + 3.0, cy)], ink);
            ui.c.fill_rect(Rect::new(cx + 4.4, cy - 5.0, 1.6, 10.0), ink);
        }
        Gl::Link => ic(ui, &tb::LINK),
        Gl::Level => ui.c.fill_polygon(&[(cx - 5.0, cy + 4.0), (cx + 5.0, cy + 4.0), (cx + 5.0, cy - 4.0)], ink),
        Gl::Dice => {
            ui.c.stroke_rrect(Rect::new(cx - 4.5, cy - 4.5, 9.0, 9.0), 2.0, 1.2, ink);
            for (dx, dy) in [(-2.0, -2.0), (2.0, 2.0), (0.0, 0.0)] {
                ui.c.fill_circle(cx + dx, cy + dy, 0.9, ink);
            }
        }
        Gl::Wave => {
            let pts: Vec<(f32, f32)> = (0..=12).map(|i| (cx - 6.0 + i as f32, cy - (i as f32 / 12.0 * std::f32::consts::TAU).sin() * 3.5)).collect();
            ui.c.polyline(&pts, 1.3, ink);
        }
        Gl::Keys => {
            for dx in [-5.0, -2.0, 1.0, 4.0] {
                ui.c.fill_rect(Rect::new(cx + dx, cy - 4.0, 1.6, 8.0), ink);
            }
        }
        Gl::Slope => ui.c.fill_polygon(&[(cx - 5.0, cy + 4.0), (cx + 5.0, cy - 4.0), (cx + 5.0, cy + 4.0)], ink),
        Gl::Regions2 => {
            for dy in [-4.0, 0.0, 4.0] {
                ui.c.line(cx - 5.0, cy + dy, cx + 5.0, cy + dy, 1.2, ink);
            }
            ui.c.fill_rect(Rect::new(cx - 6.0, cy - 5.0, 2.0, 10.0), ink);
        }
        Gl::NoiseT => {
            for dy in [-4.0, 0.0, 4.0] {
                ui.c.line(cx - 6.0, cy + dy, cx + 6.0, cy + dy, 1.4, ink);
            }
        }
        Gl::Flag => {
            ui.c.line(cx - 3.0, cy - 7.0, cx - 3.0, cy + 7.0, 1.4, ink);
            ui.c.fill_polygon(&[(cx - 3.0, cy - 7.0), (cx + 6.0, cy - 4.0), (cx - 3.0, cy - 1.0)], ink);
        }
        Gl::Freeze => {
            for a in 0..3 {
                let t = a as f32 * std::f32::consts::PI / 3.0;
                ui.c.line(cx - 6.0 * t.cos(), cy - 6.0 * t.sin(), cx + 6.0 * t.cos(), cy + 6.0 * t.sin(), 1.3, ink);
            }
        }
        Gl::Step => {
            let pts = [(cx - 7.0, cy + 5.0), (cx - 2.0, cy + 1.0), (cx + 2.0, cy - 2.0), (cx + 7.0, cy - 6.0)];
            ui.c.polyline(&pts, 1.8, ink);
        }
        Gl::Slide => {
            ui.c.line(cx - 7.0, cy, cx + 7.0, cy, 1.4, ink);
            ui.c.fill_polygon(&[(cx - 7.0, cy - 3.0), (cx - 7.0, cy + 3.0), (cx - 11.0, cy)], ink);
            ui.c.fill_polygon(&[(cx + 7.0, cy - 3.0), (cx + 7.0, cy + 3.0), (cx + 11.0, cy)], ink);
        }
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
    // The transport: one flat group of three, its icons bright whether or not a loop is loaded.
    let group = edges(28.0, 104.0, TB_Y, TB_Y + TB_H);
    ui.c.fill_rrect(group, 3.0, BTN_FACE);
    ui.c.stroke_rrect(group.inset(0.5), 3.0, 1.0, BTN_EDGE);
    for x in [53.0, 79.0] {
        ui.c.fill_rect(Rect::new(x, TB_Y + 2.0, 1.0, TB_H - 4.0), BTN_EDGE);
    }
    for (r, c, g) in [(LOOP, Ctl::Loop, Gl::Loop), (PLAY, Ctl::PlaySlice, Gl::Play), (STOP, Ctl::Stop, Gl::Stop)] {
        let t = Target::Ext(c);
        if ui.hover(r, &t) {
            ui.c.fill_rect(r.inset(1.0), BTN_FACE.shade(0.14));
        }
        glyph(ui, r, g, Color::rgb(0xf2f4f8));
        ui.hit(r, t);
    }
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

/// A value on the info strip, as plain text (the original has no field behind it): light with a loop loaded, dim without.
fn value_text(ui: &mut Ui, r: Rect, t: Target, text: &str, enabled: bool) {
    let hov = enabled && ui.hover(r, &t);
    let ink = if !enabled { Color::rgb(0x59627a) } else if hov { Color::rgb(0xffffff) } else { LIGHT };
    ui.c.text(LABEL, r.x, 374.0, ink, text);
    ui.hit(r, t);
}

fn info(ui: &mut Ui, look: &Look) {
    let dim = Color::rgb(0x6d7791);
    ui.c.fill_rect(edges(6.0, 654.0, 370.0, 402.0), Color::rgb(0x333b50));
    let has = look.frames > 0;
    let cap = Font::new(8).bold();
    ui.c.text(cap, 12.0, 372.0, dim, "SAMPLERATE");
    ui.c.text(LABEL, 12.0, 383.0, if has { LIGHT } else { Color::rgb(0x59627a) }, &if has { format!("{} Hz", look.rate) } else { "44100 Hz".into() });
    ui.c.text(cap, 84.0, 372.0, dim, "FORMAT");
    let ink = if has { LIGHT } else { Color::rgb(0x59627a) };
    ui.c.text(LABEL, 84.0, 383.0, ink, "32");
    // The stereo icon after it: two small squares joined by a bar over them.
    let (sx, sy) = (104.0, 384.0);
    ui.c.fill_rect(Rect::new(sx, sy, 12.0, 1.6), ink);
    ui.c.fill_rect(Rect::new(sx, sy, 1.6, 8.0), ink);
    ui.c.fill_rect(Rect::new(sx + 10.4, sy, 1.6, 8.0), ink);
    ui.c.stroke_rrect(Rect::new(sx - 1.0, sy + 4.5, 4.0, 4.0), 0.5, 1.2, ink);
    ui.c.stroke_rrect(Rect::new(sx + 9.0, sy + 4.5, 4.0, 4.0), 0.5, 1.2, ink);
    ui.c.text(cap, BPM_BOX.x, 372.0, dim, "TEMPO");
    value_text(ui, BPM_BOX, Target::Bpm, &if has { format!("{:.3}", look.state.bpm()) } else { String::new() }, has);
    ui.c.text(cap, 196.0, 372.0, dim, "DECK");
    let x = look.ext;
    let (a_on, b_on) = (x.a_on, x.b_on);
    tab(ui, DECK_A, Target::Ext(Ctl::DeckToggle(0)), "A", a_on, false);
    tab(ui, DECK_B, Target::Ext(Ctl::DeckToggle(1)), "B", b_on, look.decks[1].is_empty());
    let name = look.decks[0].clone();
    ui.c.text(cap, 250.0, 372.0, dim, "TITLE");
    ui.c.text_clip(LABEL, 250.0, 383.0, 226.0, LIGHT, &name);
    ui.c.text(cap, BEATS_BOX.x, 372.0, dim, "LENGTH / SEL");
    value_text(ui, BEATS_BOX, Target::Beats, &if has { format!("{:.2}", look.state.beats()) } else { String::new() }, has);
    for (i, g) in SWITCH_GLYPHS.iter().enumerate() {
        let r = switch_cell(i);
        glyph(ui, r, *g, Color::rgb(0x707a92));
        ui.hit(r, Target::Ext(Ctl::Info(i as u8)));
    }
}

/// The loop in its well: the waveform, the slices' lines (the chosen slice lit), their numbers along the top.
fn well(ui: &mut Ui, look: &Look, hint: &str) {
    ui.c.fill_rect(WELL_R, Color::rgb(0x2e3850));
    // The original's wave sits on bands of two blues.
    for i in 0..8 {
        if i % 2 == 1 {
            ui.c.fill_rect(Rect::new(WELL_R.x, WELL_R.y + WELL_R.h * i as f32 / 8.0, WELL_R.w, WELL_R.h / 8.0), Color::rgb(0x333d56));
        }
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

/// The wave's scroll bar row: arrows at both ends and the thumb.
fn scroll_bar(ui: &mut Ui) {
    ui.c.fill_rect(SCROLL_R, Color::rgb(0x353e53));
    ui.c.fill_rect(Rect::new(SCROLL_R.x, SCROLL_R.bottom() - 1.0, SCROLL_R.w, 1.0), BTN_EDGE);
    let (l, r) = (SCROLL_R.x + 9.0, SCROLL_R.right() - 9.0);
    let cy = SCROLL_R.cy();
    ui.c.fill_polygon(&[(l + 3.0, cy - 4.0), (l + 3.0, cy + 4.0), (l - 3.0, cy)], SOFT);
    ui.c.fill_polygon(&[(r - 3.0, cy - 4.0), (r - 3.0, cy + 4.0), (r + 3.0, cy)], SOFT);
    ui.c.fill_rect(Rect::new(l + 12.0, SCROLL_R.y + 3.0, r - l - 24.0, SCROLL_R.h - 6.0), Color::rgb(0x434d66));
}

/// The row under the wave: the editor's switches at both ends.
fn switches_row(ui: &mut Ui, look: &Look) {
    ui.c.fill_rect(ROW_R, Color::rgb(0x252d3b));
    ui.c.fill_rect(Rect::new(ROW_R.x, ROW_R.y, ROW_R.w, 1.0), BTN_EDGE);
    let has = look.frames > 0 && !look.state.slices.is_empty();
    // Left: a menu arrow, refresh, add marks, then the next and previous region and two more.
    let left = [Gl::Menu, Gl::Refresh, Gl::Marks, Gl::Next, Gl::Slope, Gl::Prev, Gl::Link];
    for (i, g) in left.iter().enumerate() {
        let r = Rect::new(8.0 + 22.0 * i as f32, ROW_R.y, 20.0, ROW_R.h);
        let enabled = matches!(g, Gl::Prev | Gl::Next) && has;
        let ink = match g {
            Gl::Refresh | Gl::Marks => Color::rgb(0xe3e7ef),
            _ if enabled => Color::rgb(0xe3e7ef),
            Gl::Menu => Color::rgb(0xcfd6e6),
            _ => Color::rgb(0x59627a),
        };
        let t = Target::Ext(Ctl::Row(i as u8));
        let hov = enabled && ui.hover(r, &t);
        if hov {
            ui.c.fill_rrect(r, 3.0, BTN_FACE.shade(0.16));
        }
        glyph(ui, r, *g, ink);
        ui.hit(r, t);
    }
    // Right: the switches of the original. The flag is lit, the step curve pink.
    let right = [Gl::NoiseT, Gl::Regions2, Gl::Flag, Gl::Freeze, Gl::Step, Gl::Snap, Gl::Slide];
    for (i, g) in right.iter().enumerate() {
        let r = Rect::new(ROW_R.right() - 8.0 - 22.0 * (7 - i) as f32, ROW_R.y, 20.0, ROW_R.h);
        let ink = match g {
            Gl::Flag => Color::rgb(0xf0b030),
            Gl::Step => Color::rgb(0xf0408a),
            Gl::Snap | Gl::Freeze => Color::rgb(0xcfd6e6),
            _ => Color::rgb(0x8d97ae),
        };
        glyph(ui, r, *g, ink);
        ui.hit(r, Target::Ext(Ctl::Row(7 + i as u8)));
    }
}

/// The marker lane under the wave: two rows, and a flag with the number at each slice's start.
fn lane(ui: &mut Ui, look: &Look) {
    ui.c.fill_rect(LANE, Color::rgb(0x2c3547));
    for dy in [10.0, 21.0] {
        ui.c.fill_rect(Rect::new(LANE.x, LANE.y + dy, LANE.w, 1.0), Color::rgb(0x3a4560));
    }
    if look.frames == 0 {
        return;
    }
    let chosen = ui.st.chosen.min(look.state.slices.len().saturating_sub(1));
    for (i, s) in look.state.slices.iter().enumerate() {
        let x = x_of(s.start, look.frames).min(WAVE.right() - 1.0);
        let col = if i == chosen { brand::MAIN } else { SOFT.with_alpha(170) };
        ui.c.fill_rect(Rect::new(x, LANE.y + 2.0, 1.0, LANE.h - 4.0), col);
        ui.c.fill_polygon(&[(x, LANE.y + 2.0), (x + 6.0, LANE.y + 4.5), (x, LANE.y + 7.0)], col);
    }
}

/// A key's rect: white keys in the lower row, black keys in the upper. `None` outside the keyboard.
fn key_rect(k: i32) -> Option<(Rect, bool)> {
    // The white key's number within the octave for each pitch class; a black key sits after the white one before it.
    const WHITE: [i32; 12] = [0, -1, 1, -1, 2, 3, -1, 4, -1, 5, -1, 6];
    if !(0..128).contains(&k) {
        return None;
    }
    let (oct, n) = (k.div_euclid(12), k.rem_euclid(12) as usize);
    if WHITE[n] >= 0 {
        let x = KEY_X0 + KEY_W * (oct * 7 + WHITE[n]) as f32;
        (x + KEY_W <= 650.0).then(|| (Rect::new(x, WHITE_Y, KEY_W - 1.0, WHITE_H), false))
    } else {
        let x = KEY_X0 + KEY_W * (oct * 7 + WHITE[n - 1] + 1) as f32 - 3.0;
        (x + 6.0 <= 650.0).then(|| (Rect::new(x, KEYS_Y, 6.0, BLACK_H), true))
    }
}

fn keyboard(ui: &mut Ui, look: &Look) {
    ui.c.fill_rect(edges(6.0, 654.0, 562.0, 628.0), Color::rgb(0x1b212c));
    // The range labels at the left: five rows, the fourth marked.
    for (i, label) in ["C0", "C1", "C2", "C3", "F3"].iter().enumerate() {
        let y = 569.0 + 10.0 * i as f32;
        if i == 3 {
            ui.c.fill_rrect(Rect::new(10.0, y - 1.0, 21.0, 10.0), 2.0, Color::rgb(0x8d97ae));
            ui.c.fill_polygon(&[(32.0, y), (32.0, y + 8.0), (36.0, y + 4.0)], Color::rgb(0x8d97ae));
        }
        ui.c.text(Font::new(7).bold(), 13.0, y, if i == 3 { INK } else { LIGHT }, label);
    }
    let st = look.state;
    let chosen = ui.st.chosen.min(st.slices.len().saturating_sub(1));
    for black in [false, true] {
        for k in 0..128 {
            let Some((r, is_black)) = key_rect(k) else { continue };
            if is_black != black {
                continue;
            }
            let t = Target::Ext(Ctl::Key(k as u8));
            let hov = ui.hover(r, &t) || ui.st.pressed.as_ref() == Some(&t);
            let face = match st.slice_for(k) {
                Some(i) if i == chosen => brand::MAIN,
                Some(_) if black => Color::rgb(0x6f7ca0),
                Some(_) => Color::rgb(0xb7c4e4),
                None if black => Color::rgb(0x2d3544),
                None => Color::rgb(0xe9ecf2),
            };
            let face = if hov { face.shade(0.12) } else { face };
            ui.c.fill_rrect(r, 1.5, face);
            if black {
                ui.c.stroke_rrect(r.inset(0.5), 1.5, 1.0, Color::rgb(0x414c63));
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
        for r in [KEYS_BTN, PAD, MOD_PANEL, LAYER_PANEL, LOGO, PLATE_R, ART_PANEL, FILTER_PANEL, NAME, KEY_BOX, REVERSE, GRAPH, KNOB_BAR, LOOP, PLAY, STOP, BPM_BOX, BEATS_BOX, SCROLL_R, WELL_R, WAVE, ROW_R, LANE] {
            assert!(r.x >= 0.0 && r.right() <= win.right() && r.y >= 0.0 && r.bottom() <= win.bottom(), "{r:?}");
        }
        for (a, b) in [(MOD_PANEL, LAYER_PANEL), (LAYER_PANEL, LOGO), (PLATE_R, ART_PANEL), (ART_PANEL, FILTER_PANEL), (LOOP, PLAY), (PLAY, STOP)] {
            let apart = a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y;
            assert!(apart, "{a:?} {b:?}");
        }
        assert_eq!((key_name(60), key_name(36), key_name(61)), ("C5".into(), "C3".into(), "C#5".into()));
        // The keyboard covers the slices' keys (C5 up) inside the window.
        assert!(key_rect(60).is_some_and(|k| !k.1) && key_rect(61).is_some_and(|k| k.1) && key_rect(127).is_none_or(|k| k.0.right() <= 654.0));
    }

    /// The views keep their bands under the top strip: everything is the full window, no audio editor drops the
    /// editor's 218 px, no region settings drops the middle's 214, the top alone is 112, and the keyboard is 68.
    #[test]
    fn views_stack_their_bands() {
        assert_eq!(window_size(0, true), (W, H));
        assert_eq!(window_size(1, true).1, H - 236);
        assert_eq!(window_size(2, true).1, H - 214);
        assert_eq!(window_size(0, false).1, H - 66);
        assert_eq!(window_size(3, true).1, 112);
        let (bands, _) = arrange(2, true);
        assert_eq!(bands, vec![(326.0, 562.0, -214.0), (562.0, 628.0, -214.0)]);
    }
}
