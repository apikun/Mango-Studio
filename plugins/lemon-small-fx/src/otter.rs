// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango OTTER: the three-band upward / downward compressor (the idea behind Xfer's OTT) as a plugin. Depth,
//! the two crossovers and the follower's time; the bands split at Butterworth filters and sum back whole.
//!
// spec: plugins/lemon-otter.md

use nih_plug::prelude::*;

use os_engine::effects::otter::{self as dsp, Otter, Settings};
use os_theme::design::lemon as brand;
use os_ui::Rect;

use crate::params::leading_number;
use crate::{Card, Control, Kind, Mark, Setting, SmallFx, Sound, Spec};

/// The settings in the saved order.
pub const SETTINGS: [Setting; dsp::PARAMS] = [
    Setting { id: "depth", name: "Depth", range: dsp::RANGES[dsp::DEPTH], start: dsp::DEFAULTS[dsp::DEPTH], magnet: None },
    Setting { id: "high", name: "High crossover", range: dsp::RANGES[dsp::HIGH_CUT], start: dsp::DEFAULTS[dsp::HIGH_CUT], magnet: None },
    Setting { id: "low", name: "Low crossover", range: dsp::RANGES[dsp::LOW_CUT], start: dsp::DEFAULTS[dsp::LOW_CUT], magnet: None },
    Setting { id: "time", name: "Time", range: dsp::RANGES[dsp::TIME], start: dsp::DEFAULTS[dsp::TIME], magnet: None },
];

/// Editor layout: one card, the depth fader tall on the left, the three dials on the right.
const CARDS: [Card; 2] = [
    Card { x0: 8.0, x1: 130.0, y0: 28.0, y1: 306.0, heading: "DEPTH" },
    Card { x0: 136.0, x1: 352.0, y0: 28.0, y1: 306.0, heading: "BANDS" },
];

const CONTROLS: [Control; 4] = [
    Control::Fader { id: "depth", x: 68.0, top: 66.0, bot: 270.0, label: "DEPTH", col: brand::MAIN, bipolar: false, marks: &[0.5, 1.0] },
    Control::Knob { id: "high", cx: 220.0, cy: 120.0, r: 26.0, caption: "HIGH", bipolar: false },
    Control::Knob { id: "low", cx: 300.0, cy: 120.0, r: 26.0, caption: "LOW", bipolar: false },
    Control::Knob { id: "time", cx: 260.0, cy: 230.0, r: 26.0, caption: "TIME", bipolar: false },
];

pub const SPEC: &Spec = &Spec {
    name: "Mango OTTER",
    word: "OTTER",
    about: "Mango Studio's three-band upward compressor",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    blank: None,
    locate: None,
    size: (360, 372),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark { x: 352.0, y: 353.0, anchor: 1.0, stacked: false },
    hint: Some(Rect::new(8.0, 314.0, 344.0, 22.0)),
    labels: &[],
    text,
    parse,
    shown: None,
    sound,
    rings: false,
};

/// The crossovers' hertz text (one decimal below 100, none above).
fn hz(f: f32) -> String {
    if f < 100.0 {
        format!("{f:.1}Hz")
    } else {
        format!("{f:.0}Hz")
    }
}

/// Value texts: the depth as a percentage, the crossovers in hertz, the time as a word for the ends.
fn text(k: usize, v: i32) -> String {
    match k {
        dsp::DEPTH => format!("{:.0}%", dsp::depth(v) * 100.0),
        dsp::HIGH_CUT => hz(dsp::crossover(v, dsp::HIGH_RANGE)),
        dsp::LOW_CUT => hz(dsp::crossover(v, dsp::LOW_RANGE)),
        _ => match v {
            0 => "Quick".into(),
            127 => "Slow".into(),
            v => format!("{:.0}", f64::from(v) / 127.0 * 100.0),
        },
    }
}

fn parse(k: usize, s: &str) -> Option<i32> {
    let low = s.trim().to_ascii_lowercase();
    match k {
        dsp::DEPTH => Some(((leading_number(&low)? / 100.0 * 12800.0).round() as i32).clamp(0, 12800)),
        dsp::HIGH_CUT | dsp::LOW_CUT => {
            let x = leading_number(&low)?;
            let hz = if low.contains('k') { x * 1000.0 } else { x };
            let range = if k == dsp::HIGH_CUT { dsp::HIGH_RANGE } else { dsp::LOW_RANGE };
            // The nearest stored step: the crossover is log in the step.
            let (lo, hi) = (f64::from(range.0), f64::from(range.1));
            let step = (((hz as f64 / lo).ln() / (hi / lo).ln()) * 127.0).round();
            Some((step as i32).clamp(0, 127))
        }
        _ => {
            let name = |starts: &str| low.starts_with(starts);
            if name("quick") || name("fast") {
                Some(0)
            } else if name("slow") {
                Some(127)
            } else {
                Some(((leading_number(&low)? / 100.0 * 127.0).round() as i32).clamp(0, 127))
            }
        }
    }
}

/// The engine's settings for the plugin's.
fn settings(v: &[i32]) -> Settings {
    Settings { v: v.try_into().unwrap_or(dsp::DEFAULTS) }
}

struct Bands {
    otter: Otter,
    rate: u32,
}

impl Sound for Bands {
    fn set(&mut self, v: &[i32]) {
        self.otter.set(&settings(v), self.rate);
    }
    /// The filters and the times for the new rate.
    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate;
        self.otter.set_sample_rate(rate);
    }
    /// The followers empty; the filters stand.
    fn reset(&mut self) {
        self.otter.clear();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for i in 0..left.len().min(right.len()) {
            left[i] = self.otter.sample(0, left[i]);
            right[i] = self.otter.sample(1, right[i]);
        }
    }
}

/// The plugin starts at its settings.
fn sound(rate: u32, v: &[i32]) -> Box<dyn Sound> {
    Box::new(Bands { otter: Otter::with_settings(&settings(v), rate), rate })
}

pub struct OtterKind;

impl Kind for OtterKind {
    const SPEC: &'static Spec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.lemon-otter";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's three-band upward compressor";
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Compressor, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"LemonStudioOtter";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

pub type LemonOtter = SmallFx<OtterKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(dsp::DEPTH, 12800), "100%");
        assert_eq!(text(dsp::DEPTH, 0), "0%");
        assert_eq!(text(dsp::LOW_CUT, 0), "30.0Hz");
        assert_eq!(text(dsp::LOW_CUT, 127), "1000Hz");
        assert_eq!(text(dsp::HIGH_CUT, 127), "12000Hz");
        assert_eq!(text(dsp::TIME, 0), "Quick");
        assert_eq!(text(dsp::TIME, 127), "Slow");
        assert_eq!(parse(dsp::DEPTH, "100%"), Some(12800));
        assert_eq!(parse(dsp::LOW_CUT, "300Hz"), Some(83), "a log step");
        assert_eq!(parse(dsp::HIGH_CUT, "2kHz"), Some(55), "kHz is typed too");
        assert_eq!(parse(dsp::TIME, "quick"), Some(0));
        assert_eq!(parse(dsp::TIME, "slow"), Some(127));
        assert_eq!(parse(dsp::TIME, "50"), Some(64));
    }

    #[test]
    fn the_plugin_starts_at_its_settings_and_processes() {
        // A steady block comes out at a sane level at the classic full depth.
        let mut s = sound(44100, &[12800, 96, 34, 32]);
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (vec![0.0; 4410], vec![0.0; 4410]);
        for i in 0..4410 {
            let x = 0.5 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 44100.0).sin();
            l[i] = x;
            r[i] = x;
        }
        s.process(&mut l, &mut r);
        let peak = l.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak > 0.01 && peak < 0.4, "compressed to a sane level: {peak}");
        // Depth 0 is the input.
        let mut s = sound(44100, &[0, 96, 34, 32]);
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (vec![0.5; 8], vec![0.25; 8]);
        s.process(&mut l, &mut r);
        assert!(l.iter().all(|&x| (x - 0.5).abs() < 1.0e-5) && r.iter().all(|&x| (x - 0.25).abs() < 1.0e-5), "dry at depth 0");
        // A reset empties the followers.
        s.reset();
        let mut s = sound(44100, &[12800, 96, 34, 32]);
        let (mut l, mut r) = (vec![0.0f32; 4], vec![0.0f32; 4]);
        s.set_sample_rate(96000);
        s.process(&mut l, &mut r);
        assert!(l.iter().all(|&x| x.abs() < 1.0e-5), "silence stays silence");
    }

    #[test]
    fn settings_round_trip() {
        let v = [6400, 64, 32, 16];
        let b = settings(&v).to_state();
        assert_eq!(b.len(), dsp::STATE_LEN);
        assert_eq!(Settings::from_state(&b).unwrap().v, v);
    }
}
