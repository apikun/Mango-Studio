// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Lemon Bass: Lemon Studio's bass guitar (a recreation of FL's BooBass) as a plugin. A Karplus-Strong string per
//! note: a pluck of noise into a delay line that a damped loop keeps, its tone, sustain, pick and body on the knobs.
//! Four strings at once; a fifth steals the oldest.
//!
// spec: plugins/lemon-bass.md, plugins/boobass.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::fruit_kick::volume_gain;
use crate::params::{db_text, leading_number, nearest, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

/// The settings in stored order (spec §5).
pub const SETTINGS: [Setting; 5] = [
    Setting {
        id: "tone",
        name: "Tone",
        range: (0, 128),
        start: 64,
        magnet: None,
    },
    Setting {
        id: "sustain",
        name: "Sustain",
        range: (0, 128),
        start: 56,
        magnet: None,
    },
    Setting {
        id: "pick",
        name: "Pick",
        range: (0, 128),
        start: 48,
        magnet: None,
    },
    Setting {
        id: "body",
        name: "Body",
        range: (0, 128),
        start: 40,
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

pub const TONE: usize = 0;
pub const SUSTAIN: usize = 1;
pub const PICK: usize = 2;
pub const BODY: usize = 3;
pub const VOLUME: usize = 4;

/// Editor layout (spec §2): three cards of knobs.
const CARDS: [Card; 3] = [
    Card {
        x0: 8.0,
        x1: 122.0,
        y0: 28.0,
        y1: 124.0,
        heading: "STRING",
    },
    Card {
        x0: 128.0,
        x1: 242.0,
        y0: 28.0,
        y1: 124.0,
        heading: "RESPONSE",
    },
    Card {
        x0: 248.0,
        x1: 362.0,
        y0: 28.0,
        y1: 124.0,
        heading: "OUTPUT",
    },
];

const CONTROLS: [Control; 5] = [
    Control::Knob {
        id: "tone",
        cx: 40.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "pick",
        cx: 90.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "sustain",
        cx: 160.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "body",
        cx: 210.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "volume",
        cx: 280.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Bass",
    word: "BASS",
    about: "Mango Studio's bass guitar",
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

/// The loop's feedback: the string decays away at the low end and rings long at the top.
pub fn feedback(v: i32) -> f32 {
    0.98 + (f64::from(v) / 128.0) as f32 * 0.019
}

/// The loop filter's brightness: how much of the averaging (the damping) the tone keeps out.
pub fn tone_open(v: i32) -> f32 {
    v as f32 / 128.0
}

/// The pick's brightness: how much of the excitation stays raw noise against its smoothed half.
pub fn pick_noise(v: i32) -> f32 {
    v as f32 / 128.0
}

/// The body's low boost against the string.
pub fn body_level(v: i32) -> f32 {
    (v as f32 / 128.0).powi(2)
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        TONE | SUSTAIN | PICK | BODY => percent(SETTINGS[k].range, v),
        _ => db_text(volume_gain(v)),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    match k {
        TONE | SUSTAIN | PICK | BODY => {
            percent_parse(SETTINGS[k].range, s.trim().to_ascii_lowercase().as_str())
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

struct StringVoice {
    key: u8,
    voice_id: Option<i32>,
    /// The delay line the loop keeps.
    d: Vec<f32>,
    pos: usize,
    lp: f32,
    /// The release fade left, when the key is up.
    rel: f32,
    vel: f32,
    age: f32,
}

struct Bass {
    rate: f32,
    fb: f32,
    tone_k: f32,
    pick_k: f32,
    body_k: f32,
    gain: f32,
    strings: [Option<StringVoice>; 4],
    noise: u32,
    ended: Vec<(u8, Option<i32>)>,
}

impl Bass {
    fn pluck(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) -> StringVoice {
        let len = ((self.rate / key_hz(key)).round() as usize).clamp(2, 8_000);
        let mut d = vec![0.0f32; len];
        // The pluck: noise, brightened or darkened by the pick, over the line's first period.
        let mut smooth = 0.0f32;
        for slot in d.iter_mut() {
            let mut x = self.noise;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.noise = x;
            let raw = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
            smooth += (raw - smooth) * 0.5;
            *slot = raw * self.pick_k + smooth * (1.0 - self.pick_k);
        }
        StringVoice {
            key,
            voice_id: Some(voice_id.unwrap_or(key as i32 + 1)),
            d,
            pos: 0,
            lp: 0.0,
            rel: f32::INFINITY,
            vel: velocity.clamp(0.05, 1.0),
            age: 0.0,
        }
    }
}

impl Instrument for Bass {
    fn set(&mut self, v: &[i32]) {
        self.fb = feedback(v[SUSTAIN]);
        self.tone_k = tone_open(v[TONE]);
        self.pick_k = pick_noise(v[PICK]);
        self.body_k = body_level(v[BODY]);
        self.gain = volume_gain(v[VOLUME]);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate as f32;
    }

    fn reset(&mut self) {
        self.strings = [const { None }; 4];
    }

    fn note_on(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        // A free string first, else the oldest goes (spec §1).
        let slot = match self.strings.iter().position(|s| s.is_none()) {
            Some(i) => i,
            None => {
                // The oldest string goes; ties (all plucked in the same block) by slot order.
                let mut i = 0;
                let mut oldest = self.strings[0].as_ref().map_or(0.0, |s| s.age);
                for (j, s) in self.strings.iter().enumerate() {
                    let age = s.as_ref().map_or(0.0, |s| s.age);
                    if age > oldest {
                        oldest = age;
                        i = j;
                    }
                }
                if let Some(s) = self.strings[i].take() {
                    self.ended.push((s.key, s.voice_id));
                }
                i
            }
        };
        self.strings[slot] = Some(self.pluck(key, velocity, voice_id));
    }

    fn note_off(&mut self, key: u8, voice_id: Option<i32>) {
        for s in self.strings.iter_mut().flatten() {
            if s.key == key && (voice_id.is_none() || s.voice_id == voice_id) && s.rel.is_infinite()
            {
                s.rel = 0.12;
            }
        }
    }

    fn choke(&mut self, key: u8) {
        for i in 0..4 {
            if let Some(s) = self.strings[i].take()
                && s.key == key
            {
                self.ended.push((s.key, s.voice_id));
            }
        }
    }

    fn all_off(&mut self) {
        for s in self.strings.iter_mut() {
            if let Some(s) = s.take() {
                self.ended.push((s.key, s.voice_id));
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let dt = 1.0 / self.rate;
        let (fb, tone_k, body_k, gain) = (self.fb, self.tone_k, self.body_k, self.gain);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut out = 0.0f32;
            for s in self.strings.iter_mut().flatten() {
                let d = &mut s.d;
                let y = d[s.pos];
                let next = d[(s.pos + 1) % d.len()];
                // The loop: the tone averages neighbours (the string's damping), the sustain feeds it back.
                let filtered = y + tone_k * ((y + next) * 0.5 - y);
                d[s.pos] = filtered * fb;
                s.pos = (s.pos + 1) % d.len();
                // The body: a low shelf of the string under it.
                s.lp += (y - s.lp) * 0.04;
                let amp = match s.rel {
                    f if f.is_infinite() => 1.0,
                    mut f => {
                        f -= dt;
                        s.rel = f;
                        (f / 0.12).max(0.0)
                    }
                };
                out += (y + s.lp * body_k * 3.0) * amp * s.vel;
                s.age += dt;
            }
            let out = out * gain;
            *l += out;
            *r += out;
        }
        // The strings that faded: the loop's energy gone, or the release over.
        for i in 0..4 {
            let gone = self.strings[i].as_ref().is_some_and(|s| {
                (s.rel <= 0.0) || s.age > 20.0 || s.d.iter().all(|x| x.abs() < 1e-4)
            });
            if gone && let Some(s) = self.strings[i].take() {
                self.ended.push((s.key, s.voice_id));
            }
        }
    }

    fn take_ended(&mut self) -> Vec<(u8, Option<i32>)> {
        std::mem::take(&mut self.ended)
    }

    fn has_voices(&self) -> bool {
        self.strings.iter().any(|s| s.is_some())
    }
}

fn make(rate: u32, v: &[i32]) -> Box<dyn Instrument> {
    let mut b = Bass {
        rate: rate as f32,
        fb: feedback(v[SUSTAIN]),
        tone_k: tone_open(v[TONE]),
        pick_k: pick_noise(v[PICK]),
        body_k: body_level(v[BODY]),
        gain: volume_gain(v[VOLUME]),
        strings: [const { None }; 4],
        noise: 0xbeef_cafe,
        ended: Vec::new(),
    };
    b.set_sample_rate(rate);
    Box::new(b)
}

pub struct BassKind;

impl Kind for BassKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.lemon-bass";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's bass guitar";
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
    const VST3_CLASS_ID: [u8; 16] = *b"LemonStudioBooBa";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type LemonBass = SmallGen<BassKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(TONE, 64), "50%");
        assert_eq!(parse(TONE, "50%"), Some(64));
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
        assert_eq!(feedback(0), 0.98);
        assert!((feedback(128) - 0.999).abs() < 1e-6);
    }

    /// Four strings sound at once; a fifth takes the oldest's place.
    #[test]
    fn four_strings_a_fifth_steals_the_oldest() {
        let mut b = make(44100, &SPEC.start());
        for (i, key) in [36u8, 40, 43, 48].into_iter().enumerate() {
            b.note_on(key, 1.0, Some(i as i32 + 1));
        }
        assert!(b.has_voices());
        b.note_on(52, 1.0, Some(5));
        assert_eq!(
            b.take_ended(),
            vec![(36, Some(1))],
            "the oldest string ended"
        );
    }

    /// A key up fades the string out; a choke ends it at once.
    #[test]
    fn the_release_ends_the_string() {
        let mut b = make(44100, &SPEC.start());
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        b.note_on(36, 1.0, Some(1));
        b.note_off(36, Some(1));
        let mut blocks = 0;
        while b.has_voices() && blocks < 1000 {
            l.iter_mut().for_each(|x| *x = 0.0);
            b.render(&mut l, &mut r);
            blocks += 1;
        }
        assert!(!b.has_voices(), "the release ends the string");
        assert_eq!(b.take_ended(), vec![(36, Some(1))]);
        l.iter_mut().for_each(|x| *x = 0.0);
        b.render(&mut l, &mut r);
        assert!(
            l.iter().all(|x| x.abs() < 1e-3),
            "silence after the release"
        );
    }
}
