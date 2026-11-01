// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Pluck: Mango Studio's plucked string (the roadmap's "Pluckdeli"), a clean-room Karplus-Strong string bank.
//! A burst of shaped noise fills a delay line one period long; a lowpass in the loop takes the highs away a little
//! every round trip, so the string rings out with its own decay. Three string kinds change the burst and the loop's
//! tone, the pick position combs the burst, the strings spread across the stereo field, a held note glides.
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
    Setting { id: "string", name: "String", range: (0, 2), start: 0, magnet: None },
    Setting { id: "pick", name: "Pick", range: (0, 128), start: 28, magnet: None },
    Setting { id: "damping", name: "Damping", range: (0, 128), start: 36, magnet: None },
    Setting { id: "brightness", name: "Brightness", range: (0, 128), start: 72, magnet: None },
    Setting { id: "decay", name: "Decay", range: (0, 128), start: 74, magnet: None },
    Setting { id: "glide", name: "Glide", range: (0, 128), start: 0, magnet: None },
    Setting { id: "voices", name: "Voices", range: (0, 7), start: 3, magnet: None },
    Setting { id: "spread", name: "Spread", range: (0, 128), start: 48, magnet: None },
    Setting { id: "volume", name: "Volume", range: (0, 512), start: 256, magnet: Some(256) },
];

pub const STRING: usize = 0;
pub const PICK: usize = 1;
pub const DAMPING: usize = 2;
pub const BRIGHTNESS: usize = 3;
pub const DECAY: usize = 4;
pub const GLIDE: usize = 5;
pub const VOICES: usize = 6;
pub const SPREAD: usize = 7;
pub const VOLUME: usize = 8;

/// Editor layout (spec §2): two rows of cards over the hint and the wordmark.
const CARDS: [Card; 5] = [
    Card { x0: 8.0, x1: 122.0, y0: 28.0, y1: 134.0, heading: "STRING" },
    Card { x0: 128.0, x1: 242.0, y0: 28.0, y1: 134.0, heading: "BODY" },
    Card { x0: 248.0, x1: 362.0, y0: 28.0, y1: 134.0, heading: "TAIL" },
    Card { x0: 8.0, x1: 122.0, y0: 140.0, y1: 246.0, heading: "PLAY" },
    Card { x0: 128.0, x1: 242.0, y0: 140.0, y1: 246.0, heading: "OUTPUT" },
];

const CONTROLS: [Control; 9] = [
    Control::Choice { id: "string", x: 16.0, top: 46.0, names: &["Nylon", "Steel", "Glass"] },
    Control::Knob { id: "pick", cx: 90.0, cy: 86.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "damping", cx: 160.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "brightness", cx: 210.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "decay", cx: 280.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "glide", cx: 330.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "voices", cx: 40.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "spread", cx: 90.0, cy: 198.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "volume", cx: 185.0, cy: 198.0, r: 24.0, caption: "", bipolar: false },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Pluck",
    word: "PLUCK",
    about: "Mango Studio's plucked string",
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

/// The decay knob's seconds: the string falls to inaudible in about this long.
pub fn decay_seconds(v: i32) -> f64 {
    0.05 * 2f64.powf(f64::from(v) / 128.0 * 7.0)
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

/// A time as `123ms` under a second, `1.2s` from there.
fn seconds_text(t: f64) -> String {
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

/// The pick knob's comb position along the string, as a share of its length.
pub fn pick_share(v: i32) -> f64 {
    f64::from(v) / 128.0
}

/// The voices knob's count: 1 to 8.
pub fn voice_count(v: i32) -> i32 {
    v + 1
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        STRING => ["Nylon", "Steel", "Glass"][v.clamp(0, 2) as usize].to_string(),
        PICK => format!("{:.0}%", pick_share(v) * 100.0),
        DAMPING | BRIGHTNESS | GLIDE | SPREAD => percent(SETTINGS[k].range, v),
        DECAY => seconds_text(decay_text_seconds(v)),
        VOICES => match voice_count(v) {
            1 => "1 voice".into(),
            n => format!("{n} voices"),
        },
        _ => db_text(volume_gain(v)),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    match k {
        STRING => {
            let names = ["nylon", "steel", "glass"];
            let s = s.trim().to_ascii_lowercase();
            names
                .iter()
                .position(|n| *n == s)
                .map(|v| v as i32)
                .or_else(|| leading_number(&s).map(|x| x.round() as i32))
                .map(|v| v.clamp(0, 2))
        }
        PICK => {
            let x = leading_number(s)?;
            nearest(SETTINGS[PICK].range, x, |v| Some(pick_share(v) * 100.0))
        }
        DAMPING | BRIGHTNESS | GLIDE | SPREAD => percent_parse(SETTINGS[k].range, s),
        DECAY => decay_parse(s),
        VOICES => {
            let x = leading_number(s)?;
            Some((x.round() as i32 - 1).clamp(0, 7))
        }
        _ => parse_db(SETTINGS[VOLUME].range, s.trim().to_ascii_lowercase().as_str(), volume_gain),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The sound

/// The frequency of a key.
fn key_hz(key: u8) -> f32 {
    (440.0f64 * 2f64.powf((f64::from(key) - 69.0) / 12.0)) as f32
}

/// A string's kind: how its burst sounds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StringKind {
    Nylon,
    Steel,
    Glass,
}

/// One plucked string: a delay line of one period, a burst of shaped noise in it, and a lowpass in the loop that
/// takes a little of the highs away every round trip.
struct StringVoice {
    key: u8,
    voice_id: Option<i32>,
    kind: StringKind,
    /// The line, used from `n` samples (the rest stays silent, so a glide has nothing old to pick up).
    line: Vec<f32>,
    /// The line's length in samples (the period), and where it glides to when a held note's tuning moves.
    len: f32,
    target_len: f32,
    /// The read place along the line, 0..len.
    pos: f32,
    /// The loop lowpass's last out.
    lp: f32,
    /// The round-trip gain (the decay), the loop lowpass's share, and the note's own decay time.
    fb: f32,
    damp: f32,
    decay: f32,
    /// The velocity, the pan (spread) and the release fade left.
    vel: f32,
    pan: (f32, f32),
    rel: f32,
    age: f32,
}

impl StringVoice {
    /// A burst of one period, shaped by the string's kind, the brightness and the pick position.
    fn excite(&mut self, rng: &mut u32, brightness: f32, pick: f32) {
        let n = self.len.round().max(2.0) as usize;
        let comb = ((pick * n as f32) as usize).max(1);
        let b = brightness.clamp(0.0, 1.0);
        // The burst's lowpass: dark at the bottom of the knob, open at the top.
        let coef = (0.02 + 0.96 * b * b).min(0.98);
        let kind = self.kind;
        for i in 0..n {
            let x = noise(rng);
            let x = match kind {
                // Nylon: plain noise, the loop's lowpass does the shaping.
                StringKind::Nylon => x,
                // Steel: a harder attack, the noise edged with a ramp.
                StringKind::Steel => x + (i as f32 / n as f32) * 0.7,
                // Glass: the noise behind a soft window, so it rings rather than scratches.
                StringKind::Glass => x * (std::f32::consts::PI * i as f32 / n as f32).sin(),
            };
            // The pick position: a comb that takes a null where the pick sat.
            let prev = if i >= comb { self.line[i - comb] } else { 0.0 };
            self.line[i] = x - prev;
        }
        // The burst's own lowpass, in place, keeps it from starting too bright.
        let mut lp = 0.0f32;
        for s in self.line[..n].iter_mut() {
            lp += coef * (*s - lp);
            *s = lp;
        }
        for s in self.line[n..].iter_mut() {
            *s = 0.0;
        }
        self.pos = 0.0;
        self.lp = 0.0;
    }

    /// The pan a slot's place gives, the strings spread across the field.
    fn pan(spread: f32, slot: usize) -> (f32, f32) {
        let angle =
            (slot as f32 / 7.0 - 0.5) * spread * std::f32::consts::FRAC_2_PI + std::f32::consts::FRAC_2_PI;
        (angle.cos(), angle.sin())
    }
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

struct Pluck {
    rate: f32,
    kind: StringKind,
    pick: f32,
    damping: f32,
    brightness: f32,
    decay: f32,
    glide: f32,
    voices: usize,
    spread: f32,
    gain: f32,
    /// The glide's move toward the target length, per sample (1 = no glide).
    glide_k: f32,
    rng: u32,
    strings: [Option<StringVoice>; 8],
    ended: Vec<(u8, Option<i32>)>,
}

impl Pluck {
    /// A line long enough for the lowest note the bank tunes (27.5 Hz); lower keys still play, an octave up.
    fn line_len(rate: f32) -> usize {
        (rate / 27.5).ceil() as usize + 4
    }

    /// A string's decay: the round-trip gain that leaves the string inaudible after the decay time.
    fn feedback(decay: f32, freq: f32) -> f32 {
        let periods = (decay * freq).max(1.0);
        0.001f32.powf(1.0 / periods).clamp(0.0, 0.999_99)
    }
}

impl Instrument for Pluck {
    fn set(&mut self, v: &[i32]) {
        self.kind = match v[STRING] {
            1 => StringKind::Steel,
            2 => StringKind::Glass,
            _ => StringKind::Nylon,
        };
        self.pick = pick_share(v[PICK]) as f32;
        self.damping = f64::from(v[DAMPING]) as f32 / 128.0;
        self.brightness = f64::from(v[BRIGHTNESS]) as f32 / 128.0;
        self.decay = decay_seconds(v[DECAY]) as f32;
        self.glide = f64::from(v[GLIDE]) as f32 / 128.0;
        self.voices = voice_count(v[VOICES]) as usize;
        self.spread = f64::from(v[SPREAD]) as f32 / 128.0;
        self.gain = volume_gain(v[VOLUME]);
        // The spread moves the sounding strings too.
        for (i, s) in self.strings.iter_mut().enumerate() {
            if let Some(s) = s {
                s.pan = StringVoice::pan(self.spread, i);
            }
        }
        // The glide's step: the length walks to its target over about the glide time.
        self.glide_k = if self.glide <= 0.0 {
            1.0
        } else {
            (3.0 / (self.glide * self.rate)).min(1.0)
        };
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate as f32;
        let n = Self::line_len(self.rate);
        for s in self.strings.iter_mut().flatten() {
            s.line.resize(n, 0.0);
        }
    }

    fn reset(&mut self) {
        self.strings = [const { None }; 8];
    }

    fn note_on(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        // A free string among the first `voices`, else the oldest of them goes (spec §1).
        let limit = self.voices.min(8);
        let slot = match self.strings[..limit].iter().position(|s| s.is_none()) {
            Some(i) => i,
            None => {
                let mut i = 0;
                let mut oldest = self.strings[0].as_ref().map_or(0.0, |s| s.age);
                for (j, s) in self.strings[..limit].iter().enumerate() {
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
        let freq = key_hz(key);
        let mut v = StringVoice {
            key,
            voice_id: Some(voice_id.unwrap_or(key as i32 + 1)),
            kind: self.kind,
            line: vec![0.0; Self::line_len(self.rate)],
            len: (self.rate / freq).max(2.0),
            target_len: (self.rate / freq).max(2.0),
            pos: 0.0,
            lp: 0.0,
            fb: Self::feedback(self.decay, freq),
            damp: self.damping,
            decay: self.decay,
            vel: velocity.clamp(0.05, 1.0),
            pan: StringVoice::pan(self.spread, slot),
            rel: f32::INFINITY,
            age: 0.0,
        };
        v.excite(&mut self.rng, self.brightness, self.pick);
        self.strings[slot] = Some(v);
    }

    fn note_off(&mut self, key: u8, voice_id: Option<i32>) {
        for s in self.strings.iter_mut().flatten() {
            if s.key == key && (voice_id.is_none() || s.voice_id == voice_id) && s.rel.is_infinite() {
                s.rel = 0.05;
            }
        }
    }

    fn choke(&mut self, key: u8) {
        for s in self.strings.iter_mut() {
            if let Some(s) = s.take()
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

    fn tune(&mut self, key: u8, voice_id: Option<i32>, cents: f64) {
        // A held note's tuning moves: the string glides to its new length (portamento and slides).
        for s in self.strings.iter_mut().flatten() {
            if s.key == key && (voice_id.is_none() || s.voice_id == voice_id) {
                let f = key_hz(key) * 2f32.powf(cents as f32 / 1200.0);
                s.target_len = (self.rate / f).max(2.0);
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let dt = 1.0 / self.rate;
        let glide_k = self.glide_k;
        let gain = self.gain;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut out_l = 0.0f32;
            let mut out_r = 0.0f32;
            for s in self.strings.iter_mut().flatten() {
                // The length glides toward its target (a held note's tuning move).
                s.len += glide_k * (s.target_len - s.len);
                let n = s.len.round().max(2.0) as usize;
                // The string: a fractional read, the loop lowpass, the filtered write back at the
                // same place, so the burst rings down into the loop.
                let i = s.pos.floor() as usize;
                let frac = s.pos - i as f32;
                let i0 = i % n;
                let i1 = (i + 1) % n;
                let read = s.line[i0] + (s.line[i1] - s.line[i0]) * frac;
                let coef = 0.12 + 0.86 * s.damp;
                s.lp += coef * (read - s.lp);
                s.line[i0] = s.lp * s.fb;
                s.pos += 1.0;
                if s.pos >= n as f32 {
                    s.pos -= n as f32;
                }
                // The release fade, the level, the pan.
                let amp = match s.rel {
                    f if f.is_infinite() => 1.0,
                    mut f => {
                        f -= dt;
                        s.rel = f;
                        (f / 0.05).max(0.0)
                    }
                };
                out_l += read * s.pan.0 * amp * s.vel;
                out_r += read * s.pan.1 * amp * s.vel;
                s.age += dt;
            }
            *l += out_l * gain * 0.7;
            *r += out_r * gain * 0.7;
        }
        // The strings that faded: the release over, or the decay done (the round-trip gain was set
        // for it, so the string is inaudible by then).
        for s in self.strings.iter_mut() {
            let gone = s.as_ref().is_some_and(|v| v.rel <= 0.0 || v.age > v.decay + 0.5);
            if gone && let Some(v) = s.take() {
                self.ended.push((v.key, v.voice_id));
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
    let mut p = Pluck {
        rate: rate as f32,
        kind: StringKind::Nylon,
        pick: pick_share(v[PICK]) as f32,
        damping: f64::from(v[DAMPING]) as f32 / 128.0,
        brightness: f64::from(v[BRIGHTNESS]) as f32 / 128.0,
        decay: decay_seconds(v[DECAY]) as f32,
        glide: f64::from(v[GLIDE]) as f32 / 128.0,
        voices: voice_count(v[VOICES]) as usize,
        spread: f64::from(v[SPREAD]) as f32 / 128.0,
        gain: volume_gain(v[VOLUME]),
        glide_k: 1.0,
        rng: 0x1234_5678,
        strings: [const { None }; 8],
        ended: Vec::new(),
    };
    p.set_sample_rate(rate);
    p.set(v);
    Box::new(p)
}

pub struct PluckKind;

impl Kind for PluckKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-pluck";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's plucked string";
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioPluck";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type MangoPluck = SmallGen<PluckKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(STRING, 0), "Nylon");
        assert_eq!(text(STRING, 2), "Glass");
        assert_eq!(parse(STRING, "steel"), Some(1));
        assert_eq!(parse(STRING, "2"), Some(2));
        assert_eq!(text(PICK, 64), "50%");
        assert_eq!(parse(PICK, "50%"), Some(64));
        assert_eq!(text(DECAY, 0), "50ms");
        assert_eq!(parse(DECAY, "50ms"), Some(0));
        assert_eq!(text(DECAY, 74), "826ms");
        assert_eq!(parse(DECAY, "826ms"), Some(74));
        assert_eq!(text(VOICES, 0), "1 voice");
        assert_eq!(text(VOICES, 7), "8 voices");
        assert_eq!(parse(VOICES, "8"), Some(7));
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
    }

    /// A note plucks and rings out; a key up fades it; the pan spreads the strings.
    #[test]
    fn a_note_plucks_and_ends() {
        let mut p = make(44100, &SPEC.start());
        p.note_on(60, 1.0, Some(1));
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        let mut peak = 0.0f32;
        for _ in 0..10 {
            l.iter_mut().for_each(|x| *x = 0.0);
            p.render(&mut l, &mut r);
            peak = peak.max(l.iter().fold(0.0f32, |a, x| a.max(x.abs())));
        }
        assert!(peak > 0.01, "the string rings (peak {peak})");
        assert!(p.has_voices());
        p.note_off(60, Some(1));
        let mut blocks = 0;
        while p.has_voices() && blocks < 2000 {
            l.iter_mut().for_each(|x| *x = 0.0);
            p.render(&mut l, &mut r);
            blocks += 1;
        }
        assert!(!p.has_voices(), "the release ends the note");
        assert_eq!(p.take_ended(), vec![(60, Some(1))]);
        l.iter_mut().for_each(|x| *x = 0.0);
        r.iter_mut().for_each(|x| *x = 0.0);
        p.render(&mut l, &mut r);
        assert!(l.iter().all(|x| x.abs() < 0.001), "silence after the tail");
    }

    /// Eight notes sound at once; a ninth takes the oldest's place.
    #[test]
    fn eight_notes_a_ninth_steals_the_oldest() {
        let mut p = make(44100, &SPEC.start());
        let mut l = vec![0.0f32; 64];
        let mut r = vec![0.0f32; 64];
        for (i, key) in [48u8, 52, 55, 60, 64, 67, 72, 76].into_iter().enumerate() {
            p.note_on(key, 1.0, Some(i as i32 + 1));
            p.render(&mut l, &mut r);
        }
        assert!(p.has_voices());
        p.note_on(80, 1.0, Some(9));
        assert_eq!(
            p.take_ended(),
            vec![(48, Some(1)), (52, Some(2)), (55, Some(3)), (60, Some(4)), (64, Some(5))],
            "the oldest note ended each time"
        );
    }
}
