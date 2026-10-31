// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Monk: Lemon Studio's chanting monk, a port of Jonathan Taylor's MonkSynth (MIT, the
//! third-party notices carry its notice): a FOF (formant-wave-function) voice that sings
//! "a, e, i, o, u" — three damped formant sinusoids plus breath noise, grains overlap-added at the
//! pitch period (the algorithm is Xavier Rodet's, IRCAM), with vibrato and jitter, portamento, an
//! ADSR, up to ten detuned singers, and the stereo delay of the original behind the voice.
//!
// spec: JonET/monksynth dsp/voice.c dsp/synth.c dsp/delay.c (MIT)

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::fruit_kick::volume_gain;
use crate::params::{db_text, leading_number, nearest, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

// ---------------------------------------------------------------------------------------------------------------
// The settings (spec §5): the stored order, their ranges and their starting values

pub const SETTINGS: [Setting; 16] = [
    Setting { id: "vowel", name: "Vowel", range: (0, 128), start: 64, magnet: None },
    Setting { id: "glide", name: "Glide", range: (0, 128), start: 64, magnet: None },
    Setting { id: "voice", name: "Voice", range: (0, 128), start: 64, magnet: None },
    Setting { id: "vibrato", name: "Vibrato", range: (0, 128), start: 24, magnet: None },
    Setting { id: "vibrate", name: "Rate", range: (0, 128), start: 64, magnet: None },
    Setting { id: "breath", name: "Breath", range: (0, 128), start: 64, magnet: None },
    Setting { id: "attack", name: "Attack", range: (0, 128), start: 20, magnet: None },
    Setting { id: "decay", name: "Decay", range: (0, 128), start: 0, magnet: None },
    Setting { id: "sustain", name: "Sustain", range: (0, 128), start: 116, magnet: None },
    Setting { id: "release", name: "Release", range: (0, 128), start: 28, magnet: None },
    Setting { id: "unison", name: "Singers", range: (0, 9), start: 0, magnet: None },
    Setting { id: "detune", name: "Detune", range: (0, 128), start: 32, magnet: None },
    Setting { id: "spread", name: "Spread", range: (0, 128), start: 0, magnet: None },
    Setting { id: "dmix", name: "Echo", range: (0, 128), start: 102, magnet: None },
    Setting { id: "drate", name: "Length", range: (0, 128), start: 64, magnet: None },
    Setting { id: "volume", name: "Volume", range: (0, 512), start: 256, magnet: Some(256) },
];

pub const VOWEL: usize = 0;
pub const GLIDE: usize = 1;
pub const VOICE: usize = 2;
pub const VIBRATO: usize = 3;
pub const VIBRATE: usize = 4;
pub const BREATH: usize = 5;
pub const ATTACK: usize = 6;
pub const DECAY: usize = 7;
pub const SUSTAIN: usize = 8;
pub const RELEASE: usize = 9;
pub const UNISON: usize = 10;
pub const DETUNE: usize = 11;
pub const SPREAD: usize = 12;
pub const DMIX: usize = 13;
pub const DRATE: usize = 14;
pub const VOLUME: usize = 15;

/// Editor layout (spec §2): two rows of cards over the hint and the wordmark.
const CARDS: [Card; 7] = [
    Card { x0: 8.0, x1: 178.0, y0: 28.0, y1: 134.0, heading: "VOICE" },
    Card { x0: 184.0, x1: 384.0, y0: 28.0, y1: 134.0, heading: "UNISON" },
    Card { x0: 390.0, x1: 506.0, y0: 28.0, y1: 134.0, heading: "VIBRATO" },
    Card { x0: 512.0, x1: 592.0, y0: 28.0, y1: 134.0, heading: "BREATH" },
    Card { x0: 8.0, x1: 258.0, y0: 140.0, y1: 246.0, heading: "ENVELOPE" },
    Card { x0: 264.0, x1: 414.0, y0: 140.0, y1: 246.0, heading: "DELAY" },
    Card { x0: 420.0, x1: 592.0, y0: 140.0, y1: 246.0, heading: "OUTPUT" },
];

const CONTROLS: [Control; 16] = [
    Control::Knob { id: "vowel", cx: 42.0, cy: 86.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "glide", cx: 92.0, cy: 86.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "voice", cx: 142.0, cy: 86.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "unison", cx: 216.0, cy: 86.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "detune", cx: 286.0, cy: 86.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "spread", cx: 350.0, cy: 86.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "vibrato", cx: 422.0, cy: 86.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "vibrate", cx: 474.0, cy: 86.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "breath", cx: 552.0, cy: 86.0, r: 22.0, caption: "", bipolar: false },
    Control::Knob { id: "attack", cx: 42.0, cy: 198.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "decay", cx: 96.0, cy: 198.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "sustain", cx: 150.0, cy: 198.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "release", cx: 204.0, cy: 198.0, r: 20.0, caption: "", bipolar: false },
    Control::Knob { id: "dmix", cx: 302.0, cy: 198.0, r: 24.0, caption: "", bipolar: false },
    Control::Knob { id: "drate", cx: 376.0, cy: 198.0, r: 24.0, caption: "", bipolar: false },
    Control::Knob { id: "volume", cx: 470.0, cy: 198.0, r: 24.0, caption: "", bipolar: false },
];

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

/// A time knob's seconds: instant at the bottom, 0.005s growing to 1.28s over the range.
pub fn time_seconds(v: i32) -> f64 {
    if v <= 0 {
        0.0
    } else {
        0.005 * 2f64.powf(f64::from(v) / 128.0 * 8.0)
    }
}

/// The time a value's text names (the number `nearest` matches a typed one against).
fn time_text_seconds(v: i32) -> f64 {
    match time_seconds(v) {
        0.0 => 0.0,
        t if t < 1.0 => (t * 1000.0).round() / 1000.0,
        t => (t * 10.0).round() / 10.0,
    }
}

/// A time as `instant`, `123ms` or `1.2s`.
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
    nearest(SETTINGS[ATTACK].range, x, |v| Some(time_text_seconds(v)))
}

/// The vowel knob's share, 0 (a) to 1 (u).
pub fn vowel_share(v: i32) -> f64 {
    f64::from(v) / 128.0
}

/// The detune knob's spread between the outer singers, in cents.
pub fn detune_cents(v: i32) -> f64 {
    f64::from(v) / 128.0 * 50.0
}

/// The singers knob's count: 1 to 10.
pub fn unison_count(v: i32) -> i32 {
    v + 1
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        VOWEL => ["A", "E", "I", "O", "U"][((vowel_share(v) * 4.999).floor() as usize).min(4)].to_string(),
        GLIDE | VOICE | VIBRATO | VIBRATE | BREATH | SPREAD | SUSTAIN | DMIX | DRATE => {
            percent(SETTINGS[k].range, v)
        }
        ATTACK | DECAY | RELEASE => time_text(v),
        UNISON => match unison_count(v) {
            1 => "1 singer".into(),
            n => format!("{n} singers"),
        },
        DETUNE => format!("{:.1}ct", detune_cents(v)),
        _ => db_text(volume_gain(v)),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    match k {
        VOWEL => {
            let names = ["a", "e", "i", "o", "u"];
            let s = s.trim().to_ascii_lowercase();
            if let Some(i) = names.iter().position(|n| *n == s) {
                return Some((i * 32) as i32);
            }
            percent_parse(SETTINGS[VOWEL].range, s.as_str())
        }
        GLIDE | VOICE | VIBRATO | VIBRATE | BREATH | SPREAD | SUSTAIN | DMIX | DRATE => {
            percent_parse(SETTINGS[k].range, s)
        }
        ATTACK | DECAY | RELEASE => time_parse(s),
        UNISON => {
            let x = leading_number(s)?;
            Some((x.round() as i32 - 1).clamp(0, 9))
        }
        DETUNE => {
            let x = leading_number(s)?;
            nearest(SETTINGS[DETUNE].range, x, |v| Some(detune_cents(v)))
        }
        _ => parse_db(SETTINGS[VOLUME].range, s.trim().to_ascii_lowercase().as_str(), volume_gain),
    }
}

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Monk",
    word: "MONK",
    about: "Mango Studio's chanting monk (a port of MonkSynth)",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (600, 280),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark { x: 592.0, y: 263.0, anchor: 1.0, stacked: false },
    hint: Some(Rect::new(8.0, 252.0, 468.0, 22.0)),
    labels: &[],
    text,
    parse,
    make,
};

// ---------------------------------------------------------------------------------------------------------------
// The voice (the port of dsp/voice.c)

const TAU: f32 = std::f32::consts::TAU;
const PI: f32 = std::f32::consts::PI;

const NUM_FORMANTS: usize = 3;
const SPLINE_TBL: usize = 1280;
const SINE_TBL: usize = 1024;
const MAX_GRAIN: usize = 3840; // 192000 * 0.02: above 192 kHz the grain shortens, not the tables
const OVERLAP_BUF: usize = 10240;
const MAX_UNISON: usize = 10;

const GRAIN_DURATION: f32 = 0.02; // grain length in seconds (882 samples at 44100 Hz)
const ASPIRATION_PERIOD1: f32 = 0.000202; // ~4951 Hz breath component
const ASPIRATION_PERIOD2: f32 = 0.000263; // ~3802 Hz breath component
const ENV_ANTICLICK: f32 = 0.003; // minimum ramp to prevent clicks (~3ms)
const SPLINE_SEG: usize = 320; // samples per cubic spline segment (4 segments)
const PITCH_TABLE_BASE: f32 = 8.175799; // frequency of MIDI note 0 (C-1)
const RAMP_TICKS: i32 = 10; // the Delay Lama's 10-tick ramp

/// Formant center frequencies (Hz) at the five vowel positions [a, e, i, o, u].
const FORMANT_FREQS: [[f32; 5]; NUM_FORMANTS] = [
    [280.0, 450.0, 800.0, 350.0, 270.0], // F1
    [600.0, 800.0, 1150.0, 2000.0, 2140.0], // F2
    [2240.0, 2830.0, 2900.0, 2800.0, 2950.0], // F3
];
/// Formant bandwidths: the exponential decay rate of each formant.
const FORMANT_BW: [f32; NUM_FORMANTS] = [32.5, 47.5, 62.5];

/// Sample rates outside this range fall back to 44.1 kHz (NaN fails the comparisons).
fn sanitize_rate(rate: f32) -> f32 {
    (rate >= 1000.0 && rate <= 768000.0).then_some(rate).unwrap_or(44100.0)
}

/// A MIDI note's frequency.
fn note_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// A frequency in MIDI note space.
fn hz_note(hz: f32) -> f32 {
    12.0 * (hz / 440.0).log2() + 69.0
}

/// The tables shared by every singer: the sine, the vowels' formant splines, and the grain
/// window, decay and breath, rebuilt when the sample rate moves.
struct Tables {
    grain_len: usize,
    sine: [f32; SINE_TBL],
    formant: [[f32; SPLINE_TBL]; NUM_FORMANTS],
    window: Vec<f32>,
    decay: Vec<f32>,
    aspiration: Vec<f32>,
}

impl Tables {
    fn new(rate: f32) -> Tables {
        let rate = sanitize_rate(rate);
        let grain_len = ((rate * GRAIN_DURATION) as usize).min(MAX_GRAIN);
        let mut sine = [0.0f32; SINE_TBL];
        for (i, s) in sine.iter_mut().enumerate() {
            *s = (TAU * i as f32 / SINE_TBL as f32).sin();
        }
        // Catmull-Rom splines across the five vowel control points, the ends duplicated so the
        // curve passes through all five positions.
        let mut formant = [[0.0f32; SPLINE_TBL]; NUM_FORMANTS];
        for (f, table) in formant.iter_mut().enumerate() {
            let pts = FORMANT_FREQS[f];
            let padded = [pts[0], pts[0], pts[1], pts[2], pts[3], pts[4], pts[4]];
            for seg in 0..4 {
                let (p0, p1, p2, p3) = (padded[seg], padded[seg + 1], padded[seg + 2], padded[seg + 3]);
                for i in 0..SPLINE_SEG {
                    let t = i as f32 / SPLINE_SEG as f32;
                    let t2 = t * t;
                    let t3 = t2 * t;
                    table[seg * SPLINE_SEG + i] = 0.5
                        * ((2.0 * p1)
                            + (-p0 + p2) * t
                            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3);
                }
            }
        }
        let mut t = Tables {
            grain_len,
            sine,
            formant,
            window: vec![0.0; grain_len],
            decay: vec![0.0; grain_len * 4],
            aspiration: vec![0.0; grain_len],
        };
        t.rebuild_windows(rate);
        t
    }

    /// The grain envelope (short cosine attack, flat, an open cosine release), the exponential
    /// decay (four grain lengths, for every formant's bandwidth) and the breath: two inharmonic
    /// decaying sinusoids (~4951 Hz and ~3802 Hz), the "air".
    fn rebuild_windows(&mut self, rate: f32) {
        let n = self.grain_len;
        let attack_len = (rate * 0.0018) as usize;
        let release_start = (rate * 0.013) as usize;
        let release_len = (rate * 0.007) as usize;
        self.window.iter_mut().for_each(|w| *w = 1.0);
        for (i, w) in self.window.iter_mut().enumerate().take(attack_len.min(n)) {
            *w = 0.5 * (1.0 - (PI * i as f32 / attack_len.max(1) as f32).cos());
        }
        for i in release_start.min(n)..n {
            self.window[i] =
                0.5 * (1.0 - (PI * (release_len + i) as f32 / release_len.max(1) as f32).cos());
        }
        let bw = 50.0 * PI;
        for (i, d) in self.decay.iter_mut().enumerate() {
            *d = (-bw * i as f32 / rate).exp();
        }
        let (p1, p2) = (rate * ASPIRATION_PERIOD1, rate * ASPIRATION_PERIOD2);
        for (i, a) in self.aspiration.iter_mut().enumerate() {
            let d1 = decay_at(&self.decay, i as f32 * 3.0);
            let d2 = decay_at(&self.decay, i as f32 * 3.5);
            *a = (TAU * i as f32 / p1).sin() * d1 + (TAU * i as f32 / p2).sin() * d2;
        }
    }
}

/// The decay table's value at `x`, or 0 past it.
fn decay_at(decay: &[f32], x: f32) -> f32 {
    let i = x as usize;
    decay.get(i).copied().unwrap_or(0.0)
}

/// An envelope's stage.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// One singer: the grain engine of `dsp/voice.c`, its buffers its own.
struct Singer {
    rate: f32,
    active: bool,
    current_pitch: f32,
    target_pitch: f32,
    pitch_bend_offset: f32,
    glide_param: f32,
    min_glide: f32,
    pitch_step: f32,
    pitch_ramp_ticks: i32,
    vowel_step: f32,
    vowel_ramp_ticks: i32,
    ramp_counter: i32,
    ramp_period: i32,
    vibrato_phase: f32,
    vibrato_depth: f32,
    vibrato_rate: f32,
    random_jitter: f32,
    jitter_counter: u32,
    jitter_period: u32,
    sr_per_vib_tbl: f32,
    rng_state: u32,
    overlap: Vec<f32>,
    overlap_write_pos: usize,
    overlap_read_pos: usize,
    overlap_offset: u32,
    /// The grain being built, at the tables' length.
    grain: Vec<f32>,
    current_vowel: f32,
    target_vowel: f32,
    current_voice: f32,
    aspiration_amp: f32,
    grain_dirty: bool,
    env: Stage,
    env_level: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    /// The note this singer plays, for the host's VoiceTerminated.
    key: u8,
    voice_id: Option<i32>,
    /// Samples rendered since the singer went quiet (the overlap buffer's drain).
    drain: u32,
    /// The envelope reached its idle this block, or the tail drained: the end the host hears of.
    finished: bool,
    reported: bool,
}

impl Singer {
    fn new(rate: f32) -> Singer {
        let rate = sanitize_rate(rate);
        Singer {
            rate,
            active: false,
            current_pitch: hz_note(220.0),
            target_pitch: hz_note(220.0),
            pitch_bend_offset: 0.0,
            glide_param: 0.5,
            min_glide: 0.0,
            pitch_step: 0.0,
            pitch_ramp_ticks: 0,
            vowel_step: 0.0,
            vowel_ramp_ticks: 0,
            ramp_counter: 1,
            ramp_period: (rate * 0.01) as i32,
            vibrato_phase: 0.0,
            vibrato_depth: 0.0,
            vibrato_rate: 0.5,
            random_jitter: 4.0,
            jitter_counter: 0,
            jitter_period: 4586,
            sr_per_vib_tbl: rate / SINE_TBL as f32,
            rng_state: 12345,
            overlap: vec![0.0; OVERLAP_BUF],
            overlap_write_pos: 0,
            overlap_read_pos: 0,
            overlap_offset: 0,
            grain: Vec::new(),
            current_vowel: 0.5,
            target_vowel: 0.5,
            current_voice: 0.5,
            aspiration_amp: 0.5,
            grain_dirty: true,
            env: Stage::Idle,
            env_level: 0.0,
            attack: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: 0.0,
            key: 0,
            voice_id: None,
            drain: 0,
            finished: false,
            reported: false,
        }
    }

    fn set_rate(&mut self, rate: f32) {
        let rate = sanitize_rate(rate);
        self.rate = rate;
        self.sr_per_vib_tbl = rate / SINE_TBL as f32;
        let jp = (rate / 44100.0 * 4586.0) as u32;
        self.jitter_period = jp.max(1);
        self.ramp_period = (rate * 0.01) as i32;
        self.grain_dirty = true;
    }

    fn reset(&mut self) {
        self.active = false;
        self.current_pitch = hz_note(220.0);
        self.target_pitch = hz_note(220.0);
        self.vibrato_phase = 0.0;
        self.random_jitter = 4.0;
        self.jitter_counter = 0;
        self.rng_state = 12345;
        self.overlap.fill(0.0);
        self.overlap_write_pos = 0;
        self.overlap_read_pos = 0;
        self.overlap_offset = 0;
        self.env = Stage::Idle;
        self.env_level = 0.0;
        self.grain_dirty = true;
        self.drain = 0;
        self.finished = false;
        self.reported = false;
    }

    fn is_active(&self) -> bool {
        self.active || self.env == Stage::Release
    }

    fn has_envelope(&self) -> bool {
        self.attack > 0.0 || self.decay > 0.0 || self.sustain < 1.0 || self.release > 0.0
    }

    fn note_on(&mut self, hz: f32, key: u8, voice_id: Option<i32>) {
        let was_active = self.active;
        self.active = true;
        self.key = key;
        self.voice_id = voice_id;
        self.target_pitch = hz_note(hz.clamp(8.0, 20000.0));
        if !was_active {
            self.current_pitch = self.target_pitch;
            self.grain_dirty = true;
            // The overlap buffer drains by itself (each sample is zeroed after it is read);
            // clearing it here would pop between back-to-back notes. The write follows the read,
            // so new grains land right.
            self.overlap_write_pos = self.overlap_read_pos;
            self.overlap_offset = 0;
        }
        self.drain = 0;
        self.finished = false;
        self.reported = false;
        if self.has_envelope() {
            self.env = Stage::Attack;
            // The level is not reset: a re-strike during a release ramps from where it is.
        } else {
            self.env_level = 1.0;
            self.env = Stage::Sustain;
        }
    }

    fn note_off(&mut self) {
        // A singer that never sounded stays idle (its release would only report a false end).
        if !self.active && self.env == Stage::Idle {
            return;
        }
        if self.has_envelope() {
            // Always a ramp down: even release=0 keeps the ~3ms anti-click minimum.
            self.env = Stage::Release;
        } else {
            self.active = false;
            self.env = Stage::Idle;
            self.env_level = 0.0;
        }
    }

    fn choke(&mut self) {
        self.active = false;
        self.env = Stage::Idle;
        self.env_level = 0.0;
        self.overlap.fill(0.0);
        self.drain = 0;
        self.finished = true;
    }

    fn set_pitch_direct(&mut self, hz: f32) {
        let note = hz_note(hz.clamp(8.0, 20000.0));
        self.target_pitch = note;
        self.current_pitch = note;
    }

    /// The vowel target and the restart of its ramp (the original ramps the vowel the same way
    /// the pitch slews).
    fn set_vowel(&mut self, vowel: f32) {
        let new_target = vowel.clamp(0.0, 1.0);
        let eff = self.glide_param.max(self.min_glide);
        if eff < 0.001 {
            self.current_vowel = new_target;
            self.vowel_ramp_ticks = 0;
        } else {
            let n = if eff > 0.5 { RAMP_TICKS + ((eff - 0.5) * 180.0) as i32 } else { RAMP_TICKS };
            self.vowel_step = (new_target - self.current_vowel) / n as f32;
            self.vowel_ramp_ticks = n;
        }
        self.target_vowel = new_target;
    }

    /// The envelope's next level (one sample).
    fn env_tick(&mut self) -> f32 {
        match self.env {
            Stage::Attack => {
                let rate = 1.0 / (self.attack.max(ENV_ANTICLICK) * self.rate);
                self.env_level += rate;
                if self.env_level >= 1.0 {
                    self.env_level = 1.0;
                    self.env = if self.decay > 0.0 { Stage::Decay } else { Stage::Sustain };
                }
            }
            Stage::Decay => {
                let rate = if self.decay > 0.0 { 1.0 / (self.decay * self.rate) } else { 1.0 };
                self.env_level -= rate * (1.0 - self.sustain);
                if self.env_level <= self.sustain {
                    self.env_level = self.sustain;
                    self.env = Stage::Sustain;
                }
            }
            Stage::Release => {
                let rate = 1.0 / (self.release.max(ENV_ANTICLICK) * self.rate);
                self.env_level -= rate;
                if self.env_level <= 0.0 {
                    self.env_level = 0.0;
                    self.env = Stage::Idle;
                    self.active = false;
                }
            }
            _ => {}
        }
        self.env_level
    }

    /// The pitch's grain period, quantized to 32 steps a semitone (the voice's characteristic
    /// stepped pitch).
    fn grain_period(&self, midi_note: f32) -> f32 {
        let internal = midi_note.clamp(0.0, 150.0) - 12.0;
        let table_idx = (internal * 32.0) as i32;
        let freq = 2f32.powf(table_idx as f32 / 384.0) * PITCH_TABLE_BASE;
        (self.rate / freq).max(1.0)
    }

    /// The portamento: the tick ramp (`min_glide`), the constant-rate slew (`glide`), or a snap.
    fn apply_portamento(&mut self) {
        if self.min_glide > 0.001 {
            self.ramp_counter -= 1;
            if self.ramp_counter > 0 {
                return;
            }
            self.ramp_counter = self.ramp_period;
            if self.pitch_ramp_ticks > 0 {
                self.pitch_ramp_ticks -= 1;
                self.current_pitch += self.pitch_step;
                if self.pitch_ramp_ticks == 0 {
                    self.current_pitch = self.target_pitch;
                }
            }
            if self.vowel_ramp_ticks > 0 {
                self.vowel_ramp_ticks -= 1;
                self.current_vowel += self.vowel_step;
                if self.vowel_ramp_ticks == 0 {
                    self.current_vowel = self.target_vowel;
                }
            }
        } else if self.glide_param > 0.001 {
            let diff = self.target_pitch - self.current_pitch;
            let rate = 12.0 / ((self.glide_param + 0.01) * self.rate);
            if diff > 0.2 {
                self.current_pitch += rate;
            } else if diff < -0.2 {
                self.current_pitch -= rate;
            } else {
                self.current_pitch = self.target_pitch;
            }
            self.current_vowel = self.target_vowel;
        } else {
            self.current_pitch = self.target_pitch;
            self.current_vowel = self.target_vowel;
        }
    }

    /// The vibrato: a sine LFO whose rate carries random jitter (even at depth 0 a subtle wobble
    /// keeps the voice alive). Returns the semitone offset.
    fn compute_vibrato(&mut self, sine: &[f32; SINE_TBL]) -> f32 {
        let rate_scale = 4f32.powf(self.vibrato_rate * 2.0 - 1.0);
        let rate = (self.vibrato_depth * 0.2 + 1.0) * self.random_jitter * rate_scale;
        self.vibrato_phase += rate / self.sr_per_vib_tbl;
        if !(self.vibrato_phase < SINE_TBL as f32) {
            self.vibrato_phase %= SINE_TBL as f32;
            if !(self.vibrato_phase >= 0.0) {
                self.vibrato_phase = 0.0;
            }
        }
        self.jitter_counter += 1;
        if self.jitter_counter >= self.jitter_period {
            self.jitter_counter = 0;
            self.random_jitter = Self::next_rand(&mut self.rng_state) * 2.0 + 5.0;
        }
        let idx = (self.vibrato_phase as usize) % SINE_TBL;
        (self.vibrato_depth + 0.2) * sine[idx]
    }

    /// The LCG of the original: the same seed, the same jitter.
    fn next_rand(state: &mut u32) -> f32 {
        *state = state.wrapping_mul(1103515245).wrapping_add(12345);
        ((*state >> 16) & 0x7FFF) as f32 / 32768.0
    }

    /// The vowel spline's value at `vowel` (linear between the table's points).
    fn lookup_formant(table: &[f32; SPLINE_TBL], vowel: f32) -> f32 {
        let pos = vowel.clamp(0.0, 1.0) * (SPLINE_TBL - 1) as f32;
        let idx = (pos as i32).clamp(0, SPLINE_TBL as i32 - 2) as usize;
        let frac = pos - idx as f32;
        table[idx] * (1.0 - frac) + table[idx + 1] * frac
    }

    /// One grain: three damped formant sinusoids (the vowel's splines, scaled by the voice
    /// character) plus breath, under the window.
    fn compute_grain(&mut self, tables: &Tables) {
        let n = tables.grain_len;
        let formant_scale = self.current_voice * 0.5 + 0.75;
        let mut formants = [0.0f32; NUM_FORMANTS];
        for (f, formant) in formants.iter_mut().enumerate() {
            *formant = Self::lookup_formant(&tables.formant[f], self.current_vowel) * formant_scale;
        }
        for (i, g) in self.grain.iter_mut().enumerate().take(n) {
            let t = i as f32 / self.rate;
            let mut sample = 0.0;
            for (f, formant) in formants.iter().enumerate() {
                let phase = TAU * formant * t;
                let sine_idx = (phase / TAU * SINE_TBL as f32) as usize % SINE_TBL;
                let decay = decay_at(&tables.decay, FORMANT_BW[f] * i as f32 / 50.0);
                sample += tables.sine[sine_idx] * decay;
            }
            sample += tables.aspiration[i] * self.aspiration_amp;
            *g = sample * tables.window[i];
        }
        self.grain_dirty = false;
    }

    /// Renders `out` (the two passes of `monk_voice_process`): the grains into the overlap
    /// buffer, then the buffer's read under the envelope.
    fn process(&mut self, out: &mut [f32], tables: &Tables) {
        let has_envelope = self.has_envelope() || self.env == Stage::Release;

        // Pass 1: the grains, while the note is held or releasing.
        for _ in 0..out.len() {
            if self.active || self.env == Stage::Release {
                self.apply_portamento();
                let vib = self.compute_vibrato(&tables.sine);
                let pitch = self.current_pitch + self.pitch_bend_offset + vib;
                let period = self.grain_period(pitch) as u32;
                if self.overlap_offset >= period || self.grain_dirty {
                    if self.grain.len() < tables.grain_len {
                        self.grain.resize(tables.grain_len, 0.0);
                    }
                    self.compute_grain(tables);
                    // The grain is taken out so the overlap add can take the buffer and the
                    // singer apart (the borrow checker's own overlap-add).
                    let grain = std::mem::take(&mut self.grain);
                    overlap_add(&mut self.overlap, &mut self.overlap_write_pos, self.overlap_offset, &grain, tables.grain_len);
                    self.grain = grain;
                    self.overlap_offset = 0;
                }
            }
            self.overlap_offset = self.overlap_offset.wrapping_add(1);
        }

        // Pass 2: the overlap buffer's read, under the envelope.
        while self.overlap_read_pos >= OVERLAP_BUF {
            self.overlap_read_pos -= OVERLAP_BUF;
        }
        for sample in out.iter_mut() {
            let mut s = self.overlap[self.overlap_read_pos];
            self.overlap[self.overlap_read_pos] = 0.0;
            self.overlap_read_pos = (self.overlap_read_pos + 1) % OVERLAP_BUF;
            if has_envelope {
                s *= self.env_tick();
            }
            *sample = s;
        }
        if self.env == Stage::Idle {
            self.finished = true;
        }
    }
}

/// Accumulate the grain into the circular overlap buffer, advancing by the samples since the
/// last grain (grains overlap whenever the period is shorter than the grain).
fn overlap_add(overlap: &mut [f32], write_pos: &mut usize, offset: u32, grain: &[f32], grain_len: usize) {
    *write_pos = (*write_pos + offset as usize) % OVERLAP_BUF;
    let start = *write_pos;
    for (i, g) in grain.iter().take(grain_len).enumerate() {
        let pos = (start + i) % OVERLAP_BUF;
        overlap[pos] += *g;
    }
}

/// Pitch-dependent amplitude compensation: shorter periods overlap fewer grains, so higher notes
/// would be quieter without it.
fn pitch_amplitude(current_pitch: f32) -> f32 {
    ((current_pitch - 12.0) * (-1.0 / 72.0) + 2.0).clamp(0.1, 3.0)
}

// ---------------------------------------------------------------------------------------------------------------
// The delay (the port of dsp/delay.c): two independent feedback lines, a wide stereo image

const DELAY_LINE: usize = 96000; // ~2s at 48 kHz

/// Base delay times in samples at 44100 Hz (~310ms left, ~398ms right).
const DELAY_TIME_L: f32 = 13653.0;
const DELAY_TIME_R: f32 = 17570.0;

struct MonkDelay {
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_pos: usize,
    sample_rate: f32,
    target_l: f32,
    target_r: f32,
    current_l: f32,
    current_r: f32,
    smooth_coeff: f32,
    rate: f32,
    feedback: f32,
    mix: f32,
}

impl MonkDelay {
    fn new(rate: f32) -> MonkDelay {
        let rate = sanitize_rate(rate);
        let mut d = MonkDelay {
            buffer_l: vec![0.0; DELAY_LINE],
            buffer_r: vec![0.0; DELAY_LINE],
            write_pos: 0,
            sample_rate: rate,
            target_l: 0.0,
            target_r: 0.0,
            current_l: 0.0,
            current_r: 0.0,
            smooth_coeff: 1.0 / (0.05 * rate),
            rate: 0.5,
            feedback: 0.5,
            mix: 0.5,
        };
        d.recalc_taps();
        d.current_l = d.target_l;
        d.current_r = d.target_r;
        d
    }

    /// The length knob scales both taps together: 0.5 the original timing, 0 a slapback, 1 a
    /// spacious echo.
    fn set_rate_param(&mut self, rate: f32) {
        self.rate = rate.clamp(0.0, 1.0);
        self.recalc_taps();
    }

    fn set_sample_rate(&mut self, rate: f32) {
        let rate = sanitize_rate(rate);
        self.sample_rate = rate;
        self.smooth_coeff = 1.0 / (0.05 * rate);
        self.buffer_l.fill(0.0);
        self.buffer_r.fill(0.0);
        self.write_pos = 0;
        self.recalc_taps();
        self.current_l = self.target_l;
        self.current_r = self.target_r;
    }

    fn recalc_taps(&mut self) {
        let scale = self.sample_rate / 44100.0;
        let rate_scale = 0.1 + self.rate * 1.9;
        self.target_l = (DELAY_TIME_L * scale * rate_scale).min(DELAY_LINE as f32 - 1.0);
        self.target_r = (DELAY_TIME_R * scale * rate_scale).min(DELAY_LINE as f32 - 1.0);
    }

    fn reset(&mut self) {
        self.buffer_l.fill(0.0);
        self.buffer_r.fill(0.0);
    }

    /// One block: `input` mono in, `out_l`/`out_r` dry + wet out.
    fn process(&mut self, input: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        let coeff = self.smooth_coeff;
        let feedback = self.feedback;
        let mix = self.mix;
        for (i, input) in input.iter().enumerate() {
            self.current_l += coeff * (self.target_l - self.current_l);
            self.current_r += coeff * (self.target_r - self.current_r);

            // Fractional reads, split before the wrap so a tiny negative cannot round one past
            // the end.
            let rd_l = self.write_pos as f32 - self.current_l;
            let idx_l = rd_l.floor() as isize;
            let frac_l = rd_l - idx_l as f32;
            let idx_l = idx_l.rem_euclid(DELAY_LINE as isize) as usize;
            let tap_l = self.buffer_l[idx_l] * (1.0 - frac_l) + self.buffer_l[(idx_l + 1) % DELAY_LINE] * frac_l;

            let rd_r = self.write_pos as f32 - self.current_r;
            let idx_r = rd_r.floor() as isize;
            let frac_r = rd_r - idx_r as f32;
            let idx_r = idx_r.rem_euclid(DELAY_LINE as isize) as usize;
            let tap_r = self.buffer_r[idx_r] * (1.0 - frac_r) + self.buffer_r[(idx_r + 1) % DELAY_LINE] * frac_r;

            self.buffer_l[self.write_pos] = (tap_l * feedback + input) * mix;
            self.buffer_r[self.write_pos] = (tap_r * feedback + input) * mix;

            out_l[i] = input + tap_l;
            out_r[i] = input + tap_r;

            self.write_pos = (self.write_pos + 1) % DELAY_LINE;
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The engine (the port of dsp/synth.c): the singers, the note stack, the unison, the delay

pub struct Monk {
    tables: Tables,
    singers: [Singer; MAX_UNISON],
    unison: usize,
    unison_detune: f32,
    base_voice: f32,
    voice_spread: f32,
    last_base_hz: f32,
    held: Vec<u8>,
    delay: MonkDelay,
    gain_coeff: f32,
    current_voice_gain: f32,
    target_voice_gain: f32,
    current_out_gain: f32,
    volume: f32,
    mono: Vec<f32>,
    voice_buf: Vec<f32>,
    l_buf: Vec<f32>,
    r_buf: Vec<f32>,
    ended: Vec<(u8, Option<i32>)>,
}

/// One singer's frequency with the unison's detune spread (the outer ones out, the middle one on
/// the note).
fn detuned_hz(base_hz: f32, detune_cents: f32, idx: usize, count: usize) -> f32 {
    if count <= 1 {
        return base_hz;
    }
    let offset = detune_cents * (idx as f32 / (count - 1) as f32 * 2.0 - 1.0);
    base_hz * 2f32.powf(offset / 1200.0)
}

/// The one-pole coefficient of the ~5ms gain smoothing.
fn gain_smoothing_coeff(rate: f32) -> f32 {
    if rate <= 0.0 {
        1.0
    } else {
        1.0 - (-1.0 / (0.005 * rate)).exp()
    }
}

/// A key's frequency.
fn key_hz(key: u8) -> f32 {
    note_hz(key as f32)
}

impl Monk {
    /// The voice character across the unison: symmetric offsets from the base, clamped.
    fn apply_voice_spread(&mut self) {
        let n = self.unison;
        let (base, spread) = (self.base_voice, self.voice_spread);
        for (i, s) in self.singers.iter_mut().take(n).enumerate() {
            let offset = if n > 1 { spread * (i as f32 / (n - 1) as f32 * 2.0 - 1.0) } else { 0.0 };
            s.current_voice = (base + offset).clamp(0.0, 1.0);
        }
    }

    /// A note spreads over the singers with the detune, each starting (or sliding to) it.
    fn apply_unison_detune(&mut self, base_hz: f32, key: u8, voice_id: Option<i32>) {
        self.last_base_hz = base_hz;
        let n = self.unison;
        let detune = self.unison_detune;
        for (i, s) in self.singers.iter_mut().take(n).enumerate() {
            s.note_on(detuned_hz(base_hz, detune, i, n), key, voice_id);
        }
    }

    /// The singers knob moved: the excess fade (a release keeps the anti-click minimum), the kept
    /// ones re-pitch, the new ones start.
    fn unison_set(&mut self, count: usize) {
        let old = self.unison;
        self.unison = count.clamp(1, MAX_UNISON);
        self.target_voice_gain = 1.0 / (self.unison as f32).sqrt();
        if old == self.unison || !self.singers[0].is_active() {
            return;
        }
        let (base_hz, detune) = (self.last_base_hz, self.unison_detune);
        let key = self.held.last().copied().unwrap_or(0);
        for (i, s) in self.singers.iter_mut().enumerate() {
            if i < self.unison && i < old {
                s.set_pitch_direct(detuned_hz(base_hz, detune, i, self.unison));
            } else if i < self.unison {
                s.note_on(detuned_hz(base_hz, detune, i, self.unison), key, Some(key as i32 + 1));
            } else if i < old {
                s.env = Stage::Release;
            }
        }
        self.apply_voice_spread();
    }
}

impl Instrument for Monk {
    fn set(&mut self, v: &[i32]) {
        let vowel = vowel_share(v[VOWEL]) as f32;
        self.base_voice = vowel_share(v[VOICE]) as f32;
        self.voice_spread = vowel_share(v[SPREAD]) as f32;
        self.unison_detune = detune_cents(v[DETUNE]) as f32;
        self.volume = volume_gain(v[VOLUME]);
        for s in self.singers.iter_mut() {
            s.glide_param = vowel_share(v[GLIDE]) as f32;
            s.vibrato_depth = vowel_share(v[VIBRATO]) as f32;
            s.vibrato_rate = vowel_share(v[VIBRATE]) as f32;
            s.aspiration_amp = vowel_share(v[BREATH]) as f32;
            s.attack = time_seconds(v[ATTACK]) as f32;
            s.decay = time_seconds(v[DECAY]) as f32;
            s.sustain = vowel_share(v[SUSTAIN]) as f32;
            s.release = time_seconds(v[RELEASE]) as f32;
            s.set_vowel(vowel);
        }
        self.apply_voice_spread();
        self.unison_set(unison_count(v[UNISON]) as usize);
        self.delay.mix = vowel_share(v[DMIX]) as f32;
        self.delay.set_rate_param(vowel_share(v[DRATE]) as f32);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        let rate = sanitize_rate(rate as f32);
        self.tables = Tables::new(rate);
        for s in self.singers.iter_mut() {
            s.set_rate(rate);
        }
        self.delay.set_sample_rate(rate);
        self.gain_coeff = gain_smoothing_coeff(rate);
    }

    fn reset(&mut self) {
        for s in self.singers.iter_mut() {
            s.reset();
        }
        self.delay.reset();
        self.held.clear();
        self.current_voice_gain = self.target_voice_gain;
        self.current_out_gain = -1.0;
    }

    fn note_on(&mut self, key: u8, _velocity: f32, voice_id: Option<i32>) {
        // MIDI uses the portamento knob only: the tick ramp is the original's XY pad's way, and
        // there is no XY pad here.
        for s in self.singers.iter_mut() {
            s.min_glide = 0.0;
        }
        self.held.retain(|k| *k != key);
        if self.held.len() < MAX_UNISON {
            self.held.push(key);
        }
        self.apply_unison_detune(key_hz(key), key, voice_id.or(Some(key as i32 + 1)));
    }

    fn note_off(&mut self, key: u8, _voice_id: Option<i32>) {
        self.held.retain(|k| *k != key);
        if let Some(top) = self.held.last().copied() {
            // A held note falls back: the singers slide to it (a re-strike without a level reset).
            let n = self.unison;
            let detune = self.unison_detune;
            let top_hz = key_hz(top);
            for (i, s) in self.singers.iter_mut().take(n).enumerate() {
                s.note_on(detuned_hz(top_hz, detune, i, n), top, Some(top as i32 + 1));
            }
        } else {
            for s in self.singers.iter_mut() {
                s.note_off();
            }
        }
    }

    fn choke(&mut self, key: u8) {
        self.held.retain(|k| *k != key);
        for s in self.singers.iter_mut() {
            if s.key == key && s.is_active() {
                self.ended.push((s.key, s.voice_id));
                s.choke();
                s.reported = true;
            }
        }
    }

    fn all_off(&mut self) {
        self.held.clear();
        for s in self.singers.iter_mut() {
            if s.is_active() && !s.reported {
                self.ended.push((s.key, s.voice_id));
            }
            s.choke();
            s.reported = true;
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        self.mono.clear();
        self.mono.resize(n, 0.0);

        // Every singer in the unison renders (to drain the overlap buffers after a note-off); the
        // releasing ones beyond it too. The gain rides 1/sqrt(count), smoothed per sample.
        for (v, s) in self.singers.iter_mut().enumerate() {
            if v >= self.unison && !s.is_active() {
                continue;
            }
            self.voice_buf.clear();
            self.voice_buf.resize(n, 0.0);
            s.process(&mut self.voice_buf, &self.tables);
            // The singer whose envelope finished, or whose tail drained past the grain, is done:
            // the host hears of it once.
            if s.finished && !s.reported {
                self.ended.push((s.key, s.voice_id));
                s.reported = true;
            } else if !s.is_active() && !s.reported {
                s.drain += n as u32;
                if s.drain > self.tables.grain_len as u32 + 1 {
                    self.ended.push((s.key, s.voice_id));
                    s.reported = true;
                }
            }
            for (m, x) in self.mono.iter_mut().zip(self.voice_buf.iter()) {
                *m += *x;
            }
        }

        let k = self.gain_coeff;
        for m in self.mono.iter_mut() {
            self.current_voice_gain += k * (self.target_voice_gain - self.current_voice_gain);
            *m *= self.current_voice_gain;
        }

        self.l_buf.clear();
        self.r_buf.clear();
        self.l_buf.resize(n, 0.0);
        self.r_buf.resize(n, 0.0);
        self.delay.process(&self.mono, &mut self.l_buf, &mut self.r_buf);

        // The output: the pitch compensation follows the glide, so it smooths the same way. The
        // 0.32 is the original's CC-volume headroom.
        let target_gain = pitch_amplitude(self.singers[0].current_pitch) * self.volume * 0.32;
        if self.current_out_gain < 0.0 {
            self.current_out_gain = target_gain;
        }
        for i in 0..n {
            self.current_out_gain += k * (target_gain - self.current_out_gain);
            left[i] += self.l_buf[i] * self.current_out_gain;
            right[i] += self.r_buf[i] * self.current_out_gain;
        }
    }

    fn take_ended(&mut self) -> Vec<(u8, Option<i32>)> {
        std::mem::take(&mut self.ended)
    }

    fn has_voices(&self) -> bool {
        self.singers.iter().any(|s| s.is_active())
    }
}

fn make(rate: u32, v: &[i32]) -> Box<dyn Instrument> {
    let rate = sanitize_rate(rate as f32);
    let mut m = Monk {
        tables: Tables::new(rate),
        singers: std::array::from_fn(|i| {
            let mut s = Singer::new(rate);
            // The singers stagger their vibrato phase and their jitter, so ten of them sound like
            // ten independent monks, not one voice times ten.
            s.vibrato_phase = SINE_TBL as f32 * i as f32 / MAX_UNISON as f32;
            s.rng_state = 12345 + (i as u32).wrapping_mul(7919);
            s
        }),
        unison: 1,
        unison_detune: 0.0,
        base_voice: 0.5,
        voice_spread: 0.0,
        last_base_hz: 220.0,
        held: Vec::new(),
        delay: MonkDelay::new(rate),
        gain_coeff: gain_smoothing_coeff(rate),
        current_voice_gain: 1.0,
        target_voice_gain: 1.0,
        current_out_gain: -1.0,
        volume: 1.0,
        mono: Vec::new(),
        voice_buf: Vec::new(),
        l_buf: Vec::new(),
        r_buf: Vec::new(),
        ended: Vec::new(),
    };
    m.delay.mix = 0.8;
    m.set(v);
    Box::new(m)
}

pub struct MonkKind;

impl Kind for MonkKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-monk";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's chanting monk (a port of MonkSynth)";
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioMonk\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type MangoMonk = SmallGen<MonkKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(VOWEL, 0), "A");
        assert_eq!(text(VOWEL, 128), "U");
        assert_eq!(parse(VOWEL, "o"), Some(96));
        assert_eq!(text(VIBRATO, 64), "50%");
        assert_eq!(parse(VIBRATO, "50%"), Some(64));
        assert_eq!(text(ATTACK, 0), "instant");
        assert_eq!(parse(ATTACK, "instant"), Some(0));
        assert_eq!(text(ATTACK, 128), "1.3s");
        assert_eq!(parse(ATTACK, "1.3s"), Some(128));
        assert_eq!(text(UNISON, 0), "1 singer");
        assert_eq!(text(UNISON, 9), "10 singers");
        assert_eq!(parse(UNISON, "10"), Some(9));
        assert_eq!(text(DETUNE, 64), "25.0ct");
        assert_eq!(parse(DETUNE, "25 ct"), Some(64));
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
        assert_eq!(time_seconds(0), 0.0);
        assert!((time_seconds(128) - 1.28).abs() < 1e-9);
        assert_eq!(unison_count(9), 10);
    }

    /// A note sings: the output is not silent and the singer reports its end after the release.
    #[test]
    fn a_note_sings_and_ends() {
        let mut m = make(44100, &SPEC.start());
        m.note_on(60, 1.0, Some(1));
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        let mut peak = 0.0f32;
        for _ in 0..50 {
            l.iter_mut().for_each(|x| *x = 0.0);
            r.iter_mut().for_each(|x| *x = 0.0);
            m.render(&mut l, &mut r);
            peak = peak.max(l.iter().fold(0.0f32, |a, x| a.max(x.abs())));
        }
        assert!(peak > 0.01, "the monk sings (peak {peak})");
        assert!(m.has_voices());
        m.note_off(60, Some(1));
        let mut blocks = 0;
        while m.has_voices() && blocks < 1000 {
            m.render(&mut l, &mut r);
            blocks += 1;
        }
        assert!(!m.has_voices(), "the release ends the note");
        assert_eq!(m.take_ended(), vec![(60, Some(1))]);
    }

    /// The singers: one by default, ten at the top; the detune spreads the outer ones off the
    /// note, the middle one of an odd count stays on it.
    #[test]
    fn the_unison_spreads() {
        assert_eq!(detuned_hz(220.0, 50.0, 0, 1), 220.0);
        assert!(detuned_hz(220.0, 50.0, 0, 10) < 220.0);
        assert!(detuned_hz(220.0, 50.0, 9, 10) > 220.0);
        assert_eq!(detuned_hz(220.0, 50.0, 4, 9), 220.0);
        assert!(detuned_hz(220.0, 50.0, 4, 10) < 220.0, "an even count has no middle singer");
        assert!(detuned_hz(220.0, 50.0, 5, 10) > 220.0);
    }

    /// The delay answers: the dry passes at once, and echoes come back, the left and the right at
    /// their own times.
    #[test]
    fn the_delay_echoes() {
        let mut d = MonkDelay::new(44100.0);
        d.mix = 1.0;
        d.feedback = 0.5;
        let input = vec![1.0f32, 0.0, 0.0];
        let mut l = vec![0.0f32; 3];
        let mut r = vec![0.0f32; 3];
        d.process(&input, &mut l, &mut r);
        assert_eq!((l[0], r[0]), (1.0, 1.0), "the dry passes at once");
        let mut l = vec![0.0f32; DELAY_LINE];
        let mut r = vec![0.0f32; DELAY_LINE];
        let zeros = vec![0.0f32; DELAY_LINE];
        d.process(&zeros, &mut l, &mut r);
        let echo_l = l.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        let echo_r = r.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!(echo_l > 0.1 && echo_r > 0.1, "echoes come back ({echo_l}, {echo_r})");
    }
}
