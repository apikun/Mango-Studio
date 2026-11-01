// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Keys: Mango Studio's electric piano (the roadmap's "FL Keys"). Three partials of a tine — the fundamental,
//! its octave as the body and the fourth partial that rings and fades fast as the bell — a hammer of noise on the
//! strike, a damper's fade on the key up, the pitch stretched the way real tines go sharp, and a tremolo of its
//! own. Eight notes at once; the low ones ring longer than the high ones.
//!
// spec: plugins/lemon-small-generators.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::fruit_kick::volume_gain;
use crate::monk::time_seconds;
use crate::params::{db_text, leading_number, nearest, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

// ---------------------------------------------------------------------------------------------------------------
// The settings (spec §5): the stored order, their ranges and their starting values

pub const SETTINGS: [Setting; 10] = [
    Setting { id: "bell", name: "Bell", range: (0, 128), start: 40, magnet: None },
    Setting { id: "body", name: "Body", range: (0, 128), start: 64, magnet: None },
    Setting { id: "hammer", name: "Hammer", range: (0, 128), start: 28, magnet: None },
    Setting { id: "decay", name: "Decay", range: (0, 128), start: 78, magnet: None },
    Setting { id: "stiffness", name: "Stiffness", range: (0, 128), start: 44, magnet: None },
    Setting { id: "release", name: "Release", range: (0, 128), start: 40, magnet: None },
    Setting { id: "stretch", name: "Stretch", range: (0, 128), start: 32, magnet: None },
    Setting { id: "trem", name: "Tremolo", range: (0, 128), start: 0, magnet: None },
    Setting { id: "trate", name: "Rate", range: (0, 128), start: 48, magnet: None },
    Setting { id: "volume", name: "Volume", range: (0, 512), start: 256, magnet: Some(256) },
];

pub const BELL: usize = 0;
pub const BODY: usize = 1;
pub const HAMMER: usize = 2;
pub const DECAY: usize = 3;
pub const STIFFNESS: usize = 4;
pub const RELEASE: usize = 5;
pub const STRETCH: usize = 6;
pub const TREM: usize = 7;
pub const TRATE: usize = 8;
pub const VOLUME: usize = 9;

/// Editor layout (spec §2): two rows of cards over the hint and the wordmark.
const CARDS: [Card; 5] = [
    Card { x0: 8.0, x1: 178.0, y0: 28.0, y1: 134.0, heading: "TINE" },
    Card { x0: 184.0, x1: 298.0, y0: 28.0, y1: 134.0, heading: "TAIL" },
    Card { x0: 8.0, x1: 122.0, y0: 140.0, y1: 246.0, heading: "TOUCH" },
    Card { x0: 128.0, x1: 242.0, y0: 140.0, y1: 246.0, heading: "TREMOLO" },
    Card { x0: 248.0, x1: 362.0, y0: 140.0, y1: 246.0, heading: "OUTPUT" },
];

const CONTROLS: [Control; 10] = [
    Control::Knob { id: "bell", cx: 40.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "body", cx: 90.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "hammer", cx: 140.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "decay", cx: 216.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "stiffness", cx: 266.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "stretch", cx: 40.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "release", cx: 90.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "trem", cx: 160.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "trate", cx: 210.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "volume", cx: 305.0, cy: 198.0, r: 24.0, caption: "", bipolar: false },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Keys",
    word: "KEYS",
    about: "Mango Studio's electric piano",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (370, 280),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark { x: 362.0, y: 263.0, anchor: 1.0, stacked: false },
    hint: Some(Rect::new(8.0, 252.0, 238.0, 22.0)),
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

/// The decay knob's seconds for the lowest note; the stiffness leans it down as the keys rise.
pub fn decay_seconds(v: i32) -> f64 {
    0.2 * 2f64.powf(f64::from(v) / 128.0 * 6.6)
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

/// The stretch knob's cents per octave from middle C, 0 to 25.
pub fn stretch_cents(v: i32) -> f64 {
    f64::from(v) / 128.0 * 25.0
}

/// The tremolo rate knob's Hz, 0.5 to about 14.
pub fn trem_hz(v: i32) -> f64 {
    0.5 * 2f64.powf(f64::from(v) / 128.0 * 4.8)
}

/// A time as `instant`, `123ms` or `1.2s` (the damper's law).
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
    nearest(SETTINGS[RELEASE].range, x, |v| {
        let t = time_seconds(v);
        Some(if t < 1.0 { (t * 1000.0).round() / 1000.0 } else { (t * 10.0).round() / 10.0 })
    })
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        BELL | BODY | HAMMER | STIFFNESS | STRETCH | TREM => percent(SETTINGS[k].range, v),
        DECAY => decay_text(decay_text_seconds(v)),
        RELEASE => time_text(v),
        TRATE => format!("{:.1}Hz", trem_hz(v)),
        _ => db_text(volume_gain(v)),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    let s = s.trim().to_ascii_lowercase();
    match k {
        BELL | BODY | HAMMER | STIFFNESS | STRETCH | TREM => percent_parse(SETTINGS[k].range, &s),
        DECAY => decay_parse(&s),
        RELEASE => time_parse(&s),
        TRATE => {
            let x = leading_number(&s)?;
            nearest(SETTINGS[TRATE].range, x, |v| Some(trem_hz(v)))
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

/// xorshift32, ±1.
fn noise(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// One tine: its partials, its hammer, its damper.
struct Voice {
    key: u8,
    voice_id: Option<i32>,
    /// The fundamental's place in its cycle, 0..1 (the octave and the bell double and quadruple it).
    phase: f32,
    /// The bell's fade, the hammer's since the strike.
    bell: f32,
    t: f32,
    /// The decay this note rings over (the stiffness leaned it), and the release fade left.
    tau: f32,
    rel: f32,
    vel: f32,
    age: f32,
}

struct Keys {
    rate: f32,
    bell: f32,
    body: f32,
    hammer: f32,
    decay: f32,
    stiffness: f32,
    release: f32,
    stretch: f32,
    trem: f32,
    trate: f32,
    gain: f32,
    /// The tremolo's place in its cycle.
    trem_phase: f32,
    rng: u32,
    voices: [Option<Voice>; 8],
    ended: Vec<(u8, Option<i32>)>,
}

impl Keys {
    /// A note's decay: the low notes' time leaned down as the keys rise, the harder the tine.
    fn note_decay(&self, key: u8) -> f32 {
        tine_decay(self.decay, self.stiffness, key)
    }
}

/// A note's decay: the low notes' time leaned down as the keys rise, the harder the tine.
fn tine_decay(decay: f32, stiffness: f32, key: u8) -> f32 {
    let lean = -stiffness * (f32::from(key) - 60.0) / 12.0;
    (decay * 2f32.powf(lean)).max(0.05)
}

impl Instrument for Keys {
    fn set(&mut self, v: &[i32]) {
        self.bell = f64::from(v[BELL]) as f32 / 128.0;
        self.body = f64::from(v[BODY]) as f32 / 128.0;
        self.hammer = f64::from(v[HAMMER]) as f32 / 128.0;
        self.decay = decay_seconds(v[DECAY]) as f32;
        self.stiffness = f64::from(v[STIFFNESS]) as f32 / 128.0;
        self.release = time_seconds(v[RELEASE]) as f32;
        self.stretch = stretch_cents(v[STRETCH]) as f32;
        self.trem = f64::from(v[TREM]) as f32 / 128.0;
        self.trate = trem_hz(v[TRATE]) as f32;
        self.gain = volume_gain(v[VOLUME]);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate as f32;
    }

    fn reset(&mut self) {
        self.voices = [const { None }; 8];
        self.trem_phase = 0.0;
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
            bell: 1.0,
            t: 0.0,
            tau: (self.note_decay(key) / 4.0).max(0.005),
            rel: f32::INFINITY,
            vel: velocity.clamp(0.05, 1.0),
            age: 0.0,
        });
    }

    fn note_off(&mut self, key: u8, voice_id: Option<i32>) {
        for s in self.voices.iter_mut().flatten() {
            if s.key == key && (voice_id.is_none() || s.voice_id == voice_id) && s.rel.is_infinite() {
                s.rel = (self.release / 4.0).max(0.002);
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
        let (bell, body, hammer, gain) = (self.bell, self.body, self.hammer, self.gain);
        // The tremolo rides every note together, like the tine bar's wobble.
        let trem_depth = self.trem;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.trem_phase = (self.trem_phase + self.trate * dt).fract();
            let trem = 1.0 - trem_depth * (0.5 - 0.5 * (std::f32::consts::TAU * self.trem_phase).cos());
            let mut out = 0.0f32;
            for s in self.voices.iter_mut().flatten() {
                // The tine's pitch, stretched the way real tines go sharp as the keys rise.
                let cents = self.stretch * (f32::from(s.key) - 60.0) / 12.0;
                let f = key_hz(s.key) * 2f32.powf(cents / 1200.0);
                s.phase = (s.phase + f * dt).fract();
                let w = (std::f32::consts::TAU * s.phase).sin();
                let w2 = (std::f32::consts::TAU * (s.phase * 2.0).fract()).sin();
                let w4 = (std::f32::consts::TAU * (s.phase * 4.0).fract()).sin();
                s.bell *= (-dt / 0.08).exp();
                let strike = s.t;
                s.t += dt;
                let thump = hammer * noise(&mut self.rng) * (-strike / 0.004).exp();
                // The level: the note's own decay, the damper's fade on the key up.
                let damper = match s.rel {
                    f if f.is_infinite() => 1.0,
                    mut f => {
                        f -= dt;
                        s.rel = f;
                        (f / (self.release / 4.0).max(0.002)).max(0.0)
                    }
                };
                let level = (-strike / s.tau).exp() * damper;
                out += (w * (1.0 - body * 0.5) + w2 * body * 0.5 + w4 * bell * s.bell * 0.6 + thump * 0.4)
                    * level
                    * s.vel
                    * trem;
                s.age += dt;
            }
            let out = out * gain * 0.4;
            *l += out;
            *r += out;
        }
        // The tines that faded: the damper down, or the decay done.
        for s in self.voices.iter_mut() {
            let gone = s.as_ref().is_some_and(|v| v.rel <= 0.0 || v.age > v.tau * 25.0);
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
    let mut k = Keys {
        rate: rate as f32,
        bell: f64::from(v[BELL]) as f32 / 128.0,
        body: f64::from(v[BODY]) as f32 / 128.0,
        hammer: f64::from(v[HAMMER]) as f32 / 128.0,
        decay: decay_seconds(v[DECAY]) as f32,
        stiffness: f64::from(v[STIFFNESS]) as f32 / 128.0,
        release: time_seconds(v[RELEASE]) as f32,
        stretch: stretch_cents(v[STRETCH]) as f32,
        trem: f64::from(v[TREM]) as f32 / 128.0,
        trate: trem_hz(v[TRATE]) as f32,
        gain: volume_gain(v[VOLUME]),
        trem_phase: 0.0,
        rng: 0x1234_5678,
        voices: [const { None }; 8],
        ended: Vec::new(),
    };
    k.set_sample_rate(rate);
    Box::new(k)
}

pub struct KeysKind;

impl Kind for KeysKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-keys";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's electric piano";
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioKeys\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type MangoKeys = SmallGen<KeysKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(BELL, 64), "50%");
        assert_eq!(parse(BELL, "50%"), Some(64));
        assert_eq!(text(DECAY, 0), "200ms");
        assert_eq!(parse(DECAY, "200ms"), Some(0));
        assert_eq!(text(DECAY, 128), "19.4s");
        assert_eq!(parse(DECAY, "19.4s"), Some(128));
        assert_eq!(text(RELEASE, 0), "instant");
        assert_eq!(parse(RELEASE, "instant"), Some(0));
        assert_eq!(text(TRATE, 48), "1.7Hz");
        assert_eq!(parse(TRATE, "1.7Hz"), Some(47), "the nearest rate to 1.7");
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
    }

    /// A note rings, the low ones longer than the high ones, and a release ends it.
    #[test]
    fn a_note_rings_and_the_release_ends_it() {
        let mut k = make(44100, &SPEC.start());
        k.note_on(60, 1.0, Some(1));
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        let mut peak = 0.0f32;
        for _ in 0..10 {
            l.iter_mut().for_each(|x| *x = 0.0);
            k.render(&mut l, &mut r);
            peak = peak.max(l.iter().fold(0.0f32, |a, x| a.max(x.abs())));
        }
        assert!(peak > 0.01, "the tine rings (peak {peak})");
        assert!(k.has_voices());
        k.note_off(60, Some(1));
        let mut blocks = 0;
        while k.has_voices() && blocks < 2000 {
            l.iter_mut().for_each(|x| *x = 0.0);
            k.render(&mut l, &mut r);
            blocks += 1;
        }
        assert!(!k.has_voices(), "the damper ends the note");
        assert_eq!(k.take_ended(), vec![(60, Some(1))]);
        l.iter_mut().for_each(|x| *x = 0.0);
        r.iter_mut().for_each(|x| *x = 0.0);
        k.render(&mut l, &mut r);
        assert!(l.iter().all(|x| x.abs() < 0.001), "silence after the damper");
    }

    /// A stiff tine's high notes fade faster than its low ones.
    #[test]
    fn stiffness_leans_the_decay() {
        assert!(tine_decay(2.0, 0.5, 48) > tine_decay(2.0, 0.5, 72), "the low note rings longer");
    }

    /// The tremolo wobbles: the same note with the tremolo off differs from with it on.
    #[test]
    fn the_tremolo_wobbles() {
        let mut a = make(44100, &SPEC.start());
        let mut v = SPEC.start();
        v[TREM] = 128;
        v[TRATE] = 24;
        let mut b = make(44100, &v);
        a.note_on(60, 1.0, Some(1));
        b.note_on(60, 1.0, Some(1));
        let mut la = vec![0.0f32; 256];
        let mut ra = vec![0.0f32; 256];
        let mut lb = vec![0.0f32; 256];
        let mut rb = vec![0.0f32; 256];
        let mut diff = 0.0f32;
        for _ in 0..40 {
            la.iter_mut().for_each(|x| *x = 0.0);
            lb.iter_mut().for_each(|x| *x = 0.0);
            a.render(&mut la, &mut ra);
            b.render(&mut lb, &mut rb);
            diff = diff.max(
                la.iter()
                    .zip(lb.iter())
                    .fold(0.0f32, |a, (x, y)| a.max((x - y).abs())),
            );
        }
        assert!(diff > 1e-3, "the tremolo wobbles ({diff})");
    }
}
