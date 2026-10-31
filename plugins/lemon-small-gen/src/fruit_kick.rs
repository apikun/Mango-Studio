// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Lemon Kick: Lemon Studio's kick drum (a recreation of FL's Fruit Kick) as a plugin. A sine that sweeps from its
//! drop down to the base pitch over the sweep time, a noise click, a drive and the tail's decay: one voice, a new hit
//! cuts the one before.
//!
// spec: plugins/lemon-kick.md, plugins/fruit-kick.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::params::{db_text, leading_number, nearest, nearest_rising, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

/// The settings in stored order (spec §5).
pub const SETTINGS: [Setting; 8] = [
    Setting {
        id: "base",
        name: "Base",
        range: (0, 96),
        start: 24,
        magnet: None,
    },
    Setting {
        id: "drop",
        name: "Drop",
        range: (0, 36),
        start: 12,
        magnet: None,
    },
    Setting {
        id: "sweep",
        name: "Sweep",
        range: (0, 128),
        start: 40,
        magnet: None,
    },
    Setting {
        id: "decay",
        name: "Decay",
        range: (0, 128),
        start: 64,
        magnet: None,
    },
    Setting {
        id: "click",
        name: "Click",
        range: (0, 64),
        start: 20,
        magnet: None,
    },
    Setting {
        id: "clickdecay",
        name: "Click decay",
        range: (0, 64),
        start: 12,
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
        id: "volume",
        name: "Volume",
        range: (0, 512),
        start: 256,
        magnet: Some(256),
    },
];

pub const BASE: usize = 0;
pub const DROP: usize = 1;
pub const SWEEP: usize = 2;
pub const DECAY: usize = 3;
pub const CLICK: usize = 4;
pub const CLICK_DECAY: usize = 5;
pub const DRIVE: usize = 6;
pub const VOLUME: usize = 7;

/// Editor layout (spec §2): four cards of two knobs each.
const CARDS: [Card; 4] = [
    Card {
        x0: 8.0,
        x1: 122.0,
        y0: 28.0,
        y1: 124.0,
        heading: "PITCH",
    },
    Card {
        x0: 128.0,
        x1: 242.0,
        y0: 28.0,
        y1: 124.0,
        heading: "TAIL",
    },
    Card {
        x0: 248.0,
        x1: 362.0,
        y0: 28.0,
        y1: 124.0,
        heading: "CLICK",
    },
    Card {
        x0: 368.0,
        x1: 482.0,
        y0: 28.0,
        y1: 124.0,
        heading: "OUTPUT",
    },
];

const CONTROLS: [Control; 8] = [
    Control::Knob {
        id: "base",
        cx: 40.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "drop",
        cx: 90.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "sweep",
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
        id: "click",
        cx: 280.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "clickdecay",
        cx: 330.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "drive",
        cx: 400.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "volume",
        cx: 450.0,
        cy: 74.0,
        r: 24.0,
        caption: "",
        bipolar: false,
    },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Kick",
    word: "KICK",
    about: "Mango Studio's kick drum",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (490, 162),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark {
        x: 482.0,
        y: 143.0,
        anchor: 1.0,
        stacked: false,
    },
    hint: Some(Rect::new(8.0, 132.0, 474.0, 22.0)),
    labels: &[],
    text,
    parse,
    make,
};

// ---------------------------------------------------------------------------------------------------------------
// Laws (spec §3)

/// The base pitch's Hz: 30..160 Hz over the knob's range, in even steps of a log.
pub fn base_hz(v: i32) -> f32 {
    (30.0f64 * (160.0f64 / 30.0f64).powf(f64::from(v) / 96.0)) as f32
}

/// The seconds of a time knob: 2 ms..512 ms (the sweep, the click's decay half of it) or 10 ms..1.28 s (the decay).
pub fn time(v: i32, min: f32) -> f32 {
    min * (v as f32 / 16.0).exp2()
}

/// The value of a time knob whose seconds are nearest.
pub fn time_parse(range: (i32, i32), s: &str, min: f32) -> Option<i32> {
    let x = leading_number(s)?;
    let x = if s.to_ascii_lowercase().contains("ms") {
        x / 1000.0
    } else {
        x
    };
    nearest(range, x, |v| Some(f64::from(time(v, min))))
}

/// A time as ms under a second, else s.
pub fn time_text(t: f32) -> String {
    if t < 1.0 {
        format!("{:.1}ms", t * 1000.0)
    } else {
        format!("{:.2}s", t)
    }
}

/// The share of a 0..n knob as a percent.
fn percent(range: (i32, i32), v: i32) -> String {
    format!(
        "{:.0}%",
        f64::from(v - range.0) / f64::from(range.1 - range.0) * 100.0
    )
}

/// The value of a 0..n knob a percent names.
fn percent_parse(range: (i32, i32), s: &str) -> Option<i32> {
    let x = leading_number(s)?;
    nearest(range, x, |v| {
        Some(f64::from(v - range.0) / f64::from(range.1 - range.0) * 100.0)
    })
}

/// The volume's gain: square law, unity at 256.
pub fn volume_gain(v: i32) -> f32 {
    (f64::from(v) / 256.0).powi(2) as f32
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        BASE => format!("{:.1}Hz", base_hz(v)),
        DROP => format!("+{v}st"),
        SWEEP => time_text(time(v, 0.002)),
        DECAY => time_text(time(v, 0.01)),
        CLICK => percent(SETTINGS[CLICK].range, v),
        CLICK_DECAY => time_text(time(v, 0.0005)),
        DRIVE => percent(SETTINGS[DRIVE].range, v),
        _ => db_text(volume_gain(v)),
    }
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    let low = s.trim().to_ascii_lowercase();
    match k {
        BASE => {
            let x = leading_number(&low)?;
            Some(nearest_rising(SETTINGS[BASE].range, x, |v| {
                f64::from(base_hz(v))
            }))
        }
        DROP => leading_number(&low)
            .and_then(|x| nearest(SETTINGS[DROP].range, x, |v| Some(f64::from(v)))),
        SWEEP => time_parse(SETTINGS[SWEEP].range, &low, 0.002),
        DECAY => time_parse(SETTINGS[DECAY].range, &low, 0.01),
        CLICK => percent_parse(SETTINGS[CLICK].range, &low),
        CLICK_DECAY => time_parse(SETTINGS[CLICK_DECAY].range, &low, 0.0005),
        DRIVE => percent_parse(SETTINGS[DRIVE].range, &low),
        _ => parse_db(SETTINGS[VOLUME].range, &low, volume_gain),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The sound

struct Kick {
    rate: f32,
    base_hz: f32,
    drop: f32,
    sweep: f32,
    decay: f32,
    click: f32,
    click_decay: f32,
    drive: f32,
    gain: f32,
    /// The one voice: its key, id and where it is (seconds since the hit, the oscillator's phase, the click's).
    key: u8,
    voice: Option<i32>,
    t: f32,
    phase: f32,
    click_t: f32,
    noise: u32,
    vel: f32,
    ended: Vec<(u8, Option<i32>)>,
}

impl Kick {
    fn hit(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        // A new hit cuts the one before (mono, spec §1).
        if self.voice.is_some() {
            self.ended.push((self.key, self.voice));
        }
        self.key = key;
        self.voice = Some(voice_id.unwrap_or(key as i32 + 1));
        self.vel = velocity.clamp(0.05, 1.0);
        self.t = 0.0;
        self.click_t = 0.0;
        self.phase = 0.0;
    }
}

fn noise(state: &mut u32) -> f32 {
    // xorshift32, ±1.
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

impl Instrument for Kick {
    fn set(&mut self, v: &[i32]) {
        self.base_hz = base_hz(v[BASE]);
        self.drop = v[DROP] as f32 / 12.0;
        self.sweep = time(v[SWEEP], 0.002);
        self.decay = time(v[DECAY], 0.01);
        self.click = v[CLICK] as f32 / 64.0;
        self.click_decay = time(v[CLICK_DECAY], 0.0005);
        self.drive = v[DRIVE] as f32 / 128.0;
        self.gain = volume_gain(v[VOLUME]);
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate as f32;
    }

    fn reset(&mut self) {
        self.voice = None;
        self.t = 0.0;
        self.phase = 0.0;
        self.click_t = 0.0;
    }

    fn note_on(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        self.hit(key, velocity, voice_id);
    }

    fn note_off(&mut self, _key: u8, _voice_id: Option<i32>) {
        // A kick does not release: the tail plays out (spec §1).
    }

    fn choke(&mut self, key: u8) {
        if self.voice.is_some() && self.key == key {
            self.ended.push((self.key, self.voice));
            self.voice = None;
        }
    }

    fn all_off(&mut self) {
        if let Some(v) = self.voice.take() {
            self.ended.push((self.key, Some(v)));
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.voice.is_none() {
            return;
        }
        let dt = 1.0 / self.rate;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            // The pitch: the base, up by the drop at the hit, gliding back over the sweep time.
            let f =
                self.base_hz * (1.0 + (2f32.powf(self.drop) - 1.0) * (-self.t / self.sweep).exp());
            self.phase += std::f32::consts::TAU * f * dt;
            if self.phase >= std::f32::consts::TAU {
                self.phase -= std::f32::consts::TAU;
            }
            // The body: a short attack so the note starts without a step, then the decay.
            let attack = (self.t / 0.0005).min(1.0);
            let body = self.phase.sin() * (-self.t / self.decay).exp() * attack;
            // The click: a burst of noise over its own decay.
            let click =
                self.click * (-self.click_t / self.click_decay).exp() * noise(&mut self.noise);
            self.t += dt;
            self.click_t += dt;
            let s = body + click;
            // The drive: a soft clip that bites harder as the knob turns.
            let g = 1.0 + 4.0 * self.drive;
            let s = (g * s).tanh() / g;
            let out = s * self.gain * self.vel;
            *l += out;
            *r += out;
            if self.t > self.decay * 8.0
                || (-self.t / self.decay).exp() * attack < 1e-4
                    && self.click_t > self.click_decay * 8.0
            {
                self.ended.push((self.key, self.voice));
                self.voice = None;
                break;
            }
        }
    }

    fn take_ended(&mut self) -> Vec<(u8, Option<i32>)> {
        std::mem::take(&mut self.ended)
    }

    fn has_voices(&self) -> bool {
        self.voice.is_some()
    }
}

fn make(rate: u32, v: &[i32]) -> Box<dyn Instrument> {
    let mut k = Kick {
        rate: rate as f32,
        base_hz: base_hz(v[BASE]),
        drop: v[DROP] as f32 / 12.0,
        sweep: time(v[SWEEP], 0.002),
        decay: time(v[DECAY], 0.01),
        click: v[CLICK] as f32 / 64.0,
        click_decay: time(v[CLICK_DECAY], 0.0005),
        drive: v[DRIVE] as f32 / 128.0,
        gain: volume_gain(v[VOLUME]),
        key: 0,
        voice: None,
        t: 0.0,
        phase: 0.0,
        click_t: 0.0,
        noise: 0x1234_5678,
        vel: 1.0,
        ended: Vec::new(),
    };
    k.set_sample_rate(rate);
    Box::new(k)
}

pub struct KickKind;

impl Kind for KickKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.lemon-kick";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's kick drum";
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
    const VST3_CLASS_ID: [u8; 16] = *b"LemonStudioFrKck";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type LemonKick = SmallGen<KickKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(BASE, 24), "45.6Hz");
        assert_eq!(parse(BASE, "45.6Hz"), Some(24));
        assert_eq!(text(DROP, 12), "+12st");
        assert_eq!(parse(DROP, "12"), Some(12));
        assert_eq!(text(SWEEP, 40), "11.3ms");
        assert_eq!(parse(SWEEP, "11.3ms"), Some(40));
        assert_eq!(
            parse(SWEEP, "0.01s"),
            parse(SWEEP, "10ms"),
            "a value in s reads too"
        );
        assert_eq!(text(CLICK, 32), "50%");
        assert_eq!(parse(CLICK, "50%"), Some(32));
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
    }

    /// A hit sweeps down from the drop and ends; a second hit cuts the first.
    #[test]
    fn one_voice_a_new_hit_cuts_the_one_before() {
        let mut k = make(44100, &SPEC.start());
        let mut l = vec![0.0f32; 256];
        let mut r = vec![0.0f32; 256];
        k.note_on(48, 1.0, Some(7));
        k.render(&mut l, &mut r);
        assert!(l.iter().any(|x| x.abs() > 0.01));
        k.note_on(48, 1.0, Some(8));
        assert_eq!(k.take_ended(), vec![(48, Some(7))], "the first voice ended");
        while k.has_voices() {
            l.iter_mut().for_each(|x| *x = 0.0);
            k.render(&mut l, &mut r);
        }
        assert_eq!(
            k.take_ended(),
            vec![(48, Some(8))],
            "the second voice ends when its tail is over"
        );
        l.iter_mut().for_each(|x| *x = 0.0);
        k.render(&mut l, &mut r);
        assert!(l.iter().all(|x| x.abs() < 1e-4), "silence after the tail");
    }

    /// The base knob's pitch: a low key and a high base differ by the sweep's drop only at the start.
    #[test]
    fn the_pitch_lands_on_the_base() {
        let mut v = crate::params::default_state(SPEC);
        crate::params::set_setting(SPEC, &mut v, SWEEP, 128);
        crate::params::set_setting(SPEC, &mut v, DECAY, 128);
        let mut k = make(44100, &crate::params::values_of(SPEC, &v));
        let mut l = vec![0.0f32; 4410];
        let mut r = vec![0.0f32; 4410];
        k.note_on(48, 1.0, None);
        k.render(&mut l, &mut r);
        let zero_crossings = |s: &[f32]| s.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        let early = zero_crossings(&l[..441]);
        let late = zero_crossings(&l[3969..]);
        assert!(
            early > late * 2,
            "the pitch sweeps down: {early} crossings early, {late} late"
        );
    }
}
