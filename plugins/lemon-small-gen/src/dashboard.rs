// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Dashboard: Mango Studio's hardware panel (a recreation of FL's Dashboard) as a plugin. Twelve knobs and
//! four faders that sound nothing, laid out like a machine's face: a performance links them to anything (the app's
//! Multilink, the automation clips), records and draws their moves, and the channel rack holds the silence a voice
//! would fill.
//!
// spec: plugins/lemon-small-generators.md

use nih_plug::prelude::*;
use os_theme::design::lemon;
use os_ui::Rect;

use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

/// The settings in stored order (spec §5): the twelve knobs, then the four faders.
pub const SETTINGS: [Setting; 16] = [
    Setting { id: "k1", name: "Knob 1", range: (0, 128), start: 64, magnet: None },
    Setting { id: "k2", name: "Knob 2", range: (0, 128), start: 64, magnet: None },
    Setting { id: "k3", name: "Knob 3", range: (0, 128), start: 64, magnet: None },
    Setting { id: "k4", name: "Knob 4", range: (0, 128), start: 64, magnet: None },
    Setting { id: "k5", name: "Knob 5", range: (0, 128), start: 64, magnet: None },
    Setting { id: "k6", name: "Knob 6", range: (0, 128), start: 64, magnet: None },
    Setting { id: "k7", name: "Knob 7", range: (0, 128), start: 64, magnet: None },
    Setting { id: "k8", name: "Knob 8", range: (0, 128), start: 64, magnet: None },
    Setting { id: "k9", name: "Knob 9", range: (0, 128), start: 64, magnet: None },
    Setting { id: "k10", name: "Knob 10", range: (0, 128), start: 64, magnet: None },
    Setting { id: "k11", name: "Knob 11", range: (0, 128), start: 64, magnet: None },
    Setting { id: "k12", name: "Knob 12", range: (0, 128), start: 64, magnet: None },
    Setting { id: "f1", name: "Fader 1", range: (0, 128), start: 64, magnet: None },
    Setting { id: "f2", name: "Fader 2", range: (0, 128), start: 64, magnet: None },
    Setting { id: "f3", name: "Fader 3", range: (0, 128), start: 64, magnet: None },
    Setting { id: "f4", name: "Fader 4", range: (0, 128), start: 64, magnet: None },
];

pub const K1: usize = 0;
pub const F1: usize = 12;

/// Editor layout (spec §2): two rows of knobs over a row of faders, the machine's face.
const CARDS: [Card; 3] = [
    Card { x0: 8.0, x1: 246.0, y0: 28.0, y1: 134.0, heading: "KNOBS 1-6" },
    Card { x0: 252.0, x1: 486.0, y0: 28.0, y1: 134.0, heading: "KNOBS 7-12" },
    Card { x0: 8.0, x1: 486.0, y0: 140.0, y1: 234.0, heading: "FADERS" },
];

const CONTROLS: [Control; 16] = [
    Control::Knob { id: "k1", cx: 28.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Knob { id: "k2", cx: 66.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Knob { id: "k3", cx: 104.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Knob { id: "k4", cx: 142.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Knob { id: "k5", cx: 180.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Knob { id: "k6", cx: 218.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Knob { id: "k7", cx: 272.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Knob { id: "k8", cx: 310.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Knob { id: "k9", cx: 348.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Knob { id: "k10", cx: 386.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Knob { id: "k11", cx: 424.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Knob { id: "k12", cx: 462.0, cy: 84.0, r: 16.0, caption: "", bipolar: false },
    Control::Fader { id: "f1", x: 100.0, top: 168.0, bot: 222.0, label: "1", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
    Control::Fader { id: "f2", x: 220.0, top: 168.0, bot: 222.0, label: "2", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
    Control::Fader { id: "f3", x: 340.0, top: 168.0, bot: 222.0, label: "3", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
    Control::Fader { id: "f4", x: 460.0, top: 168.0, bot: 222.0, label: "4", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Dashboard",
    word: "DASHBOARD",
    about: "Mango Studio's hardware panel",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (494, 276),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark { x: 478.0, y: 255.0, anchor: 1.0, stacked: false },
    hint: Some(Rect::new(8.0, 244.0, 462.0, 22.0)),
    labels: &[],
    text,
    parse,
    make,
};

// ---------------------------------------------------------------------------------------------------------------
// Laws (spec §3): a control's value as the number its text names, and a typed text back as a value

/// A share of a control as a percent.
fn percent(v: i32) -> String {
    format!("{:.0}%", f64::from(v) / 128.0 * 100.0)
}

fn percent_parse(s: &str) -> Option<i32> {
    let x = crate::params::leading_number(s)?;
    crate::params::nearest(SETTINGS[K1].range, x, |v| Some(f64::from(v) / 128.0 * 100.0))
}

/// Value texts (spec §3, Lemon Studio's choice).
fn text(k: usize, v: i32) -> String {
    let _ = k;
    percent(v)
}

/// A typed value.
fn parse(k: usize, s: &str) -> Option<i32> {
    let _ = k;
    percent_parse(s.trim().to_ascii_lowercase().as_str())
}

// ---------------------------------------------------------------------------------------------------------------
// The sound: none. The dashboard's work is its parameters (the host links, records and automates them), so the
// voice the rack would hold is silence.

struct Dashboard;

impl Instrument for Dashboard {
    fn set(&mut self, _v: &[i32]) {}
    fn set_sample_rate(&mut self, _rate: u32) {}
    fn reset(&mut self) {}
    fn note_on(&mut self, _key: u8, _velocity: f32, _voice_id: Option<i32>) {}
    fn note_off(&mut self, _key: u8, _voice_id: Option<i32>) {}
    fn choke(&mut self, _key: u8) {}
    fn all_off(&mut self) {}

    /// The buffers pass through untouched: the caller adds into them, and silence adds nothing.
    fn render(&mut self, _left: &mut [f32], _right: &mut [f32]) {}

    fn take_ended(&mut self) -> Vec<(u8, Option<i32>)> {
        Vec::new()
    }

    fn has_voices(&self) -> bool {
        false
    }
}

fn make(_rate: u32, _v: &[i32]) -> Box<dyn Instrument> {
    Box::new(Dashboard)
}

pub struct DashboardKind;

impl Kind for DashboardKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-dashboard";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's hardware panel";
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioDash1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument];
}

pub type MangoDashboard = SmallGen<DashboardKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(K1, 64), "50%");
        assert_eq!(parse(K1, "50%"), Some(64));
        assert_eq!(text(K1, 0), "0%");
        assert_eq!(parse(K1, "100%"), Some(128));
        assert_eq!(parse(K1, "nope"), None);
        assert_eq!(text(F1, 32), "25%");
        for (k, s) in SETTINGS.iter().enumerate() {
            assert_eq!(s.start, 64, "control {} starts mid", k + 1);
        }
    }

    /// The dashboard sounds nothing: a note plays, the buffers come back as they went in, and no voice ever is.
    #[test]
    fn the_dashboard_is_silent() {
        let mut d = make(44100, &SPEC.start());
        d.note_on(60, 1.0, Some(1));
        let mut l = vec![0.25f32; 256];
        let mut r = vec![0.5f32; 256];
        d.render(&mut l, &mut r);
        assert!(l.iter().all(|x| *x == 0.25) && r.iter().all(|x| *x == 0.5), "silence");
        assert!(!d.has_voices());
        assert_eq!(d.take_ended(), Vec::new());
        d.note_off(60, Some(1));
        d.choke(60);
        d.all_off();
        d.reset();
        d.set(&SPEC.start());
        d.set_sample_rate(48000);
        assert!(!d.has_voices(), "still nothing");
    }
}
