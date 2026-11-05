// Copyright (C) 2026 Artig3nce, apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Master's editor picture: the top row with Auto Master and its targets, the chain's stages left to right as
//! cards of knobs, and the meters down the right. All painting, and the places the editor's hit tests ask for.

use os_theme::design::lemon as brand;
use os_ui::kit::lemon::{self, plug};
use os_ui::kit::{inst, State};
use os_ui::{Canvas, Color, Font, Rect};

pub const W: u32 = 900;
pub const H: u32 = 430;

/// A meter reading past its limit.
const WARN: Color = Color::rgb(0xe5573f);
const HEADING: Font = Font::condensed(11, 0).bold();
const PILL: Font = Font::new(12).bold();

/// A knob: its parameter, its caption and its centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knob {
    pub id: &'static str,
    pub label: &'static str,
    pub x: f32,
    pub y: f32,
    /// Its arc starts at the middle.
    pub bipolar: bool,
}

pub const KNOB_R: f32 = 21.0;
const ROW_1: f32 = 150.0;
const ROW_2: f32 = 275.0;

const fn knob(id: &'static str, label: &'static str, x: f32, y: f32, bipolar: bool) -> Knob {
    Knob { id, label, x, y, bipolar }
}

pub const KNOBS: [Knob; 13] = [
    knob("eq_low", "Low", 50.0, ROW_1, true),
    knob("eq_low_mid", "Low Mid", 118.0, ROW_1, true),
    knob("eq_mid", "Mid", 186.0, ROW_1, true),
    knob("eq_high_mid", "High Mid", 84.0, ROW_2, true),
    knob("eq_high", "High", 152.0, ROW_2, true),
    knob("comp_low", "Low", 268.0, ROW_1, false),
    knob("comp_mid", "Mid", 336.0, ROW_1, false),
    knob("comp_high", "High", 268.0, ROW_2, false),
    knob("ratio", "Ratio", 336.0, ROW_2, false),
    knob("width", "Width", 424.0, ROW_1, true),
    knob("saturation", "Drive", 508.0, ROW_1, false),
    knob("gain", "Gain", 592.0, ROW_1, false),
    knob("ceiling", "Ceiling", 592.0, ROW_2, false),
];

const CARDS: [(&str, Rect); 5] = [
    ("EQ", Rect::new(16.0, 100.0, 204.0, 250.0)),
    ("COMPRESSOR", Rect::new(232.0, 100.0, 140.0, 250.0)),
    ("WIDTH", Rect::new(384.0, 100.0, 80.0, 250.0)),
    ("SATURATE", Rect::new(476.0, 100.0, 64.0, 250.0)),
    ("MAXIMIZER", Rect::new(552.0, 100.0, 80.0, 250.0)),
];

pub const MONO_BASS: Rect = Rect::new(392.0, 262.0, 64.0, 26.0);
pub const AUTO: Rect = Rect::new(16.0, 52.0, 168.0, 36.0);
pub const COMPARE: Rect = Rect::new(538.0, 56.0, 94.0, 28.0);
pub const TARGETS_AT: [Rect; 4] = [
    Rect::new(196.0, 56.0, 80.0, 28.0),
    Rect::new(280.0, 56.0, 80.0, 28.0),
    Rect::new(364.0, 56.0, 80.0, 28.0),
    Rect::new(448.0, 56.0, 80.0, 28.0),
];
const METERS: Rect = Rect::new(644.0, 52.0, 240.0, 298.0);
pub const RESET_METERS: Rect = Rect::new(790.0, 56.0, 88.0, 24.0);

/// The knob under a point.
pub fn knob_at(x: f32, y: f32) -> Option<&'static Knob> {
    KNOBS.iter().find(|k| (x - k.x).hypot(y - k.y) <= KNOB_R + 4.0)
}

/// What the picture shows.
pub struct Display<'a> {
    pub knobs: Vec<(f32, String)>,
    pub target: usize,
    pub targets: [&'a str; 4],
    pub mono_bass: bool,
    pub compare: bool,
    pub listening: bool,
    pub solving: bool,
    /// Seconds listened to, of how many.
    pub listened: (f32, f32),
    pub can_finish: bool,
    pub status: &'a str,
    pub momentary: f32,
    pub integrated: f32,
    pub true_peak: f32,
    pub reduction: f32,
    pub correlation: f32,
    pub ceiling: f32,
    pub target_lufs: f32,
    pub hover: Option<(f32, f32)>,
}

/// A raised button as the mixer's: lit mango when `on`, a grey face otherwise.
fn button(cv: &mut Canvas, r: Rect, on: bool, hover: bool, label: &str) {
    let ink = lemon::plug_button(cv, r, 6.0, State { hover, on, ..State::NORMAL });
    cv.text_center(PILL, r, ink, label);
}

/// A knob as the plugins' and the mixer's bars: a dark slot filled with one flat mango colour. A compressor's threshold
/// hangs from the top: the lower it is the more of the slot is lit, and it is empty at Off.
fn paint_knob(cv: &mut Canvas, t: &os_theme::Palette, k: &Knob, norm: f32, text: &str, hover: bool) {
    let st = State { hover, ..State::NORMAL };
    if k.id.starts_with("comp_") {
        lemon::dial(cv, k.x, k.y, KNOB_R, 2.8, 0.0, false, brand::MAIN, State { disabled: true, ..st });
        let w = (KNOB_R * 0.9).clamp(6.0, 16.0).round();
        let slot = Rect::new((k.x - w / 2.0).round(), (k.y - KNOB_R).round(), w, (2.0 * KNOB_R).round());
        let inner = slot.inset(2.0);
        let h = ((1.0 - norm.clamp(0.0, 1.0)) * inner.h).round();
        if h >= 1.0 {
            let col = if hover { brand::MAIN.mix(Color::rgb(0xffffff), 0.18) } else { brand::MAIN };
            cv.fill_rrect(Rect::new(inner.x, inner.y, inner.w, h), (inner.w / 2.0).min(h / 2.0), col);
        }
    } else {
        lemon::dial(cv, k.x, k.y, KNOB_R, 2.8, norm, k.bipolar, brand::MAIN, st);
    }
    inst::caption(cv, t, k.x, k.y + KNOB_R + 6.0, k.label, false);
    cv.text_center(Font::new(10), Rect::new(k.x - 33.0, k.y + KNOB_R + 21.0, 66.0, 13.0), plug::FAINT, text);
}

/// A horizontal meter from `lo` to `hi`: a rounded trough with a flat mango bar, its value in the corner, and a tick where
/// the target or the ceiling is.
#[allow(clippy::too_many_arguments)]
fn meter(cv: &mut Canvas, y: f32, label: &str, value: f32, text: &str, lo: f32, hi: f32, mark: Option<f32>, warn: bool) {
    let x = METERS.x + 14.0;
    let w = METERS.w - 28.0;
    cv.text(Font::new(11), x, y, plug::DIM, label);
    cv.text_right(Font::new(13), x + w, y - 1.0, if warn { WARN } else { brand::TEXT }, text);
    let bar = Rect::new(x, y + 19.0, w, 8.0);
    cv.fill_rrect(bar, 4.0, Color::rgb(0x141618));
    let k = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
    if k > 0.0 {
        cv.fill_rrect(Rect::new(bar.x, bar.y, (bar.w * k).max(8.0), bar.h), 4.0, if warn { WARN } else { brand::MAIN });
    }
    if let Some(m) = mark {
        let at = ((m - lo) / (hi - lo)).clamp(0.0, 1.0);
        cv.fill_rect(Rect::new((bar.x + bar.w * at - 1.0).round(), bar.y - 3.0, 2.0, bar.h + 6.0), plug::DIM);
    }
}

fn number(v: f32, unit: &str) -> String {
    if v <= -69.9 { format!("-inf {unit}") } else { format!("{v:.1} {unit}") }
}

/// A card's heading: letter-spaced, centred, as the other plugins' cards have.
fn heading(cv: &mut Canvas, r: Rect, name: &str) {
    // Tighter in a narrow card.
    let track = if lemon::spaced_width(cv, HEADING, name, 2.0) > r.w - 14.0 { 0.5 } else { 2.0 };
    let w = lemon::spaced_width(cv, HEADING, name, track);
    let lh = cv.line_height(HEADING);
    lemon::spaced(cv, HEADING, (r.cx() - w / 2.0).round(), (r.y + 14.0 - lh / 2.0).round(), plug::DIM, name, track);
}

pub fn paint(cv: &mut Canvas, d: &Display) {
    let t = os_plugkit::palette();
    let hover = |r: Rect| d.hover.is_some_and(|(x, y)| r.contains(x, y));
    cv.fill_rect(Rect::new(0.0, 0.0, W as f32, H as f32), plug::BG);
    lemon::wordmark(cv, 16.0, 7.0, true, "MASTER", Font::condensed(20, 0).bold(), Font::condensed(10, 0).bold());
    let label = if d.solving {
        "Working it out...".to_string()
    } else if d.listening {
        if d.can_finish { format!("Done listening ({:.0} s)", d.listened.0) } else { format!("Listening... {:.0} s", d.listened.0) }
    } else {
        "Auto Master".to_string()
    };
    button(cv, AUTO, d.listening || d.solving, hover(AUTO), &label);
    if d.listening {
        let k = (d.listened.0 / d.listened.1.max(1.0)).clamp(0.0, 1.0);
        cv.fill_rect(Rect::new(AUTO.x + 14.0, AUTO.bottom() - 6.0, (AUTO.w - 28.0) * k, 2.0), brand::COLORED_DARK);
    }
    for (i, r) in TARGETS_AT.iter().enumerate() {
        button(cv, *r, d.target == i, hover(*r), d.targets[i]);
    }
    button(cv, COMPARE, d.compare, hover(COMPARE), if d.compare { "Original" } else { "Compare" });

    for (name, r) in CARDS {
        lemon::plug_panel(cv, r);
        heading(cv, r, name);
    }
    for (i, k) in KNOBS.iter().enumerate() {
        let over = d.hover.is_some_and(|(x, y)| (x - k.x).hypot(y - k.y) <= KNOB_R + 4.0);
        let (norm, text) = &d.knobs[i];
        paint_knob(cv, &t, k, *norm, text, over);
    }
    button(cv, MONO_BASS, d.mono_bass, hover(MONO_BASS), "Mono");
    inst::caption(cv, &t, MONO_BASS.cx(), MONO_BASS.bottom() + 6.0, "BASS", false);

    lemon::plug_panel(cv, METERS);
    let x = METERS.x + 14.0;
    cv.text(HEADING, x, METERS.y + 10.0, plug::DIM, "OUTPUT");
    button(cv, RESET_METERS, false, hover(RESET_METERS), "Reset");
    cv.text(Font::new(11), x, METERS.y + 40.0, plug::DIM, "Loudness");
    let big = if d.integrated <= -69.9 { "--".to_string() } else { format!("{:.1}", d.integrated) };
    cv.text(Font::new(34).bold(), x, METERS.y + 54.0, brand::TEXT, &big);
    cv.text_right(Font::new(11), METERS.right() - 14.0, METERS.y + 74.0, plug::DIM, &format!("LUFS, target {:.0}", d.target_lufs));
    let top = METERS.y + 110.0;
    meter(cv, top, "Now", d.momentary, &number(d.momentary, "LUFS"), -36.0, 0.0, Some(d.target_lufs), false);
    meter(cv, top + 46.0, "True peak", d.true_peak, &number(d.true_peak, "dB"), -24.0, 3.0, Some(d.ceiling), d.true_peak > d.ceiling + 0.1);
    meter(cv, top + 92.0, "Limiting", -d.reduction, &format!("{:.1} dB", -d.reduction), 0.0, 12.0, None, false);
    meter(cv, top + 138.0, "Stereo", d.correlation, &format!("{:+.2}", d.correlation), -1.0, 1.0, Some(0.0), d.correlation < -0.2);

    cv.text(Font::new(12), 16.0, 372.0, plug::DIM, d.status);
}
