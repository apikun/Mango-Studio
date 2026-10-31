// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

use nih_plug::prelude::*;
use os_plugkit::live::LiveEdits;
use std::sync::Arc;

use crate::dsp::Settings;
use crate::filter::FilterType;
use crate::warp::WarpMode;

pub const IDS: [&str; 25] = [
    "wt_pos", "table", "attack", "decay", "sustain", "release", "output",
    "unison", "detune", "blend", "spread", "phase_rand", "warp_mode", "warp_amt",
    "filter_type", "cutoff", "resonance", "drive",
    "env2_attack", "env2_decay", "env2_sustain", "env2_release",
    "lfo1_rate", "mod_lfo1_wt", "mod_env2_cutoff",
];

#[derive(Params)]
pub struct WavetableParams {
    #[id = "wt_pos"]
    pub wt_pos: FloatParam,

    #[id = "table"]
    pub table: IntParam,

    #[id = "attack"]
    pub attack: FloatParam,

    #[id = "decay"]
    pub decay: FloatParam,

    #[id = "sustain"]
    pub sustain: FloatParam,

    #[id = "release"]
    pub release: FloatParam,

    #[id = "output"]
    pub output: FloatParam,

    // --- Unison & Warp ---
    #[id = "unison"]
    pub unison: IntParam,

    #[id = "detune"]
    pub detune: FloatParam,

    #[id = "blend"]
    pub blend: FloatParam,

    #[id = "spread"]
    pub spread: FloatParam,

    #[id = "phase_rand"]
    pub phase_rand: FloatParam,

    #[id = "warp_mode"]
    pub warp_mode: IntParam,

    #[id = "warp_amt"]
    pub warp_amt: FloatParam,

    // --- Filter Section ---
    #[id = "filter_type"]
    pub filter_type: IntParam,

    #[id = "cutoff"]
    pub cutoff: FloatParam,

    #[id = "resonance"]
    pub resonance: FloatParam,

    #[id = "drive"]
    pub drive: FloatParam,

    // --- Modulation Envelope 2 ---
    #[id = "env2_attack"]
    pub env2_attack: FloatParam,

    #[id = "env2_decay"]
    pub env2_decay: FloatParam,

    #[id = "env2_sustain"]
    pub env2_sustain: FloatParam,

    #[id = "env2_release"]
    pub env2_release: FloatParam,

    // --- Modulation LFO 1 & Routes ---
    #[id = "lfo1_rate"]
    pub lfo1_rate: FloatParam,

    #[id = "mod_lfo1_wt"]
    pub mod_lfo1_wt: FloatParam,

    #[id = "mod_env2_cutoff"]
    pub mod_env2_cutoff: FloatParam,

    pub live: Arc<LiveEdits>,
}

impl Default for WavetableParams {
    fn default() -> Self {
        Self {
            wt_pos: FloatParam::new("WT Pos", 32.0, FloatRange::Linear { min: 0.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            table: IntParam::new("Wavetable", 0, IntRange::Linear { min: 0, max: 2 })
                .with_value_to_string(Arc::new(|v| match v {
                    0 => "Basic Shapes".to_string(),
                    1 => "Analog Saw".to_string(),
                    2 => "Digital Bell".to_string(),
                    _ => format!("{v}"),
                })),

            attack: FloatParam::new("Amp Attack", 12.0, FloatRange::Skewed { min: 1.0, max: 2000.0, factor: 0.25 })
                .with_unit(" ms")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            decay: FloatParam::new("Amp Decay", 450.0, FloatRange::Skewed { min: 10.0, max: 3000.0, factor: 0.35 })
                .with_unit(" ms")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            sustain: FloatParam::new("Amp Sustain", 70.0, FloatRange::Linear { min: 0.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            release: FloatParam::new("Amp Release", 250.0, FloatRange::Skewed { min: 10.0, max: 5000.0, factor: 0.35 })
                .with_unit(" ms")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            output: FloatParam::new("Output", -12.0, FloatRange::Linear { min: -24.0, max: 0.0 })
                .with_unit(" dB")
                .with_value_to_string(Arc::new(|v| format!("{v:.1}"))),

            unison: IntParam::new("Unison Voices", 7, IntRange::Linear { min: 1, max: 16 }),

            detune: FloatParam::new("Detune", 18.0, FloatRange::Linear { min: 0.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            blend: FloatParam::new("Unison Blend", 80.0, FloatRange::Linear { min: 0.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            spread: FloatParam::new("Stereo Spread", 100.0, FloatRange::Linear { min: 0.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            phase_rand: FloatParam::new("Phase Random", 100.0, FloatRange::Linear { min: 0.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            warp_mode: IntParam::new("Warp Mode", 0, IntRange::Linear { min: 0, max: 6 })
                .with_value_to_string(Arc::new(|v| match v {
                    1 => "Sync".to_string(),
                    2 => "Bend +".to_string(),
                    3 => "Bend -".to_string(),
                    4 => "PWM".to_string(),
                    5 => "Asym +".to_string(),
                    6 => "Flip".to_string(),
                    _ => "Off".to_string(),
                })),

            warp_amt: FloatParam::new("Warp Amount", 0.0, FloatRange::Linear { min: 0.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            filter_type: IntParam::new("Filter Type", 2, IntRange::Linear { min: 0, max: 5 })
                .with_value_to_string(Arc::new(|v| match v {
                    1 => "Lowpass 12dB".to_string(),
                    2 => "Lowpass 24dB".to_string(),
                    3 => "Highpass 12dB".to_string(),
                    4 => "Bandpass 12dB".to_string(),
                    5 => "Peak".to_string(),
                    _ => "Off".to_string(),
                })),

            cutoff: FloatParam::new("Cutoff", 2400.0, FloatRange::Skewed { min: 20.0, max: 20000.0, factor: 0.2 })
                .with_unit(" Hz")
                .with_value_to_string(Arc::new(|v| if v >= 1000.0 { format!("{:.1} kHz", v / 1000.0) } else { format!("{v:.0} Hz") })),

            resonance: FloatParam::new("Resonance", 40.0, FloatRange::Linear { min: 0.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            drive: FloatParam::new("Filter Drive", 15.0, FloatRange::Linear { min: 0.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            env2_attack: FloatParam::new("Env 2 Attack", 10.0, FloatRange::Skewed { min: 1.0, max: 2000.0, factor: 0.25 })
                .with_unit(" ms")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            env2_decay: FloatParam::new("Env 2 Decay", 350.0, FloatRange::Skewed { min: 10.0, max: 3000.0, factor: 0.35 })
                .with_unit(" ms")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            env2_sustain: FloatParam::new("Env 2 Sustain", 20.0, FloatRange::Linear { min: 0.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            env2_release: FloatParam::new("Env 2 Release", 200.0, FloatRange::Skewed { min: 10.0, max: 5000.0, factor: 0.35 })
                .with_unit(" ms")
                .with_value_to_string(Arc::new(|v| format!("{v:.0}"))),

            lfo1_rate: FloatParam::new("LFO 1 Rate", 2.0, FloatRange::Skewed { min: 0.1, max: 20.0, factor: 0.35 })
                .with_unit(" Hz")
                .with_value_to_string(Arc::new(|v| format!("{v:.2}"))),

            mod_lfo1_wt: FloatParam::new("LFO1 -> WT Pos", 30.0, FloatRange::Linear { min: -100.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:+.0}"))),

            mod_env2_cutoff: FloatParam::new("Env2 -> Cutoff", 50.0, FloatRange::Linear { min: -100.0, max: 100.0 })
                .with_unit(" %")
                .with_value_to_string(Arc::new(|v| format!("{v:+.0}"))),

            live: Arc::new(LiveEdits::default()),
        }
    }
}

impl WavetableParams {
    pub fn settings(&self) -> Settings {
        let db = self.live.value(&self.output);
        let linear_gain = 10.0f32.powf(db / 20.0);

        Settings {
            wt_pos: (self.live.value(&self.wt_pos) / 100.0).clamp(0.0, 1.0),
            table_idx: self.table.value() as usize,
            level: linear_gain,
            attack: (self.live.value(&self.attack) / 1000.0).max(0.001),
            decay: (self.live.value(&self.decay) / 1000.0).max(0.001),
            sustain: (self.live.value(&self.sustain) / 100.0).clamp(0.0, 1.0),
            release: (self.live.value(&self.release) / 1000.0).max(0.001),
            unison: self.unison.value() as usize,
            detune: (self.live.value(&self.detune) / 100.0).clamp(0.0, 1.0),
            blend: (self.live.value(&self.blend) / 100.0).clamp(0.0, 1.0),
            spread: (self.live.value(&self.spread) / 100.0).clamp(0.0, 1.0),
            phase_rand: (self.live.value(&self.phase_rand) / 100.0).clamp(0.0, 1.0),
            warp_mode: WarpMode::from(self.warp_mode.value()),
            warp_amt: (self.live.value(&self.warp_amt) / 100.0).clamp(0.0, 1.0),
            filter_type: FilterType::from(self.filter_type.value()),
            cutoff: self.live.value(&self.cutoff).clamp(20.0, 20000.0),
            resonance: (self.live.value(&self.resonance) / 100.0).clamp(0.0, 1.0),
            drive: (self.live.value(&self.drive) / 100.0).clamp(0.0, 1.0),
            env2_attack: (self.live.value(&self.env2_attack) / 1000.0).max(0.001),
            env2_decay: (self.live.value(&self.env2_decay) / 1000.0).max(0.001),
            env2_sustain: (self.live.value(&self.env2_sustain) / 100.0).clamp(0.0, 1.0),
            env2_release: (self.live.value(&self.env2_release) / 1000.0).max(0.001),
            lfo1_rate: self.live.value(&self.lfo1_rate).max(0.01),
            mod_lfo1_wt: self.live.value(&self.mod_lfo1_wt) / 100.0,
            mod_env2_cutoff: self.live.value(&self.mod_env2_cutoff) / 100.0,
        }
    }
}
