// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Wavetable data model, mipmap generation, and bandlimited interpolation.
//!
//! Conforms to the standard 2,048-sample/frame architecture used by Serum and Vital.

use crate::fft;
use std::f32::consts::PI;

pub const TABLE_SIZE: usize = 2048;
pub const NUM_MIPMAPS: usize = 11;

/// Maximum harmonics preserved per mipmap level (from full 1024 down to 1).
pub const MIPMAP_HARMONICS: [usize; NUM_MIPMAPS] = [1024, 512, 256, 128, 64, 32, 16, 8, 4, 2, 1];

/// A single wavetable frame holding 11 precomputed octave-bandlimited mipmap levels.
#[derive(Clone, Debug)]
pub struct Frame {
    /// 11 mipmap levels, each containing exactly `TABLE_SIZE` (2,048) samples.
    pub mipmaps: Vec<Vec<f32>>,
}

impl Frame {
    /// Creates a frame from 2,048 raw single-cycle samples, generating all 11 mipmaps via FFT bandlimiting.
    pub fn from_samples(samples: &[f32]) -> Self {
        assert_eq!(samples.len(), TABLE_SIZE, "Frame must have exactly 2,048 samples");

        // Normalize peak amplitude safely to avoid clipping
        let peak = samples.iter().copied().fold(0.0f32, f32::max);
        let norm_scale = if peak > 1.0 { 1.0 / peak } else { 1.0 };
        let normalized: Vec<f32> = samples.iter().map(|&s| s * norm_scale).collect();

        let mut mipmaps = Vec::with_capacity(NUM_MIPMAPS);
        for &max_harmonic in &MIPMAP_HARMONICS {
            let filtered = fft::bandlimit(&normalized, max_harmonic);
            mipmaps.push(filtered);
        }

        Self { mipmaps }
    }

    /// Read an interpolated sample from this frame for a given phase and mipmap level.
    #[inline]
    pub fn read_sample(&self, phase: f64, mipmap_level: usize) -> f32 {
        let level = mipmap_level.min(NUM_MIPMAPS - 1);
        let table = &self.mipmaps[level];

        // Wrap phase into [0.0, 1.0)
        let p = phase.fract();
        let p = if p < 0.0 { p + 1.0 } else { p };

        let index_f = p * TABLE_SIZE as f64;
        let index0 = index_f as usize % TABLE_SIZE;
        let index1 = (index0 + 1) % TABLE_SIZE;
        let frac = (index_f - index0 as f64) as f32;

        table[index0] * (1.0 - frac) + table[index1] * frac
    }
}

/// A complete multi-frame wavetable (1 to 256 frames).
#[derive(Clone, Debug)]
pub struct Wavetable {
    pub name: String,
    pub frames: Vec<Frame>,
}

impl Wavetable {
    pub fn new(name: impl Into<String>, frames: Vec<Frame>) -> Self {
        assert!(!frames.is_empty(), "Wavetable must have at least one frame");
        Self { name: name.into(), frames }
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Selects the highest allowable mipmap level for a note playing with frequency ratio `f / fs`
    /// such that no harmonics exceed the Nyquist frequency (0.5 * fs).
    #[inline]
    pub fn mipmap_for_freq_ratio(freq_ratio: f64) -> usize {
        if freq_ratio <= 0.0 {
            return 0;
        }
        // Nyquist limit on harmonic: k * freq_ratio < 0.5  =>  k < 0.5 / freq_ratio
        let max_harmonic = (0.5 / freq_ratio).floor() as usize;

        for (level, &harmonics) in MIPMAP_HARMONICS.iter().enumerate() {
            if harmonics <= max_harmonic {
                return level;
            }
        }
        NUM_MIPMAPS - 1
    }

    /// Performs alias-free, dual-frame interpolated sampling.
    ///
    /// - `pos`: Wavetable position [0.0, 1.0].
    /// - `phase`: Cycle phase [0.0, 1.0].
    /// - `freq_ratio`: `voice_freq / sample_rate`.
    #[inline]
    pub fn sample(&self, pos: f32, phase: f64, freq_ratio: f64) -> f32 {
        let num_frames = self.frames.len();
        if num_frames == 1 {
            let mipmap = Self::mipmap_for_freq_ratio(freq_ratio);
            return self.frames[0].read_sample(phase, mipmap);
        }

        let clamped_pos = pos.clamp(0.0, 1.0);
        let frame_pos = clamped_pos * (num_frames - 1) as f32;
        let frame0 = frame_pos.floor() as usize;
        let frame1 = (frame0 + 1).min(num_frames - 1);
        let frac = frame_pos - frame0 as f32;

        let mipmap = Self::mipmap_for_freq_ratio(freq_ratio);
        let s0 = self.frames[frame0].read_sample(phase, mipmap);
        let s1 = self.frames[frame1].read_sample(phase, mipmap);

        s0 * (1.0 - frac) + s1 * frac
    }

    /// Factory Wavetable 1: Basic Shapes
    /// Frame 0: Sine
    /// Frame 1: Triangle
    /// Frame 2: Sawtooth
    /// Frame 3: Square
    /// Frame 4: Pulse 20%
    pub fn basic_shapes() -> Self {
        let mut frames = Vec::with_capacity(5);

        // Sine
        let mut sine = vec![0.0; TABLE_SIZE];
        for i in 0..TABLE_SIZE {
            sine[i] = (i as f32 * 2.0 * PI / TABLE_SIZE as f32).sin();
        }
        frames.push(Frame::from_samples(&sine));

        // Triangle
        let mut tri = vec![0.0; TABLE_SIZE];
        for i in 0..TABLE_SIZE {
            let p = i as f32 / TABLE_SIZE as f32;
            tri[i] = if p < 0.25 {
                4.0 * p
            } else if p < 0.75 {
                2.0 - 4.0 * p
            } else {
                -4.0 + 4.0 * p
            };
        }
        frames.push(Frame::from_samples(&tri));

        // Sawtooth
        let mut saw = vec![0.0; TABLE_SIZE];
        for i in 0..TABLE_SIZE {
            let p = i as f32 / TABLE_SIZE as f32;
            saw[i] = 1.0 - 2.0 * p;
        }
        frames.push(Frame::from_samples(&saw));

        // Square
        let mut sq = vec![0.0; TABLE_SIZE];
        for i in 0..TABLE_SIZE {
            let p = i as f32 / TABLE_SIZE as f32;
            sq[i] = if p < 0.5 { 1.0 } else { -1.0 };
        }
        frames.push(Frame::from_samples(&sq));

        // Pulse 20%
        let mut pulse = vec![0.0; TABLE_SIZE];
        for i in 0..TABLE_SIZE {
            let p = i as f32 / TABLE_SIZE as f32;
            pulse[i] = if p < 0.2 { 1.0 } else { -1.0 };
        }
        frames.push(Frame::from_samples(&pulse));

        Self::new("Basic Shapes", frames)
    }

    /// Factory Wavetable 2: Analog Saw
    /// Morphs from a round vintage saw through sync-saturated analog teeth.
    pub fn analog_saw() -> Self {
        let mut frames = Vec::with_capacity(4);
        for frame_idx in 0..4 {
            let mut samples = vec![0.0; TABLE_SIZE];
            let harmonic_emphasis = 1.0 + frame_idx as f32 * 0.75;
            for i in 0..TABLE_SIZE {
                let p = i as f32 / TABLE_SIZE as f32;
                let saw = 1.0 - 2.0 * p;
                let saturated = (saw * harmonic_emphasis).tanh();
                samples[i] = saturated;
            }
            frames.push(Frame::from_samples(&samples));
        }
        Self::new("Analog Saw", frames)
    }

    /// Factory Wavetable 3: Digital Bell
    /// FM spectrum table generating metallic, shimmering bells and vocal overtones.
    pub fn digital_bell() -> Self {
        let mut frames = Vec::with_capacity(4);
        for frame_idx in 0..4 {
            let mut samples = vec![0.0; TABLE_SIZE];
            let mod_index = 0.5 + frame_idx as f32 * 1.5;
            for i in 0..TABLE_SIZE {
                let phi = i as f32 * 2.0 * PI / TABLE_SIZE as f32;
                let modulator = (phi * 3.5).sin();
                samples[i] = (phi + modulator * mod_index).sin();
            }
            frames.push(Frame::from_samples(&samples));
        }
        Self::new("Digital Bell", frames)
    }

    /// Parses a standard Serum `.wav` file (2,048 samples per frame, mono or stereo, 32-bit float or 16/24-bit PCM).
    pub fn from_serum_wav(bytes: &[u8], name: impl Into<String>) -> Result<Self, String> {
        if bytes.len() < 44 {
            return Err("WAV file too short".into());
        }
        if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err("Invalid WAV header".into());
        }

        let mut offset = 12;
        let mut channels = 1;
        let mut bits_per_sample = 16;
        let mut is_float = false;
        let mut data_offset = 0;
        let mut data_len = 0;

        while offset + 8 <= bytes.len() {
            let chunk_id = &bytes[offset..offset + 4];
            let chunk_size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
            offset += 8;

            if chunk_id == b"fmt " && chunk_size >= 16 {
                let audio_format = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
                channels = u16::from_le_bytes(bytes[offset + 2..offset + 4].try_into().unwrap()) as usize;
                bits_per_sample = u16::from_le_bytes(bytes[offset + 14..offset + 16].try_into().unwrap()) as usize;
                is_float = audio_format == 3;
            } else if chunk_id == b"data" {
                data_offset = offset;
                data_len = chunk_size.min(bytes.len() - offset);
                break;
            }
            offset += chunk_size;
        }

        if data_offset == 0 || data_len == 0 {
            return Err("No data chunk found in WAV".into());
        }

        // Decode PCM/Float samples to mono f32
        let bytes_per_sample = bits_per_sample / 8;
        if bytes_per_sample == 0 || channels == 0 {
            return Err("Invalid WAV format parameters".into());
        }

        let frame_bytes = bytes_per_sample * channels;
        let total_samples = data_len / frame_bytes;
        let mut decoded = Vec::with_capacity(total_samples);

        for i in 0..total_samples {
            let sample_offset = data_offset + i * frame_bytes;
            let val = if is_float && bits_per_sample == 32 {
                f32::from_le_bytes(bytes[sample_offset..sample_offset + 4].try_into().unwrap())
            } else if bits_per_sample == 16 {
                let s = i16::from_le_bytes(bytes[sample_offset..sample_offset + 2].try_into().unwrap());
                s as f32 / 32768.0
            } else if bits_per_sample == 24 {
                let b = [bytes[sample_offset], bytes[sample_offset + 1], bytes[sample_offset + 2]];
                let s = (b[0] as i32) | ((b[1] as i32) << 8) | (((b[2] as i8) as i32) << 16);
                s as f32 / 8388608.0
            } else {
                return Err(format!("Unsupported bit depth: {bits_per_sample}"));
            };
            decoded.push(val);
        }

        if decoded.len() < TABLE_SIZE {
            return Err(format!("Audio data has fewer than {TABLE_SIZE} samples"));
        }

        let num_frames = (decoded.len() / TABLE_SIZE).min(256);
        let mut frames = Vec::with_capacity(num_frames);

        for f in 0..num_frames {
            let start = f * TABLE_SIZE;
            let slice = &decoded[start..start + TABLE_SIZE];
            frames.push(Frame::from_samples(slice));
        }

        Ok(Self::new(name, frames))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_shapes_generation() {
        let wt = Wavetable::basic_shapes();
        assert_eq!(wt.frame_count(), 5);
        assert_eq!(wt.frames[0].mipmaps.len(), NUM_MIPMAPS);

        // Sine at low freq ratio (e.g. 100 Hz / 48000 Hz ~ 0.002) should sample mipmap 0
        let s = wt.sample(0.0, 0.25, 100.0 / 48000.0);
        assert!((s - 1.0).abs() < 1e-2, "sine peak at phase 0.25 should be ~1.0: {s}");

        // High frequency ratio (e.g. 15000 Hz / 48000 Hz ~ 0.3125) should select mipmap with 1 harmonic
        let mip = Wavetable::mipmap_for_freq_ratio(15000.0 / 48000.0);
        assert_eq!(MIPMAP_HARMONICS[mip], 1);
    }

    #[test]
    fn frame_interpolation_is_smooth() {
        let wt = Wavetable::basic_shapes();
        let s_sine = wt.sample(0.0, 0.25, 0.005);
        let s_tri = wt.sample(0.25, 0.25, 0.005);
        let s_mid = wt.sample(0.125, 0.25, 0.005);

        // Midpoint should be approximately average of sine and triangle
        let expected = (s_sine + s_tri) * 0.5;
        assert!((s_mid - expected).abs() < 0.05);
    }
}
