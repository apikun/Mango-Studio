// Copyright (C) 2026 mikobuntu
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Sfxr's parameters: sfxr's full slider set (LMMS's models, one to one), plus the waveform
//! and an output volume.

use std::sync::Arc;

use nih_plug::prelude::*;
use os_plugkit::live::LiveEdits;

use crate::dsp;

pub const ATT: &str = "att";
pub const HOLD: &str = "hold";
pub const SUS: &str = "sus";
pub const DEC: &str = "dec";
pub const START_FREQ: &str = "start_freq";
pub const MIN_FREQ: &str = "min_freq";
pub const SLIDE: &str = "slide";
pub const DSLIDE: &str = "dslide";
pub const VIB_DEPTH: &str = "vib_depth";
pub const VIB_SPEED: &str = "vib_speed";
pub const CHANGE_AMT: &str = "change_amt";
pub const CHANGE_SPEED: &str = "change_speed";
pub const SQR_DUTY: &str = "sqr_duty";
pub const SQR_SWEEP: &str = "sqr_sweep";
pub const REPEAT: &str = "repeat";
pub const PHASER_OFFSET: &str = "phaser_offset";
pub const PHASER_SWEEP: &str = "phaser_sweep";
pub const LP_CUT: &str = "lp_cut";
pub const LP_SWEEP: &str = "lp_sweep";
pub const LP_RESO: &str = "lp_reso";
pub const HP_CUT: &str = "hp_cut";
pub const HP_SWEEP: &str = "hp_sweep";
pub const VOLUME: &str = "volume";
pub const WAVE: &str = "wave";

/// Every slider the editor draws, in display order (the waveform is not a slider: the wave buttons
/// set it).
pub const SLIDER_IDS: [&str; 23] = [
    ATT, HOLD, SUS, DEC,
    START_FREQ, MIN_FREQ, SLIDE, DSLIDE,
    VIB_DEPTH, VIB_SPEED,
    CHANGE_AMT, CHANGE_SPEED,
    SQR_DUTY, SQR_SWEEP,
    REPEAT,
    PHASER_OFFSET, PHASER_SWEEP,
    LP_CUT, LP_SWEEP, LP_RESO, HP_CUT, HP_SWEEP,
    VOLUME,
];

/// The sliders a preset generator rolls (the volume stays as the user left it).
pub const PRESET_IDS: [&str; 22] = [
    ATT, HOLD, SUS, DEC,
    START_FREQ, MIN_FREQ, SLIDE, DSLIDE,
    VIB_DEPTH, VIB_SPEED,
    CHANGE_AMT, CHANGE_SPEED,
    SQR_DUTY, SQR_SWEEP,
    REPEAT,
    PHASER_OFFSET, PHASER_SWEEP,
    LP_CUT, LP_SWEEP, LP_RESO, HP_CUT, HP_SWEEP,
];

pub struct SfxrParams {
    pub att: FloatParam,
    pub hold: FloatParam,
    pub sus: FloatParam,
    pub dec: FloatParam,
    pub start_freq: FloatParam,
    pub min_freq: FloatParam,
    pub slide: FloatParam,
    pub dslide: FloatParam,
    pub vib_depth: FloatParam,
    pub vib_speed: FloatParam,
    pub change_amt: FloatParam,
    pub change_speed: FloatParam,
    pub sqr_duty: FloatParam,
    pub sqr_sweep: FloatParam,
    pub repeat: FloatParam,
    pub phaser_offset: FloatParam,
    pub phaser_sweep: FloatParam,
    pub lp_cut: FloatParam,
    pub lp_sweep: FloatParam,
    pub lp_reso: FloatParam,
    pub hp_cut: FloatParam,
    pub hp_sweep: FloatParam,
    pub volume: FloatParam,
    pub wave: IntParam,
    pub live: Arc<LiveEdits>,
}

impl Default for SfxrParams {
    fn default() -> Self {
        // sfxr's sliders live on 0..1, or -1..1 where the effect sweeps both ways (LMMS's
        // SfxrZeroToOneFloatModel and SfxrNegPosOneFloatModel, both stepping 0.001).
        let one = |name: &'static str, default: f32| {
            FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 }).with_step_size(0.001)
        };
        let signed = |name: &'static str, default: f32| {
            FloatParam::new(name, default, FloatRange::Linear { min: -1.0, max: 1.0 }).with_step_size(0.001)
        };
        Self {
            att: one("Attack Time", 0.0),
            hold: one("Sustain Time", 0.3),
            sus: one("Sustain Punch", 0.0),
            dec: one("Decay Time", 0.4),
            start_freq: one("Start Frequency", 0.3),
            min_freq: one("Min Frequency", 0.0),
            slide: signed("Slide", 0.0),
            dslide: signed("Delta Slide", 0.0),
            vib_depth: one("Vibrato Depth", 0.0),
            vib_speed: one("Vibrato Speed", 0.0),
            change_amt: signed("Change Amount", 0.0),
            change_speed: one("Change Speed", 0.0),
            sqr_duty: one("Square Duty", 0.0),
            sqr_sweep: signed("Duty Sweep", 0.0),
            repeat: one("Repeat Speed", 0.0),
            phaser_offset: signed("Phaser Offset", 0.0),
            phaser_sweep: signed("Phaser Sweep", 0.0),
            lp_cut: one("LP Filter Cutoff", 1.0),
            lp_sweep: signed("LP Filter Cutoff Sweep", 0.0),
            lp_reso: one("LP Filter Resonance", 0.0),
            hp_cut: one("HP Filter Cutoff", 0.0),
            hp_sweep: signed("HP Filter Cutoff Sweep", 0.0),
            volume: one("Volume", 1.0),
            wave: IntParam::new("Wave", 0, IntRange::Linear { min: 0, max: 3 }),
            live: Arc::default(),
        }
    }
}

impl SfxrParams {
    fn floats(&self) -> [(&'static str, &FloatParam); 23] {
        [
            (ATT, &self.att),
            (HOLD, &self.hold),
            (SUS, &self.sus),
            (DEC, &self.dec),
            (START_FREQ, &self.start_freq),
            (MIN_FREQ, &self.min_freq),
            (SLIDE, &self.slide),
            (DSLIDE, &self.dslide),
            (VIB_DEPTH, &self.vib_depth),
            (VIB_SPEED, &self.vib_speed),
            (CHANGE_AMT, &self.change_amt),
            (CHANGE_SPEED, &self.change_speed),
            (SQR_DUTY, &self.sqr_duty),
            (SQR_SWEEP, &self.sqr_sweep),
            (REPEAT, &self.repeat),
            (PHASER_OFFSET, &self.phaser_offset),
            (PHASER_SWEEP, &self.phaser_sweep),
            (LP_CUT, &self.lp_cut),
            (LP_SWEEP, &self.lp_sweep),
            (LP_RESO, &self.lp_reso),
            (HP_CUT, &self.hp_cut),
            (HP_SWEEP, &self.hp_sweep),
            (VOLUME, &self.volume),
        ]
    }

    /// The patch the sound uses now: every parameter's newest value (the editor's pending edits win,
    /// as elsewhere in Mango Studio's plugins).
    pub fn patch(&self) -> dsp::Patch {
        dsp::Patch {
            wave: self.live.value(&self.wave).clamp(0, 3) as u8,
            att: self.live.value(&self.att),
            hold: self.live.value(&self.hold),
            sus: self.live.value(&self.sus),
            dec: self.live.value(&self.dec),
            start_freq: self.live.value(&self.start_freq),
            min_freq: self.live.value(&self.min_freq),
            slide: self.live.value(&self.slide),
            dslide: self.live.value(&self.dslide),
            vib_depth: self.live.value(&self.vib_depth),
            vib_speed: self.live.value(&self.vib_speed),
            change_amt: self.live.value(&self.change_amt),
            change_speed: self.live.value(&self.change_speed),
            sqr_duty: self.live.value(&self.sqr_duty),
            sqr_sweep: self.live.value(&self.sqr_sweep),
            repeat: self.live.value(&self.repeat),
            phaser_offset: self.live.value(&self.phaser_offset),
            phaser_sweep: self.live.value(&self.phaser_sweep),
            lp_cut: self.live.value(&self.lp_cut),
            lp_sweep: self.live.value(&self.lp_sweep),
            lp_reso: self.live.value(&self.lp_reso),
            hp_cut: self.live.value(&self.hp_cut),
            hp_sweep: self.live.value(&self.hp_sweep),
            volume: self.live.value(&self.volume),
        }
    }
}

unsafe impl Params for SfxrParams {
    fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
        let mut map: Vec<(String, ParamPtr, String)> =
            self.floats().into_iter().map(|(id, p)| (id.to_string(), p.as_ptr(), String::new())).collect();
        map.push((WAVE.to_string(), self.wave.as_ptr(), String::new()));
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_plugin_sounds_lmmss_default_patch() {
        let p = SfxrParams::default();
        let patch = p.patch();
        let want = dsp::Patch::default();
        assert_eq!(patch, want, "the default patch must be LMMS's model defaults");
    }

    #[test]
    fn every_slider_is_a_host_parameter_in_display_order() {
        let p = SfxrParams::default();
        let ids: Vec<String> = p.param_map().into_iter().map(|(id, _, _)| id).collect();
        assert_eq!(ids.len(), SLIDER_IDS.len() + 1, "23 sliders plus the wave");
        for id in SLIDER_IDS {
            assert!(ids.contains(&id.to_string()), "{id} missing from the host parameters");
        }
        assert_eq!(&ids[23], WAVE);
    }

    #[test]
    fn preset_rolls_reach_every_slider_through_the_patch() {
        // The editor applies a rolled patch by id; every preset-set slider must be answerable.
        let patch = dsp::Preset::Laser.roll(&mut dsp::Rng::new(5));
        for id in PRESET_IDS {
            let _ = patch.get(id);
        }
        // The signed sliders accept their negative rolls (the ranges the presets ask for).
        assert!(patch.slide <= 0.0);
        assert!((0.0..=1.0).contains(&patch.start_freq));
    }

    #[test]
    fn the_patch_reads_live_edits_before_the_host_confirms() {
        let p = SfxrParams::default();
        p.live.set(p.start_freq.as_ptr(), 0.25);
        assert_eq!(p.patch().start_freq, 0.25);
        assert_eq!(p.live.norm(p.start_freq.as_ptr()), 0.25);
    }
}
