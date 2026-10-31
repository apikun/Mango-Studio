// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Procedural 808 drum synthesis: Kick, Clap, Hat, Snare.
//!
//! Synthesizes classic Roland TR-808 style analog drum sounds mathematically,
//! providing zero-dependency built-in factory drums for Mango Studio.

use std::f64::consts::TAU;
use std::sync::Arc;
use crate::Sample;

/// Simple deterministic pseudo-random generator (LCG) for reproducible noise.
struct Noise(u32);

impl Noise {
    fn new(seed: u32) -> Self {
        Noise(seed)
    }

    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 as f64 / u32::MAX as f64) * 2.0 - 1.0
    }
}

/// Synthesizes a punchy Roland TR-808 style kick drum.
///
/// Features an exponential pitch drop from ~180 Hz to ~48 Hz, an analog-style
/// saturation curve, and a fast transient click for punch through mixes.
pub fn kick_808(rate: u32) -> Arc<Sample> {
    let r = f64::from(rate.max(8000));
    let n = (r * 0.55) as usize;
    let mut left = Vec::with_capacity(n);

    let f_start = 180.0;
    let f_end = 48.0;
    let pitch_decay = 42.0;

    for i in 0..n {
        let t = i as f64 / r;
        let attack = (t / 0.002).min(1.0);
        let amp_decay = (-t * 5.2).exp();

        // Integrate frequency f(t) = f_end + (f_start - f_end) * exp(-pitch_decay * t)
        let phase = TAU * (f_end * t + ((f_start - f_end) / pitch_decay) * (1.0 - (-t * pitch_decay).exp()));
        let body = phase.sin();

        // Fast high-frequency transient click for punch
        let click = (TAU * 650.0 * t).sin() * (-t * 450.0).exp();

        let raw = 0.85 * body * amp_decay * attack + 0.3 * click;
        // Soft analog saturation
        let sample = (raw * 1.5).tanh() * 0.88;
        left.push(sample as f32);
    }

    Arc::new(Sample { rate, right: left.clone(), left })
}

/// Synthesizes a classic Roland TR-808 style snare drum.
///
/// Combines dual bridged-T resonant tonal modes with filtered white noise
/// for the snappy snare wire rattle.
pub fn snare_808(rate: u32) -> Arc<Sample> {
    let r = f64::from(rate.max(8000));
    let n = (r * 0.35) as usize;
    let mut left = Vec::with_capacity(n);
    let mut rng = Noise::new(0x5A8E_B961);

    // Simple 1-pole lowpass filter state for noise shaping
    let mut lp = 0.0;
    let cutoff_lp = 0.35; // Normalized frequency

    for i in 0..n {
        let t = i as f64 / r;
        let attack = (t / 0.001).min(1.0);

        // Dual resonant modes
        let phase1 = TAU * (140.0 * t + (45.0 / 35.0) * (1.0 - (-t * 35.0).exp()));
        let phase2 = TAU * (260.0 * t + (70.0 / 35.0) * (1.0 - (-t * 35.0).exp()));
        let body = (0.6 * phase1.sin() * (-t * 24.0).exp() + 0.4 * phase2.sin() * (-t * 28.0).exp()) * attack;

        // Snappy noise component
        let raw_noise = rng.next_f64();
        lp += cutoff_lp * (raw_noise - lp);
        let snappy = (raw_noise - lp) * (-t * 11.5).exp() * attack;

        let mix = 0.48 * body + 0.65 * snappy;
        let sample = mix.clamp(-1.0, 1.0) * 0.88;
        left.push(sample as f32);
    }

    Arc::new(Sample { rate, right: left.clone(), left })
}

/// Synthesizes a classic Roland TR-808 handclap.
///
/// Features 4 distinct noise bursts spaced ~11 ms apart (the "flam" effect),
/// followed by a decaying reverberant tail, bandpass filtered around 1.2 kHz.
pub fn clap_808(rate: u32) -> Arc<Sample> {
    let r = f64::from(rate.max(8000));
    let n = (r * 0.40) as usize;
    let mut left = Vec::with_capacity(n);
    let mut rng = Noise::new(0x1234_5678);

    // Biquad bandpass filter around 1200 Hz
    let f0 = 1200.0 / r;
    let q = 1.8;
    let w0 = TAU * f0;
    let alpha = w0.sin() / (2.0 * q);
    let b0 = alpha;
    let b1 = 0.0;
    let b2 = -alpha;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * w0.cos();
    let a2 = 1.0 - alpha;

    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    let burst_times = [0.0, 0.011, 0.022, 0.033];

    for i in 0..n {
        let t = i as f64 / r;

        // Flam envelope
        let mut env = 0.0;
        for &bt in &burst_times {
            if t >= bt {
                let dt = t - bt;
                env += (-dt * 160.0).exp();
            }
        }
        if t >= 0.033 {
            let dt = t - 0.033;
            env += (-dt * 13.0).exp() * 0.9;
        }

        let raw_noise = rng.next_f64() * env;
        // Apply bandpass filter
        let y = (b0 / a0) * raw_noise + (b1 / a0) * x1 + (b2 / a0) * x2 - (a1 / a0) * y1 - (a2 / a0) * y2;
        x2 = x1;
        x1 = raw_noise;
        y2 = y1;
        y1 = y;

        let sample = (y * 1.8).clamp(-1.0, 1.0) * 0.88;
        left.push(sample as f32);
    }

    Arc::new(Sample { rate, right: left.clone(), left })
}

/// Synthesizes a crisp Roland TR-808 closed hi-hat.
///
/// Features a metallic cluster of 6 square wave oscillators mixed with highpass-filtered
/// noise and an ultra-fast decay envelope.
pub fn hat_808(rate: u32) -> Arc<Sample> {
    let r = f64::from(rate.max(8000));
    let n = (r * 0.12) as usize;
    let mut left = Vec::with_capacity(n);
    let mut rng = Noise::new(0xCAFE_BABE);

    // 6 classic 808 metallic oscillator frequencies
    let freqs = [245.3, 306.5, 368.1, 412.3, 541.2, 843.5];

    // Simple 1-pole highpass filter for high-frequency sizzle
    let mut hp_state = 0.0;
    let hp_cutoff = 0.65; // Highpass around 7 kHz at 44.1 kHz

    for i in 0..n {
        let t = i as f64 / r;
        let attack = (t / 0.0005).min(1.0);
        let decay = (-t * 38.0).exp();

        // Sum metallic square waves
        let mut metal = 0.0;
        for &f in &freqs {
            let phase = TAU * f * t;
            metal += if phase.sin() > 0.0 { 1.0 } else { -1.0 };
        }
        metal /= freqs.len() as f64;

        // Add high noise for sizzle
        let noise = rng.next_f64();
        let input = 0.7 * metal + 0.3 * noise;

        // Highpass filter
        let hp = input - hp_state;
        hp_state += hp_cutoff * hp;

        let sample = hp * decay * attack * 0.85;
        left.push(sample.clamp(-1.0, 1.0) as f32);
    }

    Arc::new(Sample { rate, right: left.clone(), left })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procedural_drums_generate_valid_samples() {
        for (name, sample) in [
            ("kick", kick_808(44100)),
            ("snare", snare_808(44100)),
            ("clap", clap_808(44100)),
            ("hat", hat_808(44100)),
        ] {
            assert!(sample.frames() > 1000, "{name} has frames");
            assert_eq!(sample.left.len(), sample.right.len(), "{name} is stereo");
            assert_eq!(sample.left, sample.right, "{name} channels match");

            let peak = sample.left.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
            assert!(peak > 0.3 && peak <= 1.0, "{name} peak {peak} in range [0.3, 1.0]");
            assert!(sample.left.iter().all(|v| v.is_finite()), "{name} has finite values");
        }
    }
}
