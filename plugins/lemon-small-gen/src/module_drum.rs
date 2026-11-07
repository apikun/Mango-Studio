// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango ModuleDrum: Mango Studio's all-in-one drum machine (the roadmap's Mangomodular, the idea behind the
//! Machinedrum / LXR-2). Five synthesized drums at once — kick, snare, clap, closed hat, open hat — one per key
//! (the general MIDI keys first, then everything by its rest), each with its own tune, decay and level, and the
//! 808 / 909 kits the flavour of. Every voice is built from oscillators and noise, no samples, so it stays light
//! on the CPU. The pattern is the host's: the piano roll or the step rows play the keys.
//!
// spec: plugins/lemon-module-drum.md, plugins/lemon-small-generators.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::fruit_kick::volume_gain;
use crate::params::{db_text, leading_number, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

// ---------------------------------------------------------------------------------------------------------------
// The settings (spec §5): the kit, then the five drums' tune, decay and level

pub const SETTINGS: [Setting; 16] = [
    Setting { id: "kit", name: "Kit", range: (0, 1), start: 0, magnet: None },
    Setting { id: "kicktune", name: "Kick tune", range: (-12, 12), start: 0, magnet: Some(0) },
    Setting { id: "kickdecay", name: "Kick decay", range: (0, 128), start: 96, magnet: None },
    Setting { id: "kicklevel", name: "Kick level", range: (0, 512), start: 320, magnet: Some(256) },
    Setting { id: "snaretune", name: "Snare tune", range: (-12, 12), start: 0, magnet: Some(0) },
    Setting { id: "snaredecay", name: "Snare decay", range: (0, 128), start: 64, magnet: None },
    Setting { id: "snarelevel", name: "Snare level", range: (0, 512), start: 256, magnet: Some(256) },
    Setting { id: "claptune", name: "Clap tune", range: (-12, 12), start: 0, magnet: Some(0) },
    Setting { id: "clapdecay", name: "Clap decay", range: (0, 128), start: 56, magnet: None },
    Setting { id: "claplevel", name: "Clap level", range: (0, 512), start: 224, magnet: Some(256) },
    Setting { id: "chattune", name: "Closed hat tune", range: (-12, 12), start: 0, magnet: Some(0) },
    Setting { id: "chatdecay", name: "Closed hat decay", range: (0, 128), start: 40, magnet: None },
    Setting { id: "chatlevel", name: "Closed hat level", range: (0, 512), start: 224, magnet: Some(256) },
    Setting { id: "ohattune", name: "Open hat tune", range: (-12, 12), start: 0, magnet: Some(0) },
    Setting { id: "ohatdecay", name: "Open hat decay", range: (0, 128), start: 96, magnet: None },
    Setting { id: "ohatlevel", name: "Open hat level", range: (0, 512), start: 208, magnet: Some(256) },
];

pub const KIT: usize = 0;
/// Voice k's settings: tune 1 + 3k, decay 2 + 3k, level 3 + 3k.
pub const KICK: usize = 0;
pub const SNARE: usize = 1;
pub const CLAP: usize = 2;
pub const CLOSED_HAT: usize = 3;
pub const OPEN_HAT: usize = 4;

/// The five drums' names, in setting order.
pub const DRUMS: [&str; 5] = ["Kick", "Snare", "Clap", "Closed hat", "Open hat"];

/// The kits, in setting order.
pub const KITS: [&str; 2] = ["808", "909"];

/// A drum's key: the general MIDI keys, else everything by its rest.
pub fn drum_of(key: u8) -> usize {
    match key {
        35 | 36 | 48 => KICK,
        38 | 40 => SNARE,
        39 => CLAP,
        42 | 44 => CLOSED_HAT,
        46 => OPEN_HAT,
        k => (k % 5) as usize,
    }
}

/// Editor layout (spec §2): the kit on its own, then the five drums' cards of three knobs each.
const CARDS: [Card; 6] = [
    Card { x0: 8.0, x1: 116.0, y0: 28.0, y1: 124.0, heading: "KIT" },
    Card { x0: 122.0, x1: 236.0, y0: 28.0, y1: 124.0, heading: "KICK" },
    Card { x0: 242.0, x1: 356.0, y0: 28.0, y1: 124.0, heading: "SNARE" },
    Card { x0: 8.0, x1: 122.0, y0: 130.0, y1: 226.0, heading: "CLAP" },
    Card { x0: 128.0, x1: 242.0, y0: 130.0, y1: 226.0, heading: "CLOSED HAT" },
    Card { x0: 248.0, x1: 362.0, y0: 130.0, y1: 226.0, heading: "OPEN HAT" },
];

/// The five drums' cards' origins: (x0, y0) of the card, matching `CARDS`.
const CARD_XY: [(f32, f32); 5] = [(122.0, 28.0), (242.0, 28.0), (8.0, 130.0), (128.0, 130.0), (248.0, 130.0)];

const fn tune_id(voice: usize) -> &'static str {
    ["kicktune", "snaretune", "claptune", "chattune", "ohattune"][voice]
}

const fn decay_id(voice: usize) -> &'static str {
    ["kickdecay", "snaredecay", "clapdecay", "chatdecay", "ohatdecay"][voice]
}

const fn level_id(voice: usize) -> &'static str {
    ["kicklevel", "snarelevel", "claplevel", "chatlevel", "ohatlevel"][voice]
}

const fn tune_knob(voice: usize) -> Control {
    Control::Knob { id: tune_id(voice), cx: CARD_XY[voice].0 + 24.0, cy: CARD_XY[voice].1 + 52.0, r: 16.0, caption: "TUNE", bipolar: true }
}

const fn decay_knob(voice: usize) -> Control {
    Control::Knob { id: decay_id(voice), cx: CARD_XY[voice].0 + 62.0, cy: CARD_XY[voice].1 + 52.0, r: 16.0, caption: "DECAY", bipolar: false }
}

const fn level_knob(voice: usize) -> Control {
    Control::Knob { id: level_id(voice), cx: CARD_XY[voice].0 + 100.0, cy: CARD_XY[voice].1 + 52.0, r: 16.0, caption: "LEVEL", bipolar: false }
}

const CONTROLS: [Control; 16] = [
    Control::Choice { id: "kit", x: 20.0, top: 42.0, names: &KITS },
    tune_knob(KICK),
    decay_knob(KICK),
    level_knob(KICK),
    tune_knob(SNARE),
    decay_knob(SNARE),
    level_knob(SNARE),
    tune_knob(CLAP),
    decay_knob(CLAP),
    level_knob(CLAP),
    tune_knob(CLOSED_HAT),
    decay_knob(CLOSED_HAT),
    level_knob(CLOSED_HAT),
    tune_knob(OPEN_HAT),
    decay_knob(OPEN_HAT),
    level_knob(OPEN_HAT),
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango ModuleDrum",
    word: "MODULEDRUM",
    about: "Mango Studio's all-in-one drum machine",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (526, 342),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark { x: 516.0, y: 323.0, anchor: 1.0, stacked: false },
    hint: Some(Rect::new(8.0, 284.0, 510.0, 22.0)),
    labels: &[(510.0, 62.0, "Keys: K S C H O")],
    text,
    parse,
    make,
};

// ---------------------------------------------------------------------------------------------------------------
// The texts

fn st_text(v: i32) -> String {
    if v > 0 { format!("+{v} st") } else { format!("{v} st") }
}

/// A decay's seconds: the knob's 0..128 over a drum-shaped range (0.05 s to 2 s).
fn decay_seconds(v: i32) -> f64 {
    0.05 * (v.max(0) as f64 / 64.0)
}

fn decay_text(secs: f64) -> String {
    if secs < 1.0 { format!("{:.0}ms", secs * 1000.0) } else { format!("{secs:.2}s") }
}

fn level_gain(v: i32) -> f32 {
    volume_gain(v)
}

/// Value texts (spec §3).
fn text(k: usize, v: i32) -> String {
    match k {
        KIT => KITS[v.clamp(0, 1) as usize].to_string(),
        k if k >= 1 => match (k - 1) % 3 {
            0 => st_text(v),
            1 => decay_text(decay_seconds(v)),
            _ => db_text(level_gain(v)),
        },
        _ => String::new(),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    let s = s.trim().to_ascii_lowercase();
    match k {
        KIT => KITS.iter().position(|n| s.starts_with(n)).map(|v| v as i32),
        k if k >= 1 => {
            let which = (k - 1) % 3;
            match which {
                0 => leading_number(&s).map(|x| (x.round() as i32).clamp(-12, 12)),
                1 => leading_number(&s).map(|x| {
                    let secs = if s.contains('s') && !s.contains("ms") { x } else { x / 1000.0 };
                    ((secs / 0.05 * 64.0).round() as i32).clamp(0, 128)
                }),
                _ => parse_db(SETTINGS[k].range, &s, level_gain),
            }
        }
        _ => None,
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

/// One hit: the drum, its snapshot of the settings (with the kit's flavour), its state.
struct Voice {
    key: u8,
    voice_id: Option<i32>,
    drum: usize,
    vel: f32,
    /// The hit's place in seconds.
    t: f32,
    /// The swept sine's phase, the hats' six squares' places and the noise's state.
    phase: f32,
    squares: [f32; 6],
    rng: u32,
    /// The noise's lowpass and the hats' highpass states.
    lp: f32,
    hp: f32,
    /// The hit's own decay and tune, from its settings and the kit.
    decay: f32,
    tune: f32,
    /// The kit's shaping of the recipe.
    bright: f32,
    punch: f32,
    /// The hit's gain.
    gain: f32,
    age: f32,
}

impl Voice {
    /// The hit's envelope over its decay, a shape per drum.
    fn level(&self) -> f32 {
        let d = self.decay.max(0.005);
        match self.drum {
            KICK => (-self.t / d).exp(),
            SNARE => (-self.t / (d * 0.5)).exp(),
            CLAP => (-((self.t - 0.036).max(0.0)) / (d * 0.4)).exp(),
            CLOSED_HAT => (-self.t / (d * 0.35)).exp(),
            _ => (-self.t / d).exp(),
        }
    }

    /// One sample of the hit.
    fn sample(&mut self, dt: f32) -> f32 {
        let semi = 2f32.powf(self.tune / 12.0);
        let out = match self.drum {
            // The kick: a sine swept down from its snap to its base, a click on top; the 909 punches harder
            // and starts higher.
            KICK => {
                let base = 50.0 * semi * (1.0 + 0.15 * self.punch);
                let start = base * (1.0 + (6.0 + 3.0 * self.punch));
                let f = self.t.mul_add(-1.0 / (0.03 + 0.02 * (1.0 - self.punch)), start).max(base);
                self.phase = (self.phase + std::f32::consts::TAU * f * dt) % std::f32::consts::TAU;
                let click = 0.2 * noise(&mut self.rng) * (-self.t / 0.003).exp() * (1.0 + self.punch);
                let body = self.phase.sin();
                // The 808's long tail is rounder: a little of the octave below.
                let sub = 0.25 * self.punch * (self.phase * 0.5).sin();
                (body * 0.9 + sub) * self.level() + click
            }
            // The snare: two tones and a noise body, the hats' metal rattling behind them.
            SNARE => {
                let f1 = 185.0 * semi * (1.0 + 0.1 * self.bright);
                let f2 = 330.0 * semi;
                self.phase = (self.phase + std::f32::consts::TAU * f1 * dt) % std::f32::consts::TAU;
                let p2 = (self.phase * (f2 / f1)) % 1.0;
                let tones = self.phase.sin() * 0.6 + (std::f32::consts::TAU * p2).sin() * 0.4;
                let raw = noise(&mut self.rng);
                self.lp += 0.28 * (raw - self.lp);
                let hiss = (raw - self.lp) * (0.5 + 0.5 * self.bright);
                let rattle = metal(self, dt) * 0.25;
                (tones * 0.7 + hiss + rattle) * self.level() + 0.2 * raw * (-self.t / 0.002).exp()
            }
            // The hats: six inharmonic squares through a highpass; the 909 rings brighter.
            CLOSED_HAT | OPEN_HAT => {
                let metal = metal(self, dt);
                let raw = noise(&mut self.rng);
                self.lp += 0.9 * (raw - self.lp);
                let hiss = (raw - self.lp) * 0.5;
                (metal * 0.7 + hiss) * self.level()
            }
            // The clap: three noise pulses, then the tail.
            _ => {
                let raw = noise(&mut self.rng);
                self.lp += 0.35 * (raw - self.lp);
                let pulse = |t0: f32, t: f32| (-((t - t0).max(0.0)) / 0.004).exp() * ((t > t0) || (t == t0)) as i32 as f32;
                let hits = (pulse(0.0, self.t) + pulse(0.012, self.t) + pulse(0.024, self.t)) * (0.6 + 0.6 * self.punch);
                (self.lp * self.level() * (0.5 + 0.5 * self.bright) + raw * hits * 0.5) * 0.8
            }
        };
        self.t += dt;
        self.age += dt;
        out * self.gain * self.vel
    }
}

/// The decay knob's seconds at a strike.
fn d_of(stored: f32) -> f32 {
    (0.05 * (stored / 64.0)).max(0.008)
}

/// The hats' metal: six inharmonic squares through a highpass.
fn metal(v: &mut Voice, dt: f32) -> f32 {
    let base = 40.0 * 2f32.powf(v.tune / 12.0) * (1.0 + v.bright * 0.6);
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

struct ModuleDrum {
    rate: f32,
    /// The five drums' tune, decay (in seconds) and gain, from the settings.
    tune: [f32; 5],
    decay: [f32; 5],
    gain: [f32; 5],
    /// The kit's flavour: 0 the 808 (round), 1 the 909 (bright and punchy).
    kit: usize,
    bright: f32,
    punch: f32,
    /// Up to six hits at once; a new one on a full machine takes the oldest.
    voices: Vec<Voice>,
    ended: Vec<(u8, Option<i32>)>,
}

/// A voice's decay for its drum: the knob's seconds, the drum's own scale.
fn voice_decay(stored: f32, drum: usize) -> f32 {
    let d = d_of(stored);
    match drum {
        KICK => d * 1.4,
        SNARE => d * 0.9,
        CLAP => d * 0.7,
        CLOSED_HAT => d * 0.35,
        _ => d,
    }
}

impl Instrument for ModuleDrum {
    fn set(&mut self, d: &[i32]) {
        self.kit = d[KIT].clamp(0, 1) as usize;
        let (bright, punch) = if self.kit == 0 { (0.0, 0.0) } else { (0.45, 1.0) };
        self.bright = bright;
        self.punch = punch;
        for v in 0..5 {
            self.tune[v] = f64::from(d[1 + 3 * v]) as f32;
            self.decay[v] = voice_decay(f64::from(d[2 + 3 * v]) as f32, v);
            self.gain[v] = level_gain(d[3 + 3 * v]);
        }
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate as f32;
    }

    fn reset(&mut self) {
        self.voices.clear();
        self.ended.clear();
    }

    fn note_on(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        let v = drum_of(key);
        // A free slot, else the oldest goes.
        if self.voices.len() >= 6 {
            let i = self.voices.iter().enumerate().min_by(|a, b| a.1.age.total_cmp(&b.1.age)).map(|(i, _)| i).unwrap_or(0);
            if let Some(old) = self.voices.get(i) {
                self.ended.push((old.key, old.voice_id));
            }
            self.voices.remove(i);
        }
        self.voices.push(Voice {
            key,
            voice_id: Some(voice_id.unwrap_or(i32::from(key) + 1)),
            drum: v,
            vel: velocity.clamp(0.05, 1.0),
            t: 0.0,
            phase: 0.0,
            squares: [0.0; 6],
            rng: 0x1234_5678 ^ u32::from(key).wrapping_mul(2_654_435_761),
            lp: 0.0,
            hp: 0.0,
            decay: self.decay[v],
            tune: self.tune[v],
            bright: self.bright,
            punch: self.punch,
            gain: self.gain[v],
            age: 0.0,
        });
    }

    fn note_off(&mut self, _key: u8, _voice_id: Option<i32>) {}

    fn choke(&mut self, key: u8) {
        self.voices.retain(|v| {
            if drum_of(v.key) == drum_of(key) {
                self.ended.push((v.key, v.voice_id));
                false
            } else {
                true
            }
        });
    }

    fn all_off(&mut self) {
        for v in self.voices.drain(..) {
            self.ended.push((v.key, v.voice_id));
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let dt = 1.0 / self.rate;
        for i in 0..left.len().min(right.len()) {
            let mut out = 0.0f32;
            let ended = &mut self.ended;
            self.voices.retain_mut(|v| {
                out += v.sample(dt);
                // The hit rings out to its own end (the envelope decides), 10 s at the longest.
                if v.t < 10.0 {
                    true
                } else {
                    ended.push((v.key, v.voice_id));
                    false
                }
            });
            left[i] += out;
            right[i] += out;
        }
    }

    fn take_ended(&mut self) -> Vec<(u8, Option<i32>)> {
        std::mem::take(&mut self.ended)
    }

    fn has_voices(&self) -> bool {
        !self.voices.is_empty()
    }
}

/// The instrument, made for a sample rate from the starting settings.
fn make(rate: u32, v: &[i32]) -> Box<dyn Instrument> {
    Box::new(make_drum(rate, v))
}

/// The concrete machine, for the tests.
fn make_drum(rate: u32, v: &[i32]) -> ModuleDrum {
    let mut m = ModuleDrum {
        rate: rate as f32,
        tune: [0.0; 5],
        decay: [0.05; 5],
        gain: [1.0; 5],
        kit: 0,
        bright: 0.0,
        punch: 0.0,
        voices: Vec::new(),
        ended: Vec::new(),
    };
    m.set(&v[..SETTINGS.len().min(v.len())]);
    m
}

pub struct ModuleDrumKind;

impl Kind for ModuleDrumKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.lemon-module-drum";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's all-in-one drum machine";
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"LemonStudioModDr";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Drum];
}

pub type LemonModuleDrum = SmallGen<ModuleDrumKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(KIT, 0), "808");
        assert_eq!(text(KIT, 1), "909");
        assert_eq!(text(1, 5), "+5 st");
        assert_eq!(text(1, -7), "-7 st");
        assert_eq!(text(2, 128), "100ms");
        assert_eq!(text(3, 256), db_text(level_gain(256)));
        assert_eq!(parse(KIT, "909"), Some(1));
        assert_eq!(parse(KIT, "808"), Some(0));
        assert_eq!(parse(1, "+3"), Some(3));
        assert_eq!(parse(1, "-2 st"), Some(-2));
        assert_eq!(parse(2, "50ms"), Some(64));
        assert_eq!(parse(2, "0.1s"), Some(128), "the range's top");
    }

    #[test]
    fn keys_find_their_drums() {
        assert_eq!(drum_of(36), KICK);
        assert_eq!(drum_of(38), SNARE);
        assert_eq!(drum_of(39), CLAP);
        assert_eq!(drum_of(42), CLOSED_HAT);
        assert_eq!(drum_of(46), OPEN_HAT);
        // Everything else by its rest.
        assert_eq!(drum_of(60), KICK);
        assert_eq!(drum_of(61), SNARE);
    }

    #[test]
    fn the_machine_plays_five_drums_at_once() {
        let mut m = make_drum(44100, &SPEC.start());
        for k in [36u8, 38, 39, 42, 46] {
            m.note_on(k, 0.8, None);
        }
        assert_eq!(m.voices.len(), 5, "one hit per key");
        let (mut l, mut r) = (vec![0.0f32; 4410], vec![0.0f32; 4410]);
        m.render(&mut l, &mut r);
        let peak = l.iter().fold(0.0f32, |a, &x| a.max(x.abs()));
        assert!(peak > 0.2, "the drums sound: {peak}");
        assert!(m.has_voices());
        assert!(!m.take_ended().is_empty() || m.voices.len() == 5, "ends come back as hits die");
        // A seventh hit takes the oldest.
        m.note_on(48, 0.8, None);
        assert_eq!(m.voices.len(), 6);
    }

    #[test]
    fn kits_change_the_flavour() {
        let hit = |kit: i32| {
            let mut v = SPEC.start();
            v[KIT] = kit;
            let mut m = make_drum(44100, &v);
            m.note_on(36, 1.0, None);
            let (mut l, mut r) = (vec![0.0f32; 8820], vec![0.0f32; 8820]);
            m.render(&mut l, &mut r);
            l.iter().fold(0.0f32, |a, &x| a.max(x.abs()))
        };
        let (a808, a909) = (hit(0), hit(1));
        assert!((a808 - a909).abs() > 0.01, "the kits differ: {a808} vs {a909}");
    }

    #[test]
    fn all_off_and_choke_empty_the_machine() {
        let mut m = make_drum(44100, &SPEC.start());
        for k in [36u8, 38, 42] {
            m.note_on(k, 0.8, None);
        }
        m.choke(42);
        assert_eq!(m.voices.len(), 2, "the closed hat's choke takes only its voices");
        m.all_off();
        assert!(m.voices.is_empty());
        assert_eq!(m.take_ended().len(), 3);
    }
}
