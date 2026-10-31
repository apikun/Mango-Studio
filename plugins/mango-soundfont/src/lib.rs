// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

#![allow(semicolon_in_expressions_from_non_local_macros)]

mod editor;
mod params;
mod view;

use std::fs::File;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crossbeam_queue::ArrayQueue;
use nih_plug::prelude::*;
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};

use params::SoundFontParams;

#[derive(Clone, Copy)]
struct SoundControls {
    volume: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    lfo_delay: f32,
    lfo_amount: f32,
    lfo_speed: f32,
    cutoff: f32,
    modulation: f32,
    reverb_send: f32,
    chorus_send: f32,
}

impl SoundControls {
    fn read(params: &SoundFontParams) -> Self {
        Self {
            volume: params.volume_value(),
            attack: params.attack_value() / 1000.0,
            decay: params.decay_value() / 1000.0,
            sustain: params.sustain_value() / 100.0,
            release: params.release_value() / 1000.0,
            lfo_delay: params.lfo_delay_value() / 1000.0,
            lfo_amount: params.lfo_amount_value() / 100.0,
            lfo_speed: params.lfo_speed_value(),
            cutoff: params.cutoff_value(),
            modulation: params.modulation_value() / 100.0,
            reverb_send: params.reverb_send_value(),
            chorus_send: params.chorus_send_value(),
        }
    }

    fn envelope_enabled(self) -> bool {
        self.attack > 0.0 || self.decay > 0.0 || self.sustain < 1.0 || self.release > 0.0
    }
}

#[derive(Clone, Copy, Default)]
enum EnvelopeStage {
    #[default]
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Default)]
struct Envelope {
    stage: EnvelopeStage,
    level: f32,
    release_start: f32,
}

#[derive(Default)]
struct FilterState {
    left: f32,
    right: f32,
}

impl Envelope {
    fn note_on(&mut self) {
        self.stage = EnvelopeStage::Attack;
        self.level = 0.0;
    }

    fn note_off(&mut self) {
        if !matches!(self.stage, EnvelopeStage::Off | EnvelopeStage::Release) {
            self.release_start = self.level;
            self.stage = EnvelopeStage::Release;
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn next(&mut self, controls: SoundControls, sample_rate: f32) -> f32 {
        if !controls.envelope_enabled() {
            return 1.0;
        }
        let step = 1.0 / sample_rate;
        loop {
            match self.stage {
                EnvelopeStage::Off => return 0.0,
                EnvelopeStage::Attack if controls.attack <= 0.0 => {
                    self.level = 1.0;
                    self.stage = EnvelopeStage::Decay;
                }
                EnvelopeStage::Attack => {
                    self.level = (self.level + step / controls.attack).min(1.0);
                    if self.level >= 1.0 {
                        self.stage = EnvelopeStage::Decay;
                    }
                    return self.level;
                }
                EnvelopeStage::Decay if controls.decay <= 0.0 => {
                    self.level = controls.sustain;
                    self.stage = EnvelopeStage::Sustain;
                }
                EnvelopeStage::Decay => {
                    self.level = (self.level - step * (1.0 - controls.sustain) / controls.decay).max(controls.sustain);
                    if self.level <= controls.sustain {
                        self.stage = EnvelopeStage::Sustain;
                    }
                    return self.level;
                }
                EnvelopeStage::Sustain => {
                    self.level = controls.sustain;
                    return self.level;
                }
                EnvelopeStage::Release if controls.release <= 0.0 => {
                    self.level = 0.0;
                    self.stage = EnvelopeStage::Off;
                }
                EnvelopeStage::Release => {
                    self.level = (self.level - step * self.release_start / controls.release).max(0.0);
                    if self.level <= 0.0 {
                        self.stage = EnvelopeStage::Off;
                    }
                    return self.level;
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetInfo {
    pub name: String,
    pub bank: i32,
    pub patch: i32,
}

struct Prepared {
    synth: Synthesizer,
    presets: Arc<[PresetInfo]>,
}

pub struct Shared {
    rate: AtomicU32,
    pending: Mutex<Option<Prepared>>,
    retired: ArrayQueue<Synthesizer>,
    presets: RwLock<Arc<[PresetInfo]>>,
    path: RwLock<String>,
    status: Mutex<String>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            rate: AtomicU32::new(0),
            pending: Mutex::new(None),
            retired: ArrayQueue::new(2),
            presets: RwLock::new(Arc::from([])),
            path: RwLock::new(String::new()),
            status: Mutex::new(String::new()),
        }
    }
}

impl Shared {
    fn prepare_for_initialize(&self, path: &str) -> Result<(), String> {
        self.collect_retired();
        *self.pending.lock().map_err(|_| "SoundFont handoff lock is poisoned.".to_string())? = None;
        *self.presets.write().map_err(|_| "SoundFont preset list is unavailable.".to_string())? = Arc::from([]);
        *self.path.write().map_err(|_| "SoundFont path is unavailable.".to_string())? = path.to_string();
        *self.status.lock().map_err(|_| "SoundFont status is unavailable.".to_string())? = if path.is_empty() {
            "Choose an SF2 file, then use the preset arrows.".into()
        } else {
            "Loading SoundFont...".into()
        };
        Ok(())
    }

    fn load(&self, path: &str, rate: u32, params: &SoundFontParams) -> Result<(), String> {
        let mut file = File::open(path).map_err(|e| format!("Could not open SoundFont: {e}"))?;
        let font = Arc::new(SoundFont::new(&mut file).map_err(|e| format!("Invalid SF2 SoundFont: {e}"))?);
        let mut presets: Vec<PresetInfo> = font
            .get_presets()
            .iter()
            .map(|p| PresetInfo { name: p.get_name().to_string(), bank: p.get_bank_number(), patch: p.get_patch_number() })
            .collect();
        presets.sort_by_key(|p| (p.bank, p.patch, p.name.clone()));
        if presets.is_empty() {
            return Err("The SoundFont contains no presets.".into());
        }
        if presets.len() > 65_536 {
            return Err("This SoundFont has more than 65,536 presets, which Mango Font cannot address.".into());
        }
        let presets: Arc<[PresetInfo]> = presets.into();
        let settings = SynthesizerSettings::new(rate.clamp(16_000, 192_000) as i32);
        let mut synth = Synthesizer::new(&font, &settings).map_err(|e| format!("Could not start SoundFont: {e}"))?;
        let index = (params.preset_value().max(0) as usize).min(presets.len() - 1);
        select_preset(&mut synth, &presets[index]);

        let mut pending = self.pending.try_lock().map_err(|e| match e {
            std::sync::TryLockError::WouldBlock => "SoundFont player is busy; try loading again.".to_string(),
            std::sync::TryLockError::Poisoned(_) => "SoundFont handoff lock is poisoned.".to_string(),
        })?;
        let mut current_presets = self.presets.write().map_err(|_| "SoundFont preset list is unavailable".to_string())?;
        let mut current_path = self.path.write().map_err(|_| "SoundFont path is unavailable".to_string())?;
        let mut saved_path = params.path.write().map_err(|_| "Could not save the SoundFont path.".to_string())?;
        let mut status = self.status.lock().map_err(|_| "SoundFont status is unavailable".to_string())?;
        *current_presets = presets.clone();
        *current_path = path.to_string();
        *saved_path = path.to_string();
        *status = format!("Loaded {} presets", presets.len());
        *pending = Some(Prepared { synth, presets });
        Ok(())
    }

    fn set_status(&self, status: String) -> Result<(), String> {
        *self.status.lock().map_err(|_| "SoundFont status is unavailable".to_string())? = status;
        Ok(())
    }

    fn take_pending(&self) -> Result<Option<Prepared>, String> {
        match self.pending.try_lock() {
            Ok(mut pending) => Ok(pending.take()),
            Err(std::sync::TryLockError::WouldBlock) => Ok(None),
            Err(std::sync::TryLockError::Poisoned(_)) => Err("SoundFont handoff lock is poisoned.".into()),
        }
    }

    fn presets(&self) -> Result<Arc<[PresetInfo]>, String> {
        self.presets.read().map(|p| p.clone()).map_err(|_| "SoundFont preset list is unavailable".into())
    }

    fn collect_retired(&self) {
        while let Some(synth) = self.retired.pop() {
            drop(synth);
        }
    }

    fn display(&self, params: &SoundFontParams, mouse: Option<(f32, f32)>) -> view::Display {
        let (presets, preset_error) = match self.presets() {
            Ok(presets) => (presets, None),
            Err(error) => (Arc::<[PresetInfo]>::from([]), Some(error)),
        };
        let index = if presets.is_empty() { 0 } else { (params.preset_value().max(0) as usize).min(presets.len() - 1) };
        let selected = presets.get(index);
        let path = self.path.read().map(|p| p.clone()).unwrap_or_else(|_| "SoundFont path unavailable".into());
        let status = match self.status.lock() {
            Ok(status) => preset_error.unwrap_or_else(|| status.clone()),
            Err(_) => "SoundFont status is unavailable.".into(),
        };
        view::Display {
            path,
            preset: selected.map_or_else(String::new, |p| p.name.clone()),
            bank: selected.map_or(0, |p| p.bank),
            index,
            count: presets.len(),
            status,
            volume: params.volume_value(),
            controls: Default::default(),
            mouse,
        }
    }
}

fn select_preset(synth: &mut Synthesizer, preset: &PresetInfo) {
    for channel in 0..16 {
        synth.process_midi_message(channel, 0xB0, 0, preset.bank.max(0));
        synth.process_midi_message(channel, 0xC0, preset.patch.clamp(0, 127), 0);
    }
}

pub struct MangoSoundFont {
    params: Arc<SoundFontParams>,
    shared: Arc<Shared>,
    synth: Option<Synthesizer>,
    presets: Arc<[PresetInfo]>,
    selected: usize,
    tail_samples: usize,
    held_notes: [u8; 128],
    envelope: Envelope,
    lfo_age_samples: usize,
    filter: FilterState,
    handoff_error_logged: bool,
    retired: Option<Synthesizer>,
}

impl Default for MangoSoundFont {
    fn default() -> Self {
        Self {
            params: Arc::new(SoundFontParams::default()),
            shared: Arc::new(Shared::default()),
            synth: None,
            presets: Arc::from([]),
            selected: usize::MAX,
            tail_samples: 0,
            held_notes: [0; 128],
            envelope: Envelope::default(),
            lfo_age_samples: 0,
            filter: FilterState::default(),
            handoff_error_logged: false,
            retired: None,
        }
    }
}

impl Plugin for MangoSoundFont {
    const NAME: &'static str = "Mango Font";
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
        Some(Box::new(editor::SoundFontEditor::new(self.params.clone(), self.shared.clone())))
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, config: &BufferConfig, _ctx: &mut impl InitContext<Self>) -> bool {
        let rate = config.sample_rate.round() as u32;
        self.shared.rate.store(rate, Ordering::Release);
        self.synth = None;
        self.presets = Arc::from([]);
        self.selected = usize::MAX;
        self.held_notes.fill(0);
        self.envelope.reset();
        self.lfo_age_samples = 0;
        self.filter = FilterState::default();
        self.tail_samples = 0;
        self.handoff_error_logged = false;
        if let Some(retired) = self.retired.take() {
            drop(retired);
        }
        let path = match self.params.path.read() {
            Ok(path) => path.clone(),
            Err(_) => {
                nih_log!("Mango Font: saved SoundFont path is unavailable.");
                return true;
            }
        };
        if let Err(e) = self.shared.prepare_for_initialize(&path) {
            nih_log!("Mango Font: {e}");
            return true;
        }
        if !path.is_empty() {
            match self.shared.load(&path, rate, &self.params) {
                Ok(()) => match self.shared.take_pending() {
                    Ok(Some(prepared)) => {
                        self.synth = Some(prepared.synth);
                        self.presets = prepared.presets;
                        self.selected = usize::MAX;
                    }
                    Ok(None) => nih_log!("Mango Font: SoundFont load completed without a synth."),
                    Err(e) => nih_log!("Mango Font: {e}"),
                },
                Err(e) => {
                    if let Err(status_error) = self.shared.set_status(e.clone()) {
                        nih_log!("Mango Font: {e}; {status_error}");
                    }
                    nih_log!("Mango Font: {e}");
                }
            }
        }
        true
    }

    fn reset(&mut self) {
        if let Some(synth) = &mut self.synth {
            synth.reset();
        }
        self.held_notes.fill(0);
        self.envelope.reset();
        self.lfo_age_samples = 0;
        self.filter = FilterState::default();
        self.tail_samples = 0;
        self.selected = usize::MAX;
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, ctx: &mut impl ProcessContext<Self>) -> ProcessStatus {
        if let Some(retired) = self.retired.take() {
            if let Err(retired) = self.shared.retired.push(retired) {
                self.retired = Some(retired);
            }
        }
        let pending = if self.retired.is_none() { self.shared.take_pending() } else { Ok(None) };
        match pending {
            Ok(Some(prepared)) => {
                if let Some(old) = self.synth.replace(prepared.synth) {
                    if let Err(old) = self.shared.retired.push(old) {
                        self.retired = Some(old);
                    }
                }
                self.presets = prepared.presets;
                self.selected = usize::MAX;
                self.held_notes.fill(0);
                self.tail_samples = 0;
            }
            Ok(None) => {}
            Err(e) if !self.handoff_error_logged => {
                nih_log!("Mango Font: {e}");
                self.handoff_error_logged = true;
            }
            Err(_) => {}
        }
        let len = buffer.samples();
        let out = buffer.as_slice();
        for channel in out.iter_mut() {
            channel.fill(0.0);
        }
        let Some(synth) = &mut self.synth else {
            return ProcessStatus::Normal;
        };

        let controls = SoundControls::read(&self.params);
        if controls.envelope_enabled() && self.held_notes.iter().any(|&n| n > 0) && matches!(self.envelope.stage, EnvelopeStage::Off) {
            self.envelope.stage = EnvelopeStage::Sustain;
            self.envelope.level = controls.sustain;
        }
        for controller in 0..16 {
            synth.process_midi_message(controller, 0xB0, 91, (controls.reverb_send.clamp(0.0, 100.0) * 1.27).round() as i32);
            synth.process_midi_message(controller, 0xB0, 93, (controls.chorus_send.clamp(0.0, 100.0) * 1.27).round() as i32);
        }
        let preset = (self.params.preset_value().max(0) as usize).min(self.presets.len().saturating_sub(1));
        if preset != self.selected && !self.presets.is_empty() {
            select_preset(synth, &self.presets[preset]);
            self.selected = preset;
        }

        if out.len() < 2 {
            return ProcessStatus::Normal;
        }
        let mut at = 0;
        while let Some(event) = ctx.next_event() {
            let t = (event.timing() as usize).min(len);
            if t > at {
                let (left, right) = out.split_at_mut(1);
                render(
                    synth,
                    &mut left[0][at..t],
                    &mut right[0][at..t],
                    controls,
                    &mut self.envelope,
                    &mut self.filter,
                    &mut self.lfo_age_samples,
                    self.held_notes.iter().any(|&n| n > 0),
                    self.shared.rate.load(Ordering::Relaxed) as f32,
                );
                at = t;
            }
            match event {
                NoteEvent::NoteOn { note, velocity, .. } => {
                    let key = usize::from(note.min(127));
                    if velocity > 0.0 {
                        if !self.held_notes.iter().any(|&n| n > 0) {
                            self.envelope.note_on();
                            self.lfo_age_samples = 0;
                        }
                        self.held_notes[key] = self.held_notes[key].saturating_add(1);
                        self.tail_samples = self.shared.rate.load(Ordering::Relaxed) as usize * 5;
                    } else {
                        self.held_notes[key] = self.held_notes[key].saturating_sub(1);
                        if !self.held_notes.iter().any(|&n| n > 0) {
                            self.envelope.note_off();
                        }
                    }
                    synth.process_midi_message(0, 0x90, i32::from(note), (velocity.clamp(0.0, 1.0) * 127.0).round() as i32);
                }
                NoteEvent::NoteOff { note, .. } => {
                    let key = usize::from(note.min(127));
                    self.held_notes[key] = self.held_notes[key].saturating_sub(1);
                    self.tail_samples = self.shared.rate.load(Ordering::Relaxed) as usize * 5;
                    if !self.held_notes.iter().any(|&n| n > 0) {
                        self.envelope.note_off();
                    }
                    synth.process_midi_message(0, 0x80, i32::from(note), 0);
                }
                NoteEvent::Choke { note, .. } => {
                    self.held_notes[usize::from(note.min(127))] = 0;
                    if !self.held_notes.iter().any(|&n| n > 0) {
                        self.envelope.note_off();
                    }
                    synth.process_midi_message(0, 0x80, i32::from(note), 0);
                }
                NoteEvent::MidiPitchBend { value, .. } => {
                    let bend = (value.clamp(0.0, 1.0) * 16383.0).round() as i32;
                    synth.process_midi_message(0, 0xE0, bend & 0x7f, (bend >> 7) & 0x7f);
                }
                NoteEvent::MidiCC { cc, value, .. } => {
                    if cc == 120 || cc == 123 {
                        self.held_notes.fill(0);
                        self.tail_samples = self.shared.rate.load(Ordering::Relaxed) as usize * 5;
                        self.envelope.note_off();
                    }
                    synth.process_midi_message(0, 0xB0, i32::from(cc), (value.clamp(0.0, 1.0) * 127.0).round() as i32);
                }
                _ => {}
            }
        }
        if at < len {
            let (left, right) = out.split_at_mut(1);
            render(
                synth,
                &mut left[0][at..len],
                &mut right[0][at..len],
                controls,
                &mut self.envelope,
                &mut self.filter,
                &mut self.lfo_age_samples,
                self.held_notes.iter().any(|&n| n > 0),
                self.shared.rate.load(Ordering::Relaxed) as f32,
            );
        }
        self.tail_samples = self.tail_samples.saturating_sub(len);
        if self.held_notes.iter().any(|&n| n > 0) || self.tail_samples > 0 {
            ProcessStatus::KeepAlive
        } else {
            ProcessStatus::Normal
        }
    }
}

fn render(
    synth: &mut Synthesizer,
    left: &mut [f32],
    right: &mut [f32],
    controls: SoundControls,
    envelope: &mut Envelope,
    filter_state: &mut FilterState,
    lfo_age_samples: &mut usize,
    notes_held: bool,
    sample_rate: f32,
) {
    let lfo_elapsed = *lfo_age_samples as f32 / sample_rate;
    let lfo = if lfo_elapsed >= controls.lfo_delay {
        (std::f32::consts::TAU * controls.lfo_speed * (lfo_elapsed - controls.lfo_delay)).sin() * controls.lfo_amount
    } else {
        0.0
    };
    let modulation = (controls.modulation + lfo).clamp(0.0, 1.0);
    synth.process_midi_message(0, 0xB0, 1, (modulation * 127.0).round() as i32);
    synth.render(left, right);
    let cutoff = controls.cutoff.min(sample_rate * 0.5);
    let filter = cutoff < sample_rate * 0.45;
    let coefficient = if filter { 1.0 - (-std::f32::consts::TAU * cutoff / sample_rate).exp() } else { 1.0 };
    for (l, r) in left.iter_mut().zip(right) {
        let gain = controls.volume * envelope.next(controls, sample_rate);
        if filter {
            filter_state.left += coefficient * (*l - filter_state.left);
            filter_state.right += coefficient * (*r - filter_state.right);
            *l = filter_state.left * gain;
            *r = filter_state.right * gain;
        } else {
            filter_state.left = *l;
            filter_state.right = *r;
            *l *= gain;
            *r *= gain;
        }
    }
    if notes_held {
        *lfo_age_samples = lfo_age_samples.saturating_add(left.len());
    }
}

impl ClapPlugin for MangoSoundFont {
    const CLAP_ID: &'static str = "com.mangostudio.soundfont";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Mango Font, Mango Studio's SF2 player");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Sampler, ClapFeature::Stereo];
}

impl Vst3Plugin for MangoSoundFont {
    const VST3_CLASS_ID: [u8; 16] = *b"MangoSoundFont01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument, Vst3SubCategory::Sampler];
}

#[cfg(not(feature = "linked"))]
nih_export_clap!(MangoSoundFont);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(MangoSoundFont);
os_plugkit::linked_clap_entry!(MangoSoundFont);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_follows_attack_decay_sustain_and_release() {
        let mut envelope = Envelope::default();
        let controls = SoundControls {
            volume: 1.0,
            attack: 1.0,
            decay: 1.0,
            sustain: 0.5,
            release: 1.0,
            lfo_delay: 0.0,
            lfo_amount: 0.0,
            lfo_speed: 5.0,
            cutoff: 20_000.0,
            modulation: 0.0,
            reverb_send: 0.0,
            chorus_send: 0.0,
        };

        envelope.note_on();
        assert_eq!(envelope.next(controls, 2.0), 0.5);
        assert_eq!(envelope.next(controls, 2.0), 1.0);
        assert_eq!(envelope.next(controls, 2.0), 0.75);
        assert_eq!(envelope.next(controls, 2.0), 0.5);
        envelope.note_off();
        assert_eq!(envelope.next(controls, 2.0), 0.25);
        assert_eq!(envelope.next(controls, 2.0), 0.0);
    }

    #[test]
    fn default_envelope_does_not_change_soundfont_amplitude() {
        let controls = SoundControls::read(&SoundFontParams::default());
        let mut envelope = Envelope::default();
        envelope.note_on();

        assert_eq!(envelope.next(controls, 48_000.0), 1.0);
    }

    #[test]
    fn preset_order_is_bank_then_program() {
        let mut presets = [
            PresetInfo { name: "later".into(), bank: 1, patch: 0 },
            PresetInfo { name: "last".into(), bank: 0, patch: 4 },
            PresetInfo { name: "first".into(), bank: 0, patch: 1 },
        ];
        presets.sort_by_key(|p| (p.bank, p.patch, p.name.clone()));
        assert_eq!(presets.iter().map(|p| (p.bank, p.patch)).collect::<Vec<_>>(), [(0, 1), (0, 4), (1, 0)]);
    }

    #[test]
    fn invalid_soundfont_path_returns_an_error_without_replacing_loaded_state() {
        let shared = Shared::default();
        let params = SoundFontParams::default();
        let existing = Arc::<[PresetInfo]>::from([PresetInfo { name: "Existing".into(), bank: 0, patch: 0 }]);
        *shared.presets.write().unwrap() = existing;
        *shared.path.write().unwrap() = "existing.sf2".into();
        *params.path.write().unwrap() = "existing.sf2".into();
        let path = std::env::temp_dir().join(format!("mango-soundfont-missing-{}.sf2", std::process::id()));

        let error = shared.load(&path.to_string_lossy(), 48_000, &params).unwrap_err();

        assert!(error.starts_with("Could not open SoundFont:"));
        assert_eq!(shared.presets().unwrap().as_ref(), [PresetInfo { name: "Existing".into(), bank: 0, patch: 0 }]);
        assert_eq!(*shared.path.read().unwrap(), "existing.sf2");
        assert_eq!(*params.path.read().unwrap(), "existing.sf2");
    }

    #[test]
    #[ignore = "requires a test SF2 file; set MANGO_SOUNDFONT_TEST_SF2 to run"]
    fn loaded_soundfont_renders_a_midi_note() {
        let path = std::env::var("MANGO_SOUNDFONT_TEST_SF2").expect("test SoundFont path");
        let shared = Shared::default();
        let params = SoundFontParams::default();
        shared.load(&path, 48_000, &params).unwrap();
        let mut prepared = shared.take_pending().unwrap().expect("prepared SoundFont");
        assert!(!prepared.presets.is_empty());

        prepared.synth.note_on(0, 60, 100);
        let (mut left, mut right) = ([0.0; 512], [0.0; 512]);
        let controls = SoundControls::read(&params);
        let mut lfo_age = 0;
        render(
            &mut prepared.synth,
            &mut left,
            &mut right,
            controls,
            &mut Envelope::default(),
            &mut FilterState::default(),
            &mut lfo_age,
            true,
            48_000.0,
        );
        assert!(left.iter().chain(&right).any(|sample| sample.abs() > 0.0001), "the SoundFont should render audible samples");
    }
}
