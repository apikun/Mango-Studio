// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

#![allow(semicolon_in_expressions_from_non_local_macros)]

pub mod dsp;
// The editor window runs on Linux and macOS (`os_plugkit::gui`); on Windows the synth plays without one for now.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod editor;
pub mod env;
pub mod fft;
pub mod filter;
pub mod lfo;
pub mod mod_matrix;
pub mod params;
pub mod table;
pub mod view;
pub mod warp;

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use nih_plug::prelude::*;
use params::WavetableParams;

pub const CLAP_ID: &str = "org.lemonstudio.lemon-wavetable";

pub struct Shared {
    pub voices: AtomicU32,
}

pub struct LemonWavetable {
    params: Arc<WavetableParams>,
    engine: dsp::Engine,
    shared: Arc<Shared>,
}

impl Default for LemonWavetable {
    fn default() -> Self {
        Self {
            params: Arc::new(WavetableParams::default()),
            engine: dsp::Engine::new(48000.0),
            shared: Arc::new(Shared {
                voices: AtomicU32::new(0),
            }),
        }
    }
}

fn terminated(timing: usize, identity: dsp::Ended) -> NoteEvent<()> {
    NoteEvent::VoiceTerminated {
        timing: timing as u32,
        voice_id: identity.id,
        channel: identity.channel,
        note: identity.note,
    }
}

impl Plugin for LemonWavetable {
    const NAME: &'static str = "Mango Wavetable";
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

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(editor::WavetableEditor::new(
            self.params.clone(),
            self.shared.clone(),
        )))
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        if !config.sample_rate.is_finite() || config.sample_rate < 8000.0 {
            return false;
        }
        self.engine = dsp::Engine::new(config.sample_rate as f64);
        self.reset();
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let settings = self.params.settings();
        let num_samples = buffer.samples();
        let mut block_start = 0;

        let mut next_event = context.next_event();

        while block_start < num_samples {
            let block_end = match next_event {
                Some(event) if (event.timing() as usize) < num_samples => {
                    (event.timing() as usize).max(block_start)
                }
                _ => num_samples,
            };

            // Render audio slice up to next MIDI event
            if block_end > block_start {
                let slice_len = block_end - block_start;
                let channel_slices = buffer.as_slice();
                let (left_chan, right_chan) = channel_slices.split_at_mut(1);
                let left_slice = &mut left_chan[0][block_start..block_end];
                let right_slice = &mut right_chan[0][block_start..block_end];

                let ended = self.engine.render_block(&settings, left_slice, right_slice);
                for (idx, e) in ended.into_iter().enumerate() {
                    context.send_event(terminated(block_start + (idx % slice_len), e));
                }
                block_start = block_end;
            }

            // Process MIDI event at current sample point
            while let Some(event) = next_event {
                if event.timing() as usize > block_start {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { voice_id, channel, note, velocity, .. } => {
                        self.engine.note_on(voice_id, channel, note, velocity, &settings);
                    }
                    NoteEvent::NoteOff { voice_id, channel, note, .. } => {
                        self.engine.note_off(voice_id, channel, note);
                    }
                    NoteEvent::MidiPitchBend { value, .. } => {
                        // value is normalized -1.0 to +1.0
                        self.engine.set_pitch_bend(value);
                    }
                    _ => {}
                }
                next_event = context.next_event();
            }
        }

        self.shared
            .voices
            .store(self.engine.active_voice_count() as u32, Ordering::Relaxed);

        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for LemonWavetable {
    const CLAP_ID: &'static str = CLAP_ID;
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Mango Studio's wavetable synthesizer");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for LemonWavetable {
    const VST3_CLASS_ID: [u8; 16] = *b"LemonStudioWave0";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Instrument,
        Vst3SubCategory::Synth,
        Vst3SubCategory::Stereo,
    ];
}

#[cfg(not(feature = "linked"))]
nih_export_clap!(LemonWavetable);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(LemonWavetable);
os_plugkit::linked_clap_entry!(LemonWavetable);
