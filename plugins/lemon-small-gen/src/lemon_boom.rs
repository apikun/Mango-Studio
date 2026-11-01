// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Lemon Boom: Lemon Studio's bass drum (a recreation of FL's BassDrum) as a plugin. A kick with a longer tail and a
//! sustained body under the sweep: the body sine holds the base pitch under the hit, so the drum reads as a note too.
//! One voice, a new hit cuts the one before.
//!
// spec: plugins/lemon-boom.md, plugins/bassdrum.md

use nih_plug::prelude::*;
use os_ui::Rect;

use crate::fruit_kick::{
    BASE, DECAY, DROP, SWEEP, base_hz, time, time_parse, time_text, volume_gain,
};
use crate::params::{db_text, leading_number, nearest, nearest_rising, parse_db};
use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

/// The settings in stored order (spec §5).
pub const SETTINGS: [Setting; 9] = [
    Setting {
        id: "base",
        name: "Base",
        range: (0, 96),
        start: 20,
        magnet: None,
    },
    Setting {
        id: "drop",
        name: "Drop",
        range: (0, 36),
        start: 16,
        magnet: None,
    },
    Setting {
        id: "sweep",
        name: "Sweep",
        range: (0, 128),
        start: 48,
        magnet: None,
    },
    Setting {
        id: "decay",
        name: "Decay",
        range: (0, 128),
        start: 80,
        magnet: None,
    },
    Setting {
        id: "body",
        name: "Body",
        range: (0, 128),
        start: 64,
        magnet: None,
    },
    Setting {
        id: "click",
        name: "Click",
        range: (0, 64),
        start: 12,
        magnet: None,
    },
    Setting {
        id: "clickdecay",
        name: "Click decay",
        range: (0, 64),
        start: 10,
        magnet: None,
    },
    Setting {
        id: "drive",
        name: "Drive",
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

pub const BODY: usize = 4;
pub const CLICK: usize = 5;
pub const CLICK_DECAY: usize = 6;
pub const DRIVE: usize = 7;
pub const VOLUME: usize = 8;

/// Editor layout (spec §2): four cards, the tail's three knobs smaller.
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

const CONTROLS: [Control; 9] = [
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
        cx: 152.0,
        cy: 74.0,
        r: 17.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "decay",
        cx: 185.0,
        cy: 74.0,
        r: 17.0,
        caption: "",
        bipolar: false,
    },
    Control::Knob {
        id: "body",
        cx: 218.0,
        cy: 74.0,
        r: 17.0,
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
    name: "Mango Boom",
    word: "BOOM",
    about: "Mango Studio's bass drum",
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

/// The click's share as a percent.
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

/// The body's level under the sweep: square law over the knob.
pub fn body_level(v: i32) -> f32 {
    (f64::from(v) / 128.0).powi(2) as f32
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    match k {
        BASE => format!("{:.1}Hz", base_hz(v)),
        DROP => format!("+{v}st"),
        SWEEP => time_text(time(v, 0.002)),
        DECAY => time_text(time(v, 0.01)),
        BODY => percent(SETTINGS[BODY].range, v),
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
        BASE => leading_number(&low)
            .map(|x| nearest_rising(SETTINGS[BASE].range, x, |v| f64::from(base_hz(v)))),
        DROP => leading_number(&low)
            .and_then(|x| nearest(SETTINGS[DROP].range, x, |v| Some(f64::from(v)))),
        SWEEP => time_parse(SETTINGS[SWEEP].range, &low, 0.002),
        DECAY => time_parse(SETTINGS[DECAY].range, &low, 0.01),
        BODY => percent_parse(SETTINGS[BODY].range, &low),
        CLICK => percent_parse(SETTINGS[CLICK].range, &low),
        CLICK_DECAY => time_parse(SETTINGS[CLICK_DECAY].range, &low, 0.0005),
        DRIVE => percent_parse(SETTINGS[DRIVE].range, &low),
        _ => parse_db(SETTINGS[VOLUME].range, &low, volume_gain),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The sound

struct Boom {
    rate: f32,
    base_hz: f32,
    drop: f32,
    sweep: f32,
    decay: f32,
    body: f32,
    click: f32,
    click_decay: f32,
    drive: f32,
    gain: f32,
    key: u8,
    voice: Option<i32>,
    t: f32,
    phase: f32,
    body_phase: f32,
    click_t: f32,
    noise: u32,
    vel: f32,
    ended: Vec<(u8, Option<i32>)>,
}

impl Boom {
    fn hit(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        if self.voice.is_some() {
            self.ended.push((self.key, self.voice));
        }
        self.key = key;
        self.voice = Some(voice_id.unwrap_or(key as i32 + 1));
        self.vel = velocity.clamp(0.05, 1.0);
        self.t = 0.0;
        self.click_t = 0.0;
        self.phase = 0.0;
        self.body_phase = 0.0;
    }
}

fn noise(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

impl Instrument for Boom {
    fn set(&mut self, v: &[i32]) {
        self.base_hz = base_hz(v[BASE]);
        self.drop = v[DROP] as f32 / 12.0;
        self.sweep = time(v[SWEEP], 0.002);
        self.decay = time(v[DECAY], 0.01);
        self.body = body_level(v[BODY]);
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
        self.body_phase = 0.0;
        self.click_t = 0.0;
    }

    fn note_on(&mut self, key: u8, velocity: f32, voice_id: Option<i32>) {
        self.hit(key, velocity, voice_id);
    }

    fn note_off(&mut self, _key: u8, _voice_id: Option<i32>) {
        // The tail plays out (spec §1).
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
            let f =
                self.base_hz * (1.0 + (2f32.powf(self.drop) - 1.0) * (-self.t / self.sweep).exp());
            self.phase += std::f32::consts::TAU * f * dt;
            if self.phase >= std::f32::consts::TAU {
                self.phase -= std::f32::consts::TAU;
            }
            // The body: a sine that holds the base pitch under the hit, its own decay twice the tail's.
            self.body_phase += std::f32::consts::TAU * self.base_hz * dt;
            if self.body_phase >= std::f32::consts::TAU {
                self.body_phase -= std::f32::consts::TAU;
            }
            let attack = (self.t / 0.0005).min(1.0);
            let tail = (-self.t / self.decay).exp();
            let body = self.body * self.body_phase.sin() * (-self.t / (self.decay * 2.0)).exp();
            let click =
                self.click * (-self.click_t / self.click_decay).exp() * noise(&mut self.noise);
            self.t += dt;
            self.click_t += dt;
            let s = self.phase.sin() * tail * attack + body + click;
            let g = 1.0 + 4.0 * self.drive;
            let s = (g * s).tanh() / g;
            let out = s * self.gain * self.vel;
            *l += out;
            *r += out;
            if tail * attack < 1e-4 && self.click_t > self.click_decay * 8.0 {
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
    let mut b = Boom {
        rate: rate as f32,
        base_hz: base_hz(v[BASE]),
        drop: v[DROP] as f32 / 12.0,
        sweep: time(v[SWEEP], 0.002),
        decay: time(v[DECAY], 0.01),
        body: body_level(v[BODY]),
        click: v[CLICK] as f32 / 64.0,
        click_decay: time(v[CLICK_DECAY], 0.0005),
        drive: v[DRIVE] as f32 / 128.0,
        gain: volume_gain(v[VOLUME]),
        key: 0,
        voice: None,
        t: 0.0,
        phase: 0.0,
        body_phase: 0.0,
        click_t: 0.0,
        noise: 0x8765_4321,
        vel: 1.0,
        ended: Vec::new(),
    };
    b.set_sample_rate(rate);
    Box::new(b)
}

pub struct BoomKind;

impl Kind for BoomKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.lemon-boom";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's bass drum";
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
    const VST3_CLASS_ID: [u8; 16] = *b"LemonStudioBsDr!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

pub type LemonBoom = SmallGen<BoomKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(BASE, 20), "42.5Hz");
        assert_eq!(parse(BASE, "42.5Hz"), Some(20));
        assert_eq!(text(BODY, 64), "50%");
        assert_eq!(parse(BODY, "50%"), Some(64));
        assert_eq!(text(VOLUME, 256), "0.0dB");
        assert_eq!(parse(VOLUME, "0 dB"), Some(256));
    }

    /// The body holds the base pitch under the sweep: the crossings early in the note (up at the drop) and half a
    /// second in, when the tail and the body both sit on the base.
    #[test]
    fn the_body_holds_the_base_pitch() {
        let mut v = crate::params::default_state(SPEC);
        crate::params::set_setting(SPEC, &mut v, SWEEP, 112);
        crate::params::set_setting(SPEC, &mut v, DECAY, 128);
        crate::params::set_setting(SPEC, &mut v, CLICK, 0);
        let mut b = make(44100, &crate::params::values_of(SPEC, &v));
        let mut l = vec![0.0f32; 44100];
        let mut r = vec![0.0f32; 44100];
        b.note_on(48, 1.0, None);
        b.render(&mut l, &mut r);
        let zero_crossings = |s: &[f32]| s.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        let early = zero_crossings(&l[..882]);
        let late = zero_crossings(&l[22050..]);
        assert!(
            early >= 2,
            "the sweep starts above the base: {early} crossings in the first 20 ms"
        );
        // Half a second at the base of a 42.5 Hz note: about 21 up-crossings.
        assert!(
            late.abs_diff(21) <= 2,
            "the pitch lands on the base: {late}"
        );
    }
}
