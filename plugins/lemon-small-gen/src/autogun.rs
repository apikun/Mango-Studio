// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Autogun: Mango Studio's endless patch synthesizer (the roadmap's "Autogun"). A preset number is a seed:
//! hashed, it grows a whole patch — two oscillators and their intervals, the filter and its contour, the amp
//! envelope, the drive — so every number plays something of its own and no two need sound alike. The four macro
//! knobs bend the grown patch as it plays: the brightness, the pace, the second oscillator and the bite.
//!
// spec: plugins/lemon-small-generators.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::fruit_kick::volume_gain;
use crate::params::{db_text, leading_number, nearest, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

// ---------------------------------------------------------------------------------------------------------------
// The settings (spec §5): the stored order, their ranges and their starting values

pub const SETTINGS: [Setting; 6] = [
    Setting { id: "preset", name: "Preset", range: (0, 999_999), start: 1234, magnet: None },
    Setting { id: "a", name: "A", range: (0, 128), start: 64, magnet: None },
    Setting { id: "b", name: "B", range: (0, 128), start: 64, magnet: None },
    Setting { id: "c", name: "C", range: (0, 128), start: 128, magnet: None },
    Setting { id: "d", name: "D", range: (0, 128), start: 32, magnet: None },
    Setting { id: "volume", name: "Volume", range: (0, 512), start: 256, magnet: Some(256) },
];

pub const PRESET: usize = 0;
pub const A: usize = 1;
pub const B: usize = 2;
pub const C: usize = 3;
pub const D: usize = 4;
pub const VOLUME: usize = 5;

/// Editor layout (spec §2): one row of cards, a preset and the four macros.
const CARDS: [Card; 3] = [
    Card { x0: 8.0, x1: 122.0, y0: 28.0, y1: 124.0, heading: "PRESET" },
    Card { x0: 128.0, x1: 350.0, y0: 28.0, y1: 124.0, heading: "MACROS" },
    Card { x0: 356.0, x1: 470.0, y0: 28.0, y1: 124.0, heading: "OUTPUT" },
];

const CONTROLS: [Control; 6] = [
    Control::Knob { id: "preset", cx: 40.0, cy: 74.0, r: 24.0, caption: "", bipolar: false },
    Control::Knob { id: "a", cx: 160.0, cy: 74.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "b", cx: 210.0, cy: 74.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "c", cx: 260.0, cy: 74.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "d", cx: 310.0, cy: 74.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "volume", cx: 410.0, cy: 74.0, r: 24.0, caption: "", bipolar: false },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Autogun",
    word: "AUTOGUN",
    about: "Mango Studio's seed-grown patch synthesizer",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (478, 162),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark { x: 470.0, y: 143.0, anchor: 1.0, stacked: false },
    hint: Some(Rect::new(8.0, 132.0, 424.0, 22.0)),
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

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        PRESET => format!("{v}"),
        A | B | C | D => percent(SETTINGS[k].range, v),
        _ => db_text(volume_gain(v)),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    let s = s.trim().to_ascii_lowercase();
    match k {
        PRESET => leading_number(&s).map(|x| (x.round() as i32).clamp(0, 999_999)),
        A | B | C | D => percent_parse(SETTINGS[k].range, &s),
        _ => parse_db(SETTINGS[VOLUME].range, &s, volume_gain),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The sound

/// The frequency of a key.
fn key_hz(key: u8) -> f32 {
    (440.0f64 * 2f64.powf((f64::from(key) - 69.0) / 12.0)) as f32
}

/// The waves an oscillator can grow with.
const WAVES: [&str; 5] = ["Sine", "Triangle", "Saw", "Square", "Noise"];

/// A grown patch: the shape the seed leaves the sound in, before the macros bend it.
struct Patch {
    wave1: usize,
    tune1: f32,
    wave2: usize,
    tune2: f32,
    mix2: f32,
    cutoff: f32,
    q: f32,
    env: f32,
    fdecay: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    drive: f32,
}

/// The patch a seed grows: a stream of numbers from the seed's hash, each clamped into its place's range.
fn patch(seed: i32) -> Patch {
    let mut s = (seed as u32) ^ 0x9E37_79B9;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s
    };
    let unit = |x: u32| x as f64 / u32::MAX as f64;
    let mut p = Patch {
        wave1: (next() % WAVES.len() as u32) as usize,
        tune1: (unit(next()) * 24.0 - 12.0) as f32,
        wave2: (next() % WAVES.len() as u32) as usize,
        tune2: ((unit(next()) * 25.0 - 12.5).round() * 2.0) as f32,
        mix2: unit(next()) as f32,
        cutoff: (100.0 * 2f64.powf(unit(next()) * 6.0)) as f32,
        q: (0.5 + unit(next()) * 5.0) as f32,
        env: (unit(next()) * 5.0) as f32,
        fdecay: (0.02 * 2f64.powf(unit(next()) * 5.5)) as f32,
        attack: (0.001 * 2f64.powf(unit(next()) * 10.0)) as f32,
        decay: (0.02 * 2f64.powf(unit(next()) * 6.5)) as f32,
        sustain: unit(next()) as f32,
        release: (0.01 * 2f64.powf(unit(next()) * 8.0)) as f32,
        drive: (1.0 + unit(next()) * 7.0) as f32,
    };
    // The second oscillator sits at an interval of whole tones from the first, so chords in the
    // patch stay in tune with it.
    p.tune1 = (p.tune1 * 2.0).round() / 2.0;
    p
}

/// One note: the patch's two oscillators into its filter, the amp envelope over it.
struct Voice {
    key: u8,
    voice_id: Option<i32>,
    /// The oscillators' places in their cycles, 0..1, and the noise's state.
    phase1: f32,
    phase2: f32,
    noise: u32,
    /// The filter's state: the low and band integrators.
    lp: f32,
    band: f32,
    /// The amp envelope's place (1 at the strike, falling) and stage.
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

struct Autogun {
    rate: f32,
    seed: i32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    gain: f32,
    voices: [Option<Voice>; 4],
    ended: Vec<(u8, Option<i32>)>,
}

impl Autogun {
    /// The patch the macros play: the seed's shape bent by the four knobs.
    fn grown(&self) -> Patch {
        let mut p = patch(self.seed);
        // A: the brightness, an octave down to three up.
        p.cutoff *= 2f32.powf(self.a * 4.0 - 1.0);
        // B: the pace, a quarter to four times.
        let pace = 2f32.powf(self.b * 2.0);
        p.attack = (p.attack / pace).max(0.0005);
        p.decay /= pace;
        p.fdecay /= pace;
        p.release /= pace;
        // C: how much of the second oscillator is heard.
        p.mix2 *= self.c;
        // D: the bite.
        p.drive = 1.0 + self.d * (p.drive - 1.0);
        p
    }

    /// An oscillator's sample at its place in the cycle.
    fn wave(wave: usize, phase: f32, noise: &mut u32) -> f32 {
        match wave {
            1 => 1.0 - 4.0 * (phase - phase.round().abs()).abs(),
            2 => phase * 2.0 - 1.0,
            3 => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            4 => {
                let mut x = *noise;
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                *noise = x;
                (x as f32 / u32::MAX as f32) * 2.0 - 1.0
            }
            _ => (std::f32::consts::TAU * phase).sin(),
        }
    }
}

impl Instrument for Autogun {
    fn set(&mut self, v: &[i32]) {
        self.seed = v[PRESET];
        self.a = f64::from(v[A]) as f32 / 128.0;
        self.b = f64::from(v[B]) as f32 / 128.0;
        self.c = f64::from(v[C]) as f32 / 128.0;
        self.d = f64::from(v[D]) as f32 / 128.0;
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
            phase1: 0.0,
            phase2: 0.0,
            noise: 0x1_234_567u32.wrapping_add((key as u32).wrapping_mul(2_654_435_761)),
            lp: 0.0,
            band: 0.0,
            level: 0.0,
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
        let p = self.grown();
        let gain = self.gain;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut out = 0.0f32;
            for s in self.voices.iter_mut().flatten() {
                // The oscillators, a fixed voicing around the note (the patch's intervals).
                let f = key_hz(s.key) * 2f32.powf(p.tune1 / 12.0);
                let f2 = key_hz(s.key) * 2f32.powf(p.tune2 / 12.0);
                s.phase1 = (s.phase1 + f * dt).fract();
                s.phase2 = (s.phase2 + f2 * dt).fract();
                let w1 = Self::wave(p.wave1, s.phase1, &mut s.noise);
                let w2 = Self::wave(p.wave2, s.phase2, &mut s.noise);
                let mix = w1 * (1.0 - p.mix2 * 0.5) + w2 * p.mix2 * 0.5;
                // The filter, its cutoff swept by its contour.
                let fc = (p.cutoff * 2f32.powf(p.env * (1.0 - s.level))).min(self.rate * 0.25);
                let g = (std::f32::consts::PI * fc / self.rate).tan().clamp(0.0, 8.0);
                let k = 1.0 / p.q;
                let a1 = 1.0 / (1.0 + g * (g + k));
                let a2 = g * a1;
                let a3 = g * a2;
                let v3 = mix - s.lp;
                let v1 = a1 * s.band + a2 * v3;
                let v2 = s.lp + a2 * s.band + a3 * v3;
                s.lp = 2.0 * v2 - s.lp;
                s.band = 2.0 * v1 - s.band;
                // The bite, then the envelope.
                let shaped = (s.lp * p.drive).tanh() / p.drive;
                // The envelope: an attack, a fall to the sustain, a release.
                match s.stage {
                    Stage::Attack => {
                        s.level += dt / p.attack.max(0.0005);
                        if s.level >= 1.0 {
                            s.level = 1.0;
                            s.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        s.level *= (-dt / (p.decay / 4.0).max(0.001)).exp();
                        if s.level <= p.sustain {
                            s.level = p.sustain;
                        }
                    }
                    Stage::Release => {
                        s.level *= (-dt / (p.release / 4.0).max(0.001)).exp();
                    }
                }
                out += shaped * s.level * s.vel * 0.5;
                s.age += dt;
            }
            *l += out * gain;
            *r += out * gain;
        }
        // The voices that faded, or a note held far past any patch's decay.
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
    let mut a = Autogun {
        rate: rate as f32,
        seed: v[PRESET],
        a: f64::from(v[A]) as f32 / 128.0,
        b: f64::from(v[B]) as f32 / 128.0,
        c: f64::from(v[C]) as f32 / 128.0,
        d: f64::from(v[D]) as f32 / 128.0,
        gain: volume_gain(v[VOLUME]),
        voices: [const { None }; 4],
        ended: Vec::new(),
    };
    a.set_sample_rate(rate);
    Box::new(a)
}

pub struct AutogunKind;

impl Kind for AutogunKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-autogun";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's seed-grown patch synthesizer";
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioAuto\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type MangoAutogun = SmallGen<AutogunKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(PRESET, 1234), "1234");
        assert_eq!(parse(PRESET, "1234"), Some(1234));
        assert_eq!(parse(PRESET, "999999999"), Some(999_999), "a big number clamps");
        assert_eq!(text(A, 64), "50%");
        assert_eq!(parse(A, "50%"), Some(64));
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
    }

    /// A note sounds; a different seed grows a different patch.
    #[test]
    fn a_note_sounds_and_seeds_differ() {
        let mut a = make(44100, &SPEC.start());
        a.note_on(60, 1.0, Some(1));
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        let mut peak = 0.0f32;
        for _ in 0..20 {
            l.iter_mut().for_each(|x| *x = 0.0);
            a.render(&mut l, &mut r);
            peak = peak.max(l.iter().fold(0.0f32, |a, x| a.max(x.abs())));
        }
        assert!(peak > 0.01, "the seed sounds (peak {peak})");
        // Another seed, the same note: its patch differs.
        let mut v = SPEC.start();
        v[PRESET] = 424_242;
        let mut b = make(44100, &v);
        b.note_on(60, 1.0, Some(1));
        let mut m = 0.0f32;
        for _ in 0..20 {
            l.iter_mut().for_each(|x| *x = 0.0);
            b.render(&mut l, &mut r);
            m = m.max(l.iter().fold(0.0f32, |a, x| a.max(x.abs())));
        }
        let mut d = make(44100, &SPEC.start());
        d.note_on(60, 1.0, Some(1));
        let mut n = 0.0f32;
        for _ in 0..20 {
            l.iter_mut().for_each(|x| *x = 0.0);
            d.render(&mut l, &mut r);
            n = n.max(l.iter().fold(0.0f32, |a, x| a.max(x.abs())));
        }
        assert!((m - n).abs() > 1e-4, "two seeds differ ({m} against {n})");
    }

    /// A release ends the note, whatever the patch's sustain.
    #[test]
    fn the_release_ends_the_note() {
        let mut a = make(44100, &SPEC.start());
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        a.note_on(48, 1.0, Some(1));
        a.note_off(48, Some(1));
        let mut blocks = 0;
        while a.has_voices() && blocks < 2000 {
            l.iter_mut().for_each(|x| *x = 0.0);
            a.render(&mut l, &mut r);
            blocks += 1;
        }
        assert!(!a.has_voices(), "the release ends the note");
        assert_eq!(a.take_ended(), vec![(48, Some(1))]);
    }
}
