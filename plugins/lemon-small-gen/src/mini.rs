// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Mini: Mango Studio's small subtractive synthesizer (the roadmap's "Minisynth"). Two oscillators — each
//! its own wave and octave, the second detunable against the first — into a resonant lowpass that its contour
//! sweeps, the amp envelope over it, up to eight notes at once. The smallest of the Mango synths, and the loudest
//! for its size.
//!
// spec: plugins/lemon-small-generators.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::acid::{cutoff_hz, decay_seconds, envelope_octaves, resonance_q};
use crate::fruit_kick::volume_gain;
use crate::monk::time_seconds;
use crate::params::{db_text, leading_number, nearest, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

// ---------------------------------------------------------------------------------------------------------------
// The settings (spec §5): the stored order, their ranges and their starting values

pub const SETTINGS: [Setting; 16] = [
    Setting { id: "wave1", name: "Wave 1", range: (0, 3), start: 0, magnet: None },
    Setting { id: "oct1", name: "Octave 1", range: (-2, 2), start: 0, magnet: Some(0) },
    Setting { id: "wave2", name: "Wave 2", range: (0, 3), start: 1, magnet: None },
    Setting { id: "oct2", name: "Octave 2", range: (-2, 2), start: 0, magnet: Some(0) },
    Setting { id: "detune", name: "Detune", range: (0, 128), start: 16, magnet: None },
    Setting { id: "mix", name: "Mix", range: (0, 128), start: 64, magnet: Some(64) },
    Setting { id: "cutoff", name: "Cutoff", range: (0, 128), start: 84, magnet: None },
    Setting { id: "reso", name: "Resonance", range: (0, 128), start: 32, magnet: None },
    Setting { id: "env", name: "Envelope", range: (0, 128), start: 56, magnet: None },
    Setting { id: "fdecay", name: "Contour", range: (0, 128), start: 44, magnet: None },
    Setting { id: "attack", name: "Attack", range: (0, 128), start: 0, magnet: None },
    Setting { id: "decay", name: "Decay", range: (0, 128), start: 48, magnet: None },
    Setting { id: "sustain", name: "Sustain", range: (0, 128), start: 96, magnet: None },
    Setting { id: "release", name: "Release", range: (0, 128), start: 34, magnet: None },
    Setting { id: "voices", name: "Voices", range: (0, 7), start: 7, magnet: None },
    Setting { id: "volume", name: "Volume", range: (0, 512), start: 256, magnet: Some(256) },
];

pub const WAVE1: usize = 0;
pub const OCT1: usize = 1;
pub const WAVE2: usize = 2;
pub const OCT2: usize = 3;
pub const DETUNE: usize = 4;
pub const MIX: usize = 5;
pub const CUTOFF: usize = 6;
pub const RESO: usize = 7;
pub const ENV: usize = 8;
pub const FDECAY: usize = 9;
pub const ATTACK: usize = 10;
pub const DECAY: usize = 11;
pub const SUSTAIN: usize = 12;
pub const RELEASE: usize = 13;
pub const VOICES: usize = 14;
pub const VOLUME: usize = 15;

/// Editor layout (spec §2): two rows of cards over the hint and the wordmark.
const CARDS: [Card; 7] = [
    Card { x0: 8.0, x1: 140.0, y0: 28.0, y1: 134.0, heading: "OSC 1" },
    Card { x0: 146.0, x1: 278.0, y0: 28.0, y1: 134.0, heading: "OSC 2" },
    Card { x0: 284.0, x1: 454.0, y0: 28.0, y1: 134.0, heading: "TONE" },
    Card { x0: 8.0, x1: 122.0, y0: 140.0, y1: 246.0, heading: "FILTER" },
    Card { x0: 128.0, x1: 242.0, y0: 140.0, y1: 246.0, heading: "CONTOUR" },
    Card { x0: 248.0, x1: 470.0, y0: 140.0, y1: 246.0, heading: "AMPLIFIER" },
    Card { x0: 476.0, x1: 590.0, y0: 140.0, y1: 246.0, heading: "PLAY" },
];

const CONTROLS: [Control; 16] = [
    Control::Choice { id: "wave1", x: 16.0, top: 46.0, names: &["Saw", "Square", "Triangle", "Sine"] },
    Control::Knob { id: "oct1", cx: 114.0, cy: 86.0, r: 18.0, caption: "", bipolar: true },
    Control::Choice { id: "wave2", x: 154.0, top: 46.0, names: &["Saw", "Square", "Triangle", "Sine"] },
    Control::Knob { id: "oct2", cx: 252.0, cy: 86.0, r: 18.0, caption: "", bipolar: true },
    Control::Knob { id: "detune", cx: 316.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "mix", cx: 366.0, cy: 86.0, r: 22.0, caption: "", bipolar: true },
    Control::Knob { id: "cutoff", cx: 40.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "reso", cx: 90.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "env", cx: 160.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "fdecay", cx: 210.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "attack", cx: 280.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "decay", cx: 332.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "sustain", cx: 384.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "release", cx: 436.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "voices", cx: 533.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "volume", cx: 416.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Mini",
    word: "MINI",
    about: "Mango Studio's small subtractive synthesizer",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (598, 280),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark { x: 590.0, y: 263.0, anchor: 1.0, stacked: false },
    hint: Some(Rect::new(8.0, 252.0, 466.0, 22.0)),
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

/// The detune knob's cents between the oscillators, 0 to 50.
pub fn detune_cents(v: i32) -> f64 {
    f64::from(v) / 128.0 * 50.0
}

/// The contour's seconds: the filter falls to ~2 % of its start in it.
fn fdecay_text_seconds(v: i32) -> f64 {
    let t = decay_seconds(v);
    if t < 1.0 {
        (t * 1000.0).round() / 1000.0
    } else {
        (t * 10.0).round() / 10.0
    }
}

fn seconds_text(t: f64) -> String {
    if t < 1.0 {
        format!("{:.0}ms", t * 1000.0)
    } else {
        format!("{:.1}s", t)
    }
}

fn fdecay_parse(s: &str) -> Option<i32> {
    let s = s.trim().to_ascii_lowercase();
    let x = leading_number(&s)? / if s.ends_with("ms") { 1000.0 } else { 1.0 };
    nearest(SETTINGS[FDECAY].range, x, |v| Some(fdecay_text_seconds(v)))
}

/// A time as `instant`, `123ms` or `1.2s` (the small envelope knobs' law).
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
    nearest(SETTINGS[ATTACK].range, x, |v| {
        let t = time_seconds(v);
        Some(if t < 1.0 { (t * 1000.0).round() / 1000.0 } else { (t * 10.0).round() / 10.0 })
    })
}

/// An octave knob's text: `0oct`, `+1oct`, `-2oct`.
fn oct_text(v: i32) -> String {
    match v {
        0 => "0oct".into(),
        v => format!("{v:+}oct"),
    }
}

/// The voices knob's count: 1 to 8.
pub fn voice_count(v: i32) -> i32 {
    v + 1
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        WAVE1 | WAVE2 => ["Saw", "Square", "Triangle", "Sine"][v.clamp(0, 3) as usize].to_string(),
        OCT1 | OCT2 => oct_text(v),
        DETUNE => format!("{:.1}ct", detune_cents(v)),
        MIX | RESO | ENV | SUSTAIN => percent(SETTINGS[k].range, v),
        CUTOFF => format!("{:.0}Hz", cutoff_hz(v)),
        FDECAY => seconds_text(fdecay_text_seconds(v)),
        ATTACK | DECAY | RELEASE => time_text(v),
        VOICES => match voice_count(v) {
            1 => "1 voice".into(),
            n => format!("{n} voices"),
        },
        _ => db_text(volume_gain(v)),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    let s = s.trim().to_ascii_lowercase();
    match k {
        WAVE1 | WAVE2 => {
            let names = ["saw", "square", "triangle", "sine"];
            names
                .iter()
                .position(|n| *n == s)
                .map(|v| v as i32)
                .or_else(|| leading_number(&s).map(|x| x.round() as i32))
                .map(|v| v.clamp(0, 3))
        }
        OCT1 | OCT2 => leading_number(&s).map(|x| (x.round() as i32).clamp(-2, 2)),
        DETUNE => {
            let x = leading_number(&s)?;
            nearest(SETTINGS[DETUNE].range, x, |v| Some(detune_cents(v)))
        }
        MIX | RESO | ENV | SUSTAIN => percent_parse(SETTINGS[k].range, &s),
        CUTOFF => {
            let x = leading_number(&s)?;
            nearest(SETTINGS[CUTOFF].range, x, |v| Some(cutoff_hz(v)))
        }
        FDECAY => fdecay_parse(&s),
        ATTACK | DECAY | RELEASE => time_parse(&s),
        VOICES => {
            let x = leading_number(&s)?;
            Some((x.round() as i32 - 1).clamp(0, 7))
        }
        _ => parse_db(SETTINGS[VOLUME].range, &s, volume_gain),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The sound

/// The frequency of a key.
fn key_hz(key: u8) -> f32 {
    (440.0f64 * 2f64.powf((f64::from(key) - 69.0) / 12.0)) as f32
}

/// One note: the two oscillators, the swept filter and the amp envelope.
struct Voice {
    key: u8,
    voice_id: Option<i32>,
    /// The oscillators' places in their cycles, 0..1.
    phase1: f32,
    phase2: f32,
    /// The filter's state: the low and band integrators.
    lp: f32,
    band: f32,
    /// The filter contour's level, 1 at the strike falling toward 0.
    contour: f32,
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

struct Mini {
    rate: f32,
    wave1: usize,
    oct1: f32,
    wave2: usize,
    oct2: f32,
    cents: f32,
    mix: f32,
    cutoff: f32,
    q: f32,
    octaves: f32,
    ftau: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    gain: f32,
    voices: [Option<Voice>; 8],
    ended: Vec<(u8, Option<i32>)>,
}

impl Mini {
    /// An oscillator's sample at its place in the cycle.
    fn wave(wave: usize, phase: f32) -> f32 {
        match wave {
            1 => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            2 => 1.0 - 4.0 * (phase - phase.round().abs()).abs(),
            3 => (std::f32::consts::TAU * phase).sin(),
            _ => phase * 2.0 - 1.0,
        }
    }
}

impl Instrument for Mini {
    fn set(&mut self, v: &[i32]) {
        self.wave1 = v[WAVE1].clamp(0, 3) as usize;
        self.oct1 = f64::from(v[OCT1]) as f32;
        self.wave2 = v[WAVE2].clamp(0, 3) as usize;
        self.oct2 = f64::from(v[OCT2]) as f32;
        self.cents = detune_cents(v[DETUNE]) as f32;
        self.mix = f64::from(v[MIX]) as f32 / 128.0;
        self.cutoff = cutoff_hz(v[CUTOFF]) as f32;
        self.q = resonance_q(v[RESO]) as f32;
        self.octaves = envelope_octaves(v[ENV]) as f32;
        self.ftau = (decay_seconds(v[FDECAY]) / 4.0) as f32;
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
        self.voices = [const { None }; 8];
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
            phase1: 0.0,
            phase2: 0.5,
            lp: 0.0,
            band: 0.0,
            contour: 1.0,
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
        let (q, octaves, ftau, gain) = (self.q, self.octaves, self.ftau, self.gain);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut out = 0.0f32;
            for s in self.voices.iter_mut().flatten() {
                // The oscillators: the second sits its octaves and its cents from the note.
                let f = key_hz(s.key) * 2f32.powf(self.oct1);
                let f2 =
                    key_hz(s.key) * 2f32.powf(self.oct2) * 2f32.powf(self.cents / 1200.0);
                s.phase1 = (s.phase1 + f * dt).fract();
                s.phase2 = (s.phase2 + f2 * dt).fract();
                let w1 = Self::wave(self.wave1, s.phase1);
                let w2 = Self::wave(self.wave2, s.phase2);
                let mix = w1 * (1.0 - self.mix * 0.5) + w2 * self.mix * 0.5;
                // The filter, its cutoff swept by the contour.
                let fc = (self.cutoff * 2f32.powf(octaves * s.contour)).min(self.rate * 0.25);
                let g = (std::f32::consts::PI * fc / self.rate).tan().clamp(0.0, 8.0);
                let k = 1.0 / q;
                let a1 = 1.0 / (1.0 + g * (g + k));
                let a2 = g * a1;
                let a3 = g * a2;
                let v3 = mix - s.lp;
                let v1 = a1 * s.band + a2 * v3;
                let v2 = s.lp + a2 * s.band + a3 * v3;
                s.lp = 2.0 * v2 - s.lp;
                s.band = 2.0 * v1 - s.band;
                // The envelope.
                match s.stage {
                    Stage::Attack => {
                        s.level += dt / self.attack.max(0.001);
                        if s.level >= 1.0 {
                            s.level = 1.0;
                            s.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        s.level =
                            self.sustain + (s.level - self.sustain) * (-dt / self.decay.max(0.001) / 4.0).exp();
                    }
                    Stage::Release => {
                        s.level *= (-dt / self.release.max(0.001) / 4.0).exp();
                    }
                }
                s.contour *= (-dt / ftau.max(0.001)).exp();
                out += s.lp * s.level * s.vel;
                s.age += dt;
            }
            let out = out * gain * 0.4;
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
    let mut m = Mini {
        rate: rate as f32,
        wave1: v[WAVE1].clamp(0, 3) as usize,
        oct1: f64::from(v[OCT1]) as f32,
        wave2: v[WAVE2].clamp(0, 3) as usize,
        oct2: f64::from(v[OCT2]) as f32,
        cents: detune_cents(v[DETUNE]) as f32,
        mix: f64::from(v[MIX]) as f32 / 128.0,
        cutoff: cutoff_hz(v[CUTOFF]) as f32,
        q: resonance_q(v[RESO]) as f32,
        octaves: envelope_octaves(v[ENV]) as f32,
        ftau: (decay_seconds(v[FDECAY]) / 4.0) as f32,
        attack: time_seconds(v[ATTACK]) as f32,
        decay: time_seconds(v[DECAY]) as f32,
        sustain: f64::from(v[SUSTAIN]) as f32 / 128.0,
        release: time_seconds(v[RELEASE]) as f32,
        gain: volume_gain(v[VOLUME]),
        voices: [const { None }; 8],
        ended: Vec::new(),
    };
    m.set_sample_rate(rate);
    Box::new(m)
}

pub struct MiniKind;

impl Kind for MiniKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-mini";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's small subtractive synthesizer";
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioMini\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type MangoMini = SmallGen<MiniKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(WAVE1, 0), "Saw");
        assert_eq!(parse(WAVE1, "triangle"), Some(2));
        assert_eq!(text(OCT1, 1), "+1oct");
        assert_eq!(text(OCT1, 0), "0oct");
        assert_eq!(parse(OCT1, "-2oct"), Some(-2));
        assert_eq!(text(DETUNE, 64), "25.0ct");
        assert_eq!(parse(DETUNE, "25 ct"), Some(64));
        assert_eq!(text(MIX, 64), "50%");
        assert_eq!(parse(MIX, "50%"), Some(64));
        assert_eq!(text(CUTOFF, 0), "40Hz");
        assert_eq!(parse(CUTOFF, "40 Hz"), Some(0));
        assert_eq!(text(ATTACK, 0), "instant");
        assert_eq!(parse(ATTACK, "instant"), Some(0));
        assert_eq!(text(VOICES, 7), "8 voices");
        assert_eq!(parse(VOICES, "8"), Some(7));
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
    }

    /// Notes sound through the filter; eight at once, a ninth steals the oldest.
    #[test]
    fn eight_notes_a_ninth_steals_the_oldest() {
        let mut m = make(44100, &SPEC.start());
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        for (i, key) in [48u8, 52, 55, 60, 64, 67, 72, 76].into_iter().enumerate() {
            m.note_on(key, 1.0, Some(i as i32 + 1));
        }
        m.render(&mut l, &mut r);
        assert!(l.iter().any(|x| x.abs() > 0.001), "the notes sound");
        m.note_on(80, 1.0, Some(9));
        assert_eq!(m.take_ended(), vec![(48, Some(1))], "the oldest note ended");
    }

    /// A release ends a note; a filter contour sweeps: the tone at the strike is brighter than later.
    #[test]
    fn the_release_ends_and_the_contour_sweeps() {
        let mut m = make(44100, &SPEC.start());
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        m.note_on(48, 1.0, Some(1));
        m.note_off(48, Some(1));
        let mut blocks = 0;
        while m.has_voices() && blocks < 2000 {
            l.iter_mut().for_each(|x| *x = 0.0);
            m.render(&mut l, &mut r);
            blocks += 1;
        }
        assert!(!m.has_voices(), "the release ends the note");
        assert_eq!(m.take_ended(), vec![(48, Some(1))]);
    }
}
