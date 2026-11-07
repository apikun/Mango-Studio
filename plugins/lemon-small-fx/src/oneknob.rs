// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Oneknob: the one-knob loudener (the idea behind Soundgoodizer) as a plugin. The amount knob drives a
//! fixed compressor's make-up loudness; the four preset curves stand for the original's A-D.
//!
// spec: plugins/lemon-oneknob.md

use nih_plug::prelude::*;

use os_engine::effects::oneknob::{self as dsp, Settings, Oneknob};
use os_theme::design::lemon as brand;
use os_ui::Rect;

use crate::params::{leading_number, nearest_rising};
use crate::{Card, Control, Kind, Mark, Setting, SmallFx, Sound, Spec};

/// The settings in the saved order.
pub const SETTINGS: [Setting; dsp::PARAMS] = [
    Setting { id: "amount", name: "Amount", range: dsp::RANGES[dsp::AMOUNT], start: dsp::DEFAULTS[dsp::AMOUNT], magnet: None },
    Setting { id: "preset", name: "Curve", range: dsp::RANGES[dsp::PRESET], start: dsp::DEFAULTS[dsp::PRESET], magnet: None },
];

/// The curves' names, A-D.
pub const CURVES: [&str; 4] = ["A", "B", "C", "D"];

/// Editor layout: one card, the big knob over the curves' list.
const CARDS: [Card; 1] = [Card { x0: 8.0, x1: 352.0, y0: 28.0, y1: 306.0, heading: "GOODNESS" }];

const CONTROLS: [Control; 2] = [
    Control::Knob { id: "amount", cx: 100.0, cy: 170.0, r: 52.0, caption: "AMOUNT", bipolar: false },
    Control::Choice { id: "preset", x: 240.0, top: 90.0, names: &CURVES },
];

pub const SPEC: &Spec = &Spec {
    name: "Mango Oneknob",
    word: "ONEKNOB",
    about: "Mango Studio's one-knob loudener",
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
    labels: &[(240.0, 70.0, "CURVE")],
    text,
    parse,
    shown: None,
    sound,
    rings: false,
};

/// Value texts: the amount as a percentage, the curve as its letter.
fn text(k: usize, v: i32) -> String {
    match k {
        dsp::PRESET => CURVES[(v as usize).clamp(0, 3)].into(),
        _ => format!("{:.0}%", dsp::amount(v) * 100.0),
    }
}

fn parse(k: usize, s: &str) -> Option<i32> {
    let low = s.trim().to_ascii_lowercase();
    // The curve by its letter, the last word of what was typed ("curve D").
    let last = low.split_whitespace().next_back().unwrap_or(&low);
    match k {
        dsp::PRESET => CURVES.iter().position(|n| last.starts_with(&n.to_lowercase())).map(|i| i as i32),
        _ => {
            let x = leading_number(&low)?;
            Some(((x / 100.0 * 12800.0).round() as i32).clamp(0, dsp::RANGES[dsp::AMOUNT].1))
        }
    }
}

/// The engine's settings for the plugin's.
fn settings(v: &[i32]) -> Settings {
    Settings { v: v.try_into().unwrap_or(dsp::DEFAULTS) }
}

struct Knob(Oneknob);

impl Sound for Knob {
    fn set(&mut self, v: &[i32]) {
        self.0.set(&settings(v));
    }
    /// The follower's times for the new rate.
    fn set_sample_rate(&mut self, rate: u32) {
        self.0.set_sample_rate(rate);
    }
    /// The follower empty; the gains stand at the settings.
    fn reset(&mut self) {
        self.0.clear();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        // The engine's interleaved form: two mono buffers zip together.
        for i in 0..left.len().min(right.len()) {
            left[i] = self.0.sample(0, left[i]);
            right[i] = self.0.sample(1, right[i]);
        }
    }
}

/// The plugin starts at its settings.
fn sound(rate: u32, v: &[i32]) -> Box<dyn Sound> {
    let mut o = Oneknob::with_settings(&settings(v));
    o.set_sample_rate(rate);
    Box::new(Knob(o))
}

pub struct OneknobKind;

impl Kind for OneknobKind {
    const SPEC: &'static Spec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.lemon-oneknob";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's one-knob loudener";
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Compressor, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"LemonStudio1Knob";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

pub type LemonOneknob = SmallFx<OneknobKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(0, 12800), "100%");
        assert_eq!(text(0, 6400), "50%");
        assert_eq!(text(0, 0), "0%");
        assert_eq!(text(1, 0), "A");
        assert_eq!(text(1, 3), "D");
        assert_eq!(parse(0, "100%"), Some(12800));
        assert_eq!(parse(0, "50"), Some(6400));
        assert_eq!(parse(0, "300"), Some(12800));
        assert_eq!(parse(1, "b"), Some(1));
        assert_eq!(parse(1, "Curve D"), Some(3));
    }

    #[test]
    fn the_plugin_starts_at_its_settings_and_compresses() {
        // The state is the settings, in order.
        assert_eq!(settings(&[6400, 2]).v, [6400, 2]);
        // A steady loud block comes out denser (the make-up rides the compression).
        let mut s = sound(44100, &[12800, 0]);
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (vec![0.0; 4410], vec![0.0; 4410]);
        for i in 0..4410 {
            let x = 0.8 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 44100.0).sin();
            l[i] = x;
            r[i] = x;
        }
        s.process(&mut l, &mut r);
        let peak = l.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak > 0.05 && peak < 0.8, "compressed to a sane level: {peak}");
        // A reset empties the follower.
        s.reset();
        let mut s = sound(44100, &[12800, 0]);
        let (mut l, mut r) = (vec![0.0f32; 4], vec![0.0f32; 4]);
        s.set_sample_rate(96000);
        s.process(&mut l, &mut r);
        assert!(l.iter().all(|&x| x.abs() < 1.0e-5), "silence stays silence");
    }

    #[test]
    fn the_four_curves_are_there() {
        // Each curve's text round trips.
        for (i, name) in CURVES.iter().enumerate() {
            assert_eq!(text(1, i as i32), *name);
            assert_eq!(parse(1, name), Some(i as i32));
        }
        // The ranges hold the settings.
        assert_eq!(SETTINGS[0].range, dsp::RANGES[dsp::AMOUNT]);
        assert_eq!(SETTINGS[1].range, dsp::RANGES[dsp::PRESET]);
    }

    #[test]
    fn percent_helpers_agree() {
        // nearest_rising finds the stored step of a typed percentage through the same map text() shows.
        let step = nearest_rising((0, 12800), 50.0, |v| dsp::amount(v) as f64 * 100.0);
        assert_eq!(text(0, step), "50%");
    }
}
