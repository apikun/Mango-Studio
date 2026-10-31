// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Surface: Mango Studio's control surface (a recreation of FL's Control Surface) as a plugin. Eight faders
//! that sound nothing: a performance links them to anything (the app's Multilink, the automation clips), records and
//! draws their moves, and the channel rack holds the silence a voice would fill.
//!
// spec: plugins/lemon-small-generators.md

use nih_plug::prelude::*;
use os_theme::design::lemon;
use os_ui::Rect;

use crate::{Card, Control, GenSpec, Instrument, Kind, Mark, Setting, SmallGen};

/// The settings in stored order (spec §5): the eight faders, nothing else.
pub const SETTINGS: [Setting; 8] = [
    Setting { id: "c1", name: "Control 1", range: (0, 128), start: 64, magnet: None },
    Setting { id: "c2", name: "Control 2", range: (0, 128), start: 64, magnet: None },
    Setting { id: "c3", name: "Control 3", range: (0, 128), start: 64, magnet: None },
    Setting { id: "c4", name: "Control 4", range: (0, 128), start: 64, magnet: None },
    Setting { id: "c5", name: "Control 5", range: (0, 128), start: 64, magnet: None },
    Setting { id: "c6", name: "Control 6", range: (0, 128), start: 64, magnet: None },
    Setting { id: "c7", name: "Control 7", range: (0, 128), start: 64, magnet: None },
    Setting { id: "c8", name: "Control 8", range: (0, 128), start: 64, magnet: None },
];

pub const C1: usize = 0;

/// Editor layout (spec §2): one card of eight faders.
const CARDS: [Card; 1] = [Card { x0: 8.0, x1: 486.0, y0: 28.0, y1: 134.0, heading: "SURFACE" }];

const CONTROLS: [Control; 8] = [
    Control::Fader { id: "c1", x: 44.0, top: 52.0, bot: 120.0, label: "1", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
    Control::Fader { id: "c2", x: 100.0, top: 52.0, bot: 120.0, label: "2", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
    Control::Fader { id: "c3", x: 156.0, top: 52.0, bot: 120.0, label: "3", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
    Control::Fader { id: "c4", x: 212.0, top: 52.0, bot: 120.0, label: "4", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
    Control::Fader { id: "c5", x: 268.0, top: 52.0, bot: 120.0, label: "5", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
    Control::Fader { id: "c6", x: 324.0, top: 52.0, bot: 120.0, label: "6", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
    Control::Fader { id: "c7", x: 380.0, top: 52.0, bot: 120.0, label: "7", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
    Control::Fader { id: "c8", x: 436.0, top: 52.0, bot: 120.0, label: "8", col: lemon::MAIN, bipolar: false, marks: &[0.25, 0.5, 0.75] },
];

pub const SPEC: &GenSpec = &GenSpec {
    name: "Mango Surface",
    word: "SURFACE",
    about: "Mango Studio's control surface",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    size: (494, 162),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark { x: 478.0, y: 143.0, anchor: 1.0, stacked: false },
    hint: Some(Rect::new(8.0, 132.0, 462.0, 22.0)),
    labels: &[],
    text,
    parse,
    make,
};

// ---------------------------------------------------------------------------------------------------------------
// Laws (spec §3): a fader's value as the number its text names, and a typed text back as a value

/// A share of a fader as a percent.
fn percent(v: i32) -> String {
    format!("{:.0}%", f64::from(v) / 128.0 * 100.0)
}

fn percent_parse(s: &str) -> Option<i32> {
    let x = crate::params::leading_number(s)?;
    crate::params::nearest(SETTINGS[C1].range, x, |v| Some(f64::from(v) / 128.0 * 100.0))
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
// The sound: none. The surface's work is its parameters (the host links, records and automates them), so the
// voice the rack would hold is silence.

struct Surface;

impl Instrument for Surface {
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
    Box::new(Surface)
}

pub struct SurfaceKind;

impl Kind for SurfaceKind {
    const SPEC: &'static GenSpec = SPEC;
    const CLAP_ID: &'static str = "org.openstudio.mango-surface";
    const CLAP_DESCRIPTION: &'static str = "Mango Studio's control surface";
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Stereo];
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioSurfa";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument];
}

pub type MangoSurface = SmallGen<SurfaceKind>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_texts_and_typed_values() {
        assert_eq!(text(C1, 64), "50%");
        assert_eq!(parse(C1, "50%"), Some(64));
        assert_eq!(text(C1, 0), "0%");
        assert_eq!(parse(C1, "100%"), Some(128));
        assert_eq!(parse(C1, "nope"), None);
        for k in 0..8 {
            assert_eq!(SETTINGS[k].start, 64, "fader {} starts mid", k + 1);
        }
    }

    /// The surface sounds nothing: a note plays, the buffers come back as they went in, and no voice ever is.
    #[test]
    fn the_surface_is_silent() {
        let mut s = make(44100, &SPEC.start());
        s.note_on(60, 1.0, Some(1));
        let mut l = vec![0.25f32; 256];
        let mut r = vec![0.5f32; 256];
        s.render(&mut l, &mut r);
        assert!(l.iter().all(|x| *x == 0.25) && r.iter().all(|x| *x == 0.5), "silence");
        assert!(!s.has_voices());
        assert_eq!(s.take_ended(), Vec::new());
        s.note_off(60, Some(1));
        s.choke(60);
        s.all_off();
        s.reset();
        s.set(&SPEC.start());
        s.set_sample_rate(48000);
        assert!(!s.has_voices(), "still nothing");
    }
}
