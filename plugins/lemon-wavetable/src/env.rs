// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Multi-stage ADSR Envelope generator with tension curvature.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    pub stage: Stage,
    pub level: f32,
    pub attack: f32,   // seconds
    pub decay: f32,    // seconds
    pub sustain: f32,  // 0.0 to 1.0
    pub release: f32,  // seconds
    pub tension: f32,  // -1.0 to +1.0 curvature
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
            attack: 0.012,
            decay: 0.450,
            sustain: 0.70,
            release: 0.250,
            tension: 0.0,
        }
    }
}

impl Envelope {
    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    /// Advances the envelope by one sample period (`1.0 / sample_rate`) and returns the current level `[0.0, 1.0]`.
    #[inline]
    pub fn process(&mut self, sample_rate: f64) -> f32 {
        let inv_fs = (1.0 / sample_rate) as f32;

        match self.stage {
            Stage::Idle => 0.0,
            Stage::Attack => {
                let step = inv_fs / self.attack.max(0.001);
                self.level += step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
                self.level
            }
            Stage::Decay => {
                let step = inv_fs / self.decay.max(0.001);
                let target = self.sustain.clamp(0.0, 1.0);
                self.level -= step;
                if self.level <= target {
                    self.level = target;
                    self.stage = Stage::Sustain;
                }
                self.level
            }
            Stage::Sustain => {
                self.level = self.sustain.clamp(0.0, 1.0);
                self.level
            }
            Stage::Release => {
                let step = inv_fs / self.release.max(0.001);
                self.level -= step;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
                self.level
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_cycles_through_stages() {
        let mut env = Envelope::default();
        env.attack = 0.001; // 1ms
        env.decay = 0.002;  // 2ms
        env.sustain = 0.5;
        env.release = 0.002; // 2ms

        env.trigger();
        assert_eq!(env.stage, Stage::Attack);

        let fs = 48000.0;
        // Run past attack (48 samples)
        for _ in 0..60 {
            env.process(fs);
        }
        assert!(env.stage == Stage::Decay || env.stage == Stage::Sustain);

        // Run past decay (96 samples)
        for _ in 0..120 {
            env.process(fs);
        }
        assert_eq!(env.stage, Stage::Sustain);
        assert!((env.level - 0.5).abs() < 1e-2);

        // Release
        env.release();
        assert_eq!(env.stage, Stage::Release);
        for _ in 0..150 {
            env.process(fs);
        }
        assert_eq!(env.stage, Stage::Idle);
        assert_eq!(env.level, 0.0);
    }
}
