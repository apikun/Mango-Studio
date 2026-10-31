// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Lemon Acid: Lemon Studio's acid bass, a clean-room take on the classic silver-box bassline synth (the roadmap's
//! "Transistor Bass"). A saw per note into a resonant state-variable lowpass that the envelope sweeps down from the
//! cutoff, the drive rounding it off; a hard note re-strikes the filter, a loud note leans on it (the accent). Four
//! notes at once; a fifth steals the oldest.
//!
// spec: plugins/lemon-acid.md, plugins/lemon-small-generators.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::fruit_kick::volume_gain;
use crate::params::{db_text, leading_number, nearest, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

/// The settings in stored order (spec §5).
pub const SETTINGS: [Setting; 6] = [
    Setting {
        id: "cutoff",
        name: "Cutoff",
        range: (0, 128),
        start: 42,
        magnet: None,
    },
    Setting {
        id: "resonance",
        name: "Resonance",
        range: (0, 128),
        start: 64,
        magnet: None,
    },
    Setting {
        id: "envelope",
        name: "Envelope",
        range: (0, 128),
        start: 56,
        magnet: None,
    },
    Setting {
        id: "decay",
        name: "Decay",
        range: (0, 128),
        start: 44,
        magnet: None,
    },
    Setting {
        id: "drive",
        name: "Drive",
        range: (0, 128),
        start: 20,
        magnet: None,
    },
    Setting {
        id: "volume",
        name: "Volume",
        range: (0, 512),
        start: 256,
        magnet: Some(256),
    },
];

pub const CUTOFF: usize = 0;
pub const RESONANCE: usize = 1;
pub const ENVELOPE: usize = 2;
pub const DECAY: usize = 3;
pub const DRIVE: usize = 4;
pub const VOLUME: usize = 5;

/// Editor layout (spec §2): three cards of two knobs.
const CARDS: [Card; 3] = [
    Card {
        x0: 8.0,
        x1: 122.0,
        y0: 28.0,
        y1: 124.0,
        heading: "FILTER",
    },
    Card {
        x0: 128.0,
        x1: 242.0,
        y0: 28.0,
        y1: 124.0,
        heading: "ENVELOPE",
    },
    Card {
        x0: 248.0,
        x1: 362.0,
        y0: 28.0,
        y1: 124.0,
        heading: "OUTPUT",
    },
];

const CONTROLS: [Control; 6] = [
    Control::Knob {
        id: "cutoff",
        cx: 40.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "resonance",
        cx: 90.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "envelope",
        cx: 160.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "decay",
        cx: 210.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "drive",
        cx: 280.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "volume",
        cx: 330.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Acid",
    word: "ACID",
    about: "Mango Studio's acid bass",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (370, 162),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark {
        x: 354.0,
        y: 143.0,
        anchor: 1.0,
        stacked: false,
    },
    hint: Some(Rect::new(8.0, 132.0, 346.0, 22.0)),
    labels: &[],
    text,
    parse,
    make,
};

// ---------------------------------------------------------------------------------------------------------------
// Laws (spec §3)

/// The share of a 0..128 knob as a percent.
fn percent(range: (i32, i32), v: i32) -> String {
    format!(
        "{:.0}%",
        f64::from(v - range.0) / f64::from(range.1 - range.0) * 100.0
    )
}

fn percent_parse(range: (i32, i32), s: &str) -> Option<i32> {
    let x = leading_number(s)?;
    nearest(range, x, |v| {
        Some(f64::from(v - range.0) / f64::from(range.1 - range.0) * 100.0)
    })
}

/// The cutoff knob's corner frequency: 40 Hz to 40 * 2⁷ Hz over the range.
pub fn cutoff_hz(v: i32) -> f64 {
    40.0 * 2f64.powf(f64::from(v) / 128.0 * 7.0)
}

/// The resonance knob's Q: gentle at the bottom, singing (never self-oscillating) at the top.
pub fn resonance_q(v: i32) -> f64 {
    let x = f64::from(v) / 128.0;
    0.5 + x * x * 8.5
}

/// The envelope knob's depth, in octaves of cutoff sweep.
pub fn envelope_octaves(v: i32) -> f64 {
    let x = f64::from(v) / 128.0;
    x * x.sqrt() * 5.0
}

/// The decay knob's time: the envelope falls to ~2 % of its start in it.
pub fn decay_seconds(v: i32) -> f64 {
    0.05 * 2f64.powf(f64::from(v) / 128.0 * 5.3)
}

/// The decay a value's text names (the number `nearest` matches a typed one against): the text's own rounding, so a
/// typed text gives back the value that printed it.
fn decay_text_seconds(v: i32) -> f64 {
    let t = decay_seconds(v);
    if t < 1.0 {
        (t * 1000.0).round() / 1000.0
    } else {
        (t * 10.0).round() / 10.0
    }
}

/// The drive knob's gain into the soft clip, 1 (clean) to 10 (saturating).
pub fn drive_gain(v: i32) -> f64 {
    1.0 + f64::from(v) / 128.0 * 9.0
}

/// A time as `123ms` under a second, `1.2s` from there.
fn seconds_text(t: f64) -> String {
    if t < 1.0 {
        format!("{:.0}ms", t * 1000.0)
    } else {
        format!("{:.1}s", t)
    }
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        CUTOFF => format!("{:.0}Hz", cutoff_hz(v)),
        RESONANCE | ENVELOPE | DRIVE => percent(SETTINGS[k].range, v),
        DECAY => seconds_text(decay_seconds(v)),
        _ => db_text(volume_gain(v)),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    match k {
        CUTOFF => {
            let x = leading_number(s)?;
            nearest(SETTINGS[CUTOFF].range, x, |v| Some(cutoff_hz(v)))
        }
        RESONANCE | ENVELOPE | DRIVE => percent_parse(SETTINGS[k].range, s),
        DECAY => {
            let s = s.trim().to_ascii_lowercase();
            let x = leading_number(&s)? / if s.ends_with("ms") { 1000.0 } else { 1.0 };
            nearest(SETTINGS[DECAY].range, x, |v| Some(decay_text_seconds(v)))
        }
        _ => parse_db(
            SETTINGS[VOLUME].range,
            s.trim().to_ascii_lowercase().as_str(),
            volume_gain,
        ),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The sound

/// The frequency of a key.
fn key_hz(key: u8) -> f32 {
    (440.0f64 * 2f64.powf((f64::from(key) - 69.0) / 12.0)) as f32
}

/// One note: a saw, its filter (a trapezoidal state-variable lowpass, the cutoff riding the envelope) and the fade
/// of its release.
struct Voice {
    key: u8,
    voice_id: Option<i32>,
    /// The saw's place in a cycle, 0..1, and its step per sample.
    phase: f32,
    step: f32,
    /// The filter's state: the low and band integrators.
    lp: f32,
    band: f32,
    /// The envelope's place, 1 at the strike falling toward 0.
    env: f32,
    /// The release fade left, when the key is up.
    rel: f32,
    vel: f32,
    age: f32,
}

struct Acid {
    rate: f32,
    cutoff: f32,
    q: f32,
    octaves: f32,
    tau: f32,
    drive: f32,
    gain: f32,
    voices: [Option<Voice>; 4],
    ended: Vec<(u8, Option<i32>)>,
}

impl Acid {
    /// A struck note: the envelope restarts, the saw keeps whatever phase the last note left (the silver box's
    /// oscillator ran free), the filter's state stays too, so a hard re-strike thumps like the real thing.
    fn strike(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) -> Voice {
        Voice {
            key,
            voice_id: Some(voice_id.unwrap_or(key as i32 + 1)),
            phase: 0.0,
            step: (key_hz(key) / self.rate).clamp(0.0, 0.5),
            lp: 0.0,
            band: 0.0,
            env: 1.0,
            rel: f32::INFINITY,
            vel: velocity.clamp(0.05, 1.0),
            age: 0.0,
        }
    }

    /// The accent: a note struck loud leans on the filter and the level, like the accent step of the silver box.
    fn accent(vel: f32) -> f32 {
        ((vel - 0.75) / 0.25).clamp(0.0, 1.0)
    }
}

impl Instrument for Acid {
    fn set(&mut self, v: &[i32]) {
        self.cutoff = cutoff_hz(v[CUTOFF]) as f32;
        self.q = resonance_q(v[RESONANCE]) as f32;
        self.octaves = envelope_octaves(v[ENVELOPE]) as f32;
        self.tau = (decay_seconds(v[DECAY]) / 4.0) as f32;
        self.drive = drive_gain(v[DRIVE]) as f32;
        self.gain = volume_gain(v[VOLUME]);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate as f32;
    }

    fn reset(&mut self) {
        self.voices = [const { None }; 4];
    }

    fn note_on(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        // A free voice first, else the oldest goes (spec §1).
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
        self.voices[slot] = Some(self.strike(key, velocity, voice_id));
    }

    fn note_off(&mut self, key: u8, voice_id: Option<i32>) {
        for s in self.voices.iter_mut().flatten() {
            if s.key == key && (voice_id.is_none() || s.voice_id == voice_id) && s.rel.is_infinite()
            {
                s.rel = 0.06;
            }
        }
    }

    fn choke(&mut self, key: u8) {
        for i in 0..4 {
            if let Some(s) = self.voices[i].take()
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
        let (q, octaves, tau, drive, gain) = (self.q, self.octaves, self.tau, self.drive, self.gain);
        // The envelope falls by this share per sample, whatever the block size.
        let fall = (-dt / tau.max(1e-4)).exp();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut out = 0.0f32;
            for s in self.voices.iter_mut().flatten() {
                // The cutoff: the knob's frequency, up as many octaves as the envelope still has; a loud note leans
                // on it (the accent).
                let acc = Self::accent(s.vel);
                let fc = cutoff_at(self.cutoff, octaves * (1.0 + acc * 0.5), s.env).min(self.rate * 0.25);
                let g = (std::f32::consts::PI * fc / self.rate).tan().clamp(0.0, 8.0);
                let k = 1.0 / q;
                let a1 = 1.0 / (1.0 + g * (g + k));
                let a2 = g * a1;
                let a3 = g * a2;
                // The saw, its phase kept across blocks.
                s.phase += s.step;
                s.phase -= s.phase.floor();
                let saw = s.phase * 2.0 - 1.0;
                // The filter (trapezoidal state variable, TPT): low out, the resonance pushing where it cuts.
                let v3 = saw - s.lp;
                let v1 = a1 * s.band + a2 * v3;
                let v2 = s.lp + a2 * s.band + a3 * v3;
                s.lp = 2.0 * v2 - s.lp;
                s.band = 2.0 * v1 - s.band;
                // The drive: a soft clip the knob leans into, its edges rounded.
                let shaped = (s.lp * drive).tanh();
                // The level: the velocity, the accent's lean, the release fade.
                let amp = match s.rel {
                    f if f.is_infinite() => 1.0,
                    mut f => {
                        f -= dt;
                        s.rel = f;
                        (f / 0.06).max(0.0)
                    }
                };
                out += shaped * amp * s.vel * (1.0 + acc * 0.3);
                s.env *= fall;
                s.age += dt;
            }
            let out = out * gain;
            *l += out;
            *r += out;
        }
        // The voices that faded: the release over, or a note held far past the longest decay.
        for i in 0..4 {
            let gone = self.voices[i]
                .as_ref()
                .is_some_and(|s| s.rel <= 0.0 || s.age > 20.0);
            if gone && let Some(s) = self.voices[i].take() {
                self.ended.push((s.key, s.voice_id));
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

/// The cutoff of a sample: the knob's frequency swept up by the envelope's remaining octaves.
fn cutoff_at(base: f32, octaves: f32, env: f32) -> f32 {
    base * 2f32.powf(octaves * env)
}

fn make(rate: u32, v: &[i32]) -> Box<dyn Instrument> {
    let mut a = Acid {
        rate: rate as f32,
        cutoff: cutoff_hz(v[CUTOFF]) as f32,
        q: resonance_q(v[RESONANCE]) as f32,
        octaves: envelope_octaves(v[ENVELOPE]) as f32,
        tau: (decay_seconds(v[DECAY]) / 4.0) as f32,
        drive: drive_gain(v[DRIVE]) as f32,
        gain: volume_gain(v[VOLUME]),
        voices: [const { None }; 4],
        ended: Vec::new(),
    };
    a.set_sample_rate(rate);
    Box::new(a)
}

pub struct AcidKind;

impl Kind for AcidKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.lemon-acid";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's acid bass";
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
    const VST3_CLASS_ID: [u8; 16] = *b"LemonStudioAcid0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type LemonAcid = SmallGen<AcidKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(CUTOFF, 0), "40Hz");
        assert_eq!(parse(CUTOFF, "40 Hz"), Some(0));
        assert_eq!(text(CUTOFF, 128), "5120Hz");
        assert_eq!(text(RESONANCE, 64), "50%");
        assert_eq!(parse(RESONANCE, "50%"), Some(64));
        assert_eq!(text(DECAY, 0), "50ms");
        assert_eq!(parse(DECAY, "50ms"), Some(0));
        assert_eq!(parse(DECAY, "0.05s"), Some(0));
        assert_eq!(text(DECAY, 128), "2.0s");
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
        assert_eq!(drive_gain(0), 1.0);
        assert!((drive_gain(128) - 10.0).abs() < 1e-6);
        assert_eq!(resonance_q(0), 0.5);
        assert!((resonance_q(128) - 9.0).abs() < 1e-6);
        assert_eq!(envelope_octaves(0), 0.0);
        assert!((envelope_octaves(128) - 5.0).abs() < 1e-6);
    }

    /// Four notes sound at once; a fifth takes the oldest's place.
    #[test]
    fn four_notes_a_fifth_steals_the_oldest() {
        let mut a = make(44100, &SPEC.start());
        for (i, key) in [36u8, 40, 43, 48].into_iter().enumerate() {
            a.note_on(key, 1.0, Some(i as i32 + 1));
        }
        assert!(a.has_voices());
        a.note_on(52, 1.0, Some(5));
        assert_eq!(
            a.take_ended(),
            vec![(36, Some(1))],
            "the oldest note ended"
        );
    }

    /// A key up fades the note out; a choke ends it at once.
    #[test]
    fn the_release_ends_the_note() {
        let mut a = make(44100, &SPEC.start());
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        a.note_on(36, 1.0, Some(1));
        a.note_off(36, Some(1));
        let mut blocks = 0;
        while a.has_voices() && blocks < 1000 {
            l.iter_mut().for_each(|x| *x = 0.0);
            a.render(&mut l, &mut r);
            blocks += 1;
        }
        assert!(!a.has_voices(), "the release ends the note");
        assert_eq!(a.take_ended(), vec![(36, Some(1))]);
        l.iter_mut().for_each(|x| *x = 0.0);
        a.render(&mut l, &mut r);
        assert!(
            l.iter().all(|x| x.abs() < 1e-3),
            "silence after the release"
        );
    }

    /// The filter sweeps: right after the strike the cutoff rides high (a bright start), late in the decay it is at
    /// the knob's frequency. A loud note leans harder than a quiet one (the accent).
    #[test]
    fn the_envelope_sweeps_and_a_loud_note_accents() {
        let v = SPEC.start();
        let base = cutoff_hz(v[CUTOFF]) as f32;
        // The envelope at its deepest: the cutoff opens five octaves over the knob's frequency.
        let octaves = envelope_octaves(128) as f32;
        let bright = cutoff_at(base, octaves, 1.0);
        let rest = cutoff_at(base, octaves, 0.0);
        assert!(
            bright > rest * 8.0,
            "the envelope opens the filter: {bright} over {rest}"
        );
        // The accent: a note struck at full velocity sweeps half as far again.
        let acc = Acid::accent(1.0);
        assert!(
            cutoff_at(base, octaves * (1.0 + acc * 0.5), 1.0) > bright,
            "the accent sweeps further"
        );
        assert_eq!(Acid::accent(0.5), 0.0, "a quiet note does not accent");
    }
}
