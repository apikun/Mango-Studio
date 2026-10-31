// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use nih_plug::prelude::*;
use os_plugkit::live::LiveEdits;

pub const PRESET: &str = "preset";
pub const VOLUME: &str = "volume";
pub const ATTACK: &str = "attack";
pub const DECAY: &str = "decay";
pub const SUSTAIN: &str = "sustain";
pub const RELEASE: &str = "release";
pub const LFO_DELAY: &str = "lfo_delay";
pub const LFO_AMOUNT: &str = "lfo_amount";
pub const LFO_SPEED: &str = "lfo_speed";
pub const CUTOFF: &str = "cutoff";
pub const MODULATION: &str = "modulation";
pub const REVERB_SEND: &str = "reverb_send";
pub const CHORUS_SEND: &str = "chorus_send";
const PATH_FIELD: &str = "soundfont-path";
pub const CONTROL_IDS: [&str; 11] = [
    REVERB_SEND,
    CHORUS_SEND,
    ATTACK,
    DECAY,
    SUSTAIN,
    RELEASE,
    LFO_DELAY,
    LFO_AMOUNT,
    LFO_SPEED,
    CUTOFF,
    MODULATION,
];

pub struct SoundFontParams {
    pub preset: IntParam,
    pub volume: FloatParam,
    pub attack: FloatParam,
    pub decay: FloatParam,
    pub sustain: FloatParam,
    pub release: FloatParam,
    pub lfo_delay: FloatParam,
    pub lfo_amount: FloatParam,
    pub lfo_speed: FloatParam,
    pub cutoff: FloatParam,
    pub modulation: FloatParam,
    pub reverb_send: FloatParam,
    pub chorus_send: FloatParam,
    pub path: RwLock<String>,
    pub live: Arc<LiveEdits>,
}

impl Default for SoundFontParams {
    fn default() -> Self {
        Self {
            preset: IntParam::new("Bank / preset", 0, IntRange::Linear { min: 0, max: 65535 }),
            volume: FloatParam::new("Volume", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 }),
            attack: FloatParam::new("Envelope attack", 0.0, FloatRange::Linear { min: 0.0, max: 5000.0 }).with_unit(" ms"),
            decay: FloatParam::new("Envelope decay", 0.0, FloatRange::Linear { min: 0.0, max: 5000.0 }).with_unit(" ms"),
            sustain: FloatParam::new("Envelope sustain", 100.0, FloatRange::Linear { min: 0.0, max: 100.0 }).with_unit(" %"),
            release: FloatParam::new("Envelope release", 0.0, FloatRange::Linear { min: 0.0, max: 5000.0 }).with_unit(" ms"),
            lfo_delay: FloatParam::new("LFO delay", 0.0, FloatRange::Linear { min: 0.0, max: 5000.0 }).with_unit(" ms"),
            lfo_amount: FloatParam::new("LFO amount", 0.0, FloatRange::Linear { min: 0.0, max: 100.0 }).with_unit(" %"),
            lfo_speed: FloatParam::new("LFO speed", 5.0, FloatRange::Skewed { min: 0.1, max: 20.0, factor: 0.35 }).with_unit(" Hz"),
            cutoff: FloatParam::new("Cutoff", 20_000.0, FloatRange::Skewed { min: 20.0, max: 20_000.0, factor: 0.2 }).with_unit(" Hz"),
            modulation: FloatParam::new("Modulation", 0.0, FloatRange::Linear { min: 0.0, max: 100.0 }).with_unit(" %"),
            reverb_send: FloatParam::new("Reverb send", 40.0 / 127.0 * 100.0, FloatRange::Linear { min: 0.0, max: 100.0 }).with_unit(" %"),
            chorus_send: FloatParam::new("Chorus send", 0.0, FloatRange::Linear { min: 0.0, max: 100.0 }).with_unit(" %"),
            path: RwLock::new(String::new()),
            live: Arc::default(),
        }
    }
}

impl SoundFontParams {
    pub fn preset_value(&self) -> i32 {
        self.live.value(&self.preset)
    }

    pub fn volume_value(&self) -> f32 {
        self.live.value(&self.volume)
    }

    pub fn attack_value(&self) -> f32 {
        self.live.value(&self.attack)
    }

    pub fn decay_value(&self) -> f32 {
        self.live.value(&self.decay)
    }

    pub fn sustain_value(&self) -> f32 {
        self.live.value(&self.sustain)
    }

    pub fn release_value(&self) -> f32 {
        self.live.value(&self.release)
    }

    pub fn lfo_delay_value(&self) -> f32 {
        self.live.value(&self.lfo_delay)
    }

    pub fn lfo_amount_value(&self) -> f32 {
        self.live.value(&self.lfo_amount)
    }

    pub fn lfo_speed_value(&self) -> f32 {
        self.live.value(&self.lfo_speed)
    }

    pub fn cutoff_value(&self) -> f32 {
        self.live.value(&self.cutoff)
    }

    pub fn modulation_value(&self) -> f32 {
        self.live.value(&self.modulation)
    }

    pub fn reverb_send_value(&self) -> f32 {
        self.live.value(&self.reverb_send)
    }

    pub fn chorus_send_value(&self) -> f32 {
        self.live.value(&self.chorus_send)
    }
}

unsafe impl Params for SoundFontParams {
    fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
        vec![
            (PRESET.to_string(), self.preset.as_ptr(), String::new()),
            (VOLUME.to_string(), self.volume.as_ptr(), String::new()),
            (ATTACK.to_string(), self.attack.as_ptr(), String::new()),
            (DECAY.to_string(), self.decay.as_ptr(), String::new()),
            (SUSTAIN.to_string(), self.sustain.as_ptr(), String::new()),
            (RELEASE.to_string(), self.release.as_ptr(), String::new()),
            (LFO_DELAY.to_string(), self.lfo_delay.as_ptr(), String::new()),
            (LFO_AMOUNT.to_string(), self.lfo_amount.as_ptr(), String::new()),
            (LFO_SPEED.to_string(), self.lfo_speed.as_ptr(), String::new()),
            (CUTOFF.to_string(), self.cutoff.as_ptr(), String::new()),
            (MODULATION.to_string(), self.modulation.as_ptr(), String::new()),
            (REVERB_SEND.to_string(), self.reverb_send.as_ptr(), String::new()),
            (CHORUS_SEND.to_string(), self.chorus_send.as_ptr(), String::new()),
        ]
    }

    fn serialize_fields(&self) -> BTreeMap<String, String> {
        let path = serde_json::to_string(&*self.path.read().expect("SoundFont path lock poisoned")).expect("serializing a string cannot fail");
        BTreeMap::from([(PATH_FIELD.to_string(), path)])
    }

    fn deserialize_fields(&self, fields: &BTreeMap<String, String>) {
        if let Some(raw) = fields.get(PATH_FIELD) {
            match serde_json::from_str::<String>(raw) {
                Ok(path) => match self.path.write() {
                    Ok(mut saved) => *saved = path,
                    Err(_) => eprintln!("Mango Font: saved SoundFont path lock is poisoned."),
                },
                Err(e) => eprintln!("Mango Font: could not read saved SoundFont path: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soundfont_path_roundtrips_in_plugin_state() {
        let params = SoundFontParams::default();
        *params.path.write().unwrap() = "C:/Sounds/Grand Piano.sf2".into();

        let fields = params.serialize_fields();
        let restored = SoundFontParams::default();
        restored.deserialize_fields(&fields);

        assert_eq!(*restored.path.read().unwrap(), "C:/Sounds/Grand Piano.sf2");
    }

    #[test]
    fn fl_style_sound_controls_are_exposed_as_host_parameters() {
        let params = SoundFontParams::default();
        let ids: Vec<String> = params.param_map().into_iter().map(|(id, _, _)| id).collect();

        assert_eq!(
            ids,
            [
                PRESET,
                VOLUME,
                ATTACK,
                DECAY,
                SUSTAIN,
                RELEASE,
                LFO_DELAY,
                LFO_AMOUNT,
                LFO_SPEED,
                CUTOFF,
                MODULATION,
                REVERB_SEND,
                CHORUS_SEND,
            ]
        );
    }
}
