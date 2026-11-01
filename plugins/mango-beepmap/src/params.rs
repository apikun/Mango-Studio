// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use nih_plug::prelude::*;
use os_plugkit::live::LiveEdits;

pub const SPEED: &str = "speed";
pub const OCTAVE: &str = "octave";
pub const BRIGHT: &str = "bright";
pub const GATE: &str = "gate";
pub const TONE: &str = "tone";
pub const VOLUME: &str = "volume";
const PATH_FIELD: &str = "beepmap-path";
pub const CONTROL_IDS: [&str; 6] = [SPEED, OCTAVE, BRIGHT, GATE, TONE, VOLUME];

pub struct BeepMapParams {
    /// The scan's time across the picture, in seconds.
    pub speed: FloatParam,
    /// The bottom edge's pitch, in octaves off the played key.
    pub octave: IntParam,
    /// The pixels' gamma: dark pictures puff up, bright ones thin out.
    pub bright: FloatParam,
    /// Pixels darker than this stay silent, as a share of full white.
    pub gate: FloatParam,
    /// The lowpass over the bank of sines.
    pub tone: FloatParam,
    pub volume: FloatParam,
    /// The image the state remembers ("" = none).
    pub path: RwLock<String>,
    pub live: Arc<LiveEdits>,
}

impl Default for BeepMapParams {
    fn default() -> Self {
        Self {
            speed: FloatParam::new("Scan time", 4.0, FloatRange::Skewed { min: 0.25, max: 16.0, factor: 0.35 }).with_unit(" s"),
            octave: IntParam::new("Octave", 0, IntRange::Linear { min: -3, max: 3 }),
            bright: FloatParam::new("Bright", 1.0, FloatRange::Skewed { min: 0.25, max: 4.0, factor: 0.3 }).with_unit("×"),
            gate: FloatParam::new("Gate", 5.0, FloatRange::Linear { min: 0.0, max: 50.0 }).with_unit(" %"),
            tone: FloatParam::new("Tone", 20_000.0, FloatRange::Skewed { min: 200.0, max: 20_000.0, factor: 0.2 }).with_unit(" Hz"),
            volume: FloatParam::new("Volume", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 }),
            path: RwLock::new(String::new()),
            live: Arc::default(),
        }
    }
}

impl BeepMapParams {
    pub fn speed_value(&self) -> f32 {
        self.live.value(&self.speed)
    }

    pub fn octave_value(&self) -> i32 {
        self.live.value(&self.octave)
    }

    pub fn bright_value(&self) -> f32 {
        self.live.value(&self.bright)
    }

    pub fn gate_value(&self) -> f32 {
        self.live.value(&self.gate)
    }

    pub fn tone_value(&self) -> f32 {
        self.live.value(&self.tone)
    }

    pub fn volume_value(&self) -> f32 {
        self.live.value(&self.volume)
    }
}

unsafe impl Params for BeepMapParams {
    fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
        vec![
            (SPEED.to_string(), self.speed.as_ptr(), String::new()),
            (OCTAVE.to_string(), self.octave.as_ptr(), String::new()),
            (BRIGHT.to_string(), self.bright.as_ptr(), String::new()),
            (GATE.to_string(), self.gate.as_ptr(), String::new()),
            (TONE.to_string(), self.tone.as_ptr(), String::new()),
            (VOLUME.to_string(), self.volume.as_ptr(), String::new()),
        ]
    }

    fn serialize_fields(&self) -> BTreeMap<String, String> {
        let path = serde_json::to_string(&*self.path.read().expect("BeepMap path lock poisoned")).expect("serializing a string cannot fail");
        BTreeMap::from([(PATH_FIELD.to_string(), path)])
    }

    fn deserialize_fields(&self, fields: &BTreeMap<String, String>) {
        if let Some(raw) = fields.get(PATH_FIELD) {
            match serde_json::from_str::<String>(raw) {
                Ok(path) => match self.path.write() {
                    Ok(mut saved) => *saved = path,
                    Err(_) => eprintln!("Mango BeepMap: saved image path lock is poisoned."),
                },
                Err(e) => eprintln!("Mango BeepMap: could not read saved image path: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_path_roundtrips_in_plugin_state() {
        let params = BeepMapParams::default();
        *params.path.write().unwrap() = "C:/Pictures/glow.png".into();

        let fields = params.serialize_fields();
        let restored = BeepMapParams::default();
        restored.deserialize_fields(&fields);

        assert_eq!(*restored.path.read().unwrap(), "C:/Pictures/glow.png");
    }

    #[test]
    fn the_six_controls_are_exposed_as_host_parameters() {
        let params = BeepMapParams::default();
        let ids: Vec<String> = params.param_map().into_iter().map(|(id, _, _)| id).collect();

        assert_eq!(ids, [SPEED, OCTAVE, BRIGHT, GATE, TONE, VOLUME]);
    }
}
