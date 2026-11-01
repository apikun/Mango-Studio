// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Wavetable Warp Engine (Serum-style phase manipulation).
//!
//! Applies non-linear phase transformations before wavetable lookup, enabling
//! Hard Sync, Bend (+/-), PWM, Asymmetry, and Phase Flip.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarpMode {
    None = 0,
    Sync = 1,
    BendPlus = 2,
    BendMinus = 3,
    Pwm = 4,
    AsymPlus = 5,
    Flip = 6,
}

impl From<i32> for WarpMode {
    fn from(val: i32) -> Self {
        match val {
            1 => WarpMode::Sync,
            2 => WarpMode::BendPlus,
            3 => WarpMode::BendMinus,
            4 => WarpMode::Pwm,
            5 => WarpMode::AsymPlus,
            6 => WarpMode::Flip,
            _ => WarpMode::None,
        }
    }
}

/// Transforms oscillator phase `[0.0, 1.0)` according to the chosen warp mode and normalized amount `[0.0, 1.0]`.
#[inline]
pub fn warp_phase(phase: f64, mode: WarpMode, amount: f32) -> f64 {
    let p = phase.fract();
    let p = if p < 0.0 { p + 1.0 } else { p };

    if amount <= 0.0001 && mode != WarpMode::Sync {
        return p;
    }

    let amt = amount.clamp(0.0, 1.0) as f64;

    match mode {
        WarpMode::None => p,

        // Hard Sync: Multiplies phase frequency by up to 8x and wraps per cycle
        WarpMode::Sync => {
            let sync_factor = 1.0 + amt * 7.0; // 1x to 8x sync
            (p * sync_factor).fract()
        }

        // Bend (+): Compresses phase towards the center of the waveform (0.5)
        WarpMode::BendPlus => {
            let power = 1.0 + amt * 5.0;
            if p < 0.5 {
                0.5 * (2.0 * p).powf(power)
            } else {
                1.0 - 0.5 * (2.0 * (1.0 - p)).powf(power)
            }
        }

        // Bend (-): Expands phase outward from center towards the edges
        WarpMode::BendMinus => {
            let power = 1.0 / (1.0 + amt * 5.0);
            if p < 0.5 {
                0.5 * (2.0 * p).powf(power)
            } else {
                1.0 - 0.5 * (2.0 * (1.0 - p)).powf(power)
            }
        }

        // Pulse Width Modulation: Skews the split point of any wave shape
        WarpMode::Pwm => {
            let split = (0.5 + (amt - 0.5) * 0.85).clamp(0.05, 0.95);
            if p < split {
                (p / split) * 0.5
            } else {
                0.5 + ((p - split) / (1.0 - split)) * 0.5
            }
        }

        // Asymmetry (+): Non-linear skew accelerating through the cycle
        WarpMode::AsymPlus => {
            let exponent = 1.0 + amt * 3.5;
            p.powf(exponent)
        }

        // Flip: Mirrors the second half of the cycle
        WarpMode::Flip => {
            if p < 0.5 {
                p * 2.0
            } else {
                (1.0 - p) * 2.0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warp_none_returns_exact_phase() {
        assert_eq!(warp_phase(0.25, WarpMode::None, 0.5), 0.25);
        assert_eq!(warp_phase(0.75, WarpMode::None, 1.0), 0.75);
    }

    #[test]
    fn warp_sync_multiplies_phase() {
        // At 1.0 sync (8x sync), phase 0.125 should wrap to 0.0
        let p = warp_phase(0.125, WarpMode::Sync, 1.0);
        assert!(p.abs() < 1e-4 || (p - 1.0).abs() < 1e-4);

        // Phase 0.5 should wrap halfway
        let p2 = warp_phase(0.5, WarpMode::Sync, 0.0);
        assert!((p2 - 0.5).abs() < 1e-4);
    }

    #[test]
    fn warp_bend_preserves_endpoints() {
        assert!((warp_phase(0.0, WarpMode::BendPlus, 0.8) - 0.0).abs() < 1e-4);
        assert!((warp_phase(0.5, WarpMode::BendPlus, 0.8) - 0.5).abs() < 1e-4);
        assert!((warp_phase(1.0, WarpMode::BendPlus, 0.8) - 0.0).abs() < 1e-4);

        assert!((warp_phase(0.0, WarpMode::BendMinus, 0.8) - 0.0).abs() < 1e-4);
        assert!((warp_phase(0.5, WarpMode::BendMinus, 0.8) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn warp_pwm_monotonic_and_bounded() {
        for amt in [0.1, 0.5, 0.9] {
            let mut prev = 0.0;
            for step in 0..100 {
                let phase = step as f64 / 100.0;
                let warped = warp_phase(phase, WarpMode::Pwm, amt);
                assert!(warped >= 0.0 && warped <= 1.0);
                assert!(warped >= prev, "monotonicity failed at step {step}: {warped} < {prev}");
                prev = warped;
            }
        }
    }
}
