// Copyright (C) 2026 Artig3nce, apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The mastering chain: five-band EQ, three-band compressor, stereo width, saturation and a look-ahead maximizer, in
//! that order. It knows nothing of the plugin: `Settings` in, audio through, so `auto` can run it over a captured song.

use std::f64::consts::PI;

/// The EQ's bands, low to high: a low shelf, three bells and a high shelf.
pub const EQ_HZ: [f64; 5] = [80.0, 300.0, 1500.0, 5000.0, 12000.0];
const EQ_Q: [f64; 5] = [0.7, 1.0, 1.0, 1.0, 0.7];
/// Where the compressor's three bands meet.
const CROSSOVER_HZ: [f64; 2] = [200.0, 3000.0];
/// Each compressor band's attack and release, ms.
const COMP_TIMES: [[f64; 2]; 3] = [[30.0, 200.0], [15.0, 150.0], [5.0, 100.0]];
/// Below this the side signal is left out with Mono bass on.
const MONO_HZ: f64 = 120.0;
/// The maximizer's look-ahead and release, ms.
const LOOK_MS: f64 = 5.0;
const RELEASE_MS: f64 = 100.0;
/// A compressor threshold at or above this is off.
pub const COMP_OFF_DB: f32 = -0.05;

/// Everything the chain's sound depends on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub eq_db: [f32; 5],
    /// Each compressor band's threshold, dB (0: off).
    pub comp_db: [f32; 3],
    pub ratio: f32,
    /// The side signal's level: 1 leaves the stereo image as it is.
    pub width: f32,
    pub mono_bass: bool,
    /// 0..1.
    pub saturation: f32,
    /// Into the maximizer, dB.
    pub gain_db: f32,
    pub ceiling_db: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { eq_db: [0.0; 5], comp_db: [0.0; 3], ratio: 2.0, width: 1.0, mono_bass: false, saturation: 0.0, gain_db: 0.0, ceiling_db: 0.0 }
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn gain_to_db(g: f32) -> f32 {
    20.0 * g.max(1e-9).log10()
}

/// One second-order filter's coefficients (a0 = 1).
#[derive(Clone, Copy, Debug)]
pub struct Coef {
    b: [f64; 3],
    a: [f64; 2],
}

impl Coef {
    const THROUGH: Coef = Coef { b: [1.0, 0.0, 0.0], a: [0.0, 0.0] };

    fn norm(b: [f64; 3], a: [f64; 3]) -> Coef {
        Coef { b: [b[0] / a[0], b[1] / a[0], b[2] / a[0]], a: [a[1] / a[0], a[2] / a[0]] }
    }

    fn parts(hz: f64, q: f64, rate: f64) -> (f64, f64) {
        let w = 2.0 * PI * (hz / rate).min(0.49);
        (w.cos(), w.sin() / (2.0 * q))
    }

    pub fn lowpass(hz: f64, q: f64, rate: f64) -> Coef {
        let (c, al) = Self::parts(hz, q, rate);
        Self::norm([(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al])
    }

    pub fn highpass(hz: f64, q: f64, rate: f64) -> Coef {
        let (c, al) = Self::parts(hz, q, rate);
        Self::norm([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al])
    }

    pub fn bandpass(hz: f64, q: f64, rate: f64) -> Coef {
        let (c, al) = Self::parts(hz, q, rate);
        Self::norm([al, 0.0, -al], [1.0 + al, -2.0 * c, 1.0 - al])
    }

    pub fn bell(hz: f64, q: f64, db: f64, rate: f64) -> Coef {
        let (c, al) = Self::parts(hz, q, rate);
        let a = 10f64.powf(db / 40.0);
        Self::norm([1.0 + al * a, -2.0 * c, 1.0 - al * a], [1.0 + al / a, -2.0 * c, 1.0 - al / a])
    }

    pub fn low_shelf(hz: f64, q: f64, db: f64, rate: f64) -> Coef {
        let (c, al) = Self::parts(hz, q, rate);
        let a = 10f64.powf(db / 40.0);
        let s = 2.0 * a.sqrt() * al;
        Self::norm(
            [a * ((a + 1.0) - (a - 1.0) * c + s), 2.0 * a * ((a - 1.0) - (a + 1.0) * c), a * ((a + 1.0) - (a - 1.0) * c - s)],
            [(a + 1.0) + (a - 1.0) * c + s, -2.0 * ((a - 1.0) + (a + 1.0) * c), (a + 1.0) + (a - 1.0) * c - s],
        )
    }

    pub fn high_shelf(hz: f64, q: f64, db: f64, rate: f64) -> Coef {
        let (c, al) = Self::parts(hz, q, rate);
        let a = 10f64.powf(db / 40.0);
        let s = 2.0 * a.sqrt() * al;
        Self::norm(
            [a * ((a + 1.0) + (a - 1.0) * c + s), -2.0 * a * ((a - 1.0) + (a + 1.0) * c), a * ((a + 1.0) + (a - 1.0) * c - s)],
            [(a + 1.0) - (a - 1.0) * c + s, 2.0 * ((a - 1.0) - (a + 1.0) * c), (a + 1.0) - (a - 1.0) * c - s],
        )
    }

    /// The two stages of ITU-R BS.1770's K weighting at `rate`.
    pub fn k_weighting(rate: f64) -> [Coef; 2] {
        // The standard gives them at 48 kHz: these are its analogue prototypes, so any rate gets the same curve.
        let (db, f0, q) = (3.999_843_853_973_347, 1_681.974_450_955_533, 0.707_175_236_955_419_6);
        let k = (PI * f0 / rate).tan();
        let (vh, vb) = (10f64.powf(db / 20.0), 10f64.powf(db / 20.0).powf(0.499_666_774_154_541_6));
        let a0 = 1.0 + k / q + k * k;
        let shelf = Coef {
            b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        };
        let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
        let k = (PI * f0 / rate).tan();
        let a0 = 1.0 + k / q + k * k;
        let high = Coef { b: [1.0, -2.0, 1.0], a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0] };
        [shelf, high]
    }
}

/// One channel's filter memory.
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    z: [f64; 2],
}

impl Biquad {
    #[inline]
    pub fn run(&mut self, c: &Coef, x: f64) -> f64 {
        let y = c.b[0] * x + self.z[0];
        self.z[0] = c.b[1] * x - c.a[0] * y + self.z[1];
        self.z[1] = c.b[2] * x - c.a[1] * y;
        y
    }
}

/// A fourth-order Linkwitz-Riley split into two bands that add back to an all-pass.
#[derive(Clone, Copy, Debug)]
struct Split {
    low: Coef,
    high: Coef,
}

impl Split {
    fn new(hz: f64, rate: f64) -> Split {
        Split { low: Coef::lowpass(hz, std::f64::consts::FRAC_1_SQRT_2, rate), high: Coef::highpass(hz, std::f64::consts::FRAC_1_SQRT_2, rate) }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct SplitState {
    low: [Biquad; 2],
    high: [Biquad; 2],
}

impl SplitState {
    #[inline]
    fn run(&mut self, s: &Split, x: f64) -> (f64, f64) {
        let l = self.low[0].run(&s.low, x);
        let h = self.high[0].run(&s.high, x);
        (self.low[1].run(&s.low, l), self.high[1].run(&s.high, h))
    }
}

/// The four-times oversampled peak of a signal (ITU-R BS.1770's true peak, by a windowed-sinc interpolator).
#[derive(Clone, Debug)]
pub struct TruePeak {
    hist: [f32; TruePeak::TAPS],
    at: usize,
}

impl TruePeak {
    const TAPS: usize = 12;
    /// Samples between a sample going in and its peak being reported.
    pub const DELAY: usize = Self::TAPS / 2;

    pub fn new() -> TruePeak {
        TruePeak { hist: [0.0; Self::TAPS], at: 0 }
    }

    fn table() -> &'static [[f32; TruePeak::TAPS]; 4] {
        static T: std::sync::OnceLock<[[f32; TruePeak::TAPS]; 4]> = std::sync::OnceLock::new();
        T.get_or_init(|| {
            let mut t = [[0f32; Self::TAPS]; 4];
            for (p, row) in t.iter_mut().enumerate() {
                for (i, v) in row.iter_mut().enumerate() {
                    // Time of this tap from the point being interpolated.
                    let x = i as f64 - (Self::TAPS as f64 / 2.0 - 1.0) - p as f64 / 4.0;
                    let sinc = if x.abs() < 1e-9 { 1.0 } else { (PI * x).sin() / (PI * x) };
                    let w = (PI * x / (Self::TAPS as f64 / 2.0 + 1.0)).cos().max(0.0).powi(2);
                    *v = (sinc * w) as f32;
                }
            }
            t
        })
    }

    /// Takes a sample; the largest level the signal reaches around it.
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        self.hist[self.at] = x;
        self.at = (self.at + 1) % Self::TAPS;
        let mut peak = 0f32;
        for row in Self::table() {
            let mut sum = 0f32;
            for (i, c) in row.iter().enumerate() {
                sum += c * self.hist[(self.at + i) % Self::TAPS];
            }
            peak = peak.max(sum.abs());
        }
        peak
    }
}

impl Default for TruePeak {
    fn default() -> Self {
        Self::new()
    }
}

/// The largest gain that keeps a level at the ceiling, smoothed so it reaches each peak in time: a sliding minimum over
/// the look-ahead, a release, then a moving average over the same look-ahead. The average of values that are all at or
/// under what a peak needs is under it too, so the audio delayed by the look-ahead cannot pass the ceiling.
#[derive(Clone, Debug)]
struct Maximizer {
    look: usize,
    release: f32,
    /// The needed gains of the look-ahead (a ring), and the current release's level.
    need: Vec<f32>,
    at: usize,
    level: f32,
    /// The moving average's ring and sum.
    avg: Vec<f32>,
    avg_at: usize,
    sum: f64,
    tp: [TruePeak; 2],
    /// The delay line of both channels.
    delay: Vec<[f32; 2]>,
    delay_at: usize,
}

impl Maximizer {
    fn new(rate: f64) -> Maximizer {
        let look = ((LOOK_MS / 1000.0 * rate) as usize).max(2);
        let length = look + TruePeak::DELAY;
        Maximizer {
            look,
            release: (1.0 - (-1.0 / (RELEASE_MS / 1000.0 * rate)).exp()) as f32,
            need: vec![1.0; look],
            at: 0,
            level: 1.0,
            avg: vec![1.0; look],
            avg_at: 0,
            sum: look as f64,
            tp: [TruePeak::new(), TruePeak::new()],
            delay: vec![[0.0; 2]; length],
            delay_at: 0,
        }
    }

    fn latency(&self) -> usize {
        self.delay.len() - 1
    }

    /// One stereo frame at `ceiling` (linear): the frame `latency` samples ago, limited, and the gain used.
    #[inline]
    fn run(&mut self, l: f32, r: f32, ceiling: f32) -> (f32, f32, f32) {
        let peak = self.tp[0].run(l).max(self.tp[1].run(r));
        let need = if peak > ceiling { ceiling / peak } else { 1.0 };
        self.need[self.at] = need;
        self.at = (self.at + 1) % self.look;
        let least = self.need.iter().copied().fold(1f32, f32::min);
        self.level = if least < self.level { least } else { self.level + (least - self.level) * self.release };
        self.sum += f64::from(self.level) - f64::from(self.avg[self.avg_at]);
        self.avg[self.avg_at] = self.level;
        self.avg_at = (self.avg_at + 1) % self.look;
        let gain = (self.sum / self.look as f64).min(1.0) as f32;
        self.delay[self.delay_at] = [l, r];
        self.delay_at = (self.delay_at + 1) % self.delay.len();
        let [dl, dr] = self.delay[self.delay_at];
        (dl * gain, dr * gain, gain)
    }
}

/// One compressor band's level detector and gain (stereo linked).
#[derive(Clone, Copy, Debug)]
struct Band {
    attack: f32,
    release: f32,
    /// Gain reduction now, dB (0 or more).
    cut: f32,
}

impl Band {
    fn new(times: [f64; 2], rate: f64) -> Band {
        let k = |ms: f64| (-1.0 / (ms / 1000.0 * rate)).exp() as f32;
        Band { attack: k(times[0]), release: k(times[1]), cut: 0.0 }
    }

    #[inline]
    fn gain(&mut self, level: f32, threshold: f32, ratio: f32) -> f32 {
        if threshold >= COMP_OFF_DB {
            self.cut = 0.0;
            return 1.0;
        }
        let over = gain_to_db(level) - threshold;
        let want = if over > 0.0 { over * (1.0 - 1.0 / ratio.max(1.0)) } else { 0.0 };
        let k = if want > self.cut { self.attack } else { self.release };
        self.cut = want + (self.cut - want) * k;
        db_to_gain(-self.cut)
    }
}

/// The whole chain for one instance.
#[derive(Clone, Debug)]
pub struct Chain {
    rate: f64,
    settings: Settings,
    eq: [Coef; 5],
    eq_state: [[Biquad; 5]; 2],
    splits: [Split; 2],
    split_state: [[SplitState; 3]; 2],
    bands: [Band; 3],
    side_high: Coef,
    side_state: [Biquad; 2],
    maximizer: Maximizer,
    gain: f32,
    gain_step: f32,
    /// Smallest maximizer gain since last read.
    reduction: f32,
}

impl Chain {
    pub fn new(rate: f64) -> Chain {
        let rate = rate.max(1000.0);
        let mut c = Chain {
            rate,
            settings: Settings::default(),
            eq: [Coef::THROUGH; 5],
            eq_state: Default::default(),
            splits: [Split::new(CROSSOVER_HZ[0], rate), Split::new(CROSSOVER_HZ[1], rate)],
            split_state: Default::default(),
            bands: COMP_TIMES.map(|t| Band::new(t, rate)),
            side_high: Coef::highpass(MONO_HZ, std::f64::consts::FRAC_1_SQRT_2, rate),
            side_state: Default::default(),
            maximizer: Maximizer::new(rate),
            gain: 1.0,
            gain_step: (1.0 - (-1.0 / (0.02 * rate)).exp()) as f32,
            reduction: 1.0,
        };
        c.set(Settings::default());
        c.gain = db_to_gain(c.settings.gain_db);
        c
    }

    /// Samples the output lags the input by.
    pub fn latency(&self) -> usize {
        self.maximizer.latency()
    }

    /// New settings: applied from the next sample on, the filters at once and the gain gliding.
    pub fn set(&mut self, s: Settings) {
        if s.eq_db != self.settings.eq_db || self.eq.iter().all(|c| c.b == Coef::THROUGH.b) {
            for (i, c) in self.eq.iter_mut().enumerate() {
                let db = f64::from(s.eq_db[i]);
                *c = if db == 0.0 {
                    Coef::THROUGH
                } else if i == 0 {
                    Coef::low_shelf(EQ_HZ[i], EQ_Q[i], db, self.rate)
                } else if i == 4 {
                    Coef::high_shelf(EQ_HZ[i], EQ_Q[i], db, self.rate)
                } else {
                    Coef::bell(EQ_HZ[i], EQ_Q[i], db, self.rate)
                };
            }
        }
        self.settings = s;
    }

    /// Settings with the gain already there, for a run from the start of a capture.
    pub fn with(rate: f64, s: Settings) -> Chain {
        let mut c = Chain::new(rate);
        c.set(s);
        c.gain = db_to_gain(s.gain_db);
        c
    }

    /// The smallest gain the maximizer used since the last call, dB (0 or less).
    pub fn take_reduction(&mut self) -> f32 {
        let g = self.reduction;
        self.reduction = 1.0;
        gain_to_db(g)
    }

    /// Forgets the audio: filters, detectors and the look-ahead.
    pub fn reset(&mut self) {
        let s = self.settings;
        *self = Chain::with(self.rate, s);
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let s = self.settings;
        let comp_on = s.comp_db.iter().any(|&t| t < COMP_OFF_DB);
        let ceiling = db_to_gain(s.ceiling_db);
        let target = db_to_gain(s.gain_db);
        let width_on = s.width != 1.0 || s.mono_bass;
        let drive = 1.0 + 4.0 * s.saturation;
        let makeup = 1.0 / drive.tanh();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (mut a, mut b) = (f64::from(*l), f64::from(*r));
            for i in 0..5 {
                a = self.eq_state[0][i].run(&self.eq[i], a);
                b = self.eq_state[1][i].run(&self.eq[i], b);
            }
            if comp_on {
                (a, b) = self.compress(a, b, &s);
            }
            if width_on {
                let (m, side) = ((a + b) * 0.5, (a - b) * 0.5);
                // A steep high-pass: subtracting a low-pass from the side would leave most of the bass in.
                let side = if s.mono_bass {
                    let once = self.side_state[0].run(&self.side_high, side);
                    self.side_state[1].run(&self.side_high, once)
                } else {
                    side
                } * f64::from(s.width);
                (a, b) = (m + side, m - side);
            }
            let (mut a, mut b) = (a as f32, b as f32);
            if s.saturation > 0.0 {
                let sat = |x: f32| (x * drive).tanh() * makeup;
                a = a + (sat(a) - a) * s.saturation;
                b = b + (sat(b) - b) * s.saturation;
            }
            self.gain += (target - self.gain) * self.gain_step;
            let (x, y, g) = self.maximizer.run(a * self.gain, b * self.gain, ceiling);
            self.reduction = self.reduction.min(g);
            *l = x;
            *r = y;
        }
    }

    #[inline]
    fn compress(&mut self, a: f64, b: f64, s: &Settings) -> (f64, f64) {
        let mut bands = [[0f64; 3]; 2];
        for (ch, x) in [a, b].into_iter().enumerate() {
            let st = &mut self.split_state[ch];
            let (low, rest) = st[0].run(&self.splits[0], x);
            let (mid, high) = st[1].run(&self.splits[1], rest);
            // The low band through the upper crossover's all-pass, so the three add back flat.
            let (l, h) = st[2].run(&self.splits[1], low);
            bands[ch] = [l + h, mid, high];
        }
        let (mut x, mut y) = (0f64, 0f64);
        for i in 0..3 {
            let level = bands[0][i].abs().max(bands[1][i].abs()) as f32;
            let g = f64::from(self.bands[i].gain(level, s.comp_db[i], s.ratio));
            x += bands[0][i] * g;
            y += bands[1][i] * g;
        }
        (x, y)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn sine(hz: f64, amp: f32, rate: f64, n: usize) -> Vec<f32> {
        (0..n).map(|i| amp * (2.0 * PI * hz * i as f64 / rate).sin() as f32).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// How a sine at `hz` comes out, dB.
    fn response(s: Settings, hz: f64) -> f32 {
        let rate = 48000.0;
        let n = 48000;
        let (mut l, mut r) = (sine(hz, 0.05, rate, n), sine(hz, 0.05, rate, n));
        Chain::with(rate, s).process(&mut l, &mut r);
        gain_to_db(rms(&l[n / 2..]) / rms(&sine(hz, 0.05, rate, n)[n / 2..]))
    }

    #[test]
    fn settings_at_rest_change_nothing() {
        for hz in [40.0, 300.0, 1000.0, 8000.0] {
            assert!(response(Settings::default(), hz).abs() < 0.05, "{hz}");
        }
    }

    #[test]
    fn each_eq_band_moves_its_own_frequencies() {
        for (band, (hz, far)) in [(0, (40.0, 5000.0)), (1, (300.0, 5000.0)), (2, (1500.0, 100.0)), (3, (5000.0, 100.0)), (4, (16000.0, 300.0))] {
            let mut s = Settings::default();
            s.eq_db[band] = 6.0;
            assert!(response(s, hz) > 4.0, "band {band} at {hz}: {}", response(s, hz));
            assert!(response(s, far).abs() < 1.0, "band {band} at {far}: {}", response(s, far));
        }
    }

    /// With the compressor's bands split but nothing over threshold, the sum stays flat.
    #[test]
    fn the_crossover_adds_back_flat() {
        let s = Settings { comp_db: [-3.0; 3], ratio: 2.0, ..Settings::default() };
        for hz in [50.0, 200.0, 800.0, 3000.0, 10000.0] {
            assert!(response(s, hz).abs() < 0.1, "{hz}: {}", response(s, hz));
        }
    }

    #[test]
    fn the_compressor_cuts_only_what_passes_its_threshold() {
        let rate = 48000.0;
        let s = Settings { comp_db: [-20.0, 0.0, 0.0], ratio: 4.0, gain_db: 0.0, ceiling_db: 0.0, ..Settings::default() };
        let (mut l, mut r) = (sine(80.0, 0.5, rate, 96000), sine(80.0, 0.5, rate, 96000));
        Chain::with(rate, s).process(&mut l, &mut r);
        assert!(gain_to_db(rms(&l[48000..]) / rms(&sine(80.0, 0.5, rate, 96000))) < -4.0, "the low band is compressed");
        let (mut l, mut r) = (sine(8000.0, 0.5, rate, 96000), sine(8000.0, 0.5, rate, 96000));
        Chain::with(rate, s).process(&mut l, &mut r);
        assert!(gain_to_db(rms(&l[48000..]) / rms(&sine(8000.0, 0.5, rate, 96000))).abs() < 0.2, "the highs aren't");
    }

    /// Whatever goes in, nothing comes out over the ceiling, between the samples either.
    #[test]
    fn the_maximizer_holds_the_ceiling() {
        let rate = 44100.0;
        let mut seed = 12345u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        };
        let mut l: Vec<f32> = (0..88200).map(|i| noise() * if i % 8000 < 200 { 1.0 } else { 0.2 }).collect();
        for (i, v) in sine(11025.0, 0.9, rate, 88200).into_iter().enumerate() {
            l[i] = (l[i] + v).clamp(-1.0, 1.0);
        }
        let mut r = l.clone();
        let s = Settings { gain_db: 18.0, ceiling_db: -1.0, ..Settings::default() };
        Chain::with(rate, s).process(&mut l, &mut r);
        let mut tp = TruePeak::new();
        let peak = l.iter().map(|&v| tp.run(v)).fold(0f32, f32::max);
        assert!(gain_to_db(peak) <= -0.9, "true peak {}", gain_to_db(peak));
        assert!(gain_to_db(peak) > -2.0, "it reaches the ceiling: {}", gain_to_db(peak));
    }

    #[test]
    fn the_output_is_delayed_by_the_reported_latency() {
        let mut c = Chain::new(48000.0);
        let (mut l, mut r) = (vec![0f32; 2000], vec![0f32; 2000]);
        l[100] = 0.5;
        r[100] = 0.5;
        c.process(&mut l, &mut r);
        let at = l.iter().position(|v| v.abs() > 0.1).unwrap();
        assert_eq!(at, 100 + c.latency());
    }

    #[test]
    fn width_and_mono_bass_shape_the_side_only() {
        let rate = 48000.0;
        let run = |s: Settings, hz: f64| {
            let (mut l, mut r) = (sine(hz, 0.2, rate, 48000), sine(hz, 0.2, rate, 48000));
            r.iter_mut().for_each(|v| *v = -*v);
            Chain::with(rate, s).process(&mut l, &mut r);
            let side: Vec<f32> = l.iter().zip(&r).map(|(a, b)| (a - b) * 0.5).collect();
            let mid: Vec<f32> = l.iter().zip(&r).map(|(a, b)| (a + b) * 0.5).collect();
            (rms(&side[24000..]), rms(&mid[24000..]))
        };
        let plain = run(Settings::default(), 2000.0).0;
        let (wide, mid) = run(Settings { width: 1.5, ..Settings::default() }, 2000.0);
        assert!((wide / plain - 1.5).abs() < 0.03 && mid < 0.001, "{wide} {plain} {mid}");
        let (narrow, _) = run(Settings { width: 0.0, ..Settings::default() }, 2000.0);
        assert!(narrow < plain * 0.05, "width 0 is close to mono above the bass: {narrow}");
        let (bass, _) = run(Settings { mono_bass: true, ..Settings::default() }, 40.0);
        assert!(bass < plain * 0.2, "the bass goes mono: {bass}");
        let (kept, _) = run(Settings { mono_bass: true, ..Settings::default() }, 2000.0);
        assert!((kept / plain - 1.0).abs() < 0.05, "and the rest stays: {kept}");
    }

    #[test]
    fn saturation_adds_harmonics() {
        let rate = 48000.0;
        let thd = |sat: f32| {
            let (mut l, mut r) = (sine(1000.0, 0.5, rate, 48000), sine(1000.0, 0.5, rate, 48000));
            Chain::with(rate, Settings { saturation: sat, ceiling_db: 0.0, ..Settings::default() }).process(&mut l, &mut r);
            // The third harmonic's strength, by correlation.
            let (mut c, mut s) = (0f64, 0f64);
            for (i, v) in l[24000..].iter().enumerate() {
                let w = 2.0 * PI * 3000.0 * i as f64 / rate;
                c += f64::from(*v) * w.cos();
                s += f64::from(*v) * w.sin();
            }
            (c * c + s * s).sqrt() / 24000.0
        };
        assert!(thd(0.0) < 1e-4, "{}", thd(0.0));
        assert!(thd(0.5) > 0.01, "{}", thd(0.5));
    }

    #[test]
    fn nothing_goes_wrong_at_other_rates_or_after_a_reset() {
        for rate in [22050.0, 44100.0, 96000.0, 192000.0] {
            let mut s = Settings { eq_db: [3.0, -3.0, 2.0, -2.0, 4.0], comp_db: [-20.0; 3], saturation: 0.3, width: 1.2, mono_bass: true, gain_db: 6.0, ..Settings::default() };
            s.ratio = 3.0;
            let mut c = Chain::with(rate, s);
            let (mut l, mut r) = (sine(1000.0, 0.8, rate, 4096), sine(50.0, 0.8, rate, 4096));
            c.process(&mut l, &mut r);
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() <= 1.0), "{rate}");
            c.reset();
            assert_eq!(c.settings, s);
        }
    }
}
