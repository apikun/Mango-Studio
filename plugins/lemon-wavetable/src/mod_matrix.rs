// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Modulation Matrix routing engine for Lemon Wavetable.

pub const MAX_ROUTES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Env1 = 0,
    Env2 = 1,
    Lfo1 = 2,
    Velocity = 3,
    KeyTrack = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    WtPos = 0,
    WarpAmt = 1,
    FilterCutoff = 2,
    FilterRes = 3,
    Pitch = 4,
}

#[derive(Clone, Copy, Debug)]
pub struct ModRoute {
    pub source: Source,
    pub dest: Dest,
    pub amount: f32, // -1.0 to +1.0
    pub bipolar: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ModValues {
    pub env1: f32,     // 0.0 to 1.0
    pub env2: f32,     // 0.0 to 1.0
    pub lfo1: f32,     // -1.0 to 1.0
    pub velocity: f32, // 0.0 to 1.0
    pub keytrack: f32, // -1.0 to 1.0 (relative to middle C)
}

impl ModValues {
    #[inline]
    pub fn get(&self, src: Source) -> f32 {
        match src {
            Source::Env1 => self.env1,
            Source::Env2 => self.env2,
            Source::Lfo1 => self.lfo1,
            Source::Velocity => self.velocity,
            Source::KeyTrack => self.keytrack,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ModMatrix {
    pub routes: Vec<ModRoute>,
}

impl ModMatrix {
    pub fn new() -> Self {
        Self { routes: Vec::with_capacity(MAX_ROUTES) }
    }

    pub fn add_route(&mut self, source: Source, dest: Dest, amount: f32, bipolar: bool) {
        if self.routes.len() < MAX_ROUTES {
            self.routes.push(ModRoute {
                source,
                dest,
                amount: amount.clamp(-1.0, 1.0),
                bipolar,
            });
        }
    }

    pub fn clear(&mut self) {
        self.routes.clear();
    }

    /// Calculates total accumulated modulation offset for a specific destination.
    #[inline]
    pub fn eval_offset(&self, dest: Dest, values: &ModValues) -> f32 {
        let mut total = 0.0f32;
        for r in &self.routes {
            if r.dest == dest {
                let src_val = values.get(r.source);
                let scaled = if r.bipolar {
                    src_val * r.amount
                } else {
                    // Unipolar map: 0..1 * amount
                    src_val.max(0.0) * r.amount
                };
                total += scaled;
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_routes_correctly() {
        let mut matrix = ModMatrix::new();
        matrix.add_route(Source::Lfo1, Dest::WtPos, 0.5, true);
        matrix.add_route(Source::Env2, Dest::FilterCutoff, 1.0, false);

        let values = ModValues {
            env1: 1.0,
            env2: 0.75,
            lfo1: -0.8,
            velocity: 1.0,
            keytrack: 0.0,
        };

        let wt_offset = matrix.eval_offset(Dest::WtPos, &values);
        assert!((wt_offset - (-0.4)).abs() < 1e-4);

        let cutoff_offset = matrix.eval_offset(Dest::FilterCutoff, &values);
        assert!((cutoff_offset - 0.75).abs() < 1e-4);
    }
}
