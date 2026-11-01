// Copyright (C) 2026 mikobuntu
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Sfxr: Mango Studio's retro sound effect generator (a port of drPetter's sfxr, after LMMS's
//! Sfxr instrument) as a VST3 / CLAP plugin (nih-plug). A key press rolls a fresh effect from the
//! current sliders — every note sounds a new take, the way LMMS's Sfxr makes one per note — and the
//! seven generator buttons roll the classic presets. The synthesis lives in `dsp`; the port's
//! credits and licenses are told there.
//!
#![allow(semicolon_in_expressions_from_non_local_macros)]

pub mod dsp;
pub mod editor;
pub mod params;
pub mod view;

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use nih_plug::prelude::*;

use dsp::Engine;
use params::SfxrParams;

/// State shared by the audio thread and the editor.
#[derive(Default)]
pub struct Shared {
    /// Keys the editor plays itself (one shot each: the Play button and the generator buttons).
    pub previews: Mutex<Vec<u8>>,
}

pub struct MangoSfxr {
    params: Arc<SfxrParams>,
    shared: Arc<Shared>,
    engine: Engine,
}

impl Default for MangoSfxr {
    fn default() -> Self {
        MangoSfxr { params: Arc::new(SfxrParams::default()), shared: Arc::new(Shared::default()), engine: Engine::new(44_100.0) }
    }
}

impl MangoSfxr {
    fn play_preview(&mut self, patch: &dsp::Patch) {
        self.engine.note_on(patch, dsp::key_hz(view::PREVIEW_KEY));
    }
}

impl Plugin for MangoSfxr {
    const NAME: &'static str = "Mango Sfxr";
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
        Some(Box::new(editor::SfxrEditor::new(self.params.clone(), self.shared.clone())))
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, config: &BufferConfig, _ctx: &mut impl InitContext<Self>) -> bool {
        self.engine.set_rate(config.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.engine.all_off();
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, ctx: &mut impl ProcessContext<Self>) -> ProcessStatus {
        // The patch the notes of this block sound (the editor's newest slider values win).
        let patch = self.params.patch();
        let previews = std::mem::take(&mut *self.shared.previews.lock().unwrap_or_else(|p| p.into_inner()));
        for _ in previews {
            self.play_preview(&patch);
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
                self.engine.render(&mut left[0][at..t], &mut right[0][at..t]);
                at = t;
            }
            match event {
                // A fresh effect per note: every key press rolls a new take of the current sliders
                // (LMMS's Sfxr makes a new SfxrSynth per note). The sound plays to its end; a note
                // off does not cut it, and neither does the velocity shape it.
                NoteEvent::NoteOn { note, velocity, .. } if velocity > 0.0 => {
                    self.engine.note_on(&patch, dsp::key_hz(note.min(127)));
                }
                NoteEvent::MidiCC { cc, .. } if cc == 120 || cc == 123 => self.engine.all_off(),
                _ => {}
            }
        }
        if at < len {
            let (left, right) = out.split_at_mut(1);
            self.engine.render(&mut left[0][at..len], &mut right[0][at..len]);
        }
        if self.engine.has_voices() {
            ProcessStatus::KeepAlive
        } else {
            ProcessStatus::Normal
        }
    }
}

impl ClapPlugin for MangoSfxr {
    const CLAP_ID: &'static str = "com.mangostudio.sfxr";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Mango Sfxr, Mango Studio's retro sound effect generator");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
}

impl Vst3Plugin for MangoSfxr {
    const VST3_CLASS_ID: [u8; 16] = *b"MangoSfxr0000001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

// Built as a plugin module: the C entry points hosts look for. Linked into Mango Studio (`linked`
// feature): only the symbol-free table, so several Mango plugins fit in one program (spec:
// plugins/plugin-formats.md).
#[cfg(not(feature = "linked"))]
nih_export_clap!(MangoSfxr);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(MangoSfxr);
os_plugkit::linked_clap_entry!(MangoSfxr);

#[cfg(test)]
mod tests {
    use super::*;

    /// The editor's Play queue sounds through the engine, and the pool holds eight voices.
    #[test]
    fn previews_sound_and_the_voice_pool_holds() {
        let mut plugin = MangoSfxr::default();
        plugin.engine.set_rate(48_000.0);
        let patch = plugin.params.patch();
        plugin.play_preview(&patch);
        assert!(plugin.engine.has_voices(), "the preview note sounds");
        for _ in 0..(dsp::VOICES + 3) {
            plugin.play_preview(&patch);
        }
        assert_eq!(plugin.engine.voices(), dsp::VOICES, "the pool steals its oldest voices");
        plugin.engine.all_off();
        assert!(!plugin.engine.has_voices());
    }

    /// A note-off does not cut the one-shot (sfxr effects play to their end); the sounding voice
    /// finishes by itself.
    #[test]
    fn a_rolled_effect_plays_to_its_end() {
        let mut plugin = MangoSfxr::default();
        plugin.engine.set_rate(44_100.0);
        let patch = dsp::Preset::Blit.roll(&mut dsp::Rng::new(6));
        plugin.engine.note_on(&patch, dsp::key_hz(69));
        let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        let mut sounded = false;
        for _ in 0..4800 {
            for x in l.iter_mut() {
                *x = 0.0;
            }
            for x in r.iter_mut() {
                *x = 0.0;
            }
            plugin.engine.render(&mut l, &mut r);
            sounded |= l.iter().any(|x| x.abs() > 0.001);
            if sounded && !plugin.engine.has_voices() {
                break;
            }
        }
        assert!(sounded, "the effect sounded");
        assert!(!plugin.engine.has_voices(), "the effect finished by itself");
    }
}
