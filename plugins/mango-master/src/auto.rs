// Copyright (C) 2026 Artig3nce, apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Measuring and Auto Master. `Loudness` follows ITU-R BS.1770 (K weighting, 400 ms blocks, the two gates). `solve` takes
//! a captured stretch of a song, measures its tone, dynamics and width, picks settings for a target, then runs the real
//! chain over the capture until the output sits at the target's loudness. All rules, no model.

use crate::dsp::{db_to_gain, gain_to_db, Biquad, Chain, Coef, Settings, TruePeak, EQ_HZ};

/// What a song is mastered for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub name: &'static str,
    /// Integrated loudness, LUFS.
    pub lufs: f32,
    pub ceiling_db: f32,
    /// How much low end and top it wants against the plain balance, dB.
    pub low_db: f32,
    pub high_db: f32,
    /// Peak over loudness it should keep, dB: a song with more gets compressed.
    pub crest_db: f32,
    pub saturation: f32,
}

pub const TARGETS: [Target; 4] = [
    Target { name: "Streaming", lufs: -14.0, ceiling_db: -1.0, low_db: 0.0, high_db: 0.0, crest_db: 14.0, saturation: 0.0 },
    Target { name: "Balanced", lufs: -11.0, ceiling_db: -1.0, low_db: 0.5, high_db: 0.5, crest_db: 12.0, saturation: 0.05 },
    Target { name: "Loud", lufs: -8.0, ceiling_db: -0.5, low_db: 0.5, high_db: 1.0, crest_db: 9.0, saturation: 0.12 },
    Target { name: "Club", lufs: -9.0, ceiling_db: -0.5, low_db: 2.0, high_db: 0.5, crest_db: 10.0, saturation: 0.1 },
];

/// Below this nothing counts as loudness (BS.1770's absolute gate).
const SILENCE_LUFS: f64 = -70.0;
/// The quietest a meter shows.
pub const FLOOR_LUFS: f32 = -70.0;
const BLOCK_S: f64 = 0.1;

fn lufs_of(power: f64) -> f64 {
    if power > 0.0 { -0.691 + 10.0 * power.log10() } else { -200.0 }
}

/// K-weighted loudness of a stereo signal.
#[derive(Clone, Debug)]
pub struct Loudness {
    coef: [Coef; 2],
    state: [[Biquad; 2]; 2],
    block_len: usize,
    count: usize,
    sum: f64,
    /// The last four 100 ms blocks' power.
    recent: [f64; 4],
    /// Blocks seen, up to the four a window needs: a window only counts once it's full.
    filled: usize,
    /// Each 400 ms window's power, as long as `keep` allows.
    windows: Vec<f64>,
    keep: usize,
    seen: usize,
    last: Option<f32>,
}

impl Loudness {
    /// `keep_s`: how long a stretch `integrated` remembers (0 for a meter that only shows the moment).
    pub fn new(rate: f64, keep_s: f64) -> Loudness {
        let keep = (keep_s / BLOCK_S) as usize;
        Loudness {
            coef: Coef::k_weighting(rate),
            state: Default::default(),
            block_len: ((BLOCK_S * rate) as usize).max(1),
            count: 0,
            sum: 0.0,
            recent: [0.0; 4],
            filled: 0,
            windows: Vec::with_capacity(keep),
            keep,
            seen: 0,
            last: None,
        }
    }

    #[inline]
    pub fn push(&mut self, l: f32, r: f32) {
        let mut power = 0f64;
        for (ch, x) in [l, r].into_iter().enumerate() {
            let [a, b] = &mut self.state[ch];
            let y = b.run(&self.coef[1], a.run(&self.coef[0], f64::from(x)));
            power += y * y;
        }
        self.sum += power;
        self.count += 1;
        if self.count == self.block_len {
            self.recent.rotate_left(1);
            self.recent[3] = self.sum / self.block_len as f64;
            self.sum = 0.0;
            self.count = 0;
            self.filled = (self.filled + 1).min(4);
            if self.filled == 4 && self.windows.len() < self.keep {
                self.windows.push(self.recent.iter().sum::<f64>() / 4.0);
            }
        }
    }

    /// The last 400 ms, LUFS.
    pub fn momentary(&self) -> f32 {
        (lufs_of(self.recent.iter().sum::<f64>() / 4.0) as f32).max(FLOOR_LUFS)
    }

    /// `integrated` for the audio thread: the windows are looked at at most once per block, and only when there's a new
    /// one, the answer in between being the last.
    pub fn integrated_quick(&mut self) -> Option<f32> {
        if self.windows.len() != self.seen {
            self.seen = self.windows.len();
            // Powers, not logarithms: this runs on the audio thread.
            let floor = 10f64.powf((SILENCE_LUFS + 0.691) / 10.0);
            let (mut n, mut sum) = (0usize, 0f64);
            for &p in self.windows.iter().filter(|&&p| p > floor) {
                n += 1;
                sum += p;
            }
            self.last = (n > 0).then(|| {
                let gate = (sum / n as f64) * 0.1;
                let (mut kept, mut total) = (0usize, 0f64);
                for &p in self.windows.iter().filter(|&&p| p > floor && p > gate) {
                    kept += 1;
                    total += p;
                }
                lufs_of(total / kept.max(1) as f64) as f32
            });
        }
        self.last
    }

    /// All of it, with BS.1770's gates; None for nothing above silence.
    pub fn integrated(&self) -> Option<f32> {
        let loud: Vec<f64> = self.windows.iter().copied().filter(|&p| lufs_of(p) > SILENCE_LUFS).collect();
        if loud.is_empty() {
            return None;
        }
        let gate = lufs_of(loud.iter().sum::<f64>() / loud.len() as f64) - 10.0;
        let kept: Vec<f64> = loud.into_iter().filter(|&p| lufs_of(p) > gate).collect();
        Some(lufs_of(kept.iter().sum::<f64>() / kept.len().max(1) as f64) as f32)
    }
}

/// What the measuring of a captured stretch found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Report {
    pub lufs: f32,
    pub true_peak_db: f32,
    /// Each EQ band's level, dB.
    pub bands_db: [f32; 5],
    /// The side signal against the mid, dB.
    pub side_db: f32,
}

/// Measures a capture; None when it holds nothing but silence.
pub fn measure(left: &[f32], right: &[f32], rate: f64) -> Option<Report> {
    let mut meter = Loudness::new(rate, left.len() as f64 / rate + 1.0);
    let mut tp = [TruePeak::new(), TruePeak::new()];
    let coef: [Coef; 5] = std::array::from_fn(|i| Coef::bandpass(EQ_HZ[i], 1.0, rate));
    let mut state = [Biquad::default(); 5];
    let mut power = [0f64; 5];
    let (mut peak, mut mid, mut side) = (0f32, 0f64, 0f64);
    for (&l, &r) in left.iter().zip(right) {
        meter.push(l, r);
        peak = peak.max(tp[0].run(l)).max(tp[1].run(r));
        let (m, s) = (f64::from(l + r) * 0.5, f64::from(l - r) * 0.5);
        mid += m * m;
        side += s * s;
        for i in 0..5 {
            let y = state[i].run(&coef[i], m);
            power[i] += y * y;
        }
    }
    let lufs = meter.integrated()?;
    let n = left.len().max(1) as f64;
    let db = |p: f64| (10.0 * (p / n).max(1e-12).log10()) as f32;
    Some(Report { lufs, true_peak_db: gain_to_db(peak), bands_db: power.map(db), side_db: db(side) - db(mid) })
}

/// The plain balance a mix's bands are compared with, dB per octave falling from the lowest: long-term music spectra
/// lose about this much per octave-wide band.
const SLOPE_DB_PER_OCTAVE: f32 = -1.5;
/// How much of a band's difference from the balance the EQ takes out, and the most it ever moves.
const EQ_SHARE: f32 = 0.5;
const EQ_MOST_DB: f32 = 4.0;
/// A smaller move isn't worth making.
const EQ_LEAST_DB: f32 = 0.5;

/// Settings for `target` from what `r` measured, the maximizer's gain still to be found (`solve`).
pub fn suggest(r: &Report, target: &Target) -> Settings {
    let mut s = Settings { ceiling_db: target.ceiling_db, saturation: target.saturation, ..Settings::default() };
    // Tone: each band's difference from the balance, its overall level taken out, so only the shape counts.
    let shape: [f32; 5] = std::array::from_fn(|i| {
        let octaves = (EQ_HZ[i] / EQ_HZ[0]).log2() as f32;
        // A bell is an octave and a bit wide at any height: the bands' widths already grow with frequency.
        let lean = if i == 0 { target.low_db } else if i == 4 { target.high_db } else { 0.0 };
        r.bands_db[i] - SLOPE_DB_PER_OCTAVE * octaves - lean
    });
    let mean = shape.iter().sum::<f32>() / 5.0;
    for i in 0..5 {
        let cut = -(shape[i] - mean) * EQ_SHARE;
        let cut = cut.clamp(-EQ_MOST_DB, EQ_MOST_DB);
        s.eq_db[i] = if cut.abs() < EQ_LEAST_DB { 0.0 } else { (cut * 10.0).round() / 10.0 };
    }
    // Dynamics: more peak over loudness than the target keeps gets compressed, in step with how much more.
    let crest = r.true_peak_db - r.lufs;
    let amount = ((crest - target.crest_db) / 8.0).clamp(0.0, 1.0);
    if amount > 0.0 {
        s.ratio = 1.5 + 1.5 * amount;
        // Close to a band's own average level: a few dB off the peaks.
        let bands = [r.bands_db[0].max(r.bands_db[1]), r.bands_db[2].max(r.bands_db[1]), r.bands_db[3].max(r.bands_db[4])];
        s.comp_db = bands.map(|db| (db + 10.0 - 6.0 * amount).clamp(-40.0, -1.0));
    }
    // Width: a narrow mix opens a little, an over-wide one closes. The bass always stays in the middle.
    s.width = if r.side_db < -16.0 {
        1.15
    } else if r.side_db > -4.0 {
        0.85
    } else {
        1.0
    };
    s.mono_bass = true;
    s
}

/// What Auto Master arrived at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solution {
    pub settings: Settings,
    pub before: Report,
    /// How the capture measures through the chain with `settings`.
    pub lufs: f32,
    pub true_peak_db: f32,
}

/// The chain's output loudness and true peak over a capture.
fn run(left: &[f32], right: &[f32], rate: f64, s: Settings) -> (f32, f32) {
    let (mut l, mut r) = (left.to_vec(), right.to_vec());
    let mut chain = Chain::with(rate, s);
    chain.process(&mut l, &mut r);
    let skip = chain.latency().min(l.len());
    let (l, r) = (&l[skip..], &r[skip..]);
    let mut meter = Loudness::new(rate, l.len() as f64 / rate + 1.0);
    let mut tp = [TruePeak::new(), TruePeak::new()];
    let mut peak = 0f32;
    for (&a, &b) in l.iter().zip(r) {
        meter.push(a, b);
        peak = peak.max(tp[0].run(a)).max(tp[1].run(b));
    }
    (meter.integrated().unwrap_or(FLOOR_LUFS), gain_to_db(peak))
}

/// The lowest and highest gain Auto Master gives the maximizer.
pub const GAIN_RANGE: (f32, f32) = (-12.0, 24.0);

/// Auto Master: measures the capture, picks settings for `target` and finds the maximizer gain that reaches its loudness.
/// None for a capture of silence.
pub fn solve(left: &[f32], right: &[f32], rate: f64, target: &Target) -> Option<Solution> {
    let before = measure(left, right, rate)?;
    let mut s = suggest(&before, target);
    // More gain is never quieter: halve the range until the loudness is close.
    let (mut lo, mut hi) = GAIN_RANGE;
    let mut best = (f32::MAX, 0f32, FLOOR_LUFS, 0f32);
    for _ in 0..9 {
        let mid = (lo + hi) * 0.5;
        s.gain_db = mid;
        let (lufs, peak) = run(left, right, rate, s);
        let off = (lufs - target.lufs).abs();
        if off < best.0 {
            best = (off, mid, lufs, peak);
        }
        if off < 0.05 {
            break;
        }
        if lufs < target.lufs { lo = mid } else { hi = mid }
    }
    s.gain_db = (best.1 * 10.0).round() / 10.0;
    let (lufs, true_peak_db) = run(left, right, rate, s);
    Some(Solution { settings: s, before, lufs, true_peak_db })
}

/// A stretch of a song being captured for Auto Master.
#[derive(Clone, Debug)]
pub struct Capture {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    capacity: usize,
}

impl Capture {
    /// Room for `seconds` at `rate`, made here so the audio thread never allocates.
    pub fn new(seconds: f64, rate: f64) -> Capture {
        let capacity = (seconds * rate) as usize;
        Capture { left: Vec::with_capacity(capacity), right: Vec::with_capacity(capacity), capacity }
    }

    /// Takes a block; silence doesn't count, so stopping the song doesn't dilute it.
    pub fn push(&mut self, l: &[f32], r: &[f32]) {
        if !l.iter().chain(r).any(|v| v.abs() > db_to_gain(-70.0)) {
            return;
        }
        let n = l.len().min(r.len()).min(self.capacity - self.left.len());
        self.left.extend_from_slice(&l[..n]);
        self.right.extend_from_slice(&r[..n]);
    }

    pub fn len(&self) -> usize {
        self.left.len()
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }

    pub fn full(&self) -> bool {
        self.left.len() >= self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::tests::sine;

    /// BS.1770's own check: a 997 Hz sine at full scale in one channel reads -3.01 LUFS.
    #[test]
    fn the_meter_reads_the_standards_reference() {
        for rate in [44100.0, 48000.0, 96000.0] {
            let n = (rate * 3.0) as usize;
            let s = sine(997.0, 1.0, rate, n);
            let mut m = Loudness::new(rate, 4.0);
            for &v in &s {
                m.push(v, 0.0);
            }
            let got = m.integrated().unwrap();
            assert!((got + 3.01).abs() < 0.1, "{rate}: {got}");
            assert!((m.momentary() + 3.01).abs() < 0.1, "{rate}: {}", m.momentary());
        }
    }

    #[test]
    fn silence_has_no_loudness_and_quiet_parts_are_gated_out() {
        let rate = 48000.0;
        let mut m = Loudness::new(rate, 10.0);
        for _ in 0..96000 {
            m.push(0.0, 0.0);
        }
        assert_eq!(m.integrated(), None);
        assert_eq!(m.momentary(), FLOOR_LUFS);
        for &v in &sine(997.0, 0.1, rate, 144000) {
            m.push(v, v);
        }
        let loud = m.integrated().unwrap();
        for &v in &sine(997.0, 0.001, rate, 144000) {
            m.push(v, v);
        }
        assert!((m.integrated().unwrap() - loud).abs() < 0.3, "the quiet end doesn't drag it down");
    }

    /// Something like music: a kick, a bass note, mid tones and noisy highs, with `tone` scaling the top.
    fn song(rate: f64, seconds: f64, level: f32, top: f32, wide: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (rate * seconds) as usize;
        let mut seed = 99u32;
        let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
        for i in 0..n {
            let t = i as f64 / rate;
            let beat = (t * 2.0).fract();
            let kick = (2.0 * std::f64::consts::PI * 60.0 * t).sin() * (-beat * 12.0).exp();
            let bass = 0.3 * (2.0 * std::f64::consts::PI * 110.0 * t).sin();
            let mids = 0.2 * (2.0 * std::f64::consts::PI * 440.0 * t).sin() + 0.15 * (2.0 * std::f64::consts::PI * 1500.0 * t).sin();
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let hat = ((seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * (-(t * 8.0).fract() * 20.0).exp() as f32;
            let mono = (kick + bass + mids) as f32 + hat * 0.05;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let air = ((seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0) * 0.05 * top;
            l[i] = (mono + air + 0.1 * wide * (2.0 * std::f64::consts::PI * 700.0 * t).sin() as f32) * level;
            r[i] = (mono + air - 0.1 * wide * (2.0 * std::f64::consts::PI * 700.0 * t).sin() as f32) * level;
        }
        (l, r)
    }

    #[test]
    fn auto_master_reaches_each_targets_loudness_under_its_ceiling() {
        let rate = 44100.0;
        let (l, r) = song(rate, 12.0, 0.2, 1.0, 1.0);
        for target in &TARGETS {
            let s = solve(&l, &r, rate, target).unwrap();
            assert!((s.lufs - target.lufs).abs() < 0.5, "{}: {} LUFS", target.name, s.lufs);
            assert!(s.true_peak_db <= target.ceiling_db + 0.1, "{}: true peak {}", target.name, s.true_peak_db);
            assert!(s.settings.gain_db > 0.0, "{}: quiet song, so gain: {}", target.name, s.settings.gain_db);
        }
    }

    #[test]
    fn a_song_already_too_loud_comes_down() {
        let rate = 44100.0;
        let (l, r) = song(rate, 8.0, 0.8, 1.0, 1.0);
        let s = solve(&l, &r, rate, &TARGETS[0]).unwrap();
        assert!((s.lufs + 14.0).abs() < 0.5, "{}", s.lufs);
        assert!(s.settings.gain_db < 0.0, "{}", s.settings.gain_db);
    }

    #[test]
    fn a_dull_mix_gets_more_top_than_a_bright_one() {
        let rate = 44100.0;
        let (dl, dr) = song(rate, 8.0, 0.2, 0.05, 1.0);
        let (bl, br) = song(rate, 8.0, 0.2, 6.0, 1.0);
        let dull = suggest(&measure(&dl, &dr, rate).unwrap(), &TARGETS[0]);
        let bright = suggest(&measure(&bl, &br, rate).unwrap(), &TARGETS[0]);
        assert!(dull.eq_db[4] > bright.eq_db[4], "{} {}", dull.eq_db[4], bright.eq_db[4]);
        for s in [dull, bright] {
            assert!(s.eq_db.iter().all(|d| d.abs() <= EQ_MOST_DB), "never a big move: {:?}", s.eq_db);
        }
    }

    #[test]
    fn dynamics_and_width_are_read_from_the_song() {
        let rate = 44100.0;
        // A tone with short loud bursts: far more peak than loudness.
        let spiky: Vec<f32> = sine(200.0, 0.05, rate, 220500).iter().enumerate().map(|(i, v)| if i % 22050 < 400 { v * 18.0 } else { *v }).collect();
        let r = measure(&spiky, &spiky, rate).unwrap();
        let s = suggest(&r, &TARGETS[2]);
        assert!(s.comp_db.iter().any(|&t| t < -1.0) && s.ratio > 1.5, "{s:?} from {r:?}");
        let flat = sine(200.0, 0.5, rate, 88200);
        let r = measure(&flat, &flat, rate).unwrap();
        let s = suggest(&r, &TARGETS[0]);
        assert_eq!(s.comp_db, [0.0; 3], "nothing to compress: {r:?}");
        assert_eq!(s.width, 1.15, "mono opens up a little");
        assert!(s.mono_bass);
        let other: Vec<f32> = flat.iter().map(|v| -v).collect();
        let r = measure(&flat, &other, rate).unwrap();
        assert_eq!(suggest(&r, &TARGETS[0]).width, 0.85, "all side closes");
    }

    #[test]
    fn silence_gives_no_answer() {
        assert_eq!(solve(&[0.0; 48000], &[0.0; 48000], 48000.0, &TARGETS[0]), None);
    }

    #[test]
    fn a_capture_skips_silence_and_stops_when_full() {
        let mut c = Capture::new(1.0, 100.0);
        c.push(&[0.0; 50], &[0.0; 50]);
        assert!(c.is_empty());
        c.push(&[0.5; 60], &[0.5; 60]);
        c.push(&[0.5; 60], &[0.5; 60]);
        assert_eq!(c.len(), 100);
        assert!(c.full());
        c.push(&[0.5; 60], &[0.5; 60]);
        assert_eq!(c.len(), 100);
    }
}
