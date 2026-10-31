// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango DirectWave: Mango Studio's sampler (a recreation of FL's DirectWave) as a VST3 / CLAP plugin (nih-plug).
//! Eight zones, each one sample and where it plays — its key range and the key it is tuned to, its velocity range,
//! its gain, pan, tune and loop switch — through the master's envelope and filter. A folder of samples spread over
//! the keyboard plays like a program; the editor loads, lays out and previews it.
//!
// spec: plugins/mango-directwave.md
#![allow(semicolon_in_expressions_from_non_local_macros)]

pub mod dsp;
pub mod editor;
pub mod params;
pub mod program;
pub mod view;

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nih_plug::prelude::*;
use os_engine::sample::Sample;

use params::WaveParams;
use program::Program;

/// What the audio thread and the editor share.
#[derive(Default)]
pub struct Shared {
    /// Notes the editor plays itself (key, velocity).
    pub previews: Mutex<Vec<(u8, f32)>>,
    /// What the voices play; `program_version` moves on every change.
    pub kit: Mutex<Option<Arc<dsp::Kit>>>,
    pub program_version: AtomicU64,
    /// Samples opened, by file (also the ones that could not be read: `None`).
    cache: Mutex<HashMap<String, Option<Arc<Sample>>>>,
    /// Set while an editor is open.
    pub editor_open: AtomicBool,
}

impl Shared {
    /// Opens `file` once (kept for the next rebuild).
    fn sample(&self, file: &str) -> Option<Arc<Sample>> {
        if let Some(s) = self.cache.lock().ok().and_then(|c| c.get(file).cloned()) {
            return s;
        }
        let s = os_engine::sample::load_shared(Path::new(file)).ok();
        if let Ok(mut c) = self.cache.lock() {
            c.insert(file.to_string(), s.clone());
        }
        s
    }

    /// Rebuilds what the voices play from the saved program, opening the samples it names.
    pub fn rebuild(&self, params: &WaveParams) {
        let p = params.program();
        let kit = dsp::Kit::new(&p, &mut |_, stored| self.sample(stored));
        if let Ok(mut k) = self.kit.lock() {
            *k = Some(Arc::new(kit));
        }
        self.program_version.fetch_add(1, Ordering::AcqRel);
    }

    /// Whether zone `i`'s sample opened.
    pub fn loaded(&self, i: usize) -> bool {
        self.kit
            .lock()
            .ok()
            .and_then(|k| k.as_ref().and_then(|k| k.zones.get(i)).map(|z| z.sample.is_some()))
            .unwrap_or(false)
    }

    /// The first sample zone `i` plays (its name in the list, its wave in the editor).
    pub fn zone_sample(&self, i: usize) -> Option<Arc<Sample>> {
        self.kit.lock().ok()?.as_ref()?.zones.get(i)?.sample.clone()
    }

    /// Replaces the saved program and plays it.
    pub fn set_program(&self, params: &WaveParams, p: &Program) {
        params.set_program(p);
        self.rebuild(params);
    }

    /// Puts sample file `path` in zone `i` (the editor's load, a file dropped on the editor).
    pub fn load(&self, params: &WaveParams, i: usize, path: &str) -> Result<(), String> {
        os_engine::sample::load_shared(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
        if let Ok(mut c) = self.cache.lock() {
            c.remove(path);
        }
        let mut p = params.program();
        if i >= p.zones.len() {
            p.zones.resize(i + 1, program::Zone::default());
        }
        p.zones[i] = program::Zone { path: path.to_string(), ..p.zone(i) };
        self.set_program(params, &p);
        Ok(())
    }
}

pub struct MangoDirectWave {
    params: Arc<WaveParams>,
    shared: Arc<Shared>,
    engine: dsp::Engine,
    kit_seen: u64,
}

impl Default for MangoDirectWave {
    fn default() -> Self {
        MangoDirectWave {
            params: Arc::new(WaveParams::default()),
            shared: Arc::new(Shared::default()),
            engine: dsp::Engine::new(44100.0),
            kit_seen: u64::MAX,
        }
    }
}

impl MangoDirectWave {
    /// The voices take the kit last built (never waits: a busy lock is tried again next block).
    fn take_kit(&mut self) {
        let v = self.shared.program_version.load(Ordering::Acquire);
        if v != self.kit_seen
            && let Ok(k) = self.shared.kit.try_lock()
        {
            let kit = k.clone();
            drop(k);
            self.engine.set_kit(kit);
            self.kit_seen = v;
        }
    }
}

impl Plugin for MangoDirectWave {
    const NAME: &'static str = "Mango DirectWave";
    const VENDOR: &'static str = "Mango Studio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(editor::WaveEditor::new(self.params.clone(), self.shared.clone())))
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, config: &BufferConfig, _ctx: &mut impl InitContext<Self>) -> bool {
        self.engine.set_rate(config.sample_rate as f32);
        // A restored program names its samples: open them now (initialize may block).
        self.shared.rebuild(&self.params);
        self.take_kit();
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, ctx: &mut impl ProcessContext<Self>) -> ProcessStatus {
        self.take_kit();
        if let Ok(mut q) = self.shared.previews.try_lock() {
            for (key, vel) in q.drain(..) {
                self.engine.note_on(key, vel, None);
            }
        }
        // The editor's previews end unreported.
        self.engine.take_ended();
        let len = buffer.samples();
        let out = buffer.as_slice();
        for ch in out.iter_mut() {
            ch.fill(0.0);
        }
        if out.len() < 2 {
            return ProcessStatus::Normal;
        }
        let mut at = 0;
        let mut ended = Vec::new();
        while let Some(ev) = ctx.next_event() {
            let t = (ev.timing() as usize).min(len);
            if t > at {
                let (left, right) = out.split_at_mut(1);
                self.engine.render(&mut left[0][at..t], &mut right[0][at..t]);
                at = t;
            }
            match ev {
                NoteEvent::NoteOn { note, velocity, voice_id, .. } => self.engine.note_on(note, velocity, voice_id),
                NoteEvent::NoteOff { note, .. } => self.engine.note_off(note),
                NoteEvent::Choke { .. } | NoteEvent::MidiCC { cc: 120 | 123, .. } => self.engine.all_off(),
                _ => {}
            }
            ended.extend(self.engine.take_ended());
        }
        if at < len {
            let (left, right) = out.split_at_mut(1);
            self.engine.render(&mut left[0][at..len], &mut right[0][at..len]);
        }
        ended.extend(self.engine.take_ended());
        // Voice ends go back to the host, at the end of the block (spec: audio/voices.md §4).
        let last = (len as u32).saturating_sub(1);
        for (note, voice_id) in ended {
            ctx.send_event(NoteEvent::VoiceTerminated { timing: last, voice_id, channel: 0, note });
        }
        if self.engine.has_voices() { ProcessStatus::KeepAlive } else { ProcessStatus::Normal }
    }
}

impl ClapPlugin for MangoDirectWave {
    const CLAP_ID: &'static str = "org.openstudio.mango-directwave";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Mango Studio's sampler");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Sampler, ClapFeature::Stereo];
}

impl Vst3Plugin for MangoDirectWave {
    const VST3_CLASS_ID: [u8; 16] = *b"MangoDirectWave1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Sampler];
}

// Built as a plugin module: the C entry points hosts look for. Linked into Mango Studio (`linked` feature): only the
// symbol-free table, so several Mango plugins fit in one program (spec: plugins/plugin-formats.md).
#[cfg(not(feature = "linked"))]
nih_export_clap!(MangoDirectWave);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(MangoDirectWave);
os_plugkit::linked_clap_entry!(MangoDirectWave);

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A short 44.1 kHz stereo 16-bit WAV: a tone at `amp` (0..1).
    pub(crate) fn wav(name: &str, amp: f32) -> std::path::PathBuf {
        let frames = 4410usize;
        let mut data = Vec::with_capacity(frames * 4);
        for i in 0..frames {
            let v = ((i as f32 * 0.07).sin() * amp * 32767.0) as i16;
            data.extend(v.to_le_bytes());
            data.extend(v.to_le_bytes());
        }
        let mut fmt = Vec::new();
        for v in [1u16, 2] {
            fmt.extend(v.to_le_bytes());
        }
        fmt.extend(44100u32.to_le_bytes());
        fmt.extend((44100u32 * 4).to_le_bytes());
        fmt.extend(4u16.to_le_bytes());
        fmt.extend(16u16.to_le_bytes());
        let mut body = b"WAVE".to_vec();
        for (id, b) in [(b"fmt ", &fmt), (b"data", &data)] {
            body.extend(id);
            body.extend((b.len() as u32).to_le_bytes());
            body.extend(b);
        }
        let mut file = b"RIFF".to_vec();
        file.extend((body.len() as u32).to_le_bytes());
        file.extend(body);
        let p = std::env::temp_dir().join(format!("mango-wave-{}-{name}.wav", std::process::id()));
        std::fs::write(&p, file).unwrap();
        p
    }

    #[test]
    fn a_loaded_zone_plays_on_its_keys_and_other_keys_are_silent() {
        let p = wav("tone", 0.5);
        let params = WaveParams::default();
        let sh = Shared::default();
        sh.load(&params, 1, &p.to_string_lossy()).unwrap();
        assert_eq!(params.program().zone(1).path, p.to_string_lossy());
        assert!(sh.loaded(1));
        let kit = sh.kit.lock().unwrap().clone().unwrap();
        assert_eq!(kit.on(60, 100), vec![1]);
        assert!(kit.on(61, 100).contains(&1), "the fresh zone covers the keyboard");
        let mut e = dsp::Engine::new(44100.0);
        e.set_kit(Some(kit));
        e.note_on(60, 1.0, Some(7));
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        e.render(&mut l, &mut r);
        assert!(l.iter().any(|v| v.abs() > 0.2));
        assert!(sh.load(&params, 2, "Z:\\nowhere.wav").is_err());
        let _ = std::fs::remove_file(p);
    }

    /// A key up releases the note: the master's envelope falls and the voice ends.
    #[test]
    fn the_release_ends_the_note() {
        let p = wav("rel", 0.5);
        let params = WaveParams::default();
        let sh = Shared::default();
        sh.load(&params, 0, &p.to_string_lossy()).unwrap();
        let kit = sh.kit.lock().unwrap().clone().unwrap();
        let mut e = dsp::Engine::new(44100.0);
        e.set_kit(Some(kit));
        e.note_on(60, 1.0, Some(1));
        e.note_off(60);
        let mut l = vec![0.0f32; 128];
        let mut r = vec![0.0f32; 128];
        let mut blocks = 0;
        while e.has_voices() && blocks < 4000 {
            l.iter_mut().for_each(|x| *x = 0.0);
            e.render(&mut l, &mut r);
            blocks += 1;
        }
        assert!(!e.has_voices(), "the release ends the note");
        assert_eq!(e.take_ended(), [(60, Some(1))]);
        let _ = std::fs::remove_file(p);
    }
}
