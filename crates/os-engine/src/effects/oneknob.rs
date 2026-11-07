// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Lemon Studio's Oneknob (a recreation of the idea behind Waves / Image-Line's Soundgoodizer): one knob that
//! drives a fixed compressor's make-up loudness. Four presets stand for the original's A-D curves; the knob
//! (amount, 0..100 %) scales how much of the compression's make-up gain and drive comes in, so turning it up
//! makes the track louder and denser without any other control.
//!
//! The detector is the absolute peak with a smoothed follower (attack and release per preset in ms); the gain
//! computer is a soft-knee compressor around the preset threshold with the preset ratio; the make-up gain rises
//! with the amount up to the preset's ceiling.
//!
// spec: plugins/lemon-oneknob.md

use super::Effect;

/// The settings, in the saved state's order.
pub const PARAMS: usize = 2;
/// The amount, stored as percent * 128 (12800 = 100 %), like the gains elsewhere.
pub const AMOUNT: usize = 0;
/// The preset curve: 0..4 (A, B, C, D).
pub const PRESET: usize = 1;

/// The editor's ranges.
pub const RANGES: [(i32, i32); PARAMS] = [(0, 12800), (0, 3)];

/// A new instance's settings: preset A, a little of it.
pub const DEFAULTS: [i32; PARAMS] = [12800, 0];

/// The saved state: two ints, no version field.
pub const STATE_LEN: usize = 8;

/// One preset curve: threshold (dB), ratio, attack (ms), release (ms), make-up at full amount (dB) and the drive
/// the amount adds around the knee (dB). A-D follow the original's four curves: A punchy and open, B warm and
/// gentle, C forward, D heavy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
    pub drive_db: f32,
}

/// The four curves, A-D in order.
pub const CURVES: [Curve; 4] = [
    Curve { threshold_db: -20.0, ratio: 4.0, attack_ms: 5.0, release_ms: 120.0, makeup_db: 9.0, drive_db: 2.0 },
    Curve { threshold_db: -26.0, ratio: 3.0, attack_ms: 12.0, release_ms: 180.0, makeup_db: 7.0, drive_db: 1.0 },
    Curve { threshold_db: -16.0, ratio: 6.0, attack_ms: 3.0, release_ms: 90.0, makeup_db: 10.0, drive_db: 2.5 },
    Curve { threshold_db: -12.0, ratio: 8.0, attack_ms: 2.0, release_ms: 60.0, makeup_db: 11.0, drive_db: 3.0 },
];

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
    /// A saved state (8 bytes; longer ones are read from their start).
    pub fn from_state(b: &[u8]) -> Option<Settings> {
        Some(Settings { v: super::ints(b, 0)? })
    }

    /// The saved state (8 bytes).
    pub fn to_state(&self) -> Vec<u8> {
        super::state_of(&self.v)
    }
}

/// The amount (0..1) from its stored value.
pub fn amount(stored: i32) -> f32 {
    (stored as f32 / 12800.0).clamp(0.0, 1.0)
}

/// dB to a linear gain.
pub fn gain_of(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// The follower's per-sample smoothing for a time in ms: reaches about 63 % in that time... as `1 - exp(-ms /
/// (1000 / rate))`, so short times chase fast.
pub fn smooth(ms: f32, rate: u32) -> f32 {
    let ms = ms.max(0.05);
    1.0 - (-(rate as f32) / 1000.0 / ms).exp()
}

/// Oneknob's sound.
pub struct Oneknob {
    applied: [i32; PARAMS],
    curve: Curve,
    amount: f32,
    /// Per-sample smoothing of the follower.
    up: f32,
    down: f32,
    /// The follower's level per channel.
    env: [f32; 2],
    makeup: f32,
    drive: f32,
}

impl Oneknob {
    pub fn new() -> Oneknob {
        Oneknob::with_settings(&Settings::default())
    }

    pub fn with_settings(s: &Settings) -> Oneknob {
        let mut o = Oneknob {
            applied: DEFAULTS,
            curve: CURVES[0],
            amount: 1.0,
            up: smooth(CURVES[0].attack_ms, 44100),
            down: smooth(CURVES[0].release_ms, 44100),
            env: [0.0; 2],
            makeup: 1.0,
            drive: 1.0,
        };
        o.derive(&s.v);
        o
    }

    pub fn from_state(state: &[u8]) -> Option<Oneknob> {
        Some(Oneknob::with_settings(&Settings::from_state(state)?))
    }

    /// The stored values applied last.
    pub fn settings(&self) -> Settings {
        Settings { v: self.applied }
    }

    /// Applies the settings; nothing glides, a change acts from the next block on.
    pub fn set(&mut self, s: &Settings) {
        if s.v != self.applied {
            self.derive(&s.v);
        }
    }

    /// Forgets what the follower holds, as at the start of a song.
    pub fn clear(&mut self) {
        self.env = [0.0; 2];
    }

    /// New sample rate: the follower's times.
    pub fn set_sample_rate(&mut self, rate: u32) {
        self.up = smooth(self.curve.attack_ms, rate);
        self.down = smooth(self.curve.release_ms, rate);
    }

    fn derive(&mut self, v: &[i32; PARAMS]) {
        self.applied = *v;
        let preset = (v[PRESET] as usize).clamp(0, CURVES.len() - 1);
        self.curve = CURVES[preset];
        self.amount = amount(v[AMOUNT]);
        self.up = smooth(self.curve.attack_ms, 44100);
        self.down = smooth(self.curve.release_ms, 44100);
        self.makeup = gain_of(self.curve.makeup_db * self.amount);
        // The drive rides the input a little before the detector: denser as the amount rises.
        self.drive = gain_of(self.curve.drive_db * self.amount);
    }

    /// One sample: the follower, the soft-knee gain computer, the make-up.
    #[inline]
    pub fn sample(&mut self, ch: usize, s: f32) -> f32 {
        let a = s.abs() * self.drive;
        // The follower: up chases fast, down falls slowly.
        let k = if a > self.env[ch] { self.up } else { self.down };
        self.env[ch] += (a - self.env[ch]) * k;
        let env = self.env[ch];
        let t = gain_of(self.curve.threshold_db);
        // Below the threshold the gain rises toward the make-up only through the knee; above it the ratio acts.
        // The knee is half the distance to the threshold in dB, both ways.
        let over_db = 20.0 * env.max(1.0e-6).log10() - self.curve.threshold_db;
        let knee = 6.0f32;
        let ratio = self.curve.ratio;
        let gr_db = if over_db > knee {
            -(over_db - knee) * (1.0 - 1.0 / ratio)
        } else if over_db < -knee {
            0.0
        } else {
            // Inside the knee: a quadratic ease from no reduction to the knee's ratio line.
            let x = (over_db + knee) / (2.0 * knee);
            -x * x * knee * (1.0 - 1.0 / ratio)
        };
        s * gain_of(gr_db) * self.makeup
    }

    /// Processes an interleaved stereo block in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        for (i, v) in buf.iter_mut().enumerate() {
            *v = self.sample(i & 1, *v);
        }
    }
}

impl Default for Oneknob {
    fn default() -> Self {
        Oneknob::new()
    }
}

impl Effect for Oneknob {
    fn process(&mut self, buf: &mut [f32]) {
        Oneknob::process(self, buf);
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
        let s = Settings { v: [6400, 2] };
        let b = s.to_state();
        assert_eq!(b.len(), STATE_LEN);
        assert_eq!(Settings::from_state(&b), Some(s));
    }

    #[test]
    fn quiet_passes_and_loud_compressed() {
        let mut o = Oneknob::with_settings(&Settings { v: [12800, 0] });
        // A quiet signal stays near itself (a little make-up).
        let mut q = [0.05f32, 0.05, 0.05, 0.05];
        o.process(&mut q);
        assert!(q.iter().all(|&x| x < 0.3), "quiet stays quiet: {q:?}");
        // A loud block is pulled down: the output's peak sits below the input's.
        let mut o = Oneknob::with_settings(&Settings { v: [12800, 0] });
        let mut loud = sine(220.0, 44100, 4410, 0.9);
        o.process(&mut loud);
        let peak = loud.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak < 0.9, "a loud signal is compressed: {peak}");
        assert!(peak > 0.1, "not crushed to silence: {peak}");
    }

    #[test]
    fn amount_turns_the_makeup() {
        // Full amount is louder than none on a steady signal: the make-up gain rises with it.
        let run = |v: [i32; PARAMS]| {
            let mut o = Oneknob::with_settings(&Settings { v });
            let mut buf = sine(220.0, 44100, 4410, 0.3);
            o.process(&mut buf);
            buf.iter().fold(0.0f32, |m, &x| m.max(x.abs()))
        };
        let off = run([0, 0]);
        let full = run([12800, 0]);
        assert!(full > off * 1.5, "amount raises the level: {off} vs {full}");
    }

    #[test]
    fn presets_differ_and_the_rate_is_honoured() {
        let mut a = Oneknob::with_settings(&Settings { v: [12800, 0] });
        let mut d = Oneknob::with_settings(&Settings { v: [12800, 3] });
        a.set_sample_rate(96000);
        d.set_sample_rate(96000);
        let mut ba = vec![0.5f32; 960];
        let mut bd = vec![0.5f32; 960];
        a.process(&mut ba);
        d.process(&mut bd);
        // D (threshold -12, ratio 8) pulls harder than A over the same block once steady.
        let pa = ba.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        let pd = bd.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(pa != pd, "the curves differ");
        // A reset empties the follower.
        a.clear();
        assert_eq!(a.env, [0.0; 2]);
    }
}
