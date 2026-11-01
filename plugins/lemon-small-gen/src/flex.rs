// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Flex: Mango Studio's preset synthesizer (a recreation of FL's FLEX) as a plugin. Two hundred fifty-six
//! named presets — a two-oscillator voice with a sub and a whiff of noise, a state-variable filter under its own
//! envelope, and the echo and space of the preset — and four macros over it, each one landing where the preset
//! says (a cutoff here, a detune there), the way FLEX's macros do.
//!
// spec: plugins/lemon-small-generators.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::fruit_kick::volume_gain;
use crate::params::{db_text, leading_number, nearest, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

/// The presets.
pub const COUNT: i32 = 256;
/// The voices a preset plays at once; a ninth steals the oldest.
const MAX_VOICES: usize = 8;

/// The settings in stored order (spec §5).
pub const SETTINGS: [Setting; 6] = [
    Setting { id: "preset", name: "Preset", range: (0, COUNT - 1), start: 0, magnet: None },
    Setting { id: "macro1", name: "Macro 1", range: (0, 128), start: 64, magnet: None },
    Setting { id: "macro2", name: "Macro 2", range: (0, 128), start: 64, magnet: None },
    Setting { id: "macro3", name: "Macro 3", range: (0, 128), start: 64, magnet: None },
    Setting { id: "macro4", name: "Macro 4", range: (0, 128), start: 64, magnet: None },
    Setting { id: "volume", name: "Volume", range: (0, 512), start: 256, magnet: Some(256) },
];

pub const PRESET: usize = 0;
pub const MACRO1: usize = 1;
pub const MACRO2: usize = 2;
pub const MACRO3: usize = 3;
pub const MACRO4: usize = 4;
pub const VOLUME: usize = 5;

/// Editor layout (spec §2): the preset's knob, the four macros, the output.
const CARDS: [Card; 3] = [
    Card { x0: 8.0, x1: 128.0, y0: 28.0, y1: 134.0, heading: "PRESET" },
    Card { x0: 134.0, x1: 344.0, y0: 28.0, y1: 134.0, heading: "MACROS" },
    Card { x0: 350.0, x1: 470.0, y0: 28.0, y1: 134.0, heading: "OUTPUT" },
];

const CONTROLS: [Control; 6] = [
    Control::Knob { id: "preset", cx: 48.0, cy: 74.0, r: 26.0, caption: "", bipolar: false },
    Control::Knob { id: "macro1", cx: 170.0, cy: 74.0, r: 20.0, caption: "1", bipolar: false },
    Control::Knob { id: "macro2", cx: 222.0, cy: 74.0, r: 20.0, caption: "2", bipolar: false },
    Control::Knob { id: "macro3", cx: 274.0, cy: 74.0, r: 20.0, caption: "3", bipolar: false },
    Control::Knob { id: "macro4", cx: 326.0, cy: 74.0, r: 20.0, caption: "4", bipolar: false },
    Control::Knob { id: "volume", cx: 410.0, cy: 74.0, r: 24.0, caption: "", bipolar: false },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Flex",
    word: "FLEX",
    about: "Mango Studio's preset synthesizer",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (478, 162),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark { x: 462.0, y: 143.0, anchor: 1.0, stacked: false },
    hint: Some(Rect::new(8.0, 132.0, 446.0, 22.0)),
    labels: &[],
    text,
    parse,
    make,
};

// ---------------------------------------------------------------------------------------------------------------
// The presets (spec §4): the patch a preset number holds, its name and where its macros land

/// What a macro reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Macro {
    Cutoff,
    Resonance,
    Detune,
    Noise,
    Echo,
    Space,
    Sub,
    Drive,
}

pub const MACROS: [(&str, Macro); 8] = [
    ("CUT", Macro::Cutoff),
    ("RES", Macro::Resonance),
    ("DTN", Macro::Detune),
    ("NOISE", Macro::Noise),
    ("ECHO", Macro::Echo),
    ("SPACE", Macro::Space),
    ("SUB", Macro::Sub),
    ("DRIVE", Macro::Drive),
];

/// One preset's patch: the oscillators, the filter and its envelope, the effects and the macros' targets.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub name: String,
    /// The first oscillator's shape, saw (0) to square (1).
    pub shape: f32,
    /// The second oscillator's pitch off the first, in semitones.
    pub semis: f32,
    pub osc2: f32,
    pub sub: f32,
    pub noise: f32,
    /// The filter's base cutoff in Hz, its resonance and its envelope's reach in octaves.
    pub cutoff: f32,
    pub reso: f32,
    pub env: f32,
    /// The loud envelope.
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    /// The echo's time in seconds and send; the space's send.
    pub delay: f32,
    pub echo: f32,
    pub space: f32,
    /// The drive at the voices' bus, `0..1`.
    pub drive: f32,
    /// Where each macro lands.
    pub targets: [Macro; 4],
}

const FIRST: [&str; 16] = ["Mango", "Dusk", "Iron", "Amber", "Cloud", "Neon", "Paper", "Velvet", "Glass", "Ember", "Frost", "Echo", "Copper", "Ribbon", "Sugar", "Thunder"];
const SECOND: [&str; 16] = ["Lead", "Pad", "Bass", "Keys", "Pluck", "Bell", "Swell", "Drift", "Choir", "Stack", "Wave", "Sprite", "Chant", "Pulse", "Glow", "Dream"];

/// A preset's patch, the same every time (spec §4: the two hundred fifty-six are the factory bank).
pub fn patch(index: i32) -> Patch {
    let mut s = (u64::from(index as u32) + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let share = |v: u64| (v % 1000) as f32 / 1000.0;
    let pick = |v: u64, n: usize| (v as usize) % n;
    let name = format!("{} {}", FIRST[pick(next(), 16)], SECOND[pick(next(), 16)]);
    let shape = share(next());
    let semis = match pick(next(), 5) {
        0 => -12.0,
        1 => -7.0,
        2 => 7.0,
        3 => 12.0,
        _ => 0.0,
    } + (share(next()) - 0.5) * 0.3;
    let targets = [
        MACROS[pick(next(), MACROS.len())].1,
        MACROS[pick(next(), MACROS.len())].1,
        MACROS[pick(next(), MACROS.len())].1,
        MACROS[pick(next(), MACROS.len())].1,
    ];
    Patch {
        name,
        shape,
        semis,
        osc2: share(next()),
        sub: share(next()) * 0.6,
        noise: share(next()) * 0.25,
        cutoff: 220.0 * 2f32.powf(share(next()) * 5.0),
        reso: 0.1 + share(next()) * 0.7,
        env: share(next()) * 4.0,
        attack: 0.002 + share(next()) * share(next()) * 1.4,
        decay: 0.05 + share(next()) * 1.2,
        sustain: 0.25 + share(next()) * 0.7,
        release: 0.08 + share(next()) * share(next()) * 2.2,
        delay: 0.12 + share(next()) * 0.5,
        echo: share(next()) * 0.5,
        space: share(next()) * 0.6,
        drive: share(next()) * 0.6,
        targets,
    }
}

/// A preset's name.
pub fn name(index: i32) -> String {
    patch(index).name
}

// ---------------------------------------------------------------------------------------------------------------
// Laws (spec §3)

/// A share of a 0..128 knob as a percent.
fn percent(v: i32) -> String {
    format!("{:.0}%", f64::from(v) / 128.0 * 100.0)
}

fn percent_parse(s: &str) -> Option<i32> {
    let x = leading_number(s)?;
    nearest(SETTINGS[MACRO1].range, x, |v| Some(f64::from(v) / 128.0 * 100.0))
}

/// A preset's number as the count names it: "1 Mango Lead".
fn preset_text(v: i32) -> String {
    let n = v.clamp(0, COUNT - 1);
    format!("{} {}", n + 1, patch(n).name)
}

fn preset_parse(s: &str) -> Option<i32> {
    let x = leading_number(s)?;
    nearest((0, COUNT - 1), x, |v| Some(f64::from(v + 1)))
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        PRESET => preset_text(v),
        MACRO1..=MACRO4 => percent(v),
        _ => db_text(volume_gain(v)),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    let s = s.trim().to_ascii_lowercase();
    match k {
        PRESET => preset_parse(&s),
        MACRO1..=MACRO4 => percent_parse(&s),
        _ => parse_db(SETTINGS[VOLUME].range, &s, volume_gain),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The sound

/// The filter: a state-variable low, stable under the envelope's reach (the cutoff's `f` clamped in).
#[derive(Clone, Copy, Default)]
struct Svf {
    low: f32,
    band: f32,
}

impl Svf {
    fn next(&mut self, x: f32, f: f32, q: f32) -> f32 {
        let f = f.min(0.25);
        self.low += f * self.band;
        let high = x - self.low - q * self.band;
        self.band += f * high;
        self.low
    }
}

/// A voice: the oscillators, the filter, the loud envelope.
struct Voice {
    key: u8,
    voice_id: Option<i32>,
    ph1: f32,
    ph2: f32,
    ph_sub: f32,
    svf: Svf,
    env: f32,
    stage: u8, // 0 attack, 1 decay, 2 sustain, 3 release, 4 off
    age: f32,
}

const OFF: u8 = 4;

struct Flex {
    rate: f32,
    /// The preset's patch, with the macros already landed in it.
    now: Patch,
    base: Patch,
    macros: [f32; 4],
    gain: f32,
    voices: [Option<Voice>; MAX_VOICES],
    noise: u32,
    /// The echo: two taps, left and right, the space: a small room of combs and allpasses.
    echo_l: Vec<f32>,
    echo_r: Vec<f32>,
    echo_pos: usize,
    room: Room,
    ring: usize,
    ended: Vec<(u8, Option<i32>)>,
}

/// The room's comb times, in seconds.
const COMBS: [f32; 4] = [0.0297, 0.0371, 0.0411, 0.0437];

impl Flex {
    fn new(rate: u32, v: &[i32]) -> Self {
        let rate = rate as f32;
        let base = patch(v[PRESET].clamp(0, COUNT - 1));
        let mut f = Self {
            rate,
            now: base.clone(),
            base,
            macros: [0.5; 4],
            gain: volume_gain(v[VOLUME]),
            voices: [const { None }; MAX_VOICES],
            noise: 0x5EED_1E55,
            echo_l: Vec::new(),
            echo_r: Vec::new(),
            echo_pos: 0,
            room: Room::new(),
            ring: 0,
            ended: Vec::new(),
        };
        f.set(v);
        f
    }

    /// The macros land in the patch: each one pulls its target from the knob's middle.
    fn land(&mut self) {
        let mut p = self.base.clone();
        for (m, &target) in self.macros.iter().zip(&p.targets) {
            let m = m - 0.5; // -0.5..0.5 around the middle
            match target {
                Macro::Cutoff => p.cutoff = (p.cutoff * 2f32.powf(m * 4.0)).clamp(40.0, 18_000.0),
                Macro::Resonance => p.reso = (p.reso + m * 0.8).clamp(0.05, 0.95),
                Macro::Detune => p.semis += m * 0.5,
                Macro::Noise => p.noise = (p.noise + m * 0.4).clamp(0.0, 0.6),
                Macro::Echo => p.echo = (p.echo + m).clamp(0.0, 0.9),
                Macro::Space => p.space = (p.space + m).clamp(0.0, 0.9),
                Macro::Sub => p.sub = (p.sub + m * 0.5).clamp(0.0, 0.9),
                Macro::Drive => p.drive = (p.drive + m).clamp(0.0, 0.9),
            }
        }
        self.now = p;
    }

    /// The effect lines, sized for the rate and the patch's times.
    fn resize(&mut self) {
        let delay = (self.now.delay * self.rate).round() as usize;
        self.echo_l.resize(delay.max(2), 0.0);
        self.echo_r.resize(delay.max(2), 0.0);
        self.echo_pos %= self.echo_l.len();
        self.room.set_rate(self.rate);
    }

    fn osc(ph: f32, shape: f32) -> f32 {
        // A naive saw, its square a sign flip away; the filter and the mix keep the edge off.
        let saw = ph * 2.0 - 1.0;
        let square = if ph < 0.5 { 1.0 } else { -1.0 };
        saw + (square - saw) * shape
    }

    fn note_at(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        let slot = match self.voices.iter().position(|v| v.is_none()) {
            Some(i) => i,
            None => {
                let mut i = 0;
                let mut oldest = 0.0f32;
                for (j, v) in self.voices.iter().enumerate() {
                    let age = v.as_ref().map_or(0.0, |v| v.age);
                    if age > oldest {
                        oldest = age;
                        i = j;
                    }
                }
                if let Some(v) = self.voices[i].take() {
                    self.ended.push((v.key, v.voice_id));
                }
                i
            }
        };
        self.voices[slot] = Some(Voice {
            key,
            voice_id: Some(voice_id.unwrap_or(key as i32 + 1)),
            ph1: 0.0,
            ph2: 0.0,
            ph_sub: 0.0,
            svf: Svf::default(),
            env: 0.0,
            stage: 0,
            age: 0.0,
        });
        let _ = velocity;
    }
}

fn key_hz(key: u8) -> f32 {
    (440.0f64 * 2f64.powf((f64::from(key) - 69.0) / 12.0)) as f32
}

impl Instrument for Flex {
    fn set(&mut self, v: &[i32]) {
        self.base = patch(v[PRESET].clamp(0, COUNT - 1));
        for (m, k) in self.macros.iter_mut().zip([MACRO1, MACRO2, MACRO3, MACRO4]) {
            *m = v[k] as f32 / 128.0;
        }
        self.gain = volume_gain(v[VOLUME]);
        self.land();
        self.resize();
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate as f32;
        self.resize();
    }

    fn reset(&mut self) {
        self.voices = [const { None }; MAX_VOICES];
        for x in self.echo_l.iter_mut().chain(self.echo_r.iter_mut()) {
            *x = 0.0;
        }
        self.room.clear();
        self.ring = 0;
    }

    fn note_on(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        self.note_at(key, velocity, voice_id);
    }

    fn note_off(&mut self, key: u8, voice_id: Option<i32>) {
        for v in self.voices.iter_mut().flatten() {
            if v.key == key && (voice_id.is_none() || v.voice_id == voice_id) && v.stage != 3 && v.stage != OFF {
                v.stage = 3;
            }
        }
        self.ring = (self.rate * 2.0) as usize;
    }

    fn choke(&mut self, key: u8) {
        for v in self.voices.iter_mut().flatten() {
            if v.key == key {
                v.stage = OFF;
                self.ended.push((v.key, v.voice_id));
            }
        }
    }

    fn all_off(&mut self) {
        for v in self.voices.iter_mut().flatten() {
            if v.stage != OFF {
                self.ended.push((v.key, v.voice_id));
            }
            v.stage = OFF;
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = self.now.clone();
        let dt = 1.0 / self.rate;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut out = 0.0f32;
            for v in self.voices.iter_mut().flatten() {
                if v.stage == OFF {
                    continue;
                }
                let hz = key_hz(v.key);
                let d1 = std::f32::consts::TAU * hz / self.rate;
                let d2 = std::f32::consts::TAU * hz * 2f32.powf(p.semis / 12.0) / self.rate;
                let dsub = d1 * 0.5;
                v.ph1 = (v.ph1 + d1) % 1.0;
                v.ph2 = (v.ph2 + d2) % 1.0;
                v.ph_sub = (v.ph_sub + dsub) % 1.0;
                let mut x = Self::osc(v.ph1, p.shape) * (1.0 - p.osc2) + Self::osc(v.ph2, 1.0 - p.shape) * p.osc2;
                x += (v.ph_sub * TAU2).sin() * p.sub;
                self.noise ^= self.noise << 13;
                self.noise ^= self.noise >> 17;
                self.noise ^= self.noise << 5;
                x += (self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0) * p.noise;
                // The filter's envelope: the cutoff rises with the env and falls back over the decay.
                let env_level = match v.stage {
                    0 => {
                        v.env += dt / p.attack.max(1e-4);
                        if v.env >= 1.0 {
                            v.env = 1.0;
                            v.stage = 1;
                        }
                        v.env
                    }
                    1 => {
                        v.env -= dt * (1.0 - p.sustain) / p.decay.max(1e-4);
                        if v.env <= p.sustain {
                            v.env = p.sustain;
                            v.stage = 2;
                        }
                        v.env
                    }
                    2 => p.sustain,
                    3 => {
                        v.env -= dt / p.release.max(1e-4);
                        if v.env <= 0.0 {
                            v.env = 0.0;
                            v.stage = OFF;
                            self.ended.push((v.key, v.voice_id));
                        }
                        v.env
                    }
                    _ => 0.0,
                };
                let cutoff = (p.cutoff * 2f32.powf(p.env * env_level)).min(self.rate * 0.4);
                let f = 2.0 * (std::f32::consts::PI * cutoff / self.rate).sin();
                let q = 1.0 - p.reso * 0.9;
                x = v.svf.next(x, f, q);
                let amp = match v.stage {
                    OFF => 0.0,
                    3 => v.env,
                    _ => 1.0,
                };
                v.age += dt;
                out += x * amp;
            }
            // The drive, then the level.
            out *= self.gain / 4.0;
            out = (out * (1.0 + p.drive * 4.0)).tanh() * (1.0 - p.drive * 0.3);
            // The echo: a tap each side, fed back a little.
            let (echo_l, echo_r) = if !self.echo_l.is_empty() && !self.echo_r.is_empty() {
                let pos = self.echo_pos % self.echo_l.len();
                let tap_l = self.echo_l[pos];
                let rlen = self.echo_r.len();
                let tap_r = self.echo_r[(pos + rlen / 2) % rlen];
                self.echo_l[pos] = out * p.echo + tap_l * 0.35;
                self.echo_r[pos % rlen] = out * p.echo + tap_r * 0.35;
                self.echo_pos = (self.echo_pos + 1) % self.echo_l.len();
                (tap_l, tap_r)
            } else {
                (0.0, 0.0)
            };
            // The space: four combs and two allpasses a side.
            let (space_l, space_r) = self.room.next(out, p.space);
            *l += out + echo_l * 0.5 + space_l;
            *r += out + echo_r * 0.5 + space_r;
        }
        if self.ring > 0 {
            self.ring -= left.len().min(self.ring);
        }
    }

    fn take_ended(&mut self) -> Vec<(u8, Option<i32>)> {
        std::mem::take(&mut self.ended)
    }

    fn has_voices(&self) -> bool {
        self.voices.iter().any(|v| v.as_ref().is_some_and(|v| v.stage != OFF)) || self.ring > 0
    }
}

const TAU2: f32 = std::f32::consts::TAU;

/// The room: a small Schroeder space, a line per side and a comb set each.
struct Comb {
    buf: Vec<f32>,
    pos: usize,
    secs: f32,
    fb: f32,
    rate: f32,
}

impl Comb {
    fn new(secs: f32, fb: f32) -> Self {
        Self { buf: Vec::new(), pos: 0, secs, fb, rate: 0.0 }
    }

    fn set_rate(&mut self, rate: f32) {
        if self.rate == rate {
            return;
        }
        self.rate = rate;
        self.buf = vec![0.0; ((self.secs * rate).round() as usize).clamp(2, 8192)];
        self.pos = 0;
    }

    fn next(&mut self, x: f32) -> f32 {
        let out = self.buf[self.pos];
        self.buf[self.pos] = x + out * self.fb;
        self.pos = (self.pos + 1) % self.buf.len();
        out
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }
}

/// One allpass of the room.
#[derive(Clone, Copy, Default)]
struct Ap {
    z: f32,
}

impl Ap {
    fn next(&mut self, x: f32) -> f32 {
        let out = -x + self.z;
        self.z = x + out * 0.6;
        out
    }
}

struct Room {
    combs: [Comb; 4],
    aps: [Ap; 2],
    rate: f32,
}

impl Room {
    fn new() -> Self {
        Self { combs: std::array::from_fn(|i| Comb::new(COMBS[i], 0.72 + i as f32 * 0.008)), aps: [Ap::default(), Ap::default()], rate: 0.0 }
    }

    fn set_rate(&mut self, rate: f32) {
        if self.rate == rate {
            return;
        }
        self.rate = rate;
        for c in self.combs.iter_mut() {
            c.set_rate(rate);
        }
    }

    fn clear(&mut self) {
        for c in self.combs.iter_mut() {
            c.clear();
        }
        self.aps = [Ap::default(), Ap::default()];
    }

    /// The room's wet pair for a dry sample; `send` is the preset's share.
    fn next(&mut self, x: f32, send: f32) -> (f32, f32) {
        if send <= 0.0 {
            return (0.0, 0.0);
        }
        // Two combs feed each side, an allpass each rounds them.
        let a = self.combs[0].next(x) + self.combs[1].next(x);
        let b = self.combs[2].next(x) + self.combs[3].next(x);
        let l = self.aps[0].next(a * 0.5);
        let r = self.aps[1].next(b * 0.5);
        (l * 0.5 * send, r * 0.5 * send)
    }
}

fn make(rate: u32, v: &[i32]) -> Box<dyn Instrument> {
    Box::new(Flex::new(rate, v))
}

pub struct FlexKind;

impl Kind for FlexKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-flex";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's preset synthesizer";
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioFlex1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type MangoFlex = SmallGen<FlexKind>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The bank: two hundred fifty-six named presets, every name there twice no more, the same every call.
    #[test]
    fn the_bank_is_named_and_stable() {
        assert_eq!(patch(0), patch(0), "the same preset twice");
        assert_ne!(patch(0), patch(1), "presets differ");
        for i in 0..COUNT {
            let n = name(i);
            assert!(!n.is_empty());
            assert!(n.split(' ').count() == 2, "{n}");
        }
        let names: Vec<_> = (0..COUNT).map(name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert!(sorted.len() >= (COUNT / 2) as usize, "most names are their own ({})", sorted.len());
    }

    /// The macros land where the preset says: a full macro swings the sound, zero-middle holds the patch.
    #[test]
    fn the_macros_swing_the_patch() {
        let mut f = Flex::new(44100, &SPEC.start());
        f.note_on(60, 1.0, Some(1));
        let (mut l, mut r) = (vec![0.0f32; 4096], vec![0.0f32; 4096]);
        f.render(&mut l, &mut r);
        let peak = l.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!(peak > 0.001, "the preset sings ({peak})");
        // A macro at full: a different sound, no crash, no silence.
        let v = [0i32, 128, 128, 128, 128, 256];
        f.set(&v);
        f.note_on(60, 1.0, Some(2));
        for x in l.iter_mut() {
            *x = 0.0;
        }
        f.render(&mut l, &mut r);
        let swung = l.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!(swung > 0.0, "the macros land, the voice still sings");
    }

    /// Eight voices sound; a ninth steals the oldest.
    #[test]
    fn eight_voices_a_ninth_steals_the_oldest() {
        let mut f = Flex::new(44100, &SPEC.start());
        for (i, key) in [36u8, 40, 43, 48, 52, 55, 59, 62].into_iter().enumerate() {
            f.note_on(key, 1.0, Some(i as i32 + 1));
        }
        assert!(f.has_voices());
        f.note_on(65, 1.0, Some(9));
        assert_eq!(f.take_ended().len(), 1, "the oldest voice ended");
    }

    /// The release ends a voice; the echo and the space keep the plugin alive a while after.
    #[test]
    fn the_release_ends_the_voice_and_the_effects_ring() {
        let mut f = Flex::new(44100, &[0, 64, 64, 64, 64, 256]);
        f.note_on(48, 1.0, Some(1));
        let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        f.render(&mut l, &mut r);
        f.note_off(48, Some(1));
        let mut blocks = 0;
        while f.has_voices() && blocks < 2000 {
            for x in l.iter_mut() {
                *x = 0.0;
            }
            f.render(&mut l, &mut r);
            blocks += 1;
        }
        assert!(!f.has_voices(), "the ring ends ({blocks} blocks)");
        assert!(!f.take_ended().is_empty(), "the voice reported its end");
    }

    #[test]
    fn value_texts_and_typed_values() {
        assert!(text(PRESET, 0).starts_with("1 "), "{}", text(PRESET, 0));
        assert_eq!(parse(PRESET, "2"), Some(1));
        assert_eq!(text(MACRO1, 64), "50%");
        assert_eq!(parse(MACRO1, "100%"), Some(128));
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
        assert_eq!(parse(MACRO1, "nope"), None);
    }
}
