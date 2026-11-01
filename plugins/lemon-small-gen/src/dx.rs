// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango DX: Mango Studio's small FM synthesizer (the roadmap's "Fruity DX10"). A carrier sine played by a
//! modulator of its own wave — its pitch a ratio of the note's, its depth its own little contour — with the
//! modulator's out feeding back into itself, and the amp envelope over it all. Four notes at once.
//!
// spec: plugins/lemon-small-generators.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::fruit_kick::volume_gain;
use crate::params::{db_text, leading_number, nearest, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

// ---------------------------------------------------------------------------------------------------------------
// The settings (spec §5): the stored order, their ranges and their starting values

pub const SETTINGS: [Setting; 12] = [
    Setting { id: "coarse", name: "Ratio", range: (0, 32), start: 5, magnet: None },
    Setting { id: "fine", name: "Fine", range: (0, 128), start: 64, magnet: Some(64) },
    Setting { id: "modwave", name: "Mod wave", range: (0, 3), start: 0, magnet: None },
    Setting { id: "feedback", name: "Feedback", range: (0, 128), start: 32, magnet: None },
    Setting { id: "index", name: "Index", range: (0, 128), start: 88, magnet: None },
    Setting { id: "iattack", name: "Index attack", range: (0, 128), start: 0, magnet: None },
    Setting { id: "idecay", name: "Index decay", range: (0, 128), start: 48, magnet: None },
    Setting { id: "attack", name: "Attack", range: (0, 128), start: 0, magnet: None },
    Setting { id: "decay", name: "Decay", range: (0, 128), start: 52, magnet: None },
    Setting { id: "sustain", name: "Sustain", range: (0, 128), start: 82, magnet: None },
    Setting { id: "release", name: "Release", range: (0, 128), start: 34, magnet: None },
    Setting { id: "volume", name: "Volume", range: (0, 512), start: 256, magnet: Some(256) },
];

pub const COARSE: usize = 0;
pub const FINE: usize = 1;
pub const MODWAVE: usize = 2;
pub const FEEDBACK: usize = 3;
pub const INDEX: usize = 4;
pub const IATTACK: usize = 5;
pub const IDECAY: usize = 6;
pub const ATTACK: usize = 7;
pub const DECAY: usize = 8;
pub const SUSTAIN: usize = 9;
pub const RELEASE: usize = 10;
pub const VOLUME: usize = 11;

/// Editor layout (spec §2): two rows of cards over the hint and the wordmark.
const CARDS: [Card; 5] = [
    Card { x0: 8.0, x1: 122.0, y0: 28.0, y1: 134.0, heading: "RATIO" },
    Card { x0: 128.0, x1: 262.0, y0: 28.0, y1: 134.0, heading: "MODULATOR" },
    Card { x0: 268.0, x1: 438.0, y0: 28.0, y1: 134.0, heading: "CONTOUR" },
    Card { x0: 8.0, x1: 230.0, y0: 140.0, y1: 246.0, heading: "AMPLIFIER" },
    Card { x0: 236.0, x1: 350.0, y0: 140.0, y1: 246.0, heading: "OUTPUT" },
];

const CONTROLS: [Control; 12] = [
    Control::Knob { id: "coarse", cx: 40.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "fine", cx: 90.0, cy: 86.0, r: 22.0, caption: "", bipolar: true },
    Control::Choice { id: "modwave", x: 136.0, top: 46.0, names: &["Sine", "Triangle", "Square", "Saw"] },
    Control::Knob { id: "feedback", cx: 236.0, cy: 86.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "index", cx: 300.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "iattack", cx: 350.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "idecay", cx: 400.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "attack", cx: 40.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "decay", cx: 92.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "sustain", cx: 144.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "release", cx: 196.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "volume", cx: 293.0, cy: 198.0, r: 24.0, caption: "", bipolar: false },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango DX",
    word: "DX",
    about: "Mango Studio's small FM synthesizer",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (446, 280),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark { x: 438.0, y: 263.0, anchor: 1.0, stacked: false },
    hint: Some(Rect::new(8.0, 252.0, 314.0, 22.0)),
    labels: &[],
    text,
    parse,
    make,
};

// ---------------------------------------------------------------------------------------------------------------
// Laws (spec §3): a knob's value as the number its text names, and a typed text back as a value

/// A share of a 0..128 knob as a percent.
fn percent(range: (i32, i32), v: i32) -> String {
    format!("{:.0}%", f64::from(v - range.0) / f64::from(range.1 - range.0) * 100.0)
}

fn percent_parse(range: (i32, i32), s: &str) -> Option<i32> {
    let x = leading_number(s)?;
    nearest(range, x, |v| {
        Some(f64::from(v - range.0) / f64::from(range.1 - range.0) * 100.0)
    })
}

/// The ratio knob's number: a quarter of the note's pitch to sixteen times it.
pub fn ratio(v: i32) -> f64 {
    0.25 * 2f64.powf(f64::from(v) / 32.0 * 6.0)
}

/// The fine knob's detune of the modulator, in cents (±50 around its middle).
pub fn fine_cents(v: i32) -> f64 {
    (f64::from(v) - 64.0) / 64.0 * 50.0
}

/// A time knob's seconds: instant at the bottom, 0.005s growing to 1.28s over the range.
pub fn time_seconds(v: i32) -> f64 {
    if v <= 0 {
        0.0
    } else {
        0.005 * 2f64.powf(f64::from(v) / 128.0 * 8.0)
    }
}

/// The time a value's text names (the number `nearest` matches a typed one against).
fn time_text_seconds(v: i32) -> f64 {
    match time_seconds(v) {
        0.0 => 0.0,
        t if t < 1.0 => (t * 1000.0).round() / 1000.0,
        t => (t * 10.0).round() / 10.0,
    }
}

/// A time as `instant`, `123ms` or `1.2s`.
fn time_text(v: i32) -> String {
    match time_seconds(v) {
        0.0 => "instant".into(),
        t if t < 1.0 => format!("{:.0}ms", t * 1000.0),
        t => format!("{:.1}s", t),
    }
}

fn time_parse(s: &str) -> Option<i32> {
    let s = s.trim().to_ascii_lowercase();
    if s.starts_with('i') {
        return Some(0);
    }
    let x = leading_number(&s)? / if s.ends_with("ms") { 1000.0 } else { 1.0 };
    nearest(SETTINGS[IATTACK].range, x, |v| Some(time_text_seconds(v)))
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        COARSE => {
            let r = ratio(v);
            if r >= 10.0 {
                format!("{r:.1}x")
            } else {
                format!("{r:.2}x")
            }
        }
        FINE => match fine_cents(v) {
            c if c == 0.0 => "0ct".into(),
            c => format!("{c:+.0}ct"),
        },
        MODWAVE => ["Sine", "Triangle", "Square", "Saw"][v.clamp(0, 3) as usize].to_string(),
        FEEDBACK | INDEX | SUSTAIN => percent(SETTINGS[k].range, v),
        IATTACK | IDECAY | ATTACK | DECAY | RELEASE => time_text(v),
        _ => db_text(volume_gain(v)),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    let s = s.trim().to_ascii_lowercase();
    match k {
        COARSE => {
            let x = leading_number(&s)?;
            nearest(SETTINGS[COARSE].range, x, |v| Some(ratio(v)))
        }
        FINE => {
            let x = leading_number(&s)?;
            nearest(SETTINGS[FINE].range, x, |v| Some(fine_cents(v)))
        }
        MODWAVE => {
            let names = ["sine", "triangle", "square", "saw"];
            names
                .iter()
                .position(|n| *n == s)
                .map(|v| v as i32)
                .or_else(|| leading_number(&s).map(|x| x.round() as i32))
                .map(|v| v.clamp(0, 3))
        }
        FEEDBACK | INDEX | SUSTAIN => percent_parse(SETTINGS[k].range, &s),
        IATTACK | IDECAY | ATTACK | DECAY | RELEASE => time_parse(&s),
        _ => parse_db(SETTINGS[VOLUME].range, &s, volume_gain),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The sound

/// The frequency of a key.
fn key_hz(key: u8) -> f32 {
    (440.0f64 * 2f64.powf((f64::from(key) - 69.0) / 12.0)) as f32
}

/// One note: the carrier, its modulator (its contour and its feedback) and the amp envelope.
struct Voice {
    key: u8,
    voice_id: Option<i32>,
    /// The carrier's and the modulator's places in their cycles, 0..1, and the feedback's last out.
    phase: f32,
    mod_phase: f32,
    fb: f32,
    /// The index contour: 1 at its peak (after its attack), falling over its decay.
    index: f32,
    index_up: bool,
    /// The amp envelope's level and stage.
    level: f32,
    stage: Stage,
    vel: f32,
    age: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Attack,
    Decay,
    Release,
}

struct Dx {
    rate: f32,
    ratio: f32,
    cents: f32,
    wave: usize,
    feedback: f32,
    index: f32,
    iattack: f32,
    idecay: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    gain: f32,
    voices: [Option<Voice>; 4],
    ended: Vec<(u8, Option<i32>)>,
}

impl Dx {
    /// The modulator's wave at its place in the cycle.
    fn wave(wave: usize, phase: f32) -> f32 {
        match wave {
            1 => 1.0 - 4.0 * (phase - phase.round().abs()).abs(),
            2 => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            3 => phase * 2.0 - 1.0,
            _ => (std::f32::consts::TAU * phase).sin(),
        }
    }
}

impl Instrument for Dx {
    fn set(&mut self, v: &[i32]) {
        self.ratio = ratio(v[COARSE]) as f32;
        self.cents = fine_cents(v[FINE]) as f32;
        self.wave = v[MODWAVE].clamp(0, 3) as usize;
        self.feedback = f64::from(v[FEEDBACK]) as f32 / 128.0;
        self.index = f64::from(v[INDEX]) as f32 / 128.0;
        self.iattack = time_seconds(v[IATTACK]) as f32;
        self.idecay = time_seconds(v[IDECAY]) as f32;
        self.attack = time_seconds(v[ATTACK]) as f32;
        self.decay = time_seconds(v[DECAY]) as f32;
        self.sustain = f64::from(v[SUSTAIN]) as f32 / 128.0;
        self.release = time_seconds(v[RELEASE]) as f32;
        self.gain = volume_gain(v[VOLUME]);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate as f32;
    }

    fn reset(&mut self) {
        self.voices = [const { None }; 4];
    }

    fn note_on(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        let slot = match self.voices.iter().position(|s| s.is_none()) {
            Some(i) => i,
            None => {
                let mut i = 0;
                let mut oldest = self.voices[0].as_ref().map_or(0.0, |s| s.age);
                for (j, s) in self.voices.iter().enumerate() {
                    let age = s.as_ref().map_or(0.0, |s| s.age);
                    if age > oldest {
                        oldest = age;
                        i = j;
                    }
                }
                if let Some(s) = self.voices[i].take() {
                    self.ended.push((s.key, s.voice_id));
                }
                i
            }
        };
        self.voices[slot] = Some(Voice {
            key,
            voice_id: Some(voice_id.unwrap_or(key as i32 + 1)),
            phase: 0.0,
            mod_phase: 0.0,
            fb: 0.0,
            index: if self.iattack <= 0.0 { 1.0 } else { 0.0 },
            index_up: self.iattack > 0.0,
            level: if self.attack <= 0.0 { 1.0 } else { 0.0 },
            stage: Stage::Attack,
            vel: velocity.clamp(0.05, 1.0),
            age: 0.0,
        });
    }

    fn note_off(&mut self, key: u8, voice_id: Option<i32>) {
        for s in self.voices.iter_mut().flatten() {
            if s.key == key && (voice_id.is_none() || s.voice_id == voice_id) {
                s.stage = Stage::Release;
            }
        }
    }

    fn choke(&mut self, key: u8) {
        for s in self.voices.iter_mut() {
            if let Some(s) = s.take()
                && s.key == key
            {
                self.ended.push((s.key, s.voice_id));
            }
        }
    }

    fn all_off(&mut self) {
        for s in self.voices.iter_mut() {
            if let Some(s) = s.take() {
                self.ended.push((s.key, s.voice_id));
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let dt = 1.0 / self.rate;
        let p = (
            self.ratio,
            self.cents,
            self.wave,
            self.feedback,
            self.index,
            self.iattack,
            self.idecay,
            self.attack,
            self.decay,
            self.sustain,
            self.release,
        );
        let gain = self.gain;
        // The index's depth: up to two turns of the carrier's phase.
        let depth = p.4 * std::f32::consts::TAU * 2.0;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut out = 0.0f32;
            for s in self.voices.iter_mut().flatten() {
                let f = key_hz(s.key);
                let fm = f * p.0 * 2f32.powf(p.1 / 1200.0);
                s.phase = (s.phase + f * dt).fract();
                s.mod_phase = (s.mod_phase + fm * dt).fract();
                // The modulator: its wave at its pitch, its contour on the depth, its feedback.
                let raw = Self::wave(p.2, s.mod_phase);
                let m = raw * depth * s.index + p.3 * s.fb * depth * 0.5;
                s.fb = raw.clamp(-1.5, 1.5);
                // The carrier, played by the modulator.
                out += (std::f32::consts::TAU * s.phase + m).sin() * s.level * s.vel;
                // The index contour: its attack to the peak, then its decay to silence.
                if s.index_up {
                    s.index += dt / p.5.max(0.001);
                    if s.index >= 1.0 {
                        s.index = 1.0;
                        s.index_up = false;
                    }
                } else {
                    s.index *= (-dt / (p.6 / 4.0).max(0.001)).exp();
                }
                // The amp envelope.
                match s.stage {
                    Stage::Attack => {
                        s.level += dt / p.7.max(0.001);
                        if s.level >= 1.0 {
                            s.level = 1.0;
                            s.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        s.level = p.9 + (s.level - p.9) * (-dt / (p.8 / 4.0).max(0.001)).exp();
                    }
                    Stage::Release => {
                        s.level *= (-dt / (p.10 / 4.0).max(0.001)).exp();
                    }
                }
                s.age += dt;
            }
            let out = out * gain * 0.5;
            *l += out;
            *r += out;
        }
        // The voices that faded, or a note held far past any envelope.
        for s in self.voices.iter_mut() {
            let gone = s
                .as_ref()
                .is_some_and(|v| v.stage == Stage::Release && v.level < 1e-4 || v.age > 30.0);
            if gone && let Some(v) = s.take() {
                self.ended.push((v.key, v.voice_id));
            }
        }
    }

    fn take_ended(&mut self) -> Vec<(u8, Option<i32>)> {
        std::mem::take(&mut self.ended)
    }

    fn has_voices(&self) -> bool {
        self.voices.iter().any(|s| s.is_some())
    }
}

fn make(rate: u32, v: &[i32]) -> Box<dyn Instrument> {
    let mut d = Dx {
        rate: rate as f32,
        ratio: ratio(v[COARSE]) as f32,
        cents: fine_cents(v[FINE]) as f32,
        wave: v[MODWAVE].clamp(0, 3) as usize,
        feedback: f64::from(v[FEEDBACK]) as f32 / 128.0,
        index: f64::from(v[INDEX]) as f32 / 128.0,
        iattack: time_seconds(v[IATTACK]) as f32,
        idecay: time_seconds(v[IDECAY]) as f32,
        attack: time_seconds(v[ATTACK]) as f32,
        decay: time_seconds(v[DECAY]) as f32,
        sustain: f64::from(v[SUSTAIN]) as f32 / 128.0,
        release: time_seconds(v[RELEASE]) as f32,
        gain: volume_gain(v[VOLUME]),
        voices: [const { None }; 4],
        ended: Vec::new(),
    };
    d.set_sample_rate(rate);
    Box::new(d)
}

pub struct DxKind;

impl Kind for DxKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-dx";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's small FM synthesizer";
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioDX10\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type MangoDx = SmallGen<DxKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(COARSE, 0), "0.25x");
        assert_eq!(parse(COARSE, "0.25x"), Some(0));
        assert_eq!(text(COARSE, 32), "16.0x");
        assert_eq!(parse(COARSE, "16x"), Some(32));
        assert_eq!(text(FINE, 64), "0ct");
        assert_eq!(parse(FINE, "0ct"), Some(64));
        assert_eq!(text(FINE, 96), "+25ct");
        assert_eq!(parse(FINE, "+25ct"), Some(96));
        assert_eq!(text(MODWAVE, 2), "Square");
        assert_eq!(parse(MODWAVE, "square"), Some(2));
        assert_eq!(text(ATTACK, 0), "instant");
        assert_eq!(parse(ATTACK, "instant"), Some(0));
        assert_eq!(text(RELEASE, 34), "22ms");
        assert_eq!(parse(RELEASE, "22ms"), Some(34));
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
    }

    /// A note sounds, and the index changes its tone: the same note with the index at zero is a
    /// plain sine, and the two waveforms differ.
    #[test]
    fn a_note_sounds_and_its_index_colours_it() {
        let mut a = make(44100, &SPEC.start());
        let mut v = SPEC.start();
        v[INDEX] = 0;
        let mut b = make(44100, &v);
        a.note_on(60, 1.0, Some(1));
        b.note_on(60, 1.0, Some(1));
        let mut la = vec![0.0f32; 256];
        let mut ra = vec![0.0f32; 256];
        let mut lb = vec![0.0f32; 256];
        let mut rb = vec![0.0f32; 256];
        a.render(&mut la, &mut ra);
        b.render(&mut lb, &mut rb);
        let peak_a = la.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        let peak_b = lb.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!(peak_a > 0.01 && peak_b > 0.01, "both notes sound ({peak_a}, {peak_b})");
        let diff = la
            .iter()
            .zip(lb.iter())
            .fold(0.0f32, |a, (x, y)| a.max((x - y).abs()));
        assert!(diff > 1e-3, "the index colours the tone ({diff})");
    }

    /// A release ends the note; a fifth steals the oldest.
    #[test]
    fn the_release_ends_the_note() {
        let mut d = make(44100, &SPEC.start());
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        d.note_on(48, 1.0, Some(1));
        d.note_off(48, Some(1));
        let mut blocks = 0;
        while d.has_voices() && blocks < 2000 {
            l.iter_mut().for_each(|x| *x = 0.0);
            d.render(&mut l, &mut r);
            blocks += 1;
        }
        assert!(!d.has_voices(), "the release ends the note");
        assert_eq!(d.take_ended(), vec![(48, Some(1))]);
        for (i, key) in [48u8, 52, 56, 60].into_iter().enumerate() {
            d.note_on(key, 1.0, Some(i as i32 + 1));
            d.render(&mut l, &mut r);
        }
        d.note_on(64, 1.0, Some(5));
        assert!(d.take_ended().contains(&(48, Some(1))), "the oldest note ended");
    }
}
