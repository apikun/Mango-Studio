// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Slayer: Mango Studio's electric guitar (a recreation of FL's Slayer) as a plugin. Six Karplus-Strong
//! strings a pick plucks where it sits; a pickup reads each string back along it, and the amp drives them, shapes
//! them and boxes them. The pick, tone, sustain, pickup, tilt, body, drive, bass, treble, cab, chorus and volume on
//! the knobs. A seventh steals the oldest string.
//!
// spec: plugins/lemon-small-generators.md, plugins/slayer.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::fruit_kick::volume_gain;
use crate::params::{db_text, leading_number, nearest, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

/// The settings in stored order (spec §5).
pub const SETTINGS: [Setting; 12] = [
    Setting {
        id: "pick",
        name: "Pick",
        range: (0, 128),
        start: 38,
        magnet: None,
    },
    Setting {
        id: "tone",
        name: "Tone",
        range: (0, 128),
        start: 96,
        magnet: None,
    },
    Setting {
        id: "sustain",
        name: "Sustain",
        range: (0, 128),
        start: 72,
        magnet: None,
    },
    Setting {
        id: "pickup",
        name: "Pickup",
        range: (0, 128),
        start: 56,
        magnet: None,
    },
    Setting {
        id: "tilt",
        name: "Tilt",
        range: (0, 128),
        start: 64,
        magnet: None,
    },
    Setting {
        id: "body",
        name: "Body",
        range: (0, 128),
        start: 48,
        magnet: None,
    },
    Setting {
        id: "drive",
        name: "Drive",
        range: (0, 128),
        start: 24,
        magnet: None,
    },
    Setting {
        id: "bass",
        name: "Bass",
        range: (0, 128),
        start: 64,
        magnet: None,
    },
    Setting {
        id: "treble",
        name: "Treble",
        range: (0, 128),
        start: 64,
        magnet: None,
    },
    Setting {
        id: "cab",
        name: "Cab",
        range: (0, 128),
        start: 72,
        magnet: None,
    },
    Setting {
        id: "chorus",
        name: "Chorus",
        range: (0, 128),
        start: 0,
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

pub const PICK: usize = 0;
pub const TONE: usize = 1;
pub const SUSTAIN: usize = 2;
pub const PICKUP: usize = 3;
pub const TILT: usize = 4;
pub const BODY: usize = 5;
pub const DRIVE: usize = 6;
pub const BASS: usize = 7;
pub const TREBLE: usize = 8;
pub const CAB: usize = 9;
pub const CHORUS: usize = 10;
pub const VOLUME: usize = 11;

/// Editor layout (spec §2): four cards of knobs.
const CARDS: [Card; 4] = [
    Card {
        x0: 8.0,
        x1: 130.0,
        y0: 28.0,
        y1: 124.0,
        heading: "STRING",
    },
    Card {
        x0: 136.0,
        x1: 258.0,
        y0: 28.0,
        y1: 124.0,
        heading: "PICKUP",
    },
    Card {
        x0: 264.0,
        x1: 386.0,
        y0: 28.0,
        y1: 124.0,
        heading: "AMP",
    },
    Card {
        x0: 392.0,
        x1: 514.0,
        y0: 28.0,
        y1: 124.0,
        heading: "OUTPUT",
    },
];

const CONTROLS: [Control; 12] = [
    Control::Knob { id: "pick", cx: 35.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
    Control::Knob { id: "tone", cx: 69.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
    Control::Knob { id: "sustain", cx: 103.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
    Control::Knob { id: "pickup", cx: 163.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
    Control::Knob { id: "tilt", cx: 197.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
    Control::Knob { id: "body", cx: 231.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
    Control::Knob { id: "drive", cx: 291.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
    Control::Knob { id: "bass", cx: 325.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
    Control::Knob { id: "treble", cx: 359.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
    Control::Knob { id: "cab", cx: 419.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
    Control::Knob { id: "chorus", cx: 453.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
    Control::Knob { id: "volume", cx: 487.0, cy: 74.0, r: 15.0, caption: "", bipolar: false },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Slayer",
    word: "SLAYER",
    about: "Mango Studio's electric guitar",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (522, 162),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark {
        x: 506.0,
        y: 143.0,
        anchor: 1.0,
        stacked: false,
    },
    hint: Some(Rect::new(8.0, 132.0, 490.0, 22.0)),
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

/// Where the pick sits, as a share of the string's period: the stroke's comb loses the harmonic that stands
/// there (a quarter of the way up at the top of the knob, the string's middle under it).
pub fn pick_frac(v: i32) -> f32 {
    v as f32 / 128.0 * 0.25
}

/// Where the pickup sits, as a share of the string's period: the read's comb loses the harmonic that stands
/// there (a quarter of the way up at the top of the knob, the bridge under it).
pub fn pickup_frac(v: i32) -> f32 {
    v as f32 / 128.0 * 0.25
}

/// The pickup's tilt: the cutoff of the one-pole the read goes through, bright at the top, dark at the bottom.
pub fn tilt_hz(v: i32) -> f32 {
    12_000.0f32 * (1_400.0f32 / 12_000.0).powf(v as f32 / 128.0)
}

/// The body's low boost against the strings.
pub fn body_level(v: i32) -> f32 {
    (v as f32 / 128.0).powi(2)
}

/// The amp's pre-gain: how hot the stage runs before its curve.
pub fn drive_pre(v: i32) -> f32 {
    1.0 + 15.0 * (v as f32 / 128.0).powi(2)
}

/// A shelf's share: how far the knob pushes the band up (over the middle) or pulls it down.
pub fn shelf_mix(v: i32) -> f32 {
    (v as f32 / 128.0 - 0.5) * 1.4
}

/// The cabinet's cutoff: the speaker's top, wide open at the bottom of the knob.
pub fn cab_hz(v: i32) -> f32 {
    4_200.0f32 * (700.0f32 / 4_200.0).powf(v as f32 / 128.0)
}

/// The chorus's share of the doubled voice.
pub fn chorus_mix(v: i32) -> f32 {
    v as f32 / 128.0
}

/// A one-pole coefficient for a cutoff at `hz` on `rate`.
fn pole(rate: f32, hz: f32) -> f32 {
    1.0 - (-std::f32::consts::TAU * hz / rate).exp()
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        VOLUME => db_text(volume_gain(v)),
        _ => percent(SETTINGS[k].range, v),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    match k {
        VOLUME => parse_db(
            SETTINGS[VOLUME].range,
            s.trim().to_ascii_lowercase().as_str(),
            volume_gain,
        ),
        _ => percent_parse(SETTINGS[k].range, s.trim().to_ascii_lowercase().as_str()),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The sound

/// The frequency of a key.
fn key_hz(key: u8) -> f32 {
    (440.0f64 * 2f64.powf((f64::from(key) - 69.0) / 12.0)) as f32
}

/// How many strings sound at once (a guitar's six).
const STRINGS: usize = 6;

/// The longest a chorus's doubled voice trails (the buffer keeps a power of two of it).
const WET_MS: usize = 60;

struct StringVoice {
    key: u8,
    voice_id: Option<i32>,
    /// The delay line the loop keeps.
    d: Vec<f32>,
    pos: usize,
    /// The pickup's tilt one-pole.
    lp: f32,
    /// The body's low one-pole.
    blp: f32,
    /// The release fade left, when the key is up.
    rel: f32,
    vel: f32,
    age: f32,
}

/// A chorus read: `buf`'s sample `back` samples behind `at`, the fraction between the two smoothed.
fn chorus_read(buf: &[f32], at: usize, n: usize, back: f32) -> f32 {
    let t = (at as f32 + n as f32 - back) % n as f32;
    let i = t as usize;
    let fr = t - i as f32;
    buf[i] + (buf[(i + 1) % n] - buf[i]) * fr
}

struct Slayer {
    rate: f32,
    // The string (rate-free laws, from the knobs).
    fb: f32,
    tone_k: f32,
    pick_f: f32,
    pickup_f: f32,
    body_k: f32,
    drive_d: f32,
    bass_m: f32,
    treble_m: f32,
    chorus_c: f32,
    gain: f32,
    // The rate's coefficients (the cutoffs the knobs keep, and the stack's and box's fixed ones).
    tilt_k: f32,
    cab_k: f32,
    bass_k: f32,
    treble_k: f32,
    hp_k: f32,
    // The amp's and the box's state.
    amp_low: f32,
    amp_high: f32,
    box_low: f32,
    box_sub: f32,
    // The chorus's two doubled voices.
    wet_l: Vec<f32>,
    wet_r: Vec<f32>,
    wet_at: usize,
    lfo: f32,
    strings: [Option<StringVoice>; STRINGS],
    noise: u32,
    ended: Vec<(u8, Option<i32>)>,
}

impl Slayer {
    /// The pick's stroke over the line's first period: noise combed where the pick sits.
    fn pluck(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) -> StringVoice {
        let len = ((self.rate / key_hz(key)).round() as usize).clamp(2, 8_000);
        let mut raw = Vec::with_capacity(len + len);
        for _ in 0..len + len {
            let mut x = self.noise;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.noise = x;
            raw.push((x as f32 / u32::MAX as f32) * 2.0 - 1.0);
        }
        let mut d = Vec::with_capacity(len);
        let back = (self.pick_f * len as f32).round() as usize;
        for (i, slot) in raw.iter().enumerate().take(len) {
            d.push(slot - raw[i + back]);
        }
        StringVoice {
            key,
            voice_id: Some(voice_id.unwrap_or(key as i32 + 1)),
            d,
            pos: 0,
            lp: 0.0,
            blp: 0.0,
            rel: f32::INFINITY,
            vel: velocity.clamp(0.05, 1.0),
            age: 0.0,
        }
    }
}

impl Instrument for Slayer {
    fn set(&mut self, v: &[i32]) {
        self.fb = feedback(v[SUSTAIN]);
        self.tone_k = tone_open(v[TONE]);
        self.pick_f = pick_frac(v[PICK]);
        self.pickup_f = pickup_frac(v[PICKUP]);
        self.body_k = body_level(v[BODY]);
        self.drive_d = v[DRIVE] as f32 / 128.0;
        self.bass_m = shelf_mix(v[BASS]);
        self.treble_m = shelf_mix(v[TREBLE]);
        self.chorus_c = chorus_mix(v[CHORUS]);
        self.gain = volume_gain(v[VOLUME]);
        // The pickup's tilt and the cabinet's top move with the knobs.
        self.tilt_k = pole(self.rate, tilt_hz(v[TILT]));
        self.cab_k = pole(self.rate, cab_hz(v[CAB]));
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate as f32;
        // The amp's shelves and the box's sub cut keep their cutoffs whatever the knobs do.
        self.bass_k = pole(self.rate, 90.0);
        self.treble_k = pole(self.rate, 2_200.0);
        self.hp_k = pole(self.rate, 75.0);
        let n = (self.rate as usize * WET_MS / 1_000).next_power_of_two();
        self.wet_l = vec![0.0; n];
        self.wet_r = vec![0.0; n];
        self.wet_at = 0;
    }

    fn reset(&mut self) {
        self.strings = [const { None }; STRINGS];
        self.amp_low = 0.0;
        self.amp_high = 0.0;
        self.box_low = 0.0;
        self.box_sub = 0.0;
        self.wet_l.fill(0.0);
        self.wet_r.fill(0.0);
        self.lfo = 0.0;
    }

    fn note_on(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        // A free string first, else the oldest goes (spec §1).
        let slot = match self.strings.iter().position(|s| s.is_none()) {
            Some(i) => i,
            None => {
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
                s.rel = 0.35;
            }
        }
    }

    fn choke(&mut self, key: u8) {
        for i in 0..STRINGS {
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
        // The laws and the rates' coefficients, read once (the strings take the loop's borrows).
        let (fb, tone_k, pickup_f, tilt_k, body_k) =
            (self.fb, self.tone_k, self.pickup_f, self.tilt_k, self.body_k);
        let (drive_d, bass_m, treble_m, gain) =
            (self.drive_d, self.bass_m, self.treble_m, self.gain);
        let (bass_k, treble_k, cab_k, hp_k) =
            (self.bass_k, self.treble_k, self.cab_k, self.hp_k);
        let chorus_c = if self.wet_l.is_empty() { 0.0 } else { self.chorus_c };
        let wet_n = self.wet_l.len();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut out = 0.0f32;
            let mut body = 0.0f32;
            for s in self.strings.iter_mut().flatten() {
                let d = &mut s.d;
                let len = d.len();
                let y = d[s.pos];
                let next = d[(s.pos + 1) % len];
                // The loop: the tone averages neighbours (the string's damping), the sustain feeds it back.
                let filtered = y + tone_k * ((y + next) * 0.5 - y);
                d[s.pos] = filtered * fb;
                s.pos = (s.pos + 1) % len;
                // The pickup reads the line back along the string (its comb loses the harmonic that stands
                // there), the tilt's one-pole under it.
                let o = pickup_f * len as f32 * 0.5;
                let i = (s.pos + o as usize) % len;
                let fr = o - o.floor();
                let tap = d[i] + (d[(i + 1) % len] - d[i]) * fr;
                s.lp += (y + (tap - y) - s.lp) * tilt_k;
                // The body's low one-pole under the raw string.
                s.blp += (y - s.blp) * 0.04;
                body += s.blp;
                let amp = match s.rel {
                    f if f.is_infinite() => 1.0,
                    mut f => {
                        f -= dt;
                        s.rel = f;
                        (f / 0.35).max(0.0)
                    }
                };
                out += s.lp * amp * s.vel;
                s.age += dt;
            }
            // The body: a low shelf of the strings under them.
            out += body * body_k * 3.0;
            // The amp: the stage's curve over the hot signal, the dry share beside it.
            let pre = 1.0 + 15.0 * drive_d * drive_d;
            let hot = (pre * out).tanh() / pre.sqrt();
            let amp_in = out * (1.0 - drive_d) + hot * drive_d;
            // The tone stack: the low shelf and the high shelf around the stack's middle.
            self.amp_low += (amp_in - self.amp_low) * bass_k;
            let low = amp_in + bass_m * (self.amp_low - amp_in);
            self.amp_high += (low - self.amp_high) * treble_k;
            let stacked = low + treble_m * (low - self.amp_high) * 0.5;
            // The box: the speaker's top under it, the room's bottom over it.
            self.box_low += (stacked - self.box_low) * cab_k;
            self.box_sub += (stacked - self.box_sub) * hp_k;
            let boxed = self.box_low - self.box_sub;
            // The chorus: a doubled voice a breath behind each side.
            let (mut gl, mut gr) = (boxed, boxed);
            if chorus_c > 0.0 {
                let depth = self.rate * 0.003;
                let base = self.rate * 0.018;
                let phase = self.lfo * std::f32::consts::TAU;
                let dl = base + depth * phase.sin();
                let dr = base + depth * (phase + std::f32::consts::PI).sin();
                self.wet_l[self.wet_at] = boxed;
                self.wet_r[self.wet_at] = boxed;
                let (wl, wr) = (
                    chorus_read(&self.wet_l, self.wet_at, wet_n, dl),
                    chorus_read(&self.wet_r, self.wet_at, wet_n, dr),
                );
                gl = boxed * (1.0 - chorus_c * 0.5) + wl * chorus_c * 0.5;
                gr = boxed * (1.0 - chorus_c * 0.5) + wr * chorus_c * 0.5;
            }
            *l += gl * gain;
            *r += gr * gain;
            // The sweep walks on, the doubled voices' tape rolls.
            self.lfo += 0.7 * dt;
            if self.lfo >= 1.0 {
                self.lfo -= 1.0;
            }
            if wet_n > 0 {
                self.wet_at = (self.wet_at + 1) % wet_n;
            }
        }
        // The strings that faded: the loop's energy gone, or the release over.
        for i in 0..STRINGS {
            let gone = self.strings[i].as_ref().is_some_and(|s| {
                (s.rel <= 0.0) || s.age > 30.0 || s.d.iter().all(|x| x.abs() < 1e-4)
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
    let mut g = Slayer {
        rate: rate as f32,
        fb: 0.0,
        tone_k: 0.0,
        pick_f: 0.0,
        pickup_f: 0.0,
        body_k: 0.0,
        drive_d: 0.0,
        bass_m: 0.0,
        treble_m: 0.0,
        chorus_c: 0.0,
        gain: 0.0,
        tilt_k: 0.0,
        cab_k: 0.0,
        bass_k: 0.0,
        treble_k: 0.0,
        hp_k: 0.0,
        amp_low: 0.0,
        amp_high: 0.0,
        box_low: 0.0,
        box_sub: 0.0,
        wet_l: Vec::new(),
        wet_r: Vec::new(),
        wet_at: 0,
        lfo: 0.0,
        strings: [const { None }; STRINGS],
        noise: 0xbeef_cafe,
        ended: Vec::new(),
    };
    g.set_sample_rate(rate);
    g.set(v);
    Box::new(g)
}

pub struct SlayerKind;

impl Kind for SlayerKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-slayer";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's electric guitar";
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioSlay1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type MangoSlayer = SmallGen<SlayerKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(PICK, 64), "50%");
        assert_eq!(parse(PICK, "50%"), Some(64));
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
        assert_eq!(feedback(0), 0.98);
        assert!((feedback(128) - 0.999).abs() < 1e-6);
        assert_eq!(pick_frac(0), 0.0);
        assert!((pick_frac(128) - 0.25).abs() < 1e-6);
        assert!((tilt_hz(0) - 12_000.0).abs() < 1e-3);
        assert!((tilt_hz(128) - 1_400.0).abs() < 1e-3);
        assert!((cab_hz(0) - 4_200.0).abs() < 1e-3);
        assert!((cab_hz(128) - 700.0).abs() < 1e-3);
        assert_eq!(shelf_mix(64), 0.0);
        assert!((drive_pre(128) - 16.0).abs() < 1e-6);
    }

    /// Six strings sound at once; a seventh takes the oldest's place.
    #[test]
    fn six_strings_a_seventh_steals_the_oldest() {
        let mut g = make(44100, &SPEC.start());
        for (i, key) in [36u8, 40, 43, 48, 52, 55].into_iter().enumerate() {
            g.note_on(key, 1.0, Some(i as i32 + 1));
        }
        assert!(g.has_voices());
        g.note_on(59, 1.0, Some(7));
        assert_eq!(
            g.take_ended(),
            vec![(36, Some(1))],
            "the oldest string ended"
        );
    }

    /// A key up fades the string out; a choke ends it at once.
    #[test]
    fn the_release_ends_the_string() {
        let mut g = make(44100, &SPEC.start());
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        g.note_on(36, 1.0, Some(1));
        g.note_off(36, Some(1));
        let mut blocks = 0;
        while g.has_voices() && blocks < 2000 {
            l.iter_mut().for_each(|x| *x = 0.0);
            g.render(&mut l, &mut r);
            blocks += 1;
        }
        assert!(!g.has_voices(), "the release ends the string");
        assert_eq!(g.take_ended(), vec![(36, Some(1))]);
        // The string is gone; the amp's low shelf keeps a tail that fades in a few ms.
        let mut tail = 0;
        loop {
            l.iter_mut().for_each(|x| *x = 0.0);
            g.render(&mut l, &mut r);
            if l.iter().all(|x| x.abs() < 1e-3) {
                break;
            }
            tail += 1;
            assert!(tail < 64, "the tail fades in a few ms");
        }
    }

    /// The amp's curve keeps a hot string under the ceiling; the cabinet's top darkens it.
    #[test]
    fn the_amp_compresses_and_the_cab_darkens() {
        let peak = |drive: i32| {
            let mut g = make(44100, &SPEC.start());
            let mut v = SPEC.start();
            v[DRIVE] = drive;
            g.set(&v);
            g.note_on(48, 1.0, Some(1));
            let mut l = vec![0.0f32; 8192];
            let mut r = vec![0.0f32; 8192];
            g.render(&mut l, &mut r);
            l.iter().fold(0.0f32, |m, x| m.max(x.abs()))
        };
        let clean = peak(0);
        let driven = peak(128);
        assert!(clean > 0.3, "the clean string is loud");
        assert!(driven < clean, "the amp compresses the peak");
        assert!(driven > 0.05, "the driven string is not silent");
        // The cab: the note's crossings fall when the speaker's top comes down.
        let crossings = |cab: i32| {
            let mut g = make(44100, &SPEC.start());
            let mut v = SPEC.start();
            v[CAB] = cab;
            g.set(&v);
            g.note_on(60, 1.0, Some(1));
            let mut l = vec![0.0f32; 8192];
            let mut r = vec![0.0f32; 8192];
            g.render(&mut l, &mut r);
            l.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
        };
        assert!(
            crossings(128) < crossings(0),
            "the closed cab darkens the tone"
        );
    }
}
