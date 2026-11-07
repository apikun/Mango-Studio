// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Lemon Studio's OTTER (a recreation of the idea behind Xfer's OTT): a three-band upward / downward compressor.
//! The bands split at two crossovers with state-variable filters; the low and the high take the SVF's outputs and
//! the middle is the input minus them, so the three sum back to exactly what came in. Each band runs a fixed
//! threshold with a downward ratio and an upward one (quiet parts pulled up), a follower's time from the TIME
//! setting, and the DEPTH mixes the processed band back against the dry one.
//!
// spec: plugins/lemon-otter.md

use super::Effect;

/// The settings, in the saved state's order.
pub const PARAMS: usize = 4;
/// How much of the processing comes through, stored as percent * 128.
pub const DEPTH: usize = 0;
/// The high band's crossover, stored as semitone-like steps of the range (see `crossover`).
pub const HIGH_CUT: usize = 1;
/// The low band's crossover.
pub const LOW_CUT: usize = 2;
/// The follower's time (its scale, 0 = quick).
pub const TIME: usize = 3;

/// The editor's ranges.
pub const RANGES: [(i32, i32); PARAMS] = [(0, 12800), (0, 127), (0, 127), (0, 127)];

/// A new instance's settings: the classic full-depth OTT shape.
pub const DEFAULTS: [i32; PARAMS] = [12800, 96, 34, 32];

/// The saved state: four ints, no version field.
pub const STATE_LEN: usize = 16;

/// The downward ratio (everything above the threshold folds toward it).
pub const DOWN_RATIO: f32 = 4.0;
/// The upward ratio (quiet parts move toward the threshold at this ratio, in dB).
pub const UP_RATIO: f32 = 2.0;
/// The fixed threshold, in dB.
pub const THRESHOLD_DB: f32 = -40.0;
/// The low crossover's range.
pub const LOW_RANGE: (f32, f32) = (30.0, 1000.0);
/// The high crossover's range.
pub const HIGH_RANGE: (f32, f32) = (500.0, 12000.0);

/// Everything the saved state says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub v: [i32; PARAMS],
}

impl Default for Settings {
    fn default() -> Self {
        Settings { v: DEFAULTS }
    }
}

impl Settings {
    /// A saved state (16 bytes; longer ones are read from their start).
    pub fn from_state(b: &[u8]) -> Option<Settings> {
        Some(Settings { v: super::ints(b, 0)? })
    }

    /// The saved state (16 bytes).
    pub fn to_state(&self) -> Vec<u8> {
        super::state_of(&self.v)
    }
}

/// The depth (0..1) from its stored value.
pub fn depth(stored: i32) -> f32 {
    (stored as f32 / 12800.0).clamp(0.0, 1.0)
}

/// A crossover frequency from its stored step (log across the range), clamped inside it.
pub fn crossover(stored: i32, range: (f32, f32)) -> f32 {
    let t = f64::from(stored.clamp(0, 127)) / 127.0;
    let (lo, hi) = (f64::from(range.0), f64::from(range.1));
    (lo * (hi / lo).powf(t)) as f32
}

/// The follower's time scale from the stored step: an attack of about 3 ms at the quickest to 60 ms, a release of
/// about 50 ms to 1 s, so the middle sits near OTT's.
pub fn times(stored: i32, rate: u32) -> (f32, f32) {
    let t = stored.clamp(0, 127) as f32 / 127.0;
    let attack = 3.0 + t * 57.0;
    let release = 50.0 + t * 950.0;
    let per_sample = |ms: f32| 1.0 - (-(rate as f32) / 1000.0 / ms.max(0.05)).exp();
    (per_sample(attack), per_sample(release))
}

/// dB to a linear gain.
pub fn gain_of(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// One band's dynamics: the follower and the two gain computers.
#[derive(Clone, Copy)]
struct Band {
    env: f32,
}

impl Band {
    fn new() -> Band {
        Band { env: 0.0 }
    }

    /// One sample: the follower's step and the band's gain. Both branches work in dB, like the original's gain
    /// computer.
    #[inline]
    fn gain(&mut self, x: f32, up: f32, down: f32) -> f32 {
        let a = x.abs();
        let k = if a > self.env { up } else { down };
        self.env += (a - self.env) * k;
        let env_db = 20.0 * self.env.max(1.0e-6).log10();
        let g_db = if env_db > THRESHOLD_DB {
            // Downward: everything over the threshold folds toward it at the ratio.
            -(env_db - THRESHOLD_DB) * (1.0 - 1.0 / DOWN_RATIO)
        } else {
            // Upward: quiet parts rise toward the threshold.
            (THRESHOLD_DB - env_db) * (1.0 - 1.0 / UP_RATIO)
        };
        gain_of(g_db)
    }
}

/// A 2nd-order Butterworth biquad (RBJ's cookbook) with its own state: the band splitter's low pass or high
/// pass. The middle band is the input minus the two taken bands, so the three sum back to exactly what came in.
#[derive(Clone, Copy)]
struct Biquad {
    // Normalised coefficients.
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    // Direct form 1 state.
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    /// `high` = a high pass, else a low pass, at `cutoff` (Butterworth Q).
    fn new(cutoff: f32, rate: u32, high: bool) -> Biquad {
        let w = std::f32::consts::PI * 2.0 * cutoff.min(rate as f32 * 0.45) / rate as f32;
        let (cw, sw) = (w.cos(), w.sin());
        let alpha = sw / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let (b0, b1, b2) = if high { ((1.0 + cw) / 2.0, -(1.0 + cw), (1.0 + cw) / 2.0) } else { ((1.0 - cw) / 2.0, 1.0 - cw, (1.0 - cw) / 2.0) };
        let a0 = 1.0 + alpha;
        let (a1, a2) = (-2.0 * cw, 1.0 - alpha);
        Biquad { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }

    /// One sample.
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// OTTER's sound: the four band filters, three bands' dynamics, the settings applied.
pub struct Otter {
    applied: [i32; PARAMS],
    /// Per channel: the low band's low pass, the high band's high pass, then the three bands' dynamics.
    low: [Biquad; 2],
    high: [Biquad; 2],
    bands: [[Band; 3]; 2],
    depth: f32,
    low_cut: f32,
    high_cut: f32,
    up: f32,
    down: f32,
}

impl Otter {
    pub fn new() -> Otter {
        Otter::with_settings(&Settings::default(), 44100)
    }

    pub fn with_settings(s: &Settings, rate: u32) -> Otter {
        let mut o = Otter {
            applied: DEFAULTS,
            low: [Biquad::new(200.0, rate, false), Biquad::new(200.0, rate, false)],
            high: [Biquad::new(2000.0, rate, true), Biquad::new(2000.0, rate, true)],
            bands: [[Band::new(), Band::new(), Band::new()], [Band::new(), Band::new(), Band::new()]],
            depth: 1.0,
            low_cut: 200.0,
            high_cut: 2000.0,
            up: times(32, rate).0,
            down: times(32, rate).1,
        };
        o.derive(&s.v, rate);
        o
    }

    pub fn from_state(state: &[u8], rate: u32) -> Option<Otter> {
        Some(Otter::with_settings(&Settings::from_state(state)?, rate))
    }

    /// The stored values applied last.
    pub fn settings(&self) -> Settings {
        Settings { v: self.applied }
    }

    /// Applies the settings; the crossovers and the times move at once.
    pub fn set(&mut self, s: &Settings, rate: u32) {
        if s.v != self.applied {
            self.derive(&s.v, rate);
        }
    }

    /// Forgets what the followers hold, as at the start of a song.
    pub fn clear(&mut self) {
        self.bands = [[Band::new(), Band::new(), Band::new()], [Band::new(), Band::new(), Band::new()]];
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.low = [Biquad::new(self.low_cut, rate, false), Biquad::new(self.low_cut, rate, false)];
        self.high = [Biquad::new(self.high_cut, rate, true), Biquad::new(self.high_cut, rate, true)];
        (self.up, self.down) = times(self.applied[TIME], rate);
    }

    fn derive(&mut self, v: &[i32; PARAMS], rate: u32) {
        self.applied = *v;
        self.depth = depth(v[DEPTH]);
        self.low_cut = crossover(v[LOW_CUT], LOW_RANGE);
        self.high_cut = crossover(v[HIGH_CUT], HIGH_RANGE).max(self.low_cut * 1.1);
        self.low = [Biquad::new(self.low_cut, rate, false), Biquad::new(self.low_cut, rate, false)];
        self.high = [Biquad::new(self.high_cut, rate, true), Biquad::new(self.high_cut, rate, true)];
        (self.up, self.down) = times(v[TIME], rate);
    }

    /// One sample of one channel.
    #[inline]
    pub fn sample(&mut self, ch: usize, x: f32) -> f32 {
        let low = self.low[ch].tick(x);
        let high = self.high[ch].tick(x);
        // The middle is what neither crossover took: the three sum back to the input.
        let mid = x - low - high;
        let d = self.depth;
        let wet = self.bands[ch][0].gain(low, self.up, self.down) * low
            + self.bands[ch][1].gain(mid, self.up, self.down) * mid
            + self.bands[ch][2].gain(high, self.up, self.down) * high;
        (1.0 - d) * x + d * wet
    }

    /// Processes an interleaved stereo block in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        for (i, v) in buf.iter_mut().enumerate() {
            *v = self.sample(i & 1, *v);
        }
    }
}

impl Default for Otter {
    fn default() -> Self {
        Otter::new()
    }
}

impl Effect for Otter {
    fn process(&mut self, buf: &mut [f32]) {
        Otter::process(self, buf);
    }

    fn reset(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A steady interleaved sine block, the same on both channels.
    fn sine(freq: f32, rate: u32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .flat_map(|i| {
                let s = amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin();
                [s, s]
            })
            .collect()
    }

    #[test]
    fn states_round_trip() {
        let s = Settings { v: [6400, 64, 32, 16] };
        let b = s.to_state();
        assert_eq!(b.len(), STATE_LEN);
        assert_eq!(Settings::from_state(&b), Some(s));
    }

    #[test]
    fn crossovers_cover_their_ranges_in_order() {
        assert_eq!(crossover(0, LOW_RANGE), 30.0);
        let mid = crossover(64, LOW_RANGE);
        assert!((300.0..200.0).contains(&mid) || mid > 100.0, "a log step between 30 and 1000: {mid}");
        assert!(crossover(127, LOW_RANGE) > crossover(0, LOW_RANGE));
        assert!(crossover(127, HIGH_RANGE) > crossover(0, HIGH_RANGE));
    }

    #[test]
    fn zero_depth_leaves_the_input() {
        let mut o = Otter::with_settings(&Settings { v: [0, 96, 34, 32] }, 44100);
        let mut buf = sine(220.0, 44100, 441, 0.5);
        let before = buf.clone();
        o.process(&mut buf);
        for (a, b) in buf.iter().zip(before.iter()) {
            assert!((a - b).abs() < 1.0e-5, "dry at depth 0: {a} vs {b}");
        }
    }

    #[test]
    fn loud_parts_quiet_parts() {
        // A loud band is pulled toward the threshold (downward), a quiet one rises (upward).
        let mut loud = Otter::with_settings(&Settings { v: [12800, 96, 34, 32] }, 44100);
        let mut buf = sine(440.0, 44100, 8820, 0.6); // inside the high band
        loud.process(&mut buf);
        let peak = buf.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak < 0.6, "downward compression pulls it in: {peak}");
        assert!(peak > 0.005, "not to silence: {peak}");

        let mut quiet = Otter::with_settings(&Settings { v: [12800, 96, 34, 32] }, 44100);
        let mut buf = sine(440.0, 44100, 8820, 0.005);
        quiet.process(&mut buf);
        let qpeak = buf.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(qpeak > 0.005 * 1.1, "upward compression lifts quiet parts: {qpeak}");
    }

    #[test]
    fn bands_sum_back_with_the_filters() {
        // At full depth a mid-band sine comes back compressed but whole: the split reconstructs, the gain
        // computer folds it toward the threshold in dB.
        let mut o = Otter::with_settings(&Settings { v: [12800, 96, 34, 32] }, 44100);
        let mut buf = sine(220.0, 44100, 4410, 0.5);
        o.process(&mut buf);
        let peak = buf.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak > 0.01 && peak < 0.4, "the processed mix stays sane: {peak}");
    }

    #[test]
    fn reset_empties_the_followers() {
        let mut o = Otter::new();
        let mut buf = sine(220.0, 44100, 441, 0.5);
        o.process(&mut buf);
        o.clear();
        assert!(o.bands.iter().flatten().all(|b| b.env == 0.0));
    }
}
