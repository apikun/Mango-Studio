// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango BeepMap: Mango Studio's image synthesizer (a recreation of FL's BeepMap) as a VST3 / CLAP plugin
//! (nih-plug). A loaded PNG sings: its columns are time, its rows are partials, a pixel's brightness is loudness,
//! and the played key carries the bottom edge's pitch.
//!
// spec: plugins/lemon-beepmap.md
#![allow(semicolon_in_expressions_from_non_local_macros)]

mod dsp;
mod editor;
mod params;
mod view;

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use nih_plug::prelude::*;

use dsp::{Scan, ScanParams};
use params::BeepMapParams;

pub struct Shared {
    rate: AtomicU32,
    /// Bumped on every load: the audio thread re-reads the grid when it moves.
    version: AtomicU64,
    /// The image's grid (the sound) and its picture (the editor), set together.
    grid: RwLock<Option<Arc<dsp::Grid>>>,
    picture: RwLock<Option<(usize, usize, Arc<Vec<u32>>)>>,
    path: RwLock<String>,
    status: Mutex<String>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            rate: AtomicU32::new(0),
            version: AtomicU64::new(0),
            grid: RwLock::new(None),
            picture: RwLock::new(None),
            path: RwLock::new(String::new()),
            status: Mutex::new(String::new()),
        }
    }
}

impl Shared {
    /// Loads image `path` (UI or initialize thread): the decode happens off the audio thread's locks, then the
    /// grid and the picture swap in together. A failure is the status's news, what was loaded stays.
    fn load(&self, path: &str, params: &BeepMapParams) -> Result<(), String> {
        let failed = |e: String| {
            self.set_status(e.clone());
            Err(e)
        };
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => return failed(format!("Could not open image: {e}")),
        };
        let (grid, picture) = match dsp::Grid::decode(&bytes) {
            Some(decoded) => decoded,
            None => return failed(format!("{path}: not a PNG image")),
        };
        *self.grid.write().map_err(|_| "BeepMap image lock is poisoned.".to_string())? = Some(Arc::new(grid));
        *self.picture.write().map_err(|_| "BeepMap picture lock is poisoned.".to_string())? = Some((picture.0, picture.1, Arc::new(picture.2)));
        *self.path.write().map_err(|_| "BeepMap path lock is poisoned.".to_string())? = path.to_string();
        *params.path.write().map_err(|_| "Could not save the image path.".to_string())? = path.to_string();
        *self.status.lock().map_err(|_| "BeepMap status lock is poisoned.".to_string())? = String::new();
        self.version.fetch_add(1, Ordering::Release);
        Ok(())
    }

    fn set_status(&self, status: String) {
        if let Ok(mut s) = self.status.lock() {
            *s = status;
        }
    }

    fn display(&self) -> view::Display {
        let picture = self.picture.read().ok().and_then(|p| p.clone());
        let path = self.path.read().map(|p| p.clone()).unwrap_or_default();
        let status = self.status.lock().map(|s| s.clone()).unwrap_or_default();
        view::Display { picture, path, status, ..view::Display::default() }
    }
}

pub struct MangoBeepMap {
    params: Arc<BeepMapParams>,
    shared: Arc<Shared>,
    scan: Scan,
    /// The image version the scan has taken (the load bumps the shared one).
    seen: u64,
    held: u32,
    tail_samples: usize,
}

impl Default for MangoBeepMap {
    fn default() -> Self {
        Self {
            params: Arc::new(BeepMapParams::default()),
            shared: Arc::new(Shared::default()),
            scan: Scan::new(44_100.0),
            seen: 0,
            held: 0,
            tail_samples: 0,
        }
    }
}

impl MangoBeepMap {
    /// What the scan plays this block, from the parameters and the sounding key.
    fn scan_params(&self) -> ScanParams {
        let key_hz = 440.0f64 * 2f64.powf((f64::from(self.scan.key()) - 69.0) / 12.0);
        let base = key_hz * 2f64.powf(f64::from(self.params.octave_value()));
        ScanParams {
            base_hz: base as f32,
            bright: self.params.bright_value(),
            gate: self.params.gate_value() / 100.0,
            cutoff: self.params.tone_value(),
            speed_secs: self.params.speed_value(),
        }
    }
}

impl Plugin for MangoBeepMap {
    const NAME: &'static str = "Mango BeepMap";
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

    fn editor(&mut self, _executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(editor::BeepMapEditor::new(self.params.clone(), self.shared.clone())))
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, config: &BufferConfig, _ctx: &mut impl InitContext<Self>) -> bool {
        let rate = config.sample_rate.round() as u32;
        self.shared.rate.store(rate, Ordering::Release);
        self.scan.set_rate(rate as f32);
        self.held = 0;
        self.tail_samples = 0;
        let path = match self.params.path.read() {
            Ok(path) => path.clone(),
            Err(_) => {
                nih_log!("Mango BeepMap: saved image path is unavailable.");
                return true;
            }
        };
        if !path.is_empty() && let Err(e) = self.shared.load(&path, &self.params) {
            self.shared.set_status(e.clone());
            nih_log!("Mango BeepMap: {e}");
        }
        true
    }

    fn reset(&mut self) {
        self.scan.note_off();
        self.held = 0;
        self.tail_samples = 0;
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, ctx: &mut impl ProcessContext<Self>) -> ProcessStatus {
        // A new image: the scan takes it between blocks.
        let version = self.shared.version.load(Ordering::Acquire);
        if version != self.seen {
            self.scan.set_grid(self.shared.grid.read().ok().and_then(|g| g.clone()));
            self.seen = version;
        }
        let len = buffer.samples();
        let out = buffer.as_slice();
        for channel in out.iter_mut() {
            channel.fill(0.0);
        }
        if out.len() < 2 {
            return ProcessStatus::Normal;
        }
        let mut at = 0;
        while let Some(event) = ctx.next_event() {
            let t = (event.timing() as usize).min(len);
            if t > at {
                let (left, right) = out.split_at_mut(1);
                self.scan.render(&mut left[0][at..t], &mut right[0][at..t], self.scan_params());
                at = t;
            }
            match event {
                NoteEvent::NoteOn { note, velocity, .. } => {
                    if velocity > 0.0 {
                        self.held = self.held.saturating_add(1);
                        self.scan.note_on(note.min(127), velocity);
                        self.tail_samples = self.shared.rate.load(Ordering::Relaxed) as usize / 4;
                    } else {
                        self.held = self.held.saturating_sub(1);
                        if self.held == 0 {
                            self.scan.note_off();
                        }
                    }
                }
                NoteEvent::NoteOff { .. } | NoteEvent::Choke { .. } => {
                    self.held = self.held.saturating_sub(1);
                    if self.held == 0 {
                        self.scan.note_off();
                    }
                }
                NoteEvent::MidiCC { cc, .. } if cc == 120 || cc == 123 => {
                    self.held = 0;
                    self.scan.note_off();
                }
                _ => {}
            }
        }
        if at < len {
            let (left, right) = out.split_at_mut(1);
            self.scan.render(&mut left[0][at..len], &mut right[0][at..len], self.scan_params());
        }
        self.tail_samples = self.tail_samples.saturating_sub(len);
        if self.scan.is_active() || self.tail_samples > 0 {
            ProcessStatus::KeepAlive
        } else {
            ProcessStatus::Normal
        }
    }
}

impl ClapPlugin for MangoBeepMap {
    const CLAP_ID: &'static str = "com.mangostudio.beepmap";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Mango BeepMap, Mango Studio's image synthesizer");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
}

impl Vst3Plugin for MangoBeepMap {
    const VST3_CLASS_ID: [u8; 16] = *b"MangoBeepMap0001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

#[cfg(not(feature = "linked"))]
nih_export_clap!(MangoBeepMap);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(MangoBeepMap);
os_plugkit::linked_clap_entry!(MangoBeepMap);

#[cfg(test)]
mod tests {
    use super::*;

    /// An image loads: the grid and the picture are there, the path is remembered, a bad one is an error that
    /// keeps what was loaded.
    #[test]
    fn an_image_loads_and_garbage_does_not() {
        let mut pm = tiny_skia::Pixmap::new(16, 16).unwrap();
        for p in pm.pixels_mut() {
            *p = tiny_skia::PremultipliedColorU8::from_rgba(255, 163, 58, 255).unwrap();
        }
        let dir = std::env::temp_dir().join(format!("mango-beepmap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("glow.png");
        std::fs::write(&path, pm.encode_png().unwrap()).unwrap();

        let shared = Shared::default();
        let params = BeepMapParams::default();
        shared.load(&path.to_string_lossy(), &params).unwrap();
        assert!(shared.grid.read().unwrap().is_some());
        assert!(shared.display().picture.is_some());
        assert_eq!(*params.path.read().unwrap(), path.to_string_lossy());

        let bad = dir.join("nope.png");
        std::fs::write(&bad, b"not a png").unwrap();
        let error = shared.load(&bad.to_string_lossy(), &params).unwrap_err();
        assert!(error.contains("not a PNG"), "{error}");
        assert!(shared.grid.read().unwrap().is_some(), "the loaded image stays");
        assert!(!shared.display().status.is_empty(), "the status says what went wrong");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The plugin sings a loaded image on a note, and the release ends it.
    #[test]
    fn a_loaded_image_sings() {
        let mut pm = tiny_skia::Pixmap::new(16, 16).unwrap();
        for p in pm.pixels_mut() {
            *p = tiny_skia::PremultipliedColorU8::from_rgba(255, 255, 255, 255).unwrap();
        }
        let dir = std::env::temp_dir().join(format!("mango-beepmap-sing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("white.png");
        std::fs::write(&path, pm.encode_png().unwrap()).unwrap();

        let mut plugin = MangoBeepMap::default();
        plugin.shared.load(&path.to_string_lossy(), &plugin.params).unwrap();
        plugin.scan.set_grid(plugin.shared.grid.read().unwrap().clone());
        plugin.scan.set_rate(48_000.0);
        let p = plugin.scan_params();
        assert!(p.base_hz > 200.0 && p.base_hz < 300.0, "key 60 sings near middle C ({})", p.base_hz);
        plugin.scan.note_on(60, 1.0);
        let (mut l, mut r) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
        plugin.scan.render(&mut l, &mut r, p);
        assert!(l.iter().any(|x| x.abs() > 0.0001), "the image sings");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
