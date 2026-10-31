// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Polyphonic DSP engine with 16-voice unison, phase warp, ZDF filtering, and modulation matrix.

use crate::env::Envelope;
use crate::filter::{FilterType, StereoFilter};
use crate::lfo::Lfo;
use crate::table::Wavetable;
use crate::warp::{self, WarpMode};
use std::f32::consts::FRAC_PI_2;

pub const MAX_VOICES: usize = 32;
pub const MAX_UNISON: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct UnisonSubVoice {
    pub phase: f64,
    pub detune_ratio: f64,
    pub pan_l: f32,
    pub pan_r: f32,
    pub gain: f32,
}

impl Default for UnisonSubVoice {
    fn default() -> Self {
        Self {
            phase: 0.0,
            detune_ratio: 1.0,
            pan_l: 0.7071,
            pan_r: 0.7071,
            gain: 1.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Voice {
    pub id: Option<i32>,
    pub channel: u8,
    pub note: u8,
    pub velocity: f32,
    pub base_freq: f64,
    pub active: bool,
    pub unison_count: usize,
    pub unison_voices: [UnisonSubVoice; MAX_UNISON],
    pub env1: Envelope,
    pub env2: Envelope,
    pub lfo1: Lfo,
    pub filter: StereoFilter,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            id: None,
            channel: 0,
            note: 60,
            velocity: 0.0,
            base_freq: 440.0,
            active: false,
            unison_count: 1,
            unison_voices: [UnisonSubVoice::default(); MAX_UNISON],
            env1: Envelope::default(),
            env2: Envelope::default(),
            lfo1: Lfo::default(),
            filter: StereoFilter::default(),
        }
    }
}

impl Voice {
    pub fn note_on(
        &mut self,
        id: Option<i32>,
        channel: u8,
        note: u8,
        velocity: f32,
        bend_ratio: f64,
        settings: &Settings,
    ) {
        self.id = id;
        self.channel = channel;
        self.note = note;
        self.velocity = velocity.clamp(0.0, 1.0);
        self.base_freq = midi_note_to_freq(note) * bend_ratio;
        self.active = true;

        // Initialize envelopes with current settings
        self.env1.reset();
        self.env1.attack = settings.attack;
        self.env1.decay = settings.decay;
        self.env1.sustain = settings.sustain;
        self.env1.release = settings.release;
        self.env1.trigger();

        self.env2.reset();
        self.env2.attack = settings.env2_attack;
        self.env2.decay = settings.env2_decay;
        self.env2.sustain = settings.env2_sustain;
        self.env2.release = settings.env2_release;
        self.env2.trigger();

        self.lfo1.reset();
        self.lfo1.rate_hz = settings.lfo1_rate;
        self.lfo1.trigger();

        self.filter.reset();

        let num_unison = settings.unison.clamp(1, MAX_UNISON);
        self.unison_count = num_unison;

        let norm_power = 1.0 / (num_unison as f32).sqrt();

        for i in 0..num_unison {
            let initial_phase = if settings.phase_rand > 0.001 {
                pseudo_rand(note, i) * settings.phase_rand as f64
            } else {
                0.0
            };

            let (detune_ratio, pan_l, pan_r, gain) = if num_unison == 1 {
                (1.0, 0.7071, 0.7071, 1.0)
            } else {
                let t = (2.0 * i as f32 / (num_unison - 1) as f32) - 1.0;
                let detune_cents = t.signum() * t.abs().powf(1.3) * (settings.detune * 150.0);
                let detune_ratio = 2.0f64.powf((detune_cents as f64) / 1200.0);

                let pan_pos = (0.5 + 0.5 * t * settings.spread).clamp(0.0, 1.0);
                let pan_l = (pan_pos * FRAC_PI_2).cos();
                let pan_r = (pan_pos * FRAC_PI_2).sin();

                let is_center = (num_unison % 2 == 1) && (i == num_unison / 2);
                let voice_gain = if is_center {
                    1.0
                } else {
                    settings.blend.clamp(0.1, 1.0)
                } * norm_power;

                (detune_ratio, pan_l, pan_r, voice_gain)
            };

            self.unison_voices[i] = UnisonSubVoice {
                phase: initial_phase,
                detune_ratio,
                pan_l,
                pan_r,
                gain,
            };
        }
    }

    pub fn note_off(&mut self) {
        if self.active {
            self.env1.release();
            self.env2.release();
        }
    }

    pub fn stop(&mut self) {
        self.active = false;
        self.env1.reset();
        self.env2.reset();
        self.filter.reset();
    }
}

pub struct Settings {
    pub wt_pos: f32,
    pub table_idx: usize,
    pub level: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub unison: usize,
    pub detune: f32,
    pub blend: f32,
    pub spread: f32,
    pub phase_rand: f32,
    pub warp_mode: WarpMode,
    pub warp_amt: f32,
    // Milestone 3 Parameters:
    pub filter_type: FilterType,
    pub cutoff: f32,
    pub resonance: f32,
    pub drive: f32,
    pub env2_attack: f32,
    pub env2_decay: f32,
    pub env2_sustain: f32,
    pub env2_release: f32,
    pub lfo1_rate: f32,
    pub mod_lfo1_wt: f32,
    pub mod_env2_cutoff: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wt_pos: 0.32,
            table_idx: 0,
            level: 0.25,
            attack: 0.012,
            decay: 0.450,
            sustain: 0.70,
            release: 0.250,
            unison: 7,
            detune: 0.18,
            blend: 0.80,
            spread: 1.0,
            phase_rand: 1.0,
            warp_mode: WarpMode::None,
            warp_amt: 0.0,
            filter_type: FilterType::Lowpass24,
            cutoff: 2400.0,
            resonance: 0.40,
            drive: 0.15,
            env2_attack: 0.010,
            env2_decay: 0.350,
            env2_sustain: 0.20,
            env2_release: 0.200,
            lfo1_rate: 2.0,
            mod_lfo1_wt: 0.30,
            mod_env2_cutoff: 0.50,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ended {
    pub id: Option<i32>,
    pub channel: u8,
    pub note: u8,
}

pub struct Engine {
    pub sample_rate: f64,
    pub voices: [Voice; MAX_VOICES],
    pub tables: Vec<Wavetable>,
    pub bend_semitones: f64,
}

impl Engine {
    pub fn new(sample_rate: f64) -> Self {
        let tables = vec![
            Wavetable::basic_shapes(),
            Wavetable::analog_saw(),
            Wavetable::digital_bell(),
        ];
        Self {
            sample_rate: sample_rate.max(8000.0),
            voices: std::array::from_fn(|_| Voice::default()),
            tables,
            bend_semitones: 0.0,
        }
    }

    pub fn reset(&mut self) {
        for v in &mut self.voices {
            v.stop();
        }
        self.bend_semitones = 0.0;
    }

    pub fn set_pitch_bend(&mut self, bend_norm: f32) {
        self.bend_semitones = (bend_norm as f64).clamp(-1.0, 1.0) * 2.0;
        let bend_ratio = 2.0f64.powf(self.bend_semitones / 12.0);
        for v in &mut self.voices {
            if v.active {
                v.base_freq = midi_note_to_freq(v.note) * bend_ratio;
            }
        }
    }

    pub fn note_on(
        &mut self,
        id: Option<i32>,
        channel: u8,
        note: u8,
        velocity: f32,
        settings: &Settings,
    ) {
        let bend_ratio = 2.0f64.powf(self.bend_semitones / 12.0);

        let slot = self.voices.iter().position(|v| !v.active).unwrap_or_else(|| {
            self.voices
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.env1.level.partial_cmp(&b.env1.level).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(0)
        });

        self.voices[slot].note_on(id, channel, note, velocity, bend_ratio, settings);
    }

    pub fn note_off(&mut self, id: Option<i32>, channel: u8, note: u8) {
        for v in &mut self.voices {
            if v.active && (id.is_none() || v.id == id) && v.channel == channel && v.note == note {
                v.note_off();
            }
        }
    }

    pub fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Renders a block of stereo samples with unison, warp, ZDF filtering, and modulation routing.
    pub fn render_block(
        &mut self,
        settings: &Settings,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Vec<Ended> {
        let mut terminated = Vec::new();
        let table_idx = settings.table_idx.min(self.tables.len().saturating_sub(1));
        let table = &self.tables[table_idx];

        let num_samples = left.len().min(right.len());

        for sample_idx in 0..num_samples {
            let mut sum_l = 0.0f32;
            let mut sum_r = 0.0f32;

            for voice in &mut self.voices {
                if !voice.active {
                    continue;
                }

                // Process voice modulation envelopes and LFO
                let env1_val = voice.env1.process(self.sample_rate);
                let env2_val = voice.env2.process(self.sample_rate);
                let lfo1_val = voice.lfo1.process(self.sample_rate);

                if voice.env1.is_idle() {
                    voice.active = false;
                    terminated.push(Ended {
                        id: voice.id,
                        channel: voice.channel,
                        note: voice.note,
                    });
                    continue;
                }

                // Modulate Wavetable Position via LFO 1
                let effective_wt_pos = (settings.wt_pos + lfo1_val * settings.mod_lfo1_wt).clamp(0.0, 1.0);

                // Modulate Filter Cutoff via Envelope 2 (exponential frequency scaling)
                let cutoff_mult = 2.0f32.powf(env2_val * settings.mod_env2_cutoff * 4.0);
                let effective_cutoff = (settings.cutoff * cutoff_mult).clamp(20.0, 20000.0);

                let mut voice_l = 0.0f32;
                let mut voice_r = 0.0f32;

                // Render all unison subvoices
                for u in 0..voice.unison_count {
                    let sub = &mut voice.unison_voices[u];
                    let freq_ratio = (voice.base_freq * sub.detune_ratio) / self.sample_rate;

                    let warped_phase = warp::warp_phase(sub.phase, settings.warp_mode, settings.warp_amt);
                    let osc_sample = table.sample(effective_wt_pos, warped_phase, freq_ratio);

                    voice_l += osc_sample * sub.pan_l * sub.gain;
                    voice_r += osc_sample * sub.pan_r * sub.gain;

                    sub.phase = (sub.phase + freq_ratio).fract();
                }

                // Apply per-voice Zero-Delay Feedback filter
                voice.filter.process(
                    &mut voice_l,
                    &mut voice_r,
                    settings.filter_type,
                    effective_cutoff,
                    settings.resonance,
                    settings.drive,
                    self.sample_rate,
                );

                let master_voice_gain = env1_val * voice.velocity * settings.level;
                sum_l += voice_l * master_voice_gain;
                sum_r += voice_r * master_voice_gain;
            }

            // Soft saturation limiter to protect dynamic range and prevent hard clipping
            left[sample_idx] += sum_l.tanh();
            right[sample_idx] += sum_r.tanh();
        }

        terminated
    }
}

#[inline]
pub fn midi_note_to_freq(note: u8) -> f64 {
    440.0 * 2.0f64.powf((note as f64 - 69.0) / 12.0)
}

#[inline]
fn pseudo_rand(seed1: u8, seed2: usize) -> f64 {
    let mut h = (seed1 as u32).wrapping_mul(0x45d9f3b).wrapping_add(seed2 as u32);
    h = ((h >> 16) ^ h).wrapping_mul(0x45d9f3b);
    h = (h >> 16) ^ h;
    (h as f64) / (u32::MAX as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modulation_lfo_wt_pos_and_filter_cutoff() {
        let mut engine = Engine::new(48000.0);
        let mut settings = Settings::default();
        settings.unison = 1;
        settings.filter_type = FilterType::Lowpass24;
        settings.cutoff = 500.0;
        settings.mod_env2_cutoff = 1.0; // Envelope 2 sweeps cutoff up

        engine.note_on(Some(1), 0, 60, 0.8, &settings);
        assert_eq!(engine.active_voice_count(), 1);

        let mut left = vec![0.0; 512];
        let mut right = vec![0.0; 512];
        let ended = engine.render_block(&settings, &mut left, &mut right);
        assert!(ended.is_empty());
        assert!(left.iter().any(|&s| s.abs() > 0.001));
    }
}
