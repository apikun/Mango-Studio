// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Vector Breakpoint LFO generator with arbitrary node curves and tempo/Hz sync.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoMode {
    Trigger = 0,
    Envelope = 1,
    FreeRun = 2,
}

impl From<i32> for LfoMode {
    fn from(val: i32) -> Self {
        match val {
            1 => LfoMode::Envelope,
            2 => LfoMode::FreeRun,
            _ => LfoMode::Trigger,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LfoNode {
    pub x: f32,       // 0.0 to 1.0 (horizontal position)
    pub y: f32,       // 0.0 to 1.0 (vertical value)
    pub tension: f32, // -1.0 to +1.0 (curvature)
}

#[derive(Clone, Debug)]
pub struct Lfo {
    pub nodes: Vec<LfoNode>,
    pub rate_hz: f32,
    pub mode: LfoMode,
    pub phase: f32,
    pub ended: bool,
}

impl Default for Lfo {
    fn default() -> Self {
        Self {
            nodes: vec![
                LfoNode { x: 0.0, y: 0.0, tension: 0.0 },
                LfoNode { x: 0.5, y: 1.0, tension: 0.0 },
                LfoNode { x: 1.0, y: 0.0, tension: 0.0 },
            ],
            rate_hz: 2.0,
            mode: LfoMode::Trigger,
            phase: 0.0,
            ended: false,
        }
    }
}

impl Lfo {
    pub fn trigger(&mut self) {
        if self.mode != LfoMode::FreeRun {
            self.phase = 0.0;
            self.ended = false;
        }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.ended = false;
    }

    /// Evaluates the curve at normalized phase `[0.0, 1.0]` with tension interpolation.
    pub fn eval_at(&self, phase: f32) -> f32 {
        if self.nodes.is_empty() {
            return 0.0;
        }
        if self.nodes.len() == 1 {
            return self.nodes[0].y;
        }

        let p = phase.clamp(0.0, 1.0);

        // Find surrounding node pair
        let mut idx = 0;
        while idx < self.nodes.len() - 1 && self.nodes[idx + 1].x < p {
            idx += 1;
        }

        if idx >= self.nodes.len() - 1 {
            return self.nodes.last().unwrap().y;
        }

        let n0 = self.nodes[idx];
        let n1 = self.nodes[idx + 1];

        let span = (n1.x - n0.x).max(1e-5);
        let mut t = ((p - n0.x) / span).clamp(0.0, 1.0);

        // Apply tension curve (concave / convex shaping)
        if n0.tension.abs() > 0.001 {
            let k = 1.0 + n0.tension.abs() * 3.0;
            t = if n0.tension > 0.0 {
                t.powf(k)
            } else {
                1.0 - (1.0 - t).powf(k)
            };
        }

        n0.y * (1.0 - t) + n1.y * t
    }

    /// Advances the LFO by one sample step and returns the current output level `[-1.0, +1.0]`.
    #[inline]
    pub fn process(&mut self, sample_rate: f64) -> f32 {
        if self.ended && self.mode == LfoMode::Envelope {
            let unipolar = self.nodes.last().map(|n| n.y).unwrap_or(0.0);
            return unipolar * 2.0 - 1.0;
        }

        let dt = (self.rate_hz.max(0.01) / sample_rate as f32) as f32;
        let output_unipolar = self.eval_at(self.phase);

        self.phase += dt;
        if self.phase >= 1.0 {
            if self.mode == LfoMode::Envelope {
                self.phase = 1.0;
                self.ended = true;
            } else {
                self.phase = self.phase.fract();
            }
        }

        // Return bipolar signal [-1.0, +1.0]
        output_unipolar * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfo_triangle_eval() {
        let lfo = Lfo::default(); // default triangle 0.0 -> 1.0 -> 0.0
        assert_eq!(lfo.eval_at(0.0), 0.0);
        assert!((lfo.eval_at(0.5) - 1.0).abs() < 1e-4);
        assert_eq!(lfo.eval_at(1.0), 0.0);
    }

    #[test]
    fn lfo_envelope_mode_stops_at_end() {
        let mut lfo = Lfo::default();
        lfo.mode = LfoMode::Envelope;
        lfo.rate_hz = 100.0;
        lfo.trigger();

        // Run through full cycle
        for _ in 0..1000 {
            lfo.process(48000.0);
        }

        assert!(lfo.ended);
        assert_eq!(lfo.phase, 1.0);
    }
}
