// Copyright (C) 2026 Artig3nce, apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Master's parameters: real host parameters, so all of them can be automated and saved by the host.

use std::sync::Arc;

use nih_plug::prelude::*;
use os_plugkit::live::LiveEdits;

use crate::auto::TARGETS;
use crate::dsp::{Settings, COMP_OFF_DB};

/// The ids of the knobs, as the editor names them.
pub const EQ_IDS: [&str; 5] = ["eq_low", "eq_low_mid", "eq_mid", "eq_high_mid", "eq_high"];
pub const COMP_IDS: [&str; 3] = ["comp_low", "comp_mid", "comp_high"];

#[derive(Params)]
pub struct MasterParams {
    #[id = "eq_low"]
    pub eq_low: FloatParam,
    #[id = "eq_low_mid"]
    pub eq_low_mid: FloatParam,
    #[id = "eq_mid"]
    pub eq_mid: FloatParam,
    #[id = "eq_high_mid"]
    pub eq_high_mid: FloatParam,
    #[id = "eq_high"]
    pub eq_high: FloatParam,
    #[id = "comp_low"]
    pub comp_low: FloatParam,
    #[id = "comp_mid"]
    pub comp_mid: FloatParam,
    #[id = "comp_high"]
    pub comp_high: FloatParam,
    #[id = "ratio"]
    pub ratio: FloatParam,
    #[id = "width"]
    pub width: FloatParam,
    #[id = "mono_bass"]
    pub mono_bass: BoolParam,
    #[id = "saturation"]
    pub saturation: FloatParam,
    #[id = "gain"]
    pub gain: FloatParam,
    #[id = "ceiling"]
    pub ceiling: FloatParam,
    #[id = "target"]
    pub target: IntParam,
    /// Hears the song as it came in, at the same loudness as the master.
    #[id = "compare"]
    pub compare: BoolParam,
    pub live: Arc<LiveEdits>,
}

fn db_text(v: f32) -> String {
    format!("{v:+.1} dB")
}

fn parse_number(s: &str) -> Option<f32> {
    s.trim().trim_end_matches(['B', 'b', 'd', '%', ':', '1', ' ']).trim().parse().ok().or_else(|| s.trim().parse().ok())
}

fn float(name: &str, default: f32, min: f32, max: f32, step: f32, text: fn(f32) -> String) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min, max })
        .with_step_size(step)
        .with_value_to_string(Arc::new(move |v| text(v)))
        .with_string_to_value(Arc::new(parse_number))
}

impl Default for MasterParams {
    fn default() -> Self {
        let eq = |name| float(name, 0.0, -12.0, 12.0, 0.1, db_text);
        let comp = |name| {
            float(name, 0.0, -40.0, 0.0, 0.1, |v| if v >= COMP_OFF_DB { "Off".into() } else { format!("{v:.1} dB") })
        };
        MasterParams {
            eq_low: eq("EQ Low"),
            eq_low_mid: eq("EQ Low Mid"),
            eq_mid: eq("EQ Mid"),
            eq_high_mid: eq("EQ High Mid"),
            eq_high: eq("EQ High"),
            comp_low: comp("Comp Low"),
            comp_mid: comp("Comp Mid"),
            comp_high: comp("Comp High"),
            ratio: float("Ratio", 2.0, 1.0, 8.0, 0.1, |v| format!("{v:.1}:1")),
            width: float("Width", 100.0, 0.0, 200.0, 1.0, |v| format!("{v:.0} %")),
            mono_bass: BoolParam::new("Mono Bass", false),
            saturation: float("Saturation", 0.0, 0.0, 100.0, 1.0, |v| format!("{v:.0} %")),
            gain: float("Gain", 0.0, -12.0, 24.0, 0.1, db_text),
            ceiling: float("Ceiling", 0.0, -6.0, 0.0, 0.1, |v| format!("{v:.1} dB")),
            target: IntParam::new("Target", 0, IntRange::Linear { min: 0, max: TARGETS.len() as i32 - 1 })
                .with_value_to_string(Arc::new(|v| TARGETS[v.clamp(0, TARGETS.len() as i32 - 1) as usize].name.to_string()))
                .with_string_to_value(Arc::new(|s| TARGETS.iter().position(|t| t.name.eq_ignore_ascii_case(s.trim())).map(|i| i as i32))),
            compare: BoolParam::new("Compare", false),
            live: Arc::default(),
        }
    }
}

impl MasterParams {
    /// What the chain plays now (an editor edit the host hasn't confirmed yet wins).
    pub fn settings(&self) -> Settings {
        let l = &self.live;
        Settings {
            eq_db: [l.value(&self.eq_low), l.value(&self.eq_low_mid), l.value(&self.eq_mid), l.value(&self.eq_high_mid), l.value(&self.eq_high)],
            comp_db: [l.value(&self.comp_low), l.value(&self.comp_mid), l.value(&self.comp_high)],
            ratio: l.value(&self.ratio),
            width: l.value(&self.width) / 100.0,
            mono_bass: l.value(&self.mono_bass),
            saturation: l.value(&self.saturation) / 100.0,
            gain_db: l.value(&self.gain),
            ceiling_db: l.value(&self.ceiling),
        }
    }

    pub fn comparing(&self) -> bool {
        self.live.value(&self.compare)
    }
}
