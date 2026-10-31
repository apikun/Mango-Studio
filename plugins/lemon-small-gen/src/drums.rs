// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Drums: Mango Studio's synthesized drum voice (the roadmap's "Fruity DrumSynth"). One instrument, six
//! drums: a kick and a tom swept down from their snap, a snare of two tones and a noise body, hats of six
//! inharmonic squares through a highpass, a clap of three noise pulses — all built from knobs, no samples. Four
//! hits can ring at once, so rolls play.
//!
// spec: plugins/lemon-small-generators.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::fruit_kick::volume_gain;
use crate::params::{db_text, leading_number, nearest, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

// ---------------------------------------------------------------------------------------------------------------
// The settings (spec §5): the stored order, their ranges and their starting values

pub const SETTINGS: [Setting; 9] = [
    Setting { id: "drum", name: "Drum", range: (0, 5), start: 0, magnet: None },
    Setting { id: "tune", name: "Tune", range: (-12, 12), start: 0, magnet: Some(0) },
    Setting { id: "decay", name: "Decay", range: (0, 128), start: 64, magnet: None },
    Setting { id: "snap", name: "Snap", range: (0, 128), start: 40, magnet: None },
    Setting { id: "sweep", name: "Sweep", range: (0, 128), start: 56, magnet: None },
    Setting { id: "noise", name: "Noise", range: (0, 128), start: 48, magnet: None },
    Setting { id: "tone", name: "Tone", range: (0, 128), start: 64, magnet: None },
    Setting { id: "ring", name: "Ring", range: (0, 128), start: 24, magnet: None },
    Setting { id: "volume", name: "Volume", range: (0, 512), start: 256, magnet: Some(256) },
];

pub const DRUM: usize = 0;
pub const TUNE: usize = 1;
pub const DECAY: usize = 2;
pub const SNAP: usize = 3;
pub const SWEEP: usize = 4;
pub const NOISE: usize = 5;
pub const TONE: usize = 6;
pub const RING: usize = 7;
pub const VOLUME: usize = 8;

/// Editor layout (spec §2): two rows of cards over the hint and the wordmark.
const CARDS: [Card; 5] = [
    Card { x0: 8.0, x1: 118.0, y0: 28.0, y1: 134.0, heading: "DRUM" },
    Card { x0: 124.0, x1: 238.0, y0: 28.0, y1: 134.0, heading: "TONE" },
    Card { x0: 244.0, x1: 358.0, y0: 28.0, y1: 134.0, heading: "BODY" },
    Card { x0: 8.0, x1: 122.0, y0: 140.0, y1: 246.0, heading: "TAIL" },
    Card { x0: 128.0, x1: 242.0, y0: 140.0, y1: 246.0, heading: "OUTPUT" },
];

const CONTROLS: [Control; 9] = [
    Control::Choice {
        id: "drum",
        x: 16.0,
        top: 36.0,
        names: &["Kick", "Snare", "Closed hat", "Open hat", "Tom", "Clap"],
    },
    Control::Knob { id: "tune", cx: 156.0, cy: 86.0, r: 22.0, caption: "", bipolar: true },
    Control::Knob { id: "sweep", cx: 206.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "snap", cx: 276.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "noise", cx: 326.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "decay", cx: 40.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "tone", cx: 90.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "ring", cx: 160.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "volume", cx: 210.0, cy: 198.0, r: 24.0, caption: "", bipolar: false },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Drums",
    word: "DRUMS",
    about: "Mango Studio's synthesized drum voice",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (366, 280),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark { x: 358.0, y: 263.0, anchor: 1.0, stacked: false },
    hint: Some(Rect::new(8.0, 252.0, 234.0, 22.0)),
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

/// The decay knob's seconds: 20 ms to 2.56 s, the drum's own shape scales it.
pub fn decay_seconds(v: i32) -> f64 {
    0.02 * 2f64.powf(f64::from(v) / 128.0 * 7.0)
}

/// The decay a value's text names (the number `nearest` matches a typed one against).
fn decay_text_seconds(v: i32) -> f64 {
    let t = decay_seconds(v);
    if t < 1.0 {
        (t * 1000.0).round() / 1000.0
    } else {
        (t * 10.0).round() / 10.0
    }
}

fn decay_text(t: f64) -> String {
    if t < 1.0 {
        format!("{:.0}ms", t * 1000.0)
    } else {
        format!("{:.1}s", t)
    }
}

fn decay_parse(s: &str) -> Option<i32> {
    let s = s.trim().to_ascii_lowercase();
    let x = leading_number(&s)? / if s.ends_with("ms") { 1000.0 } else { 1.0 };
    nearest(SETTINGS[DECAY].range, x, |v| Some(decay_text_seconds(v)))
}

/// A semitone knob's text: `0st`, `+3st`, `-2st`.
fn st_text(v: i32) -> String {
    match v {
        0 => "0st".into(),
        v => format!("{v:+}st"),
    }
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        DRUM => ["Kick", "Snare", "Closed hat", "Open hat", "Tom", "Clap"][v.clamp(0, 5) as usize]
            .to_string(),
        TUNE => st_text(v),
        DECAY => decay_text(decay_text_seconds(v)),
        SNAP | SWEEP | NOISE | TONE | RING => percent(SETTINGS[k].range, v),
        _ => db_text(volume_gain(v)),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    let s = s.trim().to_ascii_lowercase();
    match k {
        DRUM => {
            let names = ["kick", "snare", "closed hat", "open hat", "tom", "clap"];
            names
                .iter()
                .position(|n| *n == s)
                .map(|v| v as i32)
                .or_else(|| leading_number(&s).map(|x| x.round() as i32))
                .map(|v| v.clamp(0, 5))
        }
        TUNE => leading_number(&s).map(|x| (x.round() as i32).clamp(-12, 12)),
        DECAY => decay_parse(&s),
        SNAP | SWEEP | NOISE | TONE | RING => percent_parse(SETTINGS[k].range, &s),
        _ => parse_db(SETTINGS[VOLUME].range, &s, volume_gain),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The sound

/// xorshift32, ±1.
fn noise(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// The hats' six inharmonic squares, as multiples of the hat's base pitch.
const HAT_RATIOS: [f32; 6] = [2.0, 3.0, 4.16, 5.43, 6.79, 8.21];

/// A drum: everything one hit needs, its knob values snapshotted at the strike.
struct Voice {
    key: u8,
    voice_id: Option<i32>,
    drum: usize,
    vel: f32,
    /// The hit's place in seconds, and its pitch-swept sine's phase.
    t: f32,
    phase: f32,
    /// The hats' six squares' places, and the noise's state.
    squares: [f32; 6],
    rng: u32,
    /// The noise body's lowpass state (the snare's and the clap's shaping).
    lp: f32,
    /// The hats' highpass state.
    hp: f32,
    /// The decay the hit plays out over, its own by the drum.
    decay: f32,
    tune: f32,
    snap: f32,
    sweep: f32,
    noise: f32,
    tone: f32,
    ring: f32,
    age: f32,
}

impl Voice {
    /// The hit's level: the drum's own shape over the decay, 1 falling to nothing.
    fn level(&self) -> f32 {
        let d = self.decay.max(0.005);
        match self.drum {
            0 | 4 => (-self.t / d).exp(),
            1 => (-self.t / (d * 0.5)).exp(),
            2 => (-self.t / (d * 0.3)).exp(),
            3 => (-self.t / d).exp(),
            _ => (-((self.t - 0.036).max(0.0)) / (d * 0.4)).exp(),
        }
    }

    /// One sample of the hit: the drum's own recipe over its snapshot.
    fn sample(&mut self, dt: f32) -> f32 {
        let semi = 2f32.powf(self.tune / 12.0);
        let out = match self.drum {
            // The kick and the tom: a sine swept down from its snap to its note, a click on top.
            0 | 4 => {
                let base = if self.drum == 0 { 50.0 } else { 110.0 } * semi * (0.5 + self.tone);
                let start = base * (1.0 + 9.0 * self.sweep);
                let f = self.t.mul_add(-1.0 / 0.03, start).max(base);
                self.phase = (self.phase + std::f32::consts::TAU * f * dt) % std::f32::consts::TAU;
                let click = self.snap * noise(&mut self.rng) * (-self.t / 0.003).exp();
                self.phase.sin() * self.level() * 0.9 + click
            }
            // The snare: two tones and a noise body, the ring's squares rattling behind them.
            1 => {
                let f1 = 185.0 * semi * (0.5 + self.tone * 0.7);
                let f2 = 330.0 * semi * (0.5 + self.tone * 0.7);
                self.phase = (self.phase + std::f32::consts::TAU * f1 * dt) % std::f32::consts::TAU;
                let p2 = (self.phase * (f2 / f1)) % 1.0;
                let tones = self.phase.sin() * 0.6 + (std::f32::consts::TAU * p2).sin() * 0.4;
                let raw = noise(&mut self.rng);
                self.lp += 0.28 * (raw - self.lp);
                let hiss = (raw - self.lp) * self.noise * self.level();
                let rattle = metal(self, dt) * self.ring * 0.3;
                (tones * 0.7 + hiss + rattle) * self.level()
                    + self.snap * raw * (-self.t / 0.002).exp()
            }
            // The hats: six inharmonic squares through a highpass, the open one ringing out.
            2 | 3 => {
                let metal = metal(self, dt);
                let raw = noise(&mut self.rng);
                let hiss = (raw - self.lp) * self.noise * 0.5;
                self.lp += 0.9 * (raw - self.lp);
                (metal * 0.7 + hiss) * self.level()
                    + metal * self.ring * (-self.t / (self.decay * 1.5).max(0.05)).exp() * 0.4
            }
            // The clap: three noise pulses, then the tail.
            _ => {
                let raw = noise(&mut self.rng);
                self.lp += 0.35 * (raw * (0.3 + self.tone) - self.lp);
                let pulse = |t0: f32, t: f32| {
                    (-((t - t0).max(0.0)) / 0.004).exp() * ((t > t0) || (t == t0)) as i32 as f32
                };
                let hits =
                    (pulse(0.0, self.t) + pulse(0.012, self.t) + pulse(0.024, self.t)) * self.snap;
                (self.lp * (0.4 + self.noise) * self.level() + raw * hits * 0.5) * 0.8
            }
        };
        self.t += dt;
        self.age += dt;
        out
    }
}

/// The hats' metal: six inharmonic squares through a highpass.
fn metal(v: &mut Voice, dt: f32) -> f32 {
    let base = 40.0 * 2f32.powf(v.tune / 12.0) * (0.5 + v.tone * 1.5);
    let mut sum = 0.0f32;
    for (s, r) in v.squares.iter_mut().zip(HAT_RATIOS) {
        *s = (*s + base * r * dt).fract();
        sum += if *s < 0.5 { 1.0 } else { -1.0 };
    }
    let x = sum * 0.14;
    // A one-pole highpass keeps the metal's body out of the strike.
    v.hp = x - v.hp + 0.72 * v.hp;
    v.hp
}

struct Drums {
    rate: f32,
    drum: usize,
    tune: f32,
    decay: f32,
    snap: f32,
    sweep: f32,
    noise: f32,
    tone: f32,
    ring: f32,
    gain: f32,
    voices: [Option<Voice>; 4],
    ended: Vec<(u8, Option<i32>)>,
}

impl Instrument for Drums {
    fn set(&mut self, d: &[i32]) {
        self.drum = d[DRUM].clamp(0, 5) as usize;
        self.tune = f64::from(d[TUNE]) as f32;
        self.decay = decay_seconds(d[DECAY]) as f32;
        self.snap = f64::from(d[SNAP]) as f32 / 128.0;
        self.sweep = f64::from(d[SWEEP]) as f32 / 128.0;
        self.noise = f64::from(d[NOISE]) as f32 / 128.0;
        self.tone = f64::from(d[TONE]) as f32 / 128.0;
        self.ring = f64::from(d[RING]) as f32 / 128.0;
        self.gain = volume_gain(d[VOLUME]);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate as f32;
    }

    fn reset(&mut self) {
        self.voices = [const { None }; 4];
    }

    fn note_on(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        // A free voice, else the oldest goes (spec §1).
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
        // The drum's own decay: the knob's time, scaled to the drum (a closed hat is short, an open
        // one rings, a clap's tail is its own).
        let decay = self.decay
            * match self.drum {
                0 => 0.8,
                1 => 0.6,
                2 => 0.35,
                4 => 0.9,
                _ => 1.0,
            };
        self.voices[slot] = Some(Voice {
            key,
            voice_id: Some(voice_id.unwrap_or(key as i32 + 1)),
            drum: self.drum,
            vel: velocity.clamp(0.05, 1.0),
            t: 0.0,
            phase: 0.0,
            squares: [0.0; 6],
            rng: 0x1234_5678 ^ u32::from(key).wrapping_mul(2_654_435_761),
            lp: 0.0,
            hp: 0.0,
            decay,
            tune: self.tune,
            snap: self.snap,
            sweep: self.sweep,
            noise: self.noise,
            tone: self.tone,
            ring: self.ring,
            age: 0.0,
        });
    }

    fn note_off(&mut self, _key: u8, _voice_id: Option<i32>) {
        // A drum does not release: the hit plays out (spec §1).
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
        let gain = self.gain;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut out = 0.0f32;
            for s in self.voices.iter_mut().flatten() {
                out += s.sample(dt) * s.vel;
            }
            let out = (out * gain).clamp(-1.5, 1.5);
            *l += out;
            *r += out;
        }
        // The hits that played out: their level gone, or a rimshot held far past any decay.
        for s in self.voices.iter_mut() {
            let gone = s
                .as_ref()
                .is_some_and(|v| v.level() < 1e-4 && v.t > 0.05 || v.age > v.decay * 6.0 + 1.0);
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

fn make(rate: u32, d: &[i32]) -> Box<dyn Instrument> {
    let mut x = Drums {
        rate: rate as f32,
        drum: d[DRUM].clamp(0, 5) as usize,
        tune: f64::from(d[TUNE]) as f32,
        decay: decay_seconds(d[DECAY]) as f32,
        snap: f64::from(d[SNAP]) as f32 / 128.0,
        sweep: f64::from(d[SWEEP]) as f32 / 128.0,
        noise: f64::from(d[NOISE]) as f32 / 128.0,
        tone: f64::from(d[TONE]) as f32 / 128.0,
        ring: f64::from(d[RING]) as f32 / 128.0,
        gain: volume_gain(d[VOLUME]),
        voices: [const { None }; 4],
        ended: Vec::new(),
    };
    x.set_sample_rate(rate);
    Box::new(x)
}

pub struct DrumsKind;

impl Kind for DrumsKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-drums";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's synthesized drum voice";
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioDrum\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type MangoDrums = SmallGen<DrumsKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(DRUM, 0), "Kick");
        assert_eq!(text(DRUM, 3), "Open hat");
        assert_eq!(parse(DRUM, "clap"), Some(5));
        assert_eq!(parse(DRUM, "2"), Some(2));
        assert_eq!(text(TUNE, 3), "+3st");
        assert_eq!(text(TUNE, 0), "0st");
        assert_eq!(parse(TUNE, "-5st"), Some(-5));
        assert_eq!(text(DECAY, 0), "20ms");
        assert_eq!(parse(DECAY, "20ms"), Some(0));
        assert_eq!(text(SNAP, 64), "50%");
        assert_eq!(parse(SNAP, "50%"), Some(64));
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
    }

    /// Every drum sounds and plays out to silence, whichever one is chosen.
    #[test]
    fn every_drum_sounds_and_ends() {
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        for drum in 0..6 {
            let mut v = SPEC.start();
            v[DRUM] = drum;
            let mut d = make(44100, &v);
            d.note_on(60, 1.0, Some(1));
            let mut peak = 0.0f32;
            for _ in 0..4 {
                l.iter_mut().for_each(|x| *x = 0.0);
                d.render(&mut l, &mut r);
                peak = peak.max(l.iter().fold(0.0f32, |a, x| a.max(x.abs())));
            }
            assert!(peak > 0.001, "drum {drum} sounds (peak {peak})");
            let mut blocks = 0;
            while d.has_voices() && blocks < 3000 {
                l.iter_mut().for_each(|x| *x = 0.0);
                d.render(&mut l, &mut r);
                blocks += 1;
            }
            assert!(!d.has_voices(), "drum {drum} ends");
            l.iter_mut().for_each(|x| *x = 0.0);
            r.iter_mut().for_each(|x| *x = 0.0);
            d.render(&mut l, &mut r);
            assert!(
                l.iter().all(|x| x.abs() < 0.001),
                "drum {drum} is silent after its tail"
            );
        }
    }

    /// A closed hat is shorter than an open one; four hits ring at once.
    #[test]
    fn the_open_hat_rings_longer_and_four_hits_ring() {
        let mut closed = SPEC.start();
        closed[DRUM] = 2;
        closed[DECAY] = 96;
        let mut open = closed.clone();
        open[DRUM] = 3;
        let ends = |v: &[i32]| {
            let mut d = make(44100, v);
            d.note_on(60, 1.0, Some(1));
            let mut blocks = 0;
            let mut l = vec![0.0f32; 256];
            let mut r = vec![0.0f32; 256];
            while d.has_voices() && blocks < 3000 {
                l.iter_mut().for_each(|x| *x = 0.0);
                d.render(&mut l, &mut r);
                blocks += 1;
            }
            blocks
        };
        let (c, o) = (ends(&closed), ends(&open));
        assert!(o > c, "the open hat rings longer ({o} against {c})");
        let mut d = make(44100, &SPEC.start());
        for (i, key) in [48u8, 52, 56, 60].into_iter().enumerate() {
            d.note_on(key, 1.0, Some(i as i32 + 1));
        }
        assert!(d.has_voices());
        d.note_on(64, 1.0, Some(5));
        assert!(d.take_ended().len() == 1, "the oldest hit ended");
    }
}
