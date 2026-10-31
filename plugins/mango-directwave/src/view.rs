// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango DirectWave editor: painting and hit regions (platform independent), in the Lemon "Pith" plugin look on a
//! 6 px grid: the header band (the lemon mark and "WAVE" on one line, the spread button), the eight zones' list,
//! and the cards of the master (its envelope and filter knobs) and of the chosen zone (its sample, key and velocity
//! ranges and levels).
//!
// spec: plugins/mango-directwave.md, ui/lemon-design.md §11.1

use os_theme::design::lemon as brand;
use os_theme::Palette;
use os_ui::kit::lemon::{self, plug};
use os_ui::kit::{inst, State as Kst};
use os_ui::{Canvas, Color, Font, Rect};

use crate::program::Program;

/// Editor size in logical pixels.
pub const W: u32 = 618;
pub const H: u32 = 378;

/// Every card 6 px from the editor's edges and from its neighbours.
const GAP: f32 = 6.0;
/// A rect from its left, right, top and bottom edges (right and bottom exclusive).
const fn edges(x0: f32, x1: f32, y0: f32, y1: f32) -> Rect {
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
const RIGHT: f32 = W as f32 - GAP;
const BOTTOM: f32 = H as f32 - GAP;

/// The header band and its centre line.
const HEAD: (f32, f32) = (GAP, 36.0);
const HEAD_Y: f32 = (HEAD.0 + HEAD.1) / 2.0;
pub const LOGO: Rect = edges(GAP, 156.0, HEAD.0, HEAD.1);
/// The spread button: the loaded zones laid over the keyboard in order.
pub const SPREAD: Rect = edges(228.0, 288.0, HEAD_Y - 11.0, HEAD_Y + 11.0);
/// The zones' list; the eight rows, 30 px tall, 6 px apart, from the well's edges.
pub const LIST: Rect = edges(GAP, 336.0, HEAD.1 + GAP, BOTTOM);
pub const ROWS: usize = 8;
const ROW_H: f32 = 30.0;
/// The master's and the chosen zone's cards.
const COL: (f32, f32) = (342.0, RIGHT);
pub const MASTER_CARD: Rect = edges(COL.0, COL.1, HEAD.1 + GAP, 156.0);
pub const ZONE_CARD: Rect = edges(COL.0, COL.1, 162.0, BOTTOM);
/// A card's title band.
const BAND: f32 = 18.0;
/// The zone's name and sample field, its load button, and the boxes' and knobs' lines.
pub const NAME: Rect = edges(COL.0 + 8.0, COL.1 - 8.0, 162.0 + BAND + 6.0, 162.0 + BAND + 24.0);
const BOX_Y: f32 = 246.0;
const KNOB_Y: f32 = 306.0;
const BOX_X: [f32; 5] = [384.0, 444.0, 504.0, 552.0, 594.0];
const KNOB_X: [f32; 3] = [384.0, 474.0, 564.0];
const MASTER_KNOB_Y: f32 = 84.0;
const MASTER_X: [f32; 4] = [384.0, 444.0, 504.0, 564.0];
const KNOB_R: f32 = 17.0;

const SMALL: Font = Font::new(10);
const LABEL: Font = Font::new(11);
const PILL: Font = Font::new(10).bold();

/// The zone and master settings the editor shows as boxes and knobs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Root,
    Lo,
    Hi,
    VelLo,
    VelHi,
    ZoneGain,
    ZonePan,
    ZoneTune,
    MasterGain,
    Cut,
    Res,
    Attack,
    Decay,
    Sustain,
    Release,
}

impl Field {
    /// The value range.
    pub fn range(self) -> (i32, i32) {
        match self {
            Field::Root | Field::Lo | Field::Hi => (0, 127),
            Field::VelLo | Field::VelHi => (0, 128),
            Field::ZoneGain => (0, 256),
            Field::ZonePan => (-64, 64),
            Field::ZoneTune => (-100, 100),
            Field::MasterGain => (0, 512),
            Field::Cut | Field::Res => (0, 128),
            Field::Attack | Field::Decay | Field::Sustain | Field::Release => (0, 128),
        }
    }

    pub fn bipolar(self) -> bool {
        matches!(self, Field::ZonePan | Field::ZoneTune)
    }

    /// The hint's name.
    pub fn name(self) -> &'static str {
        match self {
            Field::Root => "Zone root key",
            Field::Lo => "Zone lowest key",
            Field::Hi => "Zone highest key",
            Field::VelLo => "Zone lowest velocity",
            Field::VelHi => "Zone highest velocity",
            Field::ZoneGain => "Zone volume",
            Field::ZonePan => "Zone panning",
            Field::ZoneTune => "Zone fine tune",
            Field::MasterGain => "Master volume",
            Field::Cut => "Cutoff",
            Field::Res => "Resonance",
            Field::Attack => "Attack",
            Field::Decay => "Decay",
            Field::Sustain => "Sustain",
            Field::Release => "Release",
        }
    }

    /// A value's text.
    pub fn text(self, v: i32) -> String {
        match self {
            Field::Root | Field::Lo | Field::Hi => key_name(v),
            Field::VelLo | Field::VelHi => v.to_string(),
            Field::ZoneGain => format!("{:.0}%", f64::from(v) / 128.0 * 100.0),
            Field::ZonePan => match v {
                0 => "Centered".into(),
                v if v < 0 => format!("{:.0}% left", f64::from(-v) / 64.0 * 100.0),
                v => format!("{:.0}% right", f64::from(v) / 64.0 * 100.0),
            },
            Field::ZoneTune => if v == 0 { "0 cents".into() } else { format!("{v:+} cents") },
            Field::MasterGain => {
                let g = (v as f32 / 256.0).powi(2);
                if g <= 0.0 { "-INFdB".into() } else { format!("{:.1}dB", 20.0 * f64::from(g).log10()) }
            }
            _ => format!("{:.0}%", f64::from(v) / 128.0 * 100.0),
        }
    }
}

/// The name of key `k` (60 = C5).
pub fn key_name(k: i32) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[k.rem_euclid(12) as usize], k.div_euclid(12))
}

/// What a region of the editor does when clicked or dragged.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Zone `n` (0..8): a click chooses it and plays it.
    Zone(usize),
    /// The loaded zones laid over the keyboard in order.
    Spread,
    /// A value box or knob: vertical drag / wheel; type-in on right click.
    Value(Field),
    /// The chosen zone's loop switch.
    Loop,
    Load,
    Clear,
    Logo,
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub r: Rect,
    pub t: Target,
}

/// The pointer, the pressed control and the chosen zone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    pub mouse: Option<(f32, f32)>,
    pub pressed: Option<Target>,
    pub zone: usize,
}

/// What a paint shows besides the state.
pub struct Look<'a> {
    pub program: &'a Program,
    /// Whether each zone has a sample that opened.
    pub loaded: &'a [bool],
    /// Zones lit by a note just now.
    pub lit: &'a [bool],
}

struct Ui<'a> {
    c: &'a mut Canvas,
    t: &'a Palette,
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

/// Zone `n`'s row, top to bottom.
pub fn row_rect(n: usize) -> Rect {
    Rect::new(LIST.x + GAP, LIST.y + GAP + n as f32 * (ROW_H + GAP), LIST.w - 2.0 * GAP, ROW_H)
}

/// The value of a field of a program.
pub fn value(p: &Program, zone: usize, f: Field) -> Option<i32> {
    let z = p.zones.get(zone);
    let m = &p.master;
    Some(match f {
        Field::Root => z?.root,
        Field::Lo => z?.lo,
        Field::Hi => z?.hi,
        Field::VelLo => z?.vel_lo,
        Field::VelHi => z?.vel_hi,
        Field::ZoneGain => z?.gain,
        Field::ZonePan => z?.pan,
        Field::ZoneTune => z?.tune,
        Field::MasterGain => m.gain,
        Field::Cut => m.cut,
        Field::Res => m.res,
        Field::Attack => m.attack,
        Field::Decay => m.decay,
        Field::Sustain => m.sustain,
        Field::Release => m.release,
    })
}

/// The file name of a stored path.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Paints the editor. `hint` (not hosted: the host shows it otherwise) goes along the list's bottom.
pub fn paint(c: &mut Canvas, t: &Palette, st: &ViewState, look: &Look, hint: &str) -> Vec<Hit> {
    let mut ui = Ui { c, t, st, hits: Vec::new() };
    ui.c.begin_frame(None);
    ui.c.clear(plug::BG);
    header(&mut ui);
    list(&mut ui, look, hint);
    master_card(&mut ui, look.program);
    zone_card(&mut ui, look.program, st.zone);
    ui.hits
}

fn header(ui: &mut Ui) {
    // The emblem: the lemon mark and "WAVE" on one line.
    let f = Font::condensed(18, 0).bold();
    let s: f32 = 6.0;
    let (wl, gap) = (2.2 * s, (s * 1.3).round());
    let x = LOGO.x + 4.0;
    let lh = ui.c.line_height(f);
    lemon::lemon_mark(ui.c, x + wl / 2.0, HEAD_Y, s, brand::MAIN);
    lemon::spaced(ui.c, f, x + wl + gap, (HEAD_Y - lh / 2.0).round(), plug::WORDMARK, "WAVE", 3.0);
    ui.hit(LOGO, Target::Logo);
    let t = Target::Spread;
    let kst = ui.state(SPREAD, &t);
    let ink = lemon::plug_button(ui.c, SPREAD, 6.0, kst);
    ui.c.text_center(PILL, SPREAD, ink, "SPREAD");
    ui.hit(SPREAD, t);
}

/// The eight zones: number, name and key range; the chosen zone lit, a loaded one marked.
fn list(ui: &mut Ui, look: &Look, hint: &str) {
    lemon::plug_well(ui.c, LIST, 8.0);
    for n in 0..ROWS {
        let z = look.program.zone(n);
        let r = row_rect(n);
        let t = Target::Zone(n);
        let mut kst = ui.state(r, &t);
        kst.on = ui.st.zone == n || look.lit.get(n).copied().unwrap_or(false);
        let ink = lemon::plug_button(ui.c, r, 6.0, kst);
        let loaded = look.loaded.get(n).copied().unwrap_or(false);
        ui.c.text(SMALL.bold(), r.x + 6.0, r.y + 5.0, ink, &(n + 1).to_string());
        let range = format!("{}..{}", key_name(z.lo), key_name(z.hi));
        let rw = ui.c.text_width(SMALL, &range);
        ui.c.text(SMALL, r.right() - 6.0 - rw, r.y + 5.0, ink, &range);
        let name = if z.loaded() { file_name(&z.path).to_string() } else { "empty".into() };
        let col = if !z.loaded() {
            plug::OFF
        } else if !loaded {
            Color::rgb(0xd06050)
        } else {
            ink
        };
        ui.c.text_clip(SMALL, r.x + 22.0, r.cy() - 2.0, r.w - 30.0 - rw, col, &name);
        if loaded {
            ui.c.fill_circle(r.right() - 14.0 - rw, r.y + 9.0, 2.5, brand::MAIN);
        }
        ui.hit(r, t);
    }
    if !hint.is_empty() {
        let (left, right) = hint.split_once('\t').unwrap_or((hint, ""));
        let s = if right.is_empty() { left.to_string() } else { format!("{left}  ({right})") };
        let lh = ui.c.line_height(SMALL);
        let y = LIST.bottom() - lh - 3.0;
        let tw = ui.c.text_width(SMALL, &s).min(LIST.w - 12.0);
        ui.c.fill_rrect(Rect::new(LIST.x + 2.0, y - 2.0, tw + 10.0, lh + 4.0), 4.0, Color::rgb(0x1a1a1a).with_alpha(200));
        ui.c.text_clip(SMALL, LIST.x + 7.0, y, LIST.w - 12.0, brand::TEXT, &s);
    }
}

/// A card with its title centred in a band and a line under the band.
fn titled(ui: &mut Ui, r: Rect, title: &str) {
    lemon::plug_panel(ui.c, r);
    ui.c.fill_rect(Rect::new(r.x + 1.0, r.y + BAND, r.w - 2.0, 1.0), plug::PANEL_LINE);
    ui.c.text_center(SMALL.bold(), Rect::new(r.x, r.y, r.w, BAND), plug::DIM, title);
}

/// A value box (drag, wheel, type-in) with its caption under it.
fn value_box(ui: &mut Ui, cx: f32, y: f32, f: Field, v: Option<i32>, caption: &str) {
    let r = Rect::new(cx - 26.0, y - 10.0, 52.0, 20.0);
    let t = Target::Value(f);
    let mut kst = ui.state(r, &t);
    kst.disabled = v.is_none();
    lemon::field(ui.c, r, kst);
    if let Some(v) = v {
        let col = if kst.hover || kst.pressed { brand::TEXT } else { brand::MAIN };
        let s = f.text(v);
        ui.c.text_center(SMALL.bold(), r, col, &s);
    }
    inst::caption(ui.c, ui.t, cx, y + 16.0, caption, v.is_none());
    ui.hit(r, t);
}

/// A knob (hit area 40 px across), caption under it.
fn knob(ui: &mut Ui, cx: f32, cy: f32, f: Field, v: Option<i32>, caption: &str) {
    let t = Target::Value(f);
    let hr = Rect::new(cx - KNOB_R - 3.0, cy - KNOB_R - 3.0, 2.0 * KNOB_R + 6.0, 2.0 * KNOB_R + 6.0);
    let mut kst = ui.state(hr, &t);
    kst.disabled = v.is_none();
    let (lo, hi) = f.range();
    let norm = v.map_or(0.0, |v| (v.clamp(lo, hi) - lo) as f32 / (hi - lo) as f32);
    lemon::dial(ui.c, cx, cy, KNOB_R, 2.8, norm, f.bipolar(), brand::MAIN, kst);
    inst::caption(ui.c, ui.t, cx, cy + KNOB_R + 8.0, caption, v.is_none());
    ui.hit(hr, t);
}

/// The master: the envelope's knobs and the filter's.
fn master_card(ui: &mut Ui, p: &Program) {
    titled(ui, MASTER_CARD, "MASTER");
    let v = |f: Field| value(p, 0, f);
    knob(ui, MASTER_X[0], MASTER_KNOB_Y, Field::MasterGain, v(Field::MasterGain), "VOLUME");
    knob(ui, MASTER_X[1], MASTER_KNOB_Y, Field::Attack, v(Field::Attack), "ATTACK");
    knob(ui, MASTER_X[2], MASTER_KNOB_Y, Field::Decay, v(Field::Decay), "DECAY");
    knob(ui, MASTER_X[3], MASTER_KNOB_Y, Field::Sustain, v(Field::Sustain), "SUSTAIN");
    knob(ui, MASTER_X[1], MASTER_KNOB_Y + 46.0, Field::Release, v(Field::Release), "RELEASE");
    knob(ui, MASTER_X[2], MASTER_KNOB_Y + 46.0, Field::Cut, v(Field::Cut), "CUTOFF");
    knob(ui, MASTER_X[3], MASTER_KNOB_Y + 46.0, Field::Res, v(Field::Res), "RESO");
}

/// The chosen zone: its name and sample, its load and clear buttons, its key and velocity ranges and its levels.
fn zone_card(ui: &mut Ui, p: &Program, zone: usize) {
    titled(ui, ZONE_CARD, &format!("ZONE {}", zone + 1));
    let z = p.zone(zone);
    let has = z.loaded();
    lemon::field(ui.c, NAME, Kst { disabled: !has, ..Kst::NORMAL });
    let file = if has { file_name(&z.path).to_string() } else { String::new() };
    ui.c.text_clip(
        LABEL,
        NAME.x + 6.0,
        NAME.cy() - 7.0,
        NAME.w - 12.0,
        if has { brand::TEXT } else { plug::FAINT },
        if has { file.as_str() } else { "No sample: LOAD opens one" },
    );
    let v = |f: Field| value(p, zone, f);
    value_box(ui, BOX_X[0], BOX_Y, Field::Root, v(Field::Root), "ROOT");
    value_box(ui, BOX_X[1], BOX_Y, Field::Lo, v(Field::Lo), "LOW KEY");
    value_box(ui, BOX_X[2], BOX_Y, Field::Hi, v(Field::Hi), "HIGH KEY");
    value_box(ui, BOX_X[3], BOX_Y, Field::VelLo, v(Field::VelLo), "VEL LOW");
    value_box(ui, BOX_X[4], BOX_Y, Field::VelHi, v(Field::VelHi), "VEL HIGH");
    knob(ui, KNOB_X[0], KNOB_Y, Field::ZoneGain, v(Field::ZoneGain), "VOLUME");
    knob(ui, KNOB_X[1], KNOB_Y, Field::ZonePan, v(Field::ZonePan), "PAN");
    knob(ui, KNOB_X[2], KNOB_Y, Field::ZoneTune, v(Field::ZoneTune), "TUNE");
    // The load and clear buttons beside the knobs' captions' line, and the loop switch.
    let load = edges(KNOB_X[0] - 26.0, KNOB_X[0] + 26.0, KNOB_Y + 28.0, KNOB_Y + 46.0);
    let kst = ui.state(load, &Target::Load);
    let ink = lemon::plug_button(ui.c, load, 5.0, kst);
    ui.c.text_center(PILL, load, ink, "LOAD");
    ui.hit(load, Target::Load);
    let clear = edges(KNOB_X[1] - 26.0, KNOB_X[1] + 26.0, KNOB_Y + 28.0, KNOB_Y + 46.0);
    let kst = ui.state(clear, &Target::Clear);
    let ink = lemon::plug_button(ui.c, clear, 5.0, kst);
    ui.c.text_center(PILL, clear, ink, "CLEAR");
    ui.hit(clear, Target::Clear);
    let loop_r = edges(KNOB_X[2] - 30.0, KNOB_X[2] + 30.0, KNOB_Y + 28.0, KNOB_Y + 46.0);
    let t = Target::Loop;
    let mut kst = ui.state(loop_r, &t);
    kst.on = z.loop_on;
    let ink = lemon::plug_button(ui.c, loop_r, 5.0, kst);
    ui.c.text_center(PILL, loop_r, ink, "LOOP");
    ui.hit(loop_r, t);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every card and row sits on the 6 px grid, inside the editor, 6 px from its neighbours.
    #[test]
    fn the_layout_keeps_its_grid() {
        assert_eq!((W % 6, H % 6), (0, 0));
        for r in [LOGO, LIST, MASTER_CARD, ZONE_CARD, SPREAD] {
            assert!(r.x >= GAP && r.right() <= RIGHT && r.y >= GAP && r.bottom() <= BOTTOM, "{r:?}");
            assert_eq!(r.x % 6.0, 0.0, "{r:?}");
        }
        assert_eq!(MASTER_CARD.x - LIST.right(), GAP);
        assert_eq!(ZONE_CARD.y - MASTER_CARD.bottom(), GAP);
        assert_eq!(SPREAD.x - LOGO.right() > GAP, true, "the spread button clears the emblem");
        // The rows start at the well's top-left with 6 px around and between them; the spare bottom carries the
        // hint.
        let (r1, r8) = (row_rect(0), row_rect(7));
        assert_eq!((r1.x - LIST.x, r1.y - LIST.y), (GAP, GAP));
        assert!(LIST.bottom() - r8.bottom() >= GAP, "the rows keep the well's margin: {} px spare", LIST.bottom() - r8.bottom());
        assert_eq!(row_rect(1).y - r1.bottom(), GAP);
        assert_eq!((key_name(60), key_name(36)), ("C5".into(), "C3".into()));
        assert_eq!((Field::ZonePan.text(-64), Field::ZonePan.text(0)), ("100% left".into(), "Centered".into()));
        assert_eq!(Field::ZoneTune.text(25), "+25 cents");
    }

    /// A program's fields read and its ranges hold. A fresh program's zones are empty: their fields are
    /// disabled until a sample lands.
    #[test]
    fn the_fields_read_the_program() {
        let p = Program::default();
        assert_eq!(value(&p, 0, Field::MasterGain), Some(256));
        assert_eq!(value(&p, 0, Field::Cut), Some(128));
        assert_eq!(value(&p, 0, Field::Root), None, "an empty program's zones");
        assert_eq!(value(&p, 9, Field::Root), None, "beyond the eight zones");
        let mut p = Program::default();
        p.zones = vec![crate::program::Zone::default()];
        assert_eq!(value(&p, 0, Field::Root), Some(60));
        assert_eq!(value(&p, 0, Field::ZoneGain), Some(128));
    }
}
