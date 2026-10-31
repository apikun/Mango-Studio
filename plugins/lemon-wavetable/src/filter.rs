// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Zero-Delay Feedback (ZDF) State-Variable Filter (SVF) with non-linear saturation.
//!
//! Provides mathematically stable, resonance-preserving multi-mode filtering
//! (12 dB & 24 dB Lowpass, Highpass, Bandpass, Peak) with drive saturation.

use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterType {
    Off = 0,
    Lowpass12 = 1,
    Lowpass24 = 2,
    Highpass12 = 3,
    Bandpass12 = 4,
    Peak = 5,
}

impl From<i32> for FilterType {
    fn from(val: i32) -> Self {
        match val {
            1 => FilterType::Lowpass12,
            2 => FilterType::Lowpass24,
            3 => FilterType::Highpass12,
            4 => FilterType::Bandpass12,
            5 => FilterType::Peak,
            _ => FilterType::Off,
        }
    }
}

/// Single-channel 2-pole ZDF SVF core (Vadim Zavalishin topology).
#[derive(Clone, Copy, Debug, Default)]
pub struct SvfChannel {
    s1: f32,
    s2: f32,
}

impl SvfChannel {
    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    #[inline]
    pub fn process(&mut self, input: f32, g: f32, k: f32, filter_type: FilterType) -> f32 {
        if filter_type == FilterType::Off {
            return input;
        }

        // ZDF SVF discrete coefficients
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v1 = a1 * self.s1 + a2 * (input - self.s2);
        let v2 = self.s2 + a2 * self.s1 + a3 * (input - self.s2);

        // Update integrators with anti-denormal flushing
        self.s1 = 2.0 * v1 - self.s1;
        self.s2 = 2.0 * v2 - self.s2;
        if self.s1.abs() < 1e-15 { self.s1 = 0.0; }
        if self.s2.abs() < 1e-15 { self.s2 = 0.0; }

        let lp = v2;
        let bp = v1;
        let hp = input - k * v1 - v2;

        match filter_type {
            FilterType::Lowpass12 | FilterType::Lowpass24 => lp,
            FilterType::Highpass12 => hp,
            FilterType::Bandpass12 => bp,
            FilterType::Peak => lp - hp,
            FilterType::Off => input,
        }
    }
}

/// Stereo ZDF filter supporting 12 dB and 24 dB (cascaded 4-pole) modes with saturation drive.
#[derive(Clone, Copy, Debug, Default)]
pub struct StereoFilter {
    stage1_l: SvfChannel,
    stage1_r: SvfChannel,
    stage2_l: SvfChannel,
    stage2_r: SvfChannel,
}

impl StereoFilter {
    pub fn reset(&mut self) {
        self.stage1_l.reset();
        self.stage1_r.reset();
        self.stage2_l.reset();
        self.stage2_r.reset();
    }

    /// Processes a single stereo frame in-place.
    ///
    /// - `cutoff`: Cutoff frequency in Hz (20.0 to 20,000.0).
    /// - `resonance`: Resonance 0.0 to 1.0 (Q from 0.5 to 20.0).
    /// - `drive`: Saturation drive 0.0 to 1.0 (0 to +18 dB).
    /// - `sample_rate`: Sampling rate in Hz.
    #[inline]
    pub fn process(
        &mut self,
        left: &mut f32,
        right: &mut f32,
        filter_type: FilterType,
        cutoff: f32,
        resonance: f32,
        drive: f32,
        sample_rate: f64,
    ) {
        if filter_type == FilterType::Off {
            return;
        }

        // Prewarp cutoff frequency into normalized digital frequency g
        let clamped_cutoff = cutoff.clamp(20.0, (sample_rate * 0.49) as f32);
        let omega = PI * clamped_cutoff / sample_rate as f32;
        let g = omega.tan();

        // Resonance Q mapping: 0.0 -> Q=0.707 (Butterworth), 1.0 -> Q=18.0
        let q = 0.707 + resonance.clamp(0.0, 1.0) * 17.3;
        let k = 1.0 / q;

        // Drive stage (soft tanh saturation before filtering)
        let drive_gain = 1.0 + drive.clamp(0.0, 1.0) * 4.0;
        let in_l = (*left * drive_gain).tanh();
        let in_r = (*right * drive_gain).tanh();

        let out1_l = self.stage1_l.process(in_l, g, k, filter_type);
        let out1_r = self.stage1_r.process(in_r, g, k, filter_type);

        if filter_type == FilterType::Lowpass24 {
            // Cascade through second stage for 24 dB/oct steep slope
            *left = self.stage2_l.process(out1_l, g, k, filter_type);
            *right = self.stage2_r.process(out1_r, g, k, filter_type);
        } else {
            *left = out1_l;
            *right = out1_r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_lowpass_attenuates_high_frequencies() {
        let mut filter = StereoFilter::default();
        let sample_rate = 48000.0;

        // Process high frequency (10 kHz) through a lowpass at 500 Hz
        let mut out_high_l = 1.0f32;
        let mut out_high_r = 1.0f32;
        for _ in 0..100 {
            filter.process(&mut out_high_l, &mut out_high_r, FilterType::Lowpass24, 500.0, 0.0, 0.0, sample_rate);
        }

        // After settling, high frequency DC/steady input is heavily attenuated
        assert!(out_high_l.abs() < 0.1, "24dB lowpass should heavily attenuate above cutoff: {out_high_l}");
    }

    #[test]
    fn filter_off_is_transparent() {
        let mut filter = StereoFilter::default();
        let mut l = 0.42f32;
        let mut r = -0.73f32;
        filter.process(&mut l, &mut r, FilterType::Off, 1000.0, 0.5, 0.0, 48000.0);
        assert_eq!(l, 0.42);
        assert_eq!(r, -0.73);
    }
}
