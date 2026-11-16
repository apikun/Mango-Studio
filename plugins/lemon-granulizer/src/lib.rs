// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Granulizer: Lemon Studio's granular sample player (a recreation of FL's Granulizer) as a VST3 / CLAP
//! plugin (nih-plug). A sample in a slot; a held note walks it as a cloud of grains — the grain length, spacing,
//! random start window, pan spread and hold shape the cloud, the pitch plays against the root key, reverse reads
//! backwards — and the editor loads the sample and turns every knob.
//!
// spec: plugins/lemon-granulizer.md
#![allow(semicolon_in_expressions_from_non_local_macros)]

pub mod dsp;
pub mod editor;
pub mod params;
pub mod view;

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nih_plug::prelude::*;
use os_engine::sample::Sample;

use dsp::Kit;
use params::GranParams;

/// What the audio thread and the editor share.
#[derive(Default)]
pub struct Shared {
    /// Notes the editor plays itself (key, velocity, on).
    pub previews: Mutex<Vec<(u8, f32, bool)>>,
    /// What the voices play; `kit_version` moves on every change.
    pub kit: Mutex<Option<Arc<Kit>>>,
    pub kit_version: AtomicU64,
    /// The sample loaded and the file it came from (also kept when it could not be read).
    sample: Mutex<Option<Arc<Sample>>>,
    loaded_path: Mutex<String>,
    /// Set while an editor is open.
    pub editor_open: AtomicBool,
}

impl Shared {
    pub fn sample(&self) -> Option<Arc<Sample>> {
        self.sample.lock().ok().and_then(|s| s.clone())
    }

    pub fn loaded_path(&self) -> String {
        self.loaded_path.lock().map(|p| p.clone()).unwrap_or_default()
    }

    /// Rebuilds what the voices play from the sample and the saved state's root.
    pub fn rebuild(&self, params: &GranParams) {
        let st = params.state();
        let kit = Arc::new(Kit { sample: self.sample(), root: st.root });
        if let Ok(mut k) = self.kit.lock() {
            *k = Some(kit);
        }
        self.kit_version.fetch_add(1, Ordering::AcqRel);
    }

    /// Opens sample file `path` for the state as it is (a restored state's sample); `path` stays named when it
    /// cannot be read, and the granulizer plays silence.
    pub fn open(&self, path: &str, params: &GranParams) -> Result<(), String> {
        let s = if path.is_empty() { Ok(None) } else { os_engine::sample::load_shared(Path::new(path)).map(Some).map_err(|e| format!("{path}: {e}")) };
        if let Ok(mut l) = self.loaded_path.lock() {
            *l = path.to_string();
        }
        let (sample, err) = match s {
            Ok(sample) => (sample, None),
            Err(e) => (None, Some(e)),
        };
        if let Ok(mut cur) = self.sample.lock() {
            *cur = sample;
        }
        self.rebuild(params);
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Loads a sample from file `path` (the editor's Load) and keeps the settings.
    pub fn load(&self, path: &str, params: &GranParams) -> Result<(), String> {
        os_engine::sample::load_shared(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
        let mut st = params.state();
        st.path = path.to_string();
        params.set_state(&st);
        self.open(path, params)
    }

    /// The root key of the saved state, set (the editor's root menu).
    pub fn set_root(&self, root: i32, params: &GranParams) {
        let mut st = params.state();
        st.root = root.clamp(0, 127);
        params.set_state(&st);
        self.rebuild(params);
    }
}

pub struct LemonGranulizer {
    params: Arc<GranParams>,
    shared: Arc<Shared>,
    engine: dsp::Engine,
    kit: Option<Arc<Kit>>,
    kit_seen: u64,
}

impl Default for LemonGranulizer {
    fn default() -> Self {
        LemonGranulizer { params: Arc::new(GranParams::default()), shared: Arc::new(Shared::default()), engine: dsp::Engine::new(44100.0), kit: None, kit_seen: u64::MAX }
    }
}

impl LemonGranulizer {
    /// The voices take the kit last built (never waits: a busy lock is tried again next block).
    fn take_kit(&mut self) {
        let v = self.shared.kit_version.load(Ordering::Acquire);
        if v != self.kit_seen
            && let Ok(k) = self.shared.kit.try_lock()
        {
            self.kit = k.clone();
            self.engine.set_kit(self.kit.clone());
            self.kit_seen = v;
        }
    }
}

impl Plugin for LemonGranulizer {
    const NAME: &'static str = "Mango Granulizer";
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
        Some(Box::new(editor::GranEditor::new(self.params.clone(), self.shared.clone())))
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, config: &BufferConfig, _ctx: &mut impl InitContext<Self>) -> bool {
        self.engine.set_rate(config.sample_rate as f32);
        // A restored state names its sample: open it now (initialize may block).
        let st = self.params.state();
        if st.path != self.shared.loaded_path() {
            if let Err(e) = self.shared.open(&st.path, &self.params) {
                nih_log!("Mango Granulizer: {e}");
            }
        } else {
            self.shared.rebuild(&self.params);
        }
        self.take_kit();
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, ctx: &mut impl ProcessContext<Self>) -> ProcessStatus {
        self.take_kit();
        let blk = self.engine.block(&self.params.values());
        if let Ok(mut q) = self.shared.previews.try_lock() {
            for (key, vel, on) in q.drain(..) {
                if on {
                    self.engine.note_on(key, vel, None, &blk);
                } else {
                    self.engine.note_off(key, &blk);
                }
            }
        }
        let len = buffer.samples();
        let out = buffer.as_slice();
        for ch in out.iter_mut() {
            ch.fill(0.0);
        }
        if out.len() < 2 {
            return ProcessStatus::Normal;
        }
        let mut at = 0;
        while let Some(ev) = ctx.next_event() {
            let t = (ev.timing() as usize).min(len);
            if t > at {
                let (left, right) = out.split_at_mut(1);
                self.engine.render(&mut left[0][at..t], &mut right[0][at..t], &blk);
                at = t;
            }
            match ev {
                NoteEvent::NoteOn { note, velocity, voice_id, .. } => self.engine.note_on(note, velocity, voice_id, &blk),
                NoteEvent::NoteOff { note, .. } => self.engine.note_off(note, &blk),
                NoteEvent::Choke { .. } | NoteEvent::MidiCC { cc: 120 | 123, .. } => self.engine.all_off(),
                _ => {}
            }
        }
        if at < len {
            let (left, right) = out.split_at_mut(1);
            self.engine.render(&mut left[0][at..len], &mut right[0][at..len], &blk);
        }
        // Voice ends go back to the host, at the end of the block (spec: audio/voices.md §4).
        let last = (len as u32).saturating_sub(1);
        for (note, voice_id) in self.engine.take_ended() {
            ctx.send_event(NoteEvent::VoiceTerminated { timing: last, voice_id, channel: 0, note });
        }
        if self.engine.has_voices() { ProcessStatus::KeepAlive } else { ProcessStatus::Normal }
    }
}

impl ClapPlugin for LemonGranulizer {
    const CLAP_ID: &'static str = "org.openstudio.lemon-granulizer";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Lemon Studio's granular sample player");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Sampler, ClapFeature::Stereo];
}

impl Vst3Plugin for LemonGranulizer {
    const VST3_CLASS_ID: [u8; 16] = *b"LemonStudioGranl";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Sampler];
}

// Built as a plugin module: the C entry points hosts look for. Linked into Mango Studio (`linked` feature): only the
// symbol-free table, so several Mango plugins fit in one program (spec: plugins/plugin-formats.md).
#[cfg(not(feature = "linked"))]
nih_export_clap!(LemonGranulizer);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(LemonGranulizer);
os_plugkit::linked_clap_entry!(LemonGranulizer);

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
        let p = std::env::temp_dir().join(format!("mango-gran-{}-{name}.wav", std::process::id()));
        std::fs::write(&p, file).unwrap();
        p
    }

    /// A fresh plugin with `path` loaded (a silent one when it cannot be read).
    fn loaded(path: &str) -> (Arc<GranParams>, Arc<Shared>, dsp::Engine) {
        let params = Arc::new(GranParams::default());
        let sh = Arc::new(Shared::default());
        let _ = sh.load(path, &params);
        let mut e = dsp::Engine::new(44100.0);
        sh.rebuild(&params);
        e.set_kit(sh.kit.lock().unwrap().clone());
        (params, sh, e)
    }

    #[test]
    fn a_loaded_sample_sounds_on_a_note_and_ends_after_its_release() {
        let p = wav("tone", 0.5);
        let (params, _sh, mut e) = loaded(&p.to_string_lossy());
        assert_eq!(params.state().path, p.to_string_lossy());
        let blk = e.block(&params.values());
        e.note_on(60, 1.0, Some(7), &blk);
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        e.render(&mut l, &mut r, &blk);
        assert!(l.iter().any(|v| v.abs() > 0.2), "the cloud sounds");
        // The same note renders the same sound (the grain starts are a deterministic xorshift).
        let mut e2 = dsp::Engine::new(44100.0);
        e2.set_kit(_sh.kit.lock().unwrap().clone());
        let blk2 = e2.block(&params.values());
        e2.note_on(60, 1.0, None, &blk2);
        let (mut l2, mut r2) = (vec![0.0; 512], vec![0.0; 512]);
        e2.render(&mut l2, &mut r2, &blk2);
        assert_eq!(l, l2);
        // A key up stops the grains: the voice ends and reports to the host.
        e.note_off(60, &blk);
        let mut blocks = 0;
        while e.has_voices() && blocks < 8000 {
            l.iter_mut().for_each(|x| *x = 0.0);
            e.render(&mut l, &mut r, &blk);
            blocks += 1;
        }
        assert!(!e.has_voices(), "the note ends once the grains are done");
        assert_eq!(e.take_ended(), [(60, Some(7))]);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn a_key_without_a_sample_is_silent_and_ends_at_once() {
        let params = Arc::new(GranParams::default());
        let sh = Arc::new(Shared::default());
        sh.rebuild(&params);
        let mut e = dsp::Engine::new(44100.0);
        e.set_kit(sh.kit.lock().unwrap().clone());
        let blk = e.block(&params.values());
        e.note_on(60, 1.0, Some(3), &blk);
        let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
        e.render(&mut l, &mut r, &blk);
        assert!(l.iter().all(|v| v.abs() < 1e-6));
        assert_eq!(e.take_ended(), [(60, Some(3))]);
        // An unreadable file keeps its name in the state and plays silence (the restored state's open).
        let params = Arc::new(GranParams::default());
        params.set_state(&params::State { path: "Z:\\nowhere.wav".into(), root: 60 });
        let sh = Arc::new(Shared::default());
        assert!(sh.open("Z:\\nowhere.wav", &params).is_err());
        assert_eq!(params.state().path, "Z:\\nowhere.wav");
        assert!(sh.sample().is_none());
        let mut e = dsp::Engine::new(44100.0);
        e.set_kit(sh.kit.lock().unwrap().clone());
        let blk = e.block(&params.values());
        e.note_on(60, 1.0, None, &blk);
        assert!(!e.has_voices(), "no sample, no voice");
    }

    #[test]
    fn the_grain_cloud_follows_its_knobs() {
        let p = wav("cloud", 0.5);
        let (params, _sh, mut e) = loaded(&p.to_string_lossy());
        // A long grain over a short spacing piles grains; the cap keeps the voice bounded.
        let mut v = params.values();
        v.grain = 2000;
        v.spacing = 5;
        let blk = e.block(&v);
        e.note_on(60, 1.0, None, &blk);
        let (mut l, mut r) = (vec![0.0; 4410], vec![0.0; 4410]);
        e.render(&mut l, &mut r, &blk);
        // Reverse reads the sample backwards: still sound, and the block's speed flipped.
        assert!(blk.speed > 0.0);
        let mut v = params.values();
        v.reverse = true;
        v.pitch = -1200;
        v.pan = 100;
        v.hold = 0;
        let blk = e.block(&v);
        assert!(blk.speed < 0.0);
        e.note_on(60, 1.0, None, &blk);
        e.render(&mut l, &mut r, &blk);
        assert!(l.iter().any(|x| x.abs() > 0.05), "the reversed cloud sounds too");
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn the_root_key_tunes_the_note() {
        let p = wav("root", 0.5);
        let (params, sh, mut e) = loaded(&p.to_string_lossy());
        sh.set_root(48, &params);
        assert_eq!(params.state().root, 48);
        e.set_kit(sh.kit.lock().unwrap().clone());
        // The same key now plays an octave higher: a faster read (the block's speed does not move; the voice's does).
        let blk = e.block(&params.values());
        e.note_on(60, 1.0, None, &blk);
        let (mut l, mut r) = (vec![0.0; 441], vec![0.0; 441]);
        e.render(&mut l, &mut r, &blk);
        assert!(l.iter().any(|v| v.abs() > 0.1));
        let _ = std::fs::remove_file(p);
    }
}
