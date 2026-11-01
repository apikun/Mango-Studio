// Copyright (C) 2026 battlemountain2
// Copyright (C) 2026 Artig3nce
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Crusher: battlemountain2's distortion (from their fork's "Dariacore" effects) as a small effect. One sound,
//! four modes: Drive (soft overdrive), Clip (hard clipping), Fold (a wavefolder) and Crush (fewer bits and a lower
//! sample rate). Drive pushes the signal into the shape, Bits and Rate are the crusher's depth and decimation, then
//! the mix with the dry signal and the output level. Not a recreation of an FL plugin: its state is its own.

use nih_plug::prelude::*;
use os_ui::{Canvas, Rect};

use crate::params::{db_text, leading_number, nearest, parse_db};
use crate::{view, Card, Control, Display, Kind, Mark, Setting, SmallFx, Sound, Spec};

pub const MODE: usize = 0;
pub const DRIVE: usize = 1;
pub const BITS: usize = 2;
pub const RATE: usize = 3;
pub const MIX: usize = 4;
pub const POST: usize = 5;

/// The modes, in the switch's order.
pub const DRIVE_MODE: i32 = 0;
pub const CLIP_MODE: i32 = 1;
pub const FOLD_MODE: i32 = 2;
pub const CRUSH_MODE: i32 = 3;

pub const SETTINGS: [Setting; 6] = [
    Setting { id: "mode", name: "Mode", range: (0, 3), start: CRUSH_MODE, magnet: None },
    // Tenths: 1.0x to 10.0x.
    Setting { id: "drive", name: "Drive", range: (10, 100), start: 15, magnet: None },
    Setting { id: "bits", name: "Bits", range: (2, 16), start: 8, magnet: None },
    // Every sample (1) to one in 32 held.
    Setting { id: "rate", name: "Rate", range: (1, 32), start: 4, magnet: None },
    Setting { id: "mix", name: "Mix", range: (0, 100), start: 100, magnet: None },
    // Percent of unity: 100 = 0 dB.
    Setting { id: "post", name: "Output", range: (0, 200), start: 90, magnet: Some(100) },
];

const CARDS: [Card; 2] = [
    Card { x0: 8.0, x1: 408.0, y0: 28.0, y1: 124.0, heading: "CRUSHER" },
    Card { x0: 414.0, x1: 564.0, y0: 28.0, y1: 154.0, heading: "CURVE" },
];

const CONTROLS: [Control; 6] = [
    Control::Choice { id: "mode", x: 18.0, top: 44.0, names: &["Drive", "Clip", "Fold", "Crush"] },
    Control::Knob { id: "drive", cx: 124.0, cy: 64.0, r: 20.0, caption: "DRIVE", bipolar: false },
    Control::Knob { id: "bits", cx: 180.0, cy: 64.0, r: 20.0, caption: "BITS", bipolar: false },
    Control::Knob { id: "rate", cx: 236.0, cy: 64.0, r: 20.0, caption: "RATE", bipolar: false },
    Control::Knob { id: "mix", cx: 292.0, cy: 64.0, r: 20.0, caption: "MIX", bipolar: false },
    Control::Knob { id: "post", cx: 362.0, cy: 64.0, r: 20.0, caption: "OUTPUT", bipolar: false },
];

pub const SPEC: &Spec = &Spec {
    name: "Mango Crusher",
    word: "CRUSHER",
    about: "Mango Studio's crusher",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    blank: None,
    locate: None,
    size: (572, 162),
    cards: &CARDS,
    controls: &CONTROLS,
    display: Some(Display { r: Rect::new(422.0, 36.0, 134.0, 110.0), draw: curve }),
    mark: Mark { x: 408.0, y: 143.0, anchor: 1.0, stacked: false },
    hint: None,
    labels: &[],
    text,
    parse,
    shown: None,
    sound,
    rings: false,
};

fn text(k: usize, v: i32) -> String {
    match k {
        MODE => ["Drive", "Clip", "Fold", "Crush"][v.clamp(0, 3) as usize].into(),
        DRIVE => format!("{:.1}x", f64::from(v) / 10.0),
        BITS => format!("{v} bits"),
        RATE if v <= 1 => "Full".into(),
        RATE => format!("1/{v}"),
        MIX => format!("{v}%"),
        _ => db_text(v as f32 / 100.0),
    }
}

fn parse(k: usize, s: &str) -> Option<i32> {
    let range = SETTINGS[k].range;
    match k {
        MODE => ["drive", "clip", "fold", "crush"].iter().position(|m| s.trim().eq_ignore_ascii_case(m)).map(|i| i as i32),
        DRIVE => nearest(range, leading_number(s)?, |v| Some(f64::from(v) / 10.0)),
        RATE if s.trim().eq_ignore_ascii_case("full") => Some(1),
        RATE => {
            let n = leading_number(s.trim().trim_start_matches("1/"))?;
            Some((n.round() as i32).clamp(range.0, range.1))
        }
        POST => parse_db(range, s, |v| v as f32 / 100.0),
        _ => Some((leading_number(s)?.round() as i32).clamp(range.0, range.1)),
    }
}

/// The sound: battlemountain2's `Distortion` (their fork's os-engine `effects::distortion`), set from the settings.
pub struct Crusher {
    mode: i32,
    drive: f32,
    levels: f32,
    rate: u32,
    mix: f32,
    post: f32,
    hold: u32,
    held: (f32, f32),
}

impl Crusher {
    pub fn new(v: &[i32]) -> Self {
        let mut c = Crusher { mode: CRUSH_MODE, drive: 1.5, levels: 256.0, rate: 4, mix: 1.0, post: 0.9, hold: 0, held: (0.0, 0.0) };
        c.set(v);
        c
    }

    pub fn set(&mut self, v: &[i32]) {
        let get = |k: usize| v.get(k).copied().unwrap_or(SETTINGS[k].start).clamp(SETTINGS[k].range.0, SETTINGS[k].range.1);
        self.mode = get(MODE);
        self.drive = get(DRIVE) as f32 / 10.0;
        self.levels = 2f32.powi(get(BITS));
        self.rate = get(RATE) as u32;
        self.mix = get(MIX) as f32 / 100.0;
        self.post = get(POST) as f32 / 100.0;
    }

    /// One sample through the mode's shape (before the mix and the output level).
    pub fn shape(&self, x: f32) -> f32 {
        let d = x * self.drive;
        match self.mode {
            DRIVE_MODE => d / (1.0 + d.abs()),
            CLIP_MODE => d.clamp(-1.0, 1.0),
            FOLD_MODE => ((d * std::f32::consts::FRAC_PI_2).sin() + d.clamp(-1.0, 1.0) * 0.5) * 0.7,
            _ => (d.clamp(-1.0, 1.0) * self.levels).round() / self.levels,
        }
    }

    /// One sample as heard, without the rate's hold (the curve).
    pub fn sample(&self, x: f32) -> f32 {
        x * (1.0 - self.mix) + self.shape(x) * self.post * self.mix
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        // The rate's hold is the crusher's: the other modes play every sample.
        let rate = if self.mode == CRUSH_MODE { self.rate.max(1) } else { 1 };
        let (dry, wet) = (1.0 - self.mix, self.post * self.mix);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.hold == 0 {
                self.held = (self.shape(*l), self.shape(*r));
            }
            self.hold = (self.hold + 1) % rate;
            *l = *l * dry + self.held.0 * wet;
            *r = *r * dry + self.held.1 * wet;
        }
    }

    pub fn reset(&mut self) {
        self.hold = 0;
        self.held = (0.0, 0.0);
    }
}

fn curve(c: &mut Canvas, r: Rect, v: &[i32]) {
    let d = Crusher::new(v);
    view::transfer(c, r, 1.5, 1.25, |x| d.sample(x));
}

struct Crush(Crusher);

impl Sound for Crush {
    fn set(&mut self, v: &[i32]) {
        self.0.set(v);
    }
    fn set_sample_rate(&mut self, _rate: u32) {}
    fn reset(&mut self) {
        self.0.reset();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.0.process(left, right);
    }
}

fn sound(_rate: u32, v: &[i32]) -> Box<dyn Sound> {
    Box::new(Crush(Crusher::new(v)))
}

pub struct CrusherKind;

impl Kind for CrusherKind {
    const SPEC: &'static Spec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-crusher";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's crusher";
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Distortion, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioCrush";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Distortion];
}

pub type MangoCrusher = SmallFx<CrusherKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!([text(MODE, 2), text(DRIVE, 35), text(BITS, 4), text(RATE, 1), text(RATE, 8), text(MIX, 75)], ["Fold", "3.5x", "4 bits", "Full", "1/8", "75%"]);
        assert_eq!(text(POST, 100), "0.0dB");
        assert_eq!([parse(MODE, "crush"), parse(DRIVE, "3.5"), parse(RATE, "1/8"), parse(RATE, "full"), parse(POST, "0dB")], [Some(3), Some(35), Some(8), Some(1), Some(100)]);
    }

    /// Crush: 2 bits round to quarters, and a rate of 2 holds every other sample. Fold folds a loud signal back
    /// under full scale; Drive keeps it under 1.
    #[test]
    fn each_mode_shapes_the_sound() {
        let mut c = Crusher::new(&[CRUSH_MODE, 10, 2, 2, 100, 100]);
        let mut l = vec![0.1f32, 0.9, 0.4, -0.7];
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        assert_eq!(l, [0.0, 0.0, 0.5, 0.5]);
        let fold = Crusher::new(&[FOLD_MODE, 30, 8, 1, 100, 100]);
        assert!(fold.shape(1.0).abs() < 1.0 && fold.shape(1.0) != fold.shape(0.5));
        let drive = Crusher::new(&[DRIVE_MODE, 100, 8, 1, 100, 100]);
        assert!(drive.shape(5.0) < 1.0 && drive.shape(5.0) > 0.9);
        // Mix 0: the dry signal.
        assert_eq!(Crusher::new(&[CRUSH_MODE, 50, 2, 1, 0, 100]).sample(0.3), 0.3);
    }
}
