// Copyright (C) 2026 mikobuntu
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango 12: the colour of a classic 12-bit sampler as a small effect. One sound: the input is gained into a
//! 26.04 kHz, 12-bit converter that samples it without an anti-alias filter (the aliasing is mandatory) and leaves
//! through a soft-saturating output stage, with an optional sixth-order Butterworth low pass in front of the
//! converter (`99` = off) for the filter-into-the-sampler workflow. The tune fader detunes the converter's clock the
//! way the original's tune knob pitches a source and compensates it: pitch and duration never change, only the
//! aliasing and the bandwidth do. The fader's marks are turntable speeds relative to 33 (45, x2 and 78); `0.1 ST`
//! steps it in tenths of a semitone where `1 ST` steps whole ones, and `Ext.` reaches twice as deep, to -30 ST. The
//! route picks what the converter hears (the stereo pair, a mono sum, one channel, the mid or the side — what it
//! does not hear passes as it is), the gains set how hard it is hit and how loud it leaves (`Link` holds their
//! total at the gain out, and while it is on the out knob shows the effective output gain, so it follows the in
//! knob), and the crush sets how the staircase leaves the converter: `Square` holds each 12-bit sample flat until
//! the next tick (the raw converter, aliasing and all), `Round` eases a sine between each pair of points one clock
//! behind — the same staircase read smoothly at a constant delay, which analysers measure steadily. Then the mix.
//! Not a recreation of an FL plugin: its state is its own (modelled on the
//! E-mu SP-1200's converter and output stage, in the spirit of Wave Tracing's SP-950).

use nih_plug::prelude::*;
use os_theme::design::lemon as brand;
use os_ui::kit::lemon::plug;
use os_ui::{Canvas, Font, Rect};

use crate::params::{leading_number, BYTE};
use crate::{Card, Control, Display, Kind, Mark, Setting, SmallFx, Sound, Spec};

pub const TUNE: usize = 0;
pub const FINE: usize = 1;
pub const EXT: usize = 2;
pub const IN: usize = 3;
pub const OUT: usize = 4;
pub const LINK: usize = 5;
pub const FILTER: usize = 6;
pub const ROUTE: usize = 7;
pub const MIX: usize = 8;
pub const CRUSH: usize = 9;

/// The routes, in the switch's order: what the converter hears.
pub const STEREO: i32 = 0;
pub const MONO_SUM: i32 = 1;
pub const MONO_L: i32 = 2;
pub const MONO_R: i32 = 3;
pub const STEREO_L: i32 = 4;
pub const STEREO_R: i32 = 5;
pub const MID: i32 = 6;
pub const SIDE: i32 = 7;

pub const ROUTE_NAMES: &[&str] = &["Stereo", "Mono Sum", "Mono L", "Mono R", "Stereo L", "Stereo R", "Mid", "Side"];

/// The crush modes: how the converter's staircase leaves it.
pub const SQUARE: i32 = 0;
pub const ROUND: i32 = 1;

pub const SETTINGS: [Setting; 10] = [
    // Tenths of a semitone of detune, down from the 33 mark (0) to 78 (150); `Ext.` doubles it.
    Setting { id: "tune", name: "Tune", range: (0, 150), start: 0, magnet: Some(0) },
    Setting { id: "fine", name: "Fine", range: (0, 1), start: 0, magnet: None },
    Setting { id: "ext", name: "Range", range: (0, 1), start: 0, magnet: None },
    // Tenths of a dB.
    Setting { id: "in", name: "Gain In", range: (-120, 120), start: 0, magnet: Some(0) },
    Setting { id: "out", name: "Gain Out", range: (-120, 120), start: 0, magnet: Some(0) },
    Setting { id: "link", name: "Link", range: (0, 1), start: 0, magnet: None },
    // 99 = the filter is bypassed.
    Setting { id: "filter", name: "Filter", range: (0, 99), start: 99, magnet: Some(99) },
    Setting { id: "route", name: "Route", range: (0, 7), start: STEREO, magnet: None },
    Setting { id: "mix", name: "Mix", range: (0, 100), start: 100, magnet: None },
    // A switch stored in the byte after the settings' ints, so sessions saved before it keep reading (as Square).
    Setting { id: "crush", name: "Crush", range: (0, 1), start: SQUARE, magnet: None },
];

const CARDS: [Card; 5] = [
    Card { x0: 8.0, x1: 148.0, y0: 28.0, y1: 190.0, heading: "TUNE" },
    Card { x0: 156.0, x1: 264.0, y0: 28.0, y1: 178.0, heading: "ROUTE" },
    Card { x0: 272.0, x1: 428.0, y0: 28.0, y1: 104.0, heading: "GAIN" },
    Card { x0: 272.0, x1: 428.0, y0: 126.0, y1: 190.0, heading: "FILTER" },
    Card { x0: 436.0, x1: 612.0, y0: 28.0, y1: 187.0, heading: "MIX" },
];

const CONTROLS: [Control; 10] = [
    Control::Fader {
        id: "tune",
        x: 104.0,
        top: 52.0,
        bot: 138.0,
        label: "TUNE",
        col: brand::MAIN,
        bipolar: false,
        // 33 at the bottom, 45, x2 and 78 up the travel (turntable speeds relative to 33).
        marks: &[0.0, 0.347, 0.8, 0.98],
    },
    Control::Choice { id: "fine", x: 20.0, top: 150.0, names: &["1 ST", "0.1 ST"] },
    Control::Choice { id: "ext", x: 88.0, top: 150.0, names: &["Int.", "Ext."] },
    Control::Choice { id: "route", x: 172.0, top: 44.0, names: ROUTE_NAMES },
    Control::Knob { id: "in", cx: 306.0, cy: 50.0, r: 16.0, caption: "IN", bipolar: true },
    Control::Knob { id: "out", cx: 352.0, cy: 50.0, r: 16.0, caption: "OUT", bipolar: true },
    Control::Choice { id: "link", x: 384.0, top: 44.0, names: &["Off", "Link"] },
    Control::Knob { id: "filter", cx: 350.0, cy: 150.0, r: 16.0, caption: "FILTER", bipolar: false },
    Control::Switch { id: "crush", cx: 300.0, cy: 148.0 },
    Control::Knob { id: "mix", cx: 524.0, cy: 152.0, r: 15.0, caption: "MIX", bipolar: false },
];

pub const SPEC: &Spec = &Spec {
    name: "Mango 12",
    word: "12",
    about: "Mango Studio's 12-bit sampler colour",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    // The crush is the byte after the settings' ints (a switch, read as Square from states without it).
    fields: &[0, 1, 2, 3, 4, 5, 6, 7, 8, BYTE],
    tail: &[0],
    blank: None,
    locate: None,
    size: (620, 214),
    cards: &CARDS,
    controls: &CONTROLS,
    display: Some(Display { r: Rect::new(444.0, 36.0, 160.0, 92.0), draw: screen }),
    mark: Mark { x: 612.0, y: 201.0, anchor: 1.0, stacked: false },
    hint: None,
    // The fader's turntable speeds, beside their ticks, and the crush switch's caption.
    labels: &[
        (83.0, 47.0, "78"),
        (83.0, 63.0, "X2"),
        (83.0, 102.0, "45"),
        (83.0, 132.0, "33"),
        (300.0, 162.0, "CRUSH"),
    ],
    text,
    parse,
    shown: Some(shown),
    sound,
    rings: false,
};

fn text(k: usize, v: i32) -> String {
    match k {
        TUNE if v == 0 => "0.0ST".into(),
        TUNE => format!("-{:.1}ST", f64::from(v) / 10.0),
        FINE => ["1 ST", "0.1 ST"][v.clamp(0, 1) as usize].into(),
        EXT => ["Int.", "Ext."][v.clamp(0, 1) as usize].into(),
        IN | OUT => format!("{:.1}dB", f64::from(v) / 10.0),
        LINK => ["Off", "Link"][v.clamp(0, 1) as usize].into(),
        FILTER if v >= 99 => "Off".into(),
        FILTER => format!("{v}"),
        ROUTE => ROUTE_NAMES[v.clamp(0, 7) as usize].into(),
        CRUSH => ["Square", "Round"][v.clamp(0, 1) as usize].into(),
        _ => format!("{v}%"),
    }
}

fn parse(k: usize, s: &str) -> Option<i32> {
    let range = SETTINGS[k].range;
    let s = s.trim();
    match k {
        TUNE => {
            let v = (-leading_number(s)? * 10.0).round() as i32;
            Some(v.clamp(range.0, range.1))
        }
        FINE => match s.to_ascii_lowercase().as_str() {
            "1 st" | "1" => Some(0),
            "0.1 st" | "0.1" => Some(1),
            _ => None,
        },
        EXT => match s.to_ascii_lowercase().trim_end_matches('.') {
            "int" => Some(0),
            "ext" => Some(1),
            _ => None,
        },
        IN | OUT => {
            let v = (leading_number(s)? * 10.0).round() as i32;
            Some(v.clamp(range.0, range.1))
        }
        LINK => match s.to_ascii_lowercase().as_str() {
            "off" => Some(0),
            "link" => Some(1),
            _ => None,
        },
        FILTER if s.eq_ignore_ascii_case("off") => Some(99),
        FILTER => {
            let v = leading_number(s)?.round() as i32;
            Some(v.clamp(range.0, range.1))
        }
        ROUTE => ROUTE_NAMES.iter().position(|m| s.eq_ignore_ascii_case(m)).map(|i| i as i32),
        CRUSH => match s.to_ascii_lowercase().trim_end_matches('.') {
            "square" | "sq" => Some(SQUARE),
            "round" | "rd" => Some(ROUND),
            _ => None,
        },
        _ => {
            let v = leading_number(s)?.round() as i32;
            Some(v.clamp(range.0, range.1))
        }
    }
}

/// While the gains are linked the out knob shows the effective output gain: the stored out less the in, what the
/// sound's output stage runs at while the total stays at the gain out. The offset is the in, so the knob follows the
/// in knob, and editing it sets the effective gain (the stored total moves by the same amount). Every other setting
/// shows its stored value.
fn shown(v: &[i32], k: usize) -> Option<i32> {
    match k {
        OUT if v.get(LINK) == Some(&1) => Some(v.get(IN).copied().unwrap_or(SETTINGS[IN].start)),
        _ => None,
    }
}

/// The converter's clock.
const CLOCK: f64 = 26_040.0;

/// The detune the sound runs at, in tenths of a semitone: the tune, doubled by `Ext.`, snapped to whole semitones
/// unless the `0.1 ST` steps are on. The pitch and the duration never follow it; only the clock does.
fn detune_tenths(v: &[i32]) -> f64 {
    let get = |k: usize| v.get(k).copied().unwrap_or(SETTINGS[k].start).clamp(SETTINGS[k].range.0, SETTINGS[k].range.1);
    let mut d = f64::from(get(TUNE)) * f64::from(1 + get(EXT));
    if get(FINE) == 0 {
        d = (d / 10.0).round() * 10.0;
    }
    d
}

/// The converter's 12 bits: round to one of 4096 levels across full scale (and clip there, as the ADC does).
fn quantize(x: f64) -> f32 {
    ((x * 2048.0).round().clamp(-2048.0, 2047.0) / 2048.0) as f32
}

/// The round crush's easing between two held points: a sine between each pair, flat-footed at both (so the
/// staircase's corners round off without its levels moving).
fn crush_ease(p: f64) -> f64 {
    (1.0 - (std::f64::consts::PI * p).cos()) / 2.0
}

/// The output stage: a soft knee that stays out of the way at the levels the original likes (peaks near -10 dBFS)
/// and grips when the gain pushes.
fn saturate(x: f32) -> f32 {
    (0.75 * x).tanh() / 0.75
}

/// The filter's cutoff for a knob value (`rate` keeps it under half the rate).
fn cutoff(k: i32, rate: f64) -> f64 {
    (100.0 * 130.2f64.powf(f64::from(k) / 98.0)).min(rate * 0.45)
}

/// A sixth-order Butterworth low pass as three biquad sections (b0, b1, b2, a1, a2) at its Qs.
fn low_pass(f: f64, rate: f64) -> [[f64; 5]; 3] {
    const Q: [f64; 3] = [0.517_638_090_205_041_5, std::f64::consts::FRAC_1_SQRT_2, 1.931_851_652_578_136_6];
    let w = std::f64::consts::TAU * f / rate;
    let (s, c) = w.sin_cos();
    Q.map(|q| {
        let a = s / (2.0 * q);
        let b = 1.0 - c;
        let a0 = 1.0 + a;
        [b / 2.0 / a0, b / a0, b / 2.0 / a0, -2.0 * c / a0, (1.0 - a) / a0]
    })
}

/// One biquad section (transposed direct form II is not needed: the state is four numbers).
fn section(x: f64, c: &[f64; 5], z: &mut [f64; 4]) -> f64 {
    let y = c[0] * x + c[1] * z[0] + c[2] * z[1] - c[3] * z[2] - c[4] * z[3];
    z[1] = z[0];
    z[0] = x;
    z[3] = z[2];
    z[2] = y;
    y
}

/// One converter lane: the filter's memory, the clock's phase, the input it just saw and the samples it holds.
struct Lane {
    taps: [[f64; 4]; 3],
    phase: f64,
    /// The converter's input now (`d0`) and one sample ago (`d1`), so a tick samples its exact moment.
    d0: f64,
    d1: f64,
    /// The held sample and the one before it (the round crush eases between the two).
    held: f32,
    prev: f32,
}

impl Default for Lane {
    fn default() -> Self {
        Lane { taps: [[0.0; 4]; 3], phase: 0.0, d0: 0.0, d1: 0.0, held: 0.0, prev: 0.0 }
    }
}

/// The sound: the SP's converter and output stage, set from the settings.
pub struct Twelve {
    rate: f64,
    route: i32,
    mix: f32,
    in_gain: f32,
    out_gain: f32,
    /// Host samples per clock tick.
    inc: f64,
    detune: f64,
    filter_k: i32,
    coeffs: [[f64; 5]; 3],
    filtering: bool,
    /// The round crush: the staircase leaves eased by a sine between its points, at a constant delay.
    round: bool,
    lanes: [Lane; 2],
}

impl Twelve {
    pub fn new(rate: u32, v: &[i32]) -> Self {
        let mut t = Twelve {
            rate: f64::from(rate),
            route: STEREO,
            mix: 1.0,
            in_gain: 1.0,
            out_gain: 1.0,
            inc: CLOCK / f64::from(rate),
            detune: 0.0,
            filter_k: 99,
            coeffs: low_pass(13_020.0, f64::from(rate)),
            filtering: false,
            round: false,
            lanes: [Lane::default(), Lane::default()],
        };
        t.set(v);
        t
    }

    pub fn set(&mut self, v: &[i32]) {
        let get = |k: usize| v.get(k).copied().unwrap_or(SETTINGS[k].start).clamp(SETTINGS[k].range.0, SETTINGS[k].range.1);
        self.route = get(ROUTE);
        self.mix = get(MIX) as f32 / 100.0;
        self.in_gain = 10f32.powf(get(IN) as f32 / 200.0);
        // Linked: the output gain follows the input gain down, so the total stays at the gain out (each knob keeps
        // to its own range).
        let out = if get(LINK) == 1 { (get(OUT) - get(IN)).clamp(-120, 120) } else { get(OUT) };
        self.out_gain = 10f32.powf(out as f32 / 200.0);
        self.detune = detune_tenths(v);
        self.inc = CLOCK * 2f64.powf(-self.detune / 120.0) / self.rate;
        self.filter_k = get(FILTER);
        self.filtering = self.filter_k != 99;
        self.round = get(CRUSH) == ROUND;
        if self.filtering {
            self.coeffs = low_pass(cutoff(self.filter_k, self.rate), self.rate);
        }
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.rate = f64::from(rate);
        self.inc = CLOCK * 2f64.powf(-self.detune / 120.0) / self.rate;
        if self.filtering {
            self.coeffs = low_pass(cutoff(self.filter_k, self.rate), self.rate);
        }
    }

    /// One sample through lane `i`'s filter, clock and output stage (before the output gain).
    fn lane(&mut self, i: usize, x: f32) -> f32 {
        let mut s = f64::from(x * self.in_gain);
        if self.filtering {
            for (c, z) in self.coeffs.iter().zip(self.lanes[i].taps.iter_mut()) {
                s = section(s, c, z);
            }
        }
        let l = &mut self.lanes[i];
        l.d1 = l.d0;
        l.d0 = s;
        l.phase += self.inc;
        if l.phase >= 1.0 {
            // The clock ticked a fraction of a sample back through the input: sample it there, exactly, so the
            // held values land on the ticks they belong to instead of the host sample that noticed them.
            let back = ((l.phase - 1.0) / self.inc).clamp(0.0, 1.0);
            l.prev = l.held;
            l.held = quantize(l.d0 + (l.d1 - l.d0) * back);
            l.phase -= 1.0;
        }
        // Square: each sample holds flat until the next tick. Round: a sine eases between the last two points,
        // one clock behind the input — a constant delay, so analysers measure the converter steadily.
        let out = if self.round {
            let b = crush_ease(l.phase);
            (f64::from(l.prev) + f64::from(l.held - l.prev) * b) as f32
        } else {
            l.held
        };
        saturate(out)
    }

    /// One block through the route's shape, the output gain and the mix.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (l0, r0) = (*l, *r);
            let (ol, or) = match self.route {
                MONO_SUM => {
                    let m = (l0 + r0) * 0.5;
                    let v = m * (1.0 - self.mix) + self.lane(0, m) * self.out_gain * self.mix;
                    (v, v)
                }
                MONO_L => {
                    let v = l0 * (1.0 - self.mix) + self.lane(0, l0) * self.out_gain * self.mix;
                    (v, v)
                }
                MONO_R => {
                    let v = r0 * (1.0 - self.mix) + self.lane(0, r0) * self.out_gain * self.mix;
                    (v, v)
                }
                STEREO_L => {
                    let v = l0 * (1.0 - self.mix) + self.lane(0, l0) * self.out_gain * self.mix;
                    (v, r0)
                }
                STEREO_R => {
                    let v = r0 * (1.0 - self.mix) + self.lane(0, r0) * self.out_gain * self.mix;
                    (l0, v)
                }
                MID => {
                    let (m, s) = ((l0 + r0) * 0.5, (l0 - r0) * 0.5);
                    let v = m * (1.0 - self.mix) + self.lane(0, m) * self.out_gain * self.mix;
                    (v + s, v - s)
                }
                SIDE => {
                    let (m, s) = ((l0 + r0) * 0.5, (l0 - r0) * 0.5);
                    let v = s * (1.0 - self.mix) + self.lane(0, s) * self.out_gain * self.mix;
                    (m + v, m - v)
                }
                _ => {
                    let wl = self.lane(0, l0) * self.out_gain * self.mix;
                    let wr = self.lane(1, r0) * self.out_gain * self.mix;
                    (l0 * (1.0 - self.mix) + wl, r0 * (1.0 - self.mix) + wr)
                }
            };
            *l = ol;
            *r = or;
        }
    }

    pub fn reset(&mut self) {
        self.lanes = [Lane::default(), Lane::default()];
    }
}

struct Sp(Twelve);

impl Sound for Sp {
    fn set(&mut self, v: &[i32]) {
        self.0.set(v);
    }
    fn set_sample_rate(&mut self, rate: u32) {
        self.0.set_sample_rate(rate);
    }
    fn reset(&mut self) {
        self.0.reset();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.0.process(left, right);
    }
}

fn sound(rate: u32, v: &[i32]) -> Box<dyn Sound> {
    Box::new(Sp(Twelve::new(rate, v)))
}

// ---------------------------------------------------------------------------------------------------------------
// The display: the LCD's line, the converter's staircase and its clock.

const HEAD: Font = Font::new(10).bold();
const FOOT: Font = Font::condensed(10, 0).bold();

fn screen(c: &mut Canvas, r: Rect, v: &[i32]) {
    let d = detune_tenths(v);
    let tune = if d == 0.0 { "0.0ST".to_string() } else { format!("-{:.1}ST", d / 10.0) };
    let k = v.get(FILTER).copied().unwrap_or(99);
    let head = format!("TUNE {tune} · FILTER {}", if k >= 99 { "OFF".to_string() } else { format!("{k}") });
    c.text_clip(HEAD, r.x + 7.0, r.y + 5.0, r.w - 14.0, brand::TEXT, &head);
    // Two cycles of a sine through the clock and the 12 bits: the staircase coarsens as the tune detunes, and the
    // round crush eases its sine between the points where the square holds them flat.
    let round = v.get(CRUSH).copied().unwrap_or(SQUARE) == ROUND;
    let (x0, x1) = (r.x + 8.0, r.right() - 8.0);
    let (top, bot) = (r.y + 22.0, r.bottom() - 22.0);
    let cy = ((top + bot) / 2.0).round();
    let half = (bot - top) / 2.0 - 3.0;
    c.fill_rect(Rect::span(x0, x1, cy, cy + 1.0), plug::GRID);
    let n = ((x1 - x0) as usize).max(2);
    let ppc = (13.0 * 2f64.powf(-d / 120.0)).max(3.0);
    let point = |k: f64| (0.8 * (std::f64::consts::TAU * k / ppc).sin() * 2048.0).round() / 2048.0;
    let pts: Vec<(f32, f32)> = (0..=n)
        .map(|i| {
            let u = i as f64 / n as f64;
            let holds = u * 2.0 * ppc;
            let y = if round {
                let k = holds.floor();
                point(k) + (point(k + 1.0) - point(k)) * crush_ease(holds - k)
            } else {
                point(holds.floor())
            };
            (x0 + (x1 - x0) * u as f32, cy - (y * f64::from(half) / 0.9) as f32)
        })
        .collect();
    c.polyline(&pts, 1.5, brand::MAIN);
    let clock = CLOCK * 2f64.powf(-d / 120.0);
    let mode = if round { " · ROUND" } else { "" };
    c.text_center(FOOT, Rect::new(r.x, r.bottom() - 16.0, r.w, 12.0), plug::DIM, &format!("12-BIT · {:.1}KHZ{mode}", clock / 1000.0));
}

pub struct TwelveKind;

impl Kind for TwelveKind {
    const SPEC: &'static Spec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-12";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's 12-bit sampler colour";
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Distortion, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudio12   ";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Distortion];
}

pub type MangoTwelve = SmallFx<TwelveKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!([text(TUNE, 0), text(TUNE, 52), text(TUNE, 147)], ["0.0ST", "-5.2ST", "-14.7ST"]);
        assert_eq!([parse(TUNE, "0.0ST"), parse(TUNE, "-5.2ST"), parse(TUNE, "-0.1ST")], [Some(0), Some(52), Some(1)]);
        assert_eq!([text(FINE, 0), text(FINE, 1)], ["1 ST", "0.1 ST"]);
        assert_eq!([parse(FINE, "1 ST"), parse(FINE, "0.1 ST")], [Some(0), Some(1)]);
        assert_eq!([text(EXT, 0), text(EXT, 1)], ["Int.", "Ext."]);
        assert_eq!([parse(EXT, "int."), parse(EXT, "ext")], [Some(0), Some(1)]);
        assert_eq!([text(IN, 0), text(IN, -55), text(OUT, 120)], ["0.0dB", "-5.5dB", "12.0dB"]);
        assert_eq!([parse(IN, "0dB"), parse(OUT, "-5.5dB")], [Some(0), Some(-55)]);
        assert_eq!([text(LINK, 0), text(LINK, 1)], ["Off", "Link"]);
        assert_eq!(parse(LINK, "link"), Some(1));
        assert_eq!([text(FILTER, 99), text(FILTER, 84)], ["Off", "84"]);
        assert_eq!([parse(FILTER, "off"), parse(FILTER, "84")], [Some(99), Some(84)]);
        assert_eq!([text(ROUTE, 0), text(ROUTE, 1), text(ROUTE, 7)], ["Stereo", "Mono Sum", "Side"]);
        assert_eq!(parse(ROUTE, "mono sum"), Some(1));
        assert_eq!([text(CRUSH, 0), text(CRUSH, 1)], ["Square", "Round"]);
        assert_eq!([parse(CRUSH, "Square"), parse(CRUSH, "round"), parse(CRUSH, "sq")], [Some(0), Some(1), Some(0)]);
        assert_eq!([text(MIX, 100), text(MIX, 0)], ["100%", "0%"]);
        assert_eq!(parse(MIX, "40%"), Some(40));
    }

    /// A sine through the converter changes only when the clock ticks: about 26040 holds a second at the SP's rate,
    /// the same on both channels of the stereo route.
    #[test]
    fn the_converter_lands_on_the_sp1200_clock() {
        let (n, rate) = (44100usize, 44100u32);
        let mut t = Twelve::new(rate, &[0, 1, 0, 0, 0, 0, 99, STEREO, 100]);
        let mut l: Vec<f32> = (0..n).map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin()).collect();
        let mut r = l.clone();
        t.process(&mut l, &mut r);
        let steps = l.windows(2).filter(|w| w[0] != w[1]).count();
        assert!((steps as f64 - 26040.0).abs() < 260.0, "{steps}");
        assert_eq!(l, r);
    }

    /// The tune detunes the clock, not the pitch: 78's detune runs it near 11 kHz and the 440 Hz sine still crosses
    /// zero 440 times a second. `Ext.` reaches twice as deep; without `0.1 ST` steps the tune snaps to whole
    /// semitones.
    #[test]
    fn tune_detunes_the_clock_and_not_the_pitch() {
        let (n, rate) = (44100usize, 44100u32);
        let sine = |hz: f32| (0..n).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin()).collect::<Vec<f32>>();
        let mut steps_of = |t: &mut Twelve, x: &mut Vec<f32>| {
            let mut r = x.clone();
            t.process(x, &mut r);
            x.windows(2).filter(|w| w[0] != w[1]).count()
        };
        let crossings = |x: &[f32]| x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        let mut t = Twelve::new(rate, &[150, 1, 0, 0, 0, 0, 99, STEREO, 100]);
        let mut x = sine(440.0);
        let steps = steps_of(&mut t, &mut x);
        assert!((steps as f64 - 10948.0).abs() < 110.0, "{steps}");
        assert!((crossings(&x) as i64 - 440).abs() <= 2, "{}", crossings(&x));
        let mut t = Twelve::new(rate, &[150, 1, 1, 0, 0, 0, 99, STEREO, 100]);
        let mut x = sine(440.0);
        let steps = steps_of(&mut t, &mut x);
        assert!((steps as f64 - 4603.0).abs() < 46.0, "{steps}");
        let mut t = Twelve::new(rate, &[52, 0, 0, 0, 0, 0, 99, STEREO, 100]);
        let mut x = sine(440.0);
        let steps = steps_of(&mut t, &mut x);
        assert!((steps as f64 - 19509.0).abs() < 195.0, "{steps}");
    }

    /// The routes pick what the converter hears; what it does not hear passes as it is.
    #[test]
    fn the_routes_pick_what_the_converter_hears() {
        let mut t = Twelve::new(44100, &[0, 1, 0, 0, 0, 0, 99, STEREO_L, 100]);
        let mut l = vec![0.5f32];
        let mut r = vec![0.25f32];
        t.process(&mut l, &mut r);
        assert_eq!(r[0], 0.25);
        assert_eq!(l[0], 0.0); // the clock has not ticked yet: the converter holds silence
        let mut t = Twelve::new(44100, &[0, 1, 0, 0, 0, 0, 99, SIDE, 100]);
        let mut l = vec![0.3f32];
        let mut r = vec![0.3f32];
        t.process(&mut l, &mut r);
        assert_eq!([l[0], r[0]], [0.3, 0.3]); // a mono signal has no side to convert
        let mut t = Twelve::new(44100, &[0, 1, 0, 0, 0, 0, 99, MONO_SUM, 100]);
        let mut l = vec![0.3f32];
        let mut r = vec![-0.1f32];
        t.process(&mut l, &mut r);
        assert_eq!(l[0], r[0]); // the converted sum feeds both channels
    }

    /// Mix 0 is the dry signal, whatever the rest of the settings do.
    #[test]
    fn mix_zero_is_the_dry_signal() {
        let mut t = Twelve::new(44100, &[120, 0, 1, 100, -90, 1, 12, STEREO, 0]);
        let mut l: Vec<f32> = (0..64).map(|i| (i as f32 * 0.37).sin().clamp(-0.9, 0.9)).collect();
        let mut r: Vec<f32> = (0..64).map(|i| (i as f32 * 0.11).sin()).collect();
        let (l0, r0) = (l.clone(), r.clone());
        t.process(&mut l, &mut r);
        assert_eq!(l, l0);
        assert_eq!(r, r0);
    }

    /// Linked gains: raising the input lowers the output by the same amount, so the total stays at the gain out.
    #[test]
    fn link_holds_the_total_gain() {
        let linked = Twelve::new(44100, &[0, 1, 0, 60, -60, 1, 99, STEREO, 100]);
        let free = Twelve::new(44100, &[0, 1, 0, 60, -60, 0, 99, STEREO, 100]);
        assert!((linked.out_gain - 10f32.powf(-0.6)).abs() < 1e-5); // -12 dB: the total is the -6 dB out
        assert!((free.out_gain - 10f32.powf(-0.3)).abs() < 1e-5); // -6 dB
        assert!(free.out_gain > linked.out_gain * 1.9);
    }

    /// The filter: 100 Hz leaves a 5 kHz line in the quantisation noise; bypassed, it rides the converter.
    #[test]
    fn the_filter_tames_the_highs() {
        let (n, rate) = (44100usize, 44100u32);
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        let mut t = Twelve::new(rate, &[0, 1, 0, 0, 0, 0, 0, STEREO, 100]);
        let mut x: Vec<f32> = (0..n).map(|i| 0.5 * (std::f32::consts::TAU * 5000.0 * i as f32 / rate as f32).sin()).collect();
        let mut r = x.clone();
        t.process(&mut x, &mut r);
        assert!(rms(&x) < 0.01, "{}", rms(&x));
        let mut u = Twelve::new(rate, &[0, 1, 0, 0, 0, 0, 99, STEREO, 100]);
        let mut y: Vec<f32> = (0..n).map(|i| 0.5 * (std::f32::consts::TAU * 5000.0 * i as f32 / rate as f32).sin()).collect();
        let mut r = y.clone();
        u.process(&mut y, &mut r);
        assert!(rms(&y) > 0.3, "{}", rms(&y));
    }

    /// The output stage grips when the gain pushes the converter past full scale, and stays under it.
    #[test]
    fn the_output_stage_holds_the_peaks() {
        let mut t = Twelve::new(44100, &[0, 1, 0, 120, 0, 0, 99, STEREO, 100]);
        let mut l: Vec<f32> = (0..4410).map(|i| 0.9 * (std::f32::consts::TAU * 110.0 * i as f32 / 44100.0).sin()).collect();
        let mut r = l.clone();
        t.process(&mut l, &mut r);
        let peak = l.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!(peak > 0.7 && peak < 1.0, "{peak}");
    }

    /// While the gains are linked the out knob shows the effective output gain (the stored out less the in, what
    /// the output stage runs at); free, it and every other setting show their stored values.
    #[test]
    fn the_linked_out_knob_shows_the_effective_output_gain() {
        let v = |link: i32, i: i32, o: i32| {
            let mut v = SPEC.start();
            v[LINK] = link;
            v[IN] = i;
            v[OUT] = o;
            v
        };
        assert_eq!(shown(&v(1, 60, 0), OUT), Some(60)); // the offset is the in: the knob shows 0 - 60 = -6 dB
        assert_eq!(shown(&v(0, 60, 0), OUT), None); // free: the stored value
        assert_eq!(shown(&v(1, 60, 0), IN), None); // the in knob is the drive, never shown otherwise
        assert_eq!(shown(&v(1, 120, -30), OUT), Some(120)); // the shown value clamps to the knob's -12 dB...
        let t = Twelve::new(44100, &v(1, 120, -30)); // ...as the sound's output stage clamps the same way
        assert!((t.out_gain - 10f32.powf(-0.6)).abs() < 1e-5);
    }

    /// The 12 bits: round to the nearest of 4096 levels, and full scale is the ceiling.
    #[test]
    fn the_twelve_bits_round_and_clip() {
        assert_eq!(quantize(0.0003), 1.0 / 2048.0);
        assert_eq!(quantize(-0.0003), -1.0 / 2048.0);
        assert_eq!(quantize(0.0001), 0.0);
        assert_eq!(quantize(3.0), 2047.0 / 2048.0);
        assert_eq!(quantize(-3.0), -1.0);
    }

    /// The crush at a rate the clock ticks exactly every other sample: the square crush holds each point flat for
    /// the tick, the round crush eases a sine between the last two points (flat-footed at both, so the levels are
    /// the same 12 bits and only the travel between them rounds off).
    #[test]
    fn the_round_crush_eases_a_sine_between_the_points() {
        let rate = 52080u32; // 26040 · 2: the clock ticks on every odd sample, `back` is always zero
        let x: Vec<f32> = (0..512)
            .map(|i| 0.9 * (std::f32::consts::TAU * 300.0 * i as f32 / rate as f32).sin())
            .collect();
        let q = |v: f32| ((f64::from(v) * 2048.0).round().clamp(-2048.0, 2047.0) / 2048.0) as f32;
        let sat = |v: f32| (0.75 * v).tanh() / 0.75;

        let mut t = Twelve::new(rate, &[0, 1, 0, 0, 0, 0, 99, STEREO, 100, SQUARE]);
        let (mut l, mut r) = (x.clone(), x.clone());
        t.process(&mut l, &mut r);
        assert_eq!(l, r);
        for m in 0..250 {
            // A tick on sample 2m+1 holds q(x[2m+1]) flat across it.
            assert_eq!(l[2 * m + 1], sat(q(x[2 * m + 1])));
            assert_eq!(l[2 * m + 2], sat(q(x[2 * m + 1])));
        }

        let mut t = Twelve::new(rate, &[0, 1, 0, 0, 0, 0, 99, STEREO, 100, ROUND]);
        let (mut l, mut r) = (x.clone(), x.clone());
        t.process(&mut l, &mut r);
        assert_eq!(l, r);
        // At the tick the ease sits on the previous point; half a tick in, the sine sits at its middle (its
        // steepest, the quarter point of the arc); the next tick lands on the new point.
        for m in 0..250 {
            let (a, b) = (if m == 0 { 0.0 } else { q(x[2 * m - 1]) }, q(x[2 * m + 1]));
            assert_eq!(l[2 * m + 1], sat(a));
            // The arc's middle is the points' mean; the ease's f64 sine leaves a last-bit breath at the exact
            // quarter, so compare with room for it.
            assert!((l[2 * m + 2] - sat((a + b) / 2.0)).abs() < 1e-12);
        }
        // And the ease never overshoots the two points it travels between.
        for m in 0..250 {
            let (a, b) = (if m == 0 { 0.0 } else { q(x[2 * m - 1]) }, q(x[2 * m + 1]));
            let (lo, hi) = (sat(a.min(b)) - 1e-9, sat(a.max(b)) + 1e-9);
            assert!(l[2 * m + 1] >= lo && l[2 * m + 1] <= hi);
            assert!(l[2 * m + 2] >= lo && l[2 * m + 2] <= hi);
        }
    }

    /// The round crush reads out one clock behind at a constant delay: a short-window analyser measures the same
    /// phase whatever the clock is doing, where the square's raw hold (and, before it, the ticks landing on the
    /// host sample that noticed them) swings the phase with the clock.
    #[test]
    fn the_round_crush_measures_at_a_constant_delay() {
        let (n, rate) = (44100, 44100.0f32);
        let hz = 10_000.0f32;
        // The phase a 256-sample window (a short analyser FFT) measures, over the tone's steady second half.
        let spread = |crush: i32| -> f64 {
            let mut t = Twelve::new(44100, &[0, 1, 0, 0, 0, 0, 99, STEREO, 100, crush]);
            let mut l: Vec<f32> =
                (0..n).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / rate).sin()).collect();
            let mut r = l.clone();
            t.process(&mut l, &mut r);
            let w = f64::from(std::f32::consts::TAU * hz / rate);
            let mut ph = Vec::new();
            let mut start = n / 2;
            while start + 256 <= n {
                let (mut re, mut im) = (0.0f64, 0.0f64);
                for (i, &v) in l[start..start + 256].iter().enumerate() {
                    let (s, c) = (w * (start + i) as f64).sin_cos();
                    re += f64::from(v) * c;
                    im -= f64::from(v) * s;
                }
                ph.push(im.atan2(re));
                start += 64;
            }
            for i in 1..ph.len() {
                while ph[i] - ph[i - 1] > std::f64::consts::PI {
                    ph[i] -= std::f64::consts::TAU;
                }
                while ph[i] - ph[i - 1] < -std::f64::consts::PI {
                    ph[i] += std::f64::consts::TAU;
                }
            }
            let min = ph.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = ph.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            (max - min).to_degrees()
        };
        let round = spread(ROUND);
        let square = spread(SQUARE);
        assert!(round < 1.0, "round {round}°");
        assert!(square > round * 4.0, "square {square}° round {round}°");
    }

    /// The routes switch at the next block, and the side crunch keeps the mid exactly as it came in while the
    /// converter chews the side.
    #[test]
    fn the_routes_switch_between_blocks_and_the_side_crunch_keeps_the_mid() {
        let (n, rate) = (44100, 44100u32);
        // A wide stereo signal: a shared mid sine and antiphase tops the converter can get its teeth into.
        let mid: Vec<f32> = (0..n).map(|i| 0.4 * (std::f32::consts::TAU * 220.0 * i as f32 / rate as f32).sin()).collect();
        let tops: Vec<f32> = (0..n)
            .map(|i| 0.3 * (std::f32::consts::TAU * 7000.0 * i as f32 / rate as f32 + 2.0 * (i as f32 * 0.13).sin()).sin())
            .collect();
        let (l0, r0): (Vec<f32>, Vec<f32>) = (
            mid.iter().zip(&tops).map(|(a, b)| a + b).collect(),
            mid.iter().zip(&tops).map(|(a, b)| a - b).collect(),
        );
        let blocks = |t: &mut Twelve, l: &mut [f32], r: &mut [f32]| {
            for (lc, rc) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                t.process(lc, rc);
            }
        };
        let mid_of = |l: &[f32], r: &[f32]| -> Vec<f32> { l.iter().zip(r).map(|(a, b)| (a + b) * 0.5).collect() };
        let side_of = |l: &[f32], r: &[f32]| -> Vec<f32> { l.iter().zip(r).map(|(a, b)| (a - b) * 0.5).collect() };

        let mut t = Twelve::new(rate, &[0, 1, 0, 0, 0, 0, 99, SIDE, 100, SQUARE]);
        let (mut l, mut r) = (l0.clone(), r0.clone());
        blocks(&mut t, &mut l, &mut r);
        let (half, out_mid, out_side) = (n / 2, mid_of(&l, &r), side_of(&l, &r));
        let (in_mid, in_side) = (mid_of(&l0, &r0), side_of(&l0, &r0));
        // The mid passes as it is, every sample of it.
        for k in 0..n {
            assert!((out_mid[k] - in_mid[k]).abs() < 1e-5, "mid moved at {k}");
        }
        // And the converter chews the side: the 12-bit grid leaves its marks all over it.
        let touched = out_side.iter().zip(&in_side).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(touched > 1.0 / 8192.0, "side untouched: {touched}");

        // Switch to the stereo pair mid-stream: from the next block the mid takes the converter too.
        t.set(&[0, 1, 0, 0, 0, 0, 99, STEREO, 100, SQUARE]);
        blocks(&mut t, &mut l, &mut r);
        let out_mid = mid_of(&l, &r);
        let moved = out_mid[half..].iter().zip(&in_mid[half..]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(moved > 1.0 / 8192.0, "stereo route left the mid: {moved}");
    }

    /// The crush is the byte after the settings' ints: sessions saved before it read as Square, its byte grows onto
    /// the state when it is written, and the byte's other bits survive the write.
    #[test]
    fn the_crush_byte_reads_old_states_and_keeps_its_bits() {
        let b = vec![0u8; 4 * 9]; // the nine ints of a session saved before the crush
        let mut v = crate::params::values_of(SPEC, &b);
        assert_eq!(v[CRUSH], SQUARE); // read as Square from the byte it does not have
        v[CRUSH] = ROUND;
        let b = crate::params::to_fl(SPEC, &v, &b);
        assert_eq!(b.len(), 37); // the byte grows onto the state
        assert_eq!(crate::params::values_of(SPEC, &b)[CRUSH], ROUND);
        let mut bits = b.clone();
        bits[36] |= 0b10; // an unrelated bit of the same byte
        let v = crate::params::values_of(SPEC, &bits);
        assert_eq!(v[CRUSH], ROUND);
        let out = crate::params::to_fl(SPEC, &v, &bits);
        assert_eq!(out[36], bits[36]); // writing the mode leaves the byte's other bits alone
    }
}
