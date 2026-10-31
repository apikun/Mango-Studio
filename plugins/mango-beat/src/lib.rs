// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Beat: Mango Studio's Gross Beat-style time manipulator as a VST3 / CLAP plugin (nih-plug). Two envelope
//! grids per beat — time (the playback rate, from +16x through the centre's 1x down to -16x) and volume (the gate) —
//! in four switchable slots each, drawn in the editor like the playlist draws automation.
//!
// spec: plugins/mango-beat.md
#![allow(semicolon_in_expressions_from_non_local_macros)]

pub mod dsp;
pub mod editor;
pub mod grid;
pub mod params;
pub mod view;
mod window;

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use nih_plug::prelude::*;

use params::BeatParams;

/// What the audio thread and the editor share.
pub struct Shared {
    pub rate: AtomicU32,
    /// Set while an editor is open.
    pub editor_open: AtomicBool,
}

impl Default for Shared {
    fn default() -> Self {
        Shared { rate: AtomicU32::new(44100), editor_open: AtomicBool::new(false) }
    }
}

pub struct MangoBeat {
    params: Arc<BeatParams>,
    shared: Arc<Shared>,
    /// Made in `initialize` (its ring is sized for the sample rate's budget).
    dsp: Option<dsp::Beat>,
    /// The grid version the sound has seen (the editor's edits move it).
    grids_seen: u64,
}

impl Default for MangoBeat {
    fn default() -> Self {
        MangoBeat { params: Arc::new(BeatParams::default()), shared: Arc::new(Shared::default()), dsp: None, grids_seen: u64::MAX }
    }
}

impl MangoBeat {
    /// The grids the sound plays, refreshed when the editor or a preset moved them.
    fn sync_grids(&mut self) {
        let v = self.params.grid.version();
        if v != self.grids_seen {
            self.grids_seen = v;
            let g = self.params.grid.load();
            if let Some(d) = &mut self.dsp {
                d.set_grids(g);
            }
        }
    }
}

impl Plugin for MangoBeat {
    const NAME: &'static str = "Mango Beat";
    const VENDOR: &'static str = "Mango Studio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] =
        &[AudioIOLayout { main_input_channels: NonZeroU32::new(2), main_output_channels: NonZeroU32::new(2), ..AudioIOLayout::const_default() }];
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(editor::BeatEditor::new(self.params.clone(), self.shared.clone())))
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, config: &BufferConfig, _ctx: &mut impl InitContext<Self>) -> bool {
        self.shared.rate.store(config.sample_rate as u32, Ordering::Relaxed);
        self.grids_seen = u64::MAX;
        let rate = config.sample_rate.round() as u32;
        let s = self.params.settings();
        match &mut self.dsp {
            // nih-plug initializes the plugin again after the host loads a state: keep the ring, take the grids.
            Some(d) => {
                d.set_sample_rate(rate);
                d.set(&s);
            }
            None => {
                let mut d = dsp::Beat::new(rate);
                d.set(&s);
                self.dsp = Some(d);
            }
        }
        true
    }

    fn reset(&mut self) {
        if let Some(d) = &mut self.dsp {
            d.reset();
        }
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, ctx: &mut impl ProcessContext<Self>) -> ProcessStatus {
        self.sync_grids();
        let Some(dsp) = &mut self.dsp else { return ProcessStatus::Normal };
        let s = self.params.settings();
        dsp.set(&s);
        let t = ctx.transport();
        if let Some(bpm) = t.tempo {
            dsp.set_tempo(bpm);
        }
        // Playing, the envelopes follow the host's beats; otherwise they free-run (the tail after a stop).
        let transport = t.playing.then(|| t.pos_beats()).flatten().map(|beats| dsp::TransportStep { playing: true, beats });
        let out = buffer.as_slice();
        if out.len() < 2 {
            return ProcessStatus::KeepAlive;
        }
        let (left, right) = out.split_at_mut(1);
        dsp.process(left[0], right[0], transport);
        // The tails ring on after the input stops (a stopped read head still has a past to drain).
        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for MangoBeat {
    const CLAP_ID: &'static str = "org.openstudio.mango-beat";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Mango Studio's time manipulator: half time, gates and scratches on two envelope grids");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Glitch, ClapFeature::Gate, ClapFeature::Stereo];
}

impl Vst3Plugin for MangoBeat {
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioBeat\0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Modulation];
}

// Built as a plugin module: the C entry points hosts look for. Linked into Mango Studio (`linked` feature): only the
// symbol-free table, so several Mango plugins fit in one program (spec: plugins/plugin-formats.md).
#[cfg(not(feature = "linked"))]
nih_export_clap!(MangoBeat);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(MangoBeat);
os_plugkit::linked_clap_entry!(MangoBeat);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Curve, GridState, Mode, Pt};
    use crate::params::{GRID_FIELD, TIME_SLOT, VOL_SLOT};

    /// A fresh Mango Beat's settings are the defaults, and slot edits reach them.
    #[test]
    fn editor_edits_reach_the_sound() {
        let mut p = MangoBeat::default();
        assert_eq!(p.params.settings(), dsp::Settings::default());
        p.params.live.set(p.params.p[TIME_SLOT].as_ptr(), 1.0 / 3.0);
        assert_eq!(p.params.settings().time_slot, 1);
        // A grid edit through the state's field reaches the shared grids.
        let mut g = GridState::default();
        g.time[1] = Curve { pts: vec![Pt::new(0.0, 0.375, 0.0, Mode::Single as u8)] };
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(GRID_FIELD.to_string(), g.to_json());
        let params = BeatParams::default();
        params.deserialize_fields(&fields);
        assert_eq!(params.grid.load(), g);
        // The serialized fields carry the grids back out.
        let out = params.serialize_fields();
        assert_eq!(GridState::from_json(out.get(GRID_FIELD).unwrap()).unwrap(), g);
    }

    /// The factory presets (written by `examples/presets.rs`) hold states this plugin reads: the header names the
    /// plugin, the blob carries its length prefix and the grids parse.
    #[test]
    fn factory_presets_hold_readable_grids() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/os-app/factory-presets/Mango Beat");
        let mut count = 0;
        for file in std::fs::read_dir(&dir).expect("preset folder") {
            let path = file.expect("file").path();
            if path.extension().and_then(|e| e.to_str()) != Some("ospreset") {
                continue;
            }
            count += 1;
            let bytes = std::fs::read(&path).expect("read");
            assert!(bytes.starts_with(b"OSPRESET 1\n"), "{}: not a preset", path.display());
            assert!(bytes.windows(17).any(|w| w == b"plugin=Mango Beat"), "{}: wrong plugin", path.display());
            // The body: after the magic and the header line, the length prefix and the state JSON.
            let Some(at) = bytes.iter().enumerate().filter(|(_, b)| **b == b'\n').nth(1).map(|(i, _)| i + 1) else {
                panic!("{}: no header", path.display())
            };
            let body = &bytes[at..];
            assert!(body.len() > 8, "{}: no state", path.display());
            let len = u64::from_le_bytes(body[..8].try_into().unwrap()) as usize;
            assert_eq!(len, body.len() - 8, "{}: the length prefix matches", path.display());
            let json: serde_json::Value = serde_json::from_slice(&body[8..]).expect("state JSON");
            let grid = json["fields"]["grid"].as_str().expect("grid field");
            let grids = GridState::from_json(grid).expect("grids read");
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name == "Half Time.ospreset" {
                assert_eq!(grids.time[0].pts.len(), 1);
                assert!((GridState::rate_of_y(grids.time[0].pts[0].y) - 0.5).abs() < 1e-3, "half time plays at half");
            }
            if name == "Trance Gate 1-8.ospreset" {
                assert_eq!(grids.volume[0].pts.len(), 4, "two on-off steps per beat");
            }
        }
        assert!(count >= 10, "the factory presets are there ({count})");
    }

    /// The sound follows the grids it is given: a store in the shared grids reaches it through `sync_grids`.
    #[test]
    fn the_sound_picks_up_grid_edits_through_the_shared_state() {
        let mut p = MangoBeat::default();
        p.dsp = Some(dsp::Beat::new(48000));
        let mut g = GridState::default();
        g.volume[0] = dsp::gate(2);
        p.params.grid.store(g);
        p.grids_seen = u64::MAX;
        p.sync_grids();
        let d = p.dsp.as_ref().unwrap();
        assert_eq!(d.gain_at(0.1), 1.0, "open on the first half of the step");
        assert_eq!(d.gain_at(0.3), 0.0, "closed on the second half");
        // And once seen, the same version does not copy again.
        p.params.grid.store(GridState::default());
        p.sync_grids();
        assert_eq!(p.dsp.as_ref().unwrap().gain_at(0.3), 1.0, "back to the transparent default");
    }
}
