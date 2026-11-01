// Copyright (C) 2026 Artig3nce
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The Mango Kit: eighteen drum sounds made from nothing but maths (oscillators, noise and filters), so no sample of
//! anyone else is in them and they can ship with the app under its own licence. Every sound is deterministic: the same
//! rate gives the same samples, bit for bit. The 808-style sounds' recipes follow `drums` (battlemountain2); these are
//! further voices in the same spirit: three kicks, three snares and rims, two claps, three hats, three toms, a cowbell,
//! a shaker, a clave and a crash.

use std::f64::consts::TAU;
use std::sync::Arc;

use crate::Sample;

/// Every sound of the kit: its file name (without `.wav`) and how it is made.
pub const SOUNDS: [(&str, fn(u32) -> Arc<Sample>); 18] = [
    ("Mango Kick Deep", kick_deep),
    ("Mango Kick Punch", kick_punch),
    ("Mango Kick Boom", kick_boom),
    ("Mango Snare Tight", snare_tight),
    ("Mango Snare Fat", snare_fat),
    ("Mango Rim", rim),
    ("Mango Clap Room", clap_room),
    ("Mango Clap Snap", clap_snap),
    ("Mango Hat Closed", hat_closed),
    ("Mango Hat Open", hat_open),
    ("Mango Hat Pedal", hat_pedal),
    ("Mango Tom Low", tom_low),
    ("Mango Tom Mid", tom_mid),
    ("Mango Tom High", tom_high),
    ("Mango Cowbell", cowbell),
    ("Mango Shaker", shaker),
    ("Mango Clave", clave),
    ("Mango Crash", crash),
];

/// The loudest sample of every sound, a little under full scale.
const PEAK: f32 = 0.89;

/// Deterministic noise: a linear congruential generator, so a sound is the same on every run.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        f64::from(self.0) / f64::from(u32::MAX) * 2.0 - 1.0
    }
}

/// A two-pole filter (RBJ's cookbook): low-pass, high-pass or band-pass at `freq` Hz.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn new(kind: Kind, rate: f64, freq: f64, q: f64) -> Biquad {
        let w = TAU * freq.min(rate * 0.45) / rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        let (b, a0) = match kind {
            Kind::Low => ([(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0], 1.0 + alpha),
            Kind::High => ([(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0], 1.0 + alpha),
            Kind::Band => ([alpha, 0.0, -alpha], 1.0 + alpha),
        };
        Biquad { b: [b[0] / a0, b[1] / a0, b[2] / a0], a: [-2.0 * cos / a0, (1.0 - alpha) / a0], x: [0.0; 2], y: [0.0; 2] }
    }

    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Low,
    High,
    Band,
}

/// `secs` seconds of `voice(t, i)` (time in seconds, sample index), faded out over the last 8 ms and scaled to `PEAK`.
fn render(rate: u32, secs: f64, mut voice: impl FnMut(f64, usize) -> f64) -> Vec<f32> {
    let r = f64::from(rate.max(8000));
    let n = (r * secs) as usize;
    let fade = (r * 0.008) as usize;
    let mut out: Vec<f32> = (0..n)
        .map(|i| {
            let tail = n.saturating_sub(i);
            let f = if tail < fade { tail as f64 / fade as f64 } else { 1.0 };
            (voice(i as f64 / r, i) * f) as f32
        })
        .collect();
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    for v in &mut out {
        *v *= PEAK / peak;
    }
    out
}

/// A mono sound as a stereo sample.
fn mono(rate: u32, v: Vec<f32>) -> Arc<Sample> {
    Arc::new(Sample { rate, right: v.clone(), left: v })
}

/// A pitch that glides from `from` to `to` Hz at `drop` per second, as a phase (integrated, so it has no clicks).
fn glide(t: f64, from: f64, to: f64, drop: f64) -> f64 {
    TAU * (to * t + (from - to) / drop * (1.0 - (-t * drop).exp()))
}

/// A kick: a sine that falls from `from` to `to` Hz, decaying at `decay`/s, `drive` pushed into a soft clipper, with a
/// short noise click of `click` strength.
fn kick(rate: u32, secs: f64, (from, to, drop): (f64, f64, f64), decay: f64, drive: f64, click: f64) -> Arc<Sample> {
    let mut noise = Noise(7);
    let mut hp = Biquad::new(Kind::High, f64::from(rate), 1500.0, 0.7);
    let out = render(rate, secs, |t, _| {
        let body = glide(t, from, to, drop).sin() * (-t * decay).exp() * (t / 0.0015).min(1.0);
        let tick = hp.run(noise.next()) * (-t * 300.0).exp() * click;
        ((body + tick) * drive).tanh()
    });
    mono(rate, out)
}

/// A deep, long sub kick.
pub fn kick_deep(rate: u32) -> Arc<Sample> {
    kick(rate, 1.3, (130.0, 38.0, 30.0), 4.6, 1.4, 0.10)
}

/// A short, hard kick with a strong click.
pub fn kick_punch(rate: u32) -> Arc<Sample> {
    kick(rate, 0.35, (230.0, 52.0, 70.0), 11.0, 1.8, 0.35)
}

/// A round, soft kick without a click.
pub fn kick_boom(rate: u32) -> Arc<Sample> {
    kick(rate, 0.8, (95.0, 46.0, 18.0), 7.5, 1.0, 0.0)
}

/// A snare: two tones (`tones`, Hz) decaying at `tone_decay`/s, and noise band-passed at `band` Hz decaying at
/// `wire_decay`/s, `wire` as loud as the tones.
fn snare(rate: u32, secs: f64, tones: [f64; 2], tone_decay: f64, band: f64, wire_decay: f64, wire: f64) -> Arc<Sample> {
    let mut noise = Noise(11);
    let mut bp = Biquad::new(Kind::Band, f64::from(rate), band, 0.8);
    let mut hp = Biquad::new(Kind::High, f64::from(rate), 1200.0, 0.7);
    let out = render(rate, secs, |t, _| {
        let tone = (TAU * tones[0] * t).sin() + 0.5 * (TAU * tones[1] * t).sin();
        let wires = hp.run(bp.run(noise.next())) * 2.2;
        (tone * (-t * tone_decay).exp() * 0.6 + wires * (-t * wire_decay).exp() * wire).tanh()
    });
    mono(rate, out)
}

/// A tight, high snare.
pub fn snare_tight(rate: u32) -> Arc<Sample> {
    snare(rate, 0.25, [200.0, 330.0], 28.0, 4200.0, 22.0, 0.9)
}

/// A fat, long snare.
pub fn snare_fat(rate: u32) -> Arc<Sample> {
    snare(rate, 0.55, [165.0, 290.0], 16.0, 3200.0, 11.0, 0.8)
}

/// A rim shot: two short ringing tones and a click.
pub fn rim(rate: u32) -> Arc<Sample> {
    let mut noise = Noise(5);
    let out = render(rate, 0.14, |t, _| {
        let ring = ((TAU * 1700.0 * t).sin() + 0.8 * (TAU * 480.0 * t).sin()) * (-t * 90.0).exp();
        (ring + noise.next() * (-t * 400.0).exp() * 0.4).tanh()
    });
    mono(rate, out)
}

/// A clap: `bursts` bursts of band-passed noise `gap` seconds apart, then a tail decaying at `tail`/s.
fn clap(rate: u32, secs: f64, bursts: usize, gap: f64, band: f64, tail: f64) -> Arc<Sample> {
    let mut noise = Noise(3);
    let mut bp = Biquad::new(Kind::Band, f64::from(rate), band, 1.1);
    let out = render(rate, secs, |t, _| {
        let n = bp.run(noise.next()) * 2.0;
        let k = (t / gap).floor();
        let burst = if (k as usize) < bursts { (-(t - k * gap) * 240.0).exp() } else { 0.0 };
        let end = (bursts as f64 * gap).min(t);
        n * (burst + 0.55 * (-(t - end).max(0.0) * tail).exp() * if t >= end { 1.0 } else { 0.0 })
    });
    mono(rate, out)
}

/// A clap with a room tail.
pub fn clap_room(rate: u32) -> Arc<Sample> {
    clap(rate, 0.45, 4, 0.011, 1150.0, 12.0)
}

/// A short, dry, snappy clap.
pub fn clap_snap(rate: u32) -> Arc<Sample> {
    clap(rate, 0.18, 3, 0.008, 1500.0, 30.0)
}

/// The six square waves of the metallic hat sound, Hz.
const METAL: [f64; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

/// A hat: the metal cluster and noise through a high-pass at `hp` Hz, decaying at `decay`/s.
fn hat(rate: u32, secs: f64, hp: f64, decay: f64) -> Arc<Sample> {
    let mut noise = Noise(13);
    let mut filter = Biquad::new(Kind::High, f64::from(rate), hp, 0.9);
    let out = render(rate, secs, |t, _| {
        let metal: f64 = METAL.iter().map(|f| if (TAU * f * 4.0 * t).sin() >= 0.0 { 1.0 } else { -1.0 }).sum::<f64>() / 6.0;
        filter.run(metal * 0.7 + noise.next() * 0.5) * (-t * decay).exp() * (t / 0.0008).min(1.0)
    });
    mono(rate, out)
}

/// A tight closed hat.
pub fn hat_closed(rate: u32) -> Arc<Sample> {
    hat(rate, 0.12, 7000.0, 60.0)
}

/// A long open hat.
pub fn hat_open(rate: u32) -> Arc<Sample> {
    hat(rate, 0.7, 6500.0, 8.5)
}

/// A pedal hat: shorter than open, a little lower.
pub fn hat_pedal(rate: u32) -> Arc<Sample> {
    hat(rate, 0.2, 5000.0, 32.0)
}

/// A tom: a sine falling from `from` to `to` Hz with a bit of skin noise.
fn tom(rate: u32, from: f64, to: f64, decay: f64) -> Arc<Sample> {
    let mut noise = Noise(17);
    let mut lp = Biquad::new(Kind::Low, f64::from(rate), 1800.0, 0.7);
    let out = render(rate, 0.55, |t, _| {
        let body = glide(t, from, to, 14.0).sin() * (-t * decay).exp();
        (body + lp.run(noise.next()) * (-t * 70.0).exp() * 0.25).tanh()
    });
    mono(rate, out)
}

/// A low tom.
pub fn tom_low(rate: u32) -> Arc<Sample> {
    tom(rate, 100.0, 68.0, 9.0)
}

/// A mid tom.
pub fn tom_mid(rate: u32) -> Arc<Sample> {
    tom(rate, 150.0, 104.0, 11.0)
}

/// A high tom.
pub fn tom_high(rate: u32) -> Arc<Sample> {
    tom(rate, 225.0, 158.0, 13.0)
}

/// A cowbell: two square waves a fifth-and-a-bit apart through a band-pass.
pub fn cowbell(rate: u32) -> Arc<Sample> {
    let mut bp = Biquad::new(Kind::Band, f64::from(rate), 2600.0, 1.4);
    let out = render(rate, 0.4, |t, _| {
        let sq = |f: f64| if (TAU * f * t).sin() >= 0.0 { 1.0 } else { -1.0 };
        bp.run(sq(540.0) + sq(800.0)) * (-t * 14.0).exp()
    });
    mono(rate, out)
}

/// A shaker in stereo: band-passed noise with a soft attack, a different grain in each ear.
pub fn shaker(rate: u32) -> Arc<Sample> {
    let make = |seed: u32| {
        let mut noise = Noise(seed);
        let mut bp = Biquad::new(Kind::Band, f64::from(rate), 6500.0, 0.9);
        render(rate, 0.16, move |t, _| bp.run(noise.next()) * (t / 0.012).min(1.0) * (-t * 24.0).exp())
    };
    Arc::new(Sample { rate, left: make(19), right: make(23) })
}

/// A clave: one short, high, woody tone.
pub fn clave(rate: u32) -> Arc<Sample> {
    let out = render(rate, 0.2, |t, _| glide(t, 2650.0, 2400.0, 40.0).sin() * (-t * 55.0).exp());
    mono(rate, out)
}

/// A crash in stereo: metal and noise through a high-pass, ringing for two seconds, a different grain in each ear.
pub fn crash(rate: u32) -> Arc<Sample> {
    let make = |seed: u32| {
        let mut noise = Noise(seed);
        let mut hp = Biquad::new(Kind::High, f64::from(rate), 5200.0, 0.8);
        render(rate, 2.0, move |t, _| {
            let metal: f64 = METAL.iter().map(|f| if (TAU * f * 5.3 * t).sin() >= 0.0 { 1.0 } else { -1.0 }).sum::<f64>() / 6.0;
            hp.run(metal * 0.4 + noise.next() * 0.7) * (t / 0.002).min(1.0) * ((-t * 2.4).exp() * 0.8 + (-t * 14.0).exp() * 0.5)
        })
    };
    Arc::new(Sample { rate, left: make(29), right: make(31) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sound is there, named once, audible, kept under full scale, ends in silence, and is the same on a second
    /// run (no clicks at the end, nothing from outside the code).
    #[test]
    fn every_sound_is_audible_clean_and_repeatable() {
        let mut names: Vec<&str> = SOUNDS.iter().map(|s| s.0).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 18);
        for (name, make) in SOUNDS {
            let (a, b) = (make(44100), make(44100));
            assert_eq!((a.left.clone(), a.right.clone()), (b.left.clone(), b.right.clone()), "{name}: repeatable");
            assert_eq!(a.left.len(), a.right.len(), "{name}");
            let peak = a.left.iter().chain(&a.right).fold(0.0f32, |m, v| m.max(v.abs()));
            assert!((0.88..=0.90).contains(&peak), "{name}: peak {peak}");
            assert!(a.left.iter().chain(&a.right).all(|v| v.is_finite()), "{name}: finite");
            let secs = a.left.len() as f32 / 44100.0;
            assert!((0.08..=2.1).contains(&secs), "{name}: {secs} s");
            assert!(a.left.last().unwrap().abs() < 1e-3 && a.right.last().unwrap().abs() < 1e-3, "{name}: ends in silence");
            // It dies away by itself: the last tenth is already quiet, so the fade-out only removes a click, not a tail.
            let tail = a.left.len() - a.left.len() / 10;
            let loud = a.left[tail..].iter().chain(&a.right[tail..]).fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(loud < 0.06, "{name}: still {loud} in its last tenth");
            // Not a flat line: the sound has energy in its first tenth.
            let head = a.left.len() / 10;
            let rms = (a.left[..head].iter().map(|v| v * v).sum::<f32>() / head as f32).sqrt();
            assert!(rms > 0.02, "{name}: rms {rms}");
        }
    }

    /// The kicks are low and the hats are high: a rough measure of each one's pitch (zero crossings per second over its
    /// first 40 ms) puts them where they belong.
    #[test]
    fn kicks_are_low_and_hats_are_high() {
        let pitch = |make: fn(u32) -> Arc<Sample>| {
            let s = make(44100);
            let n = 1764;
            let crossings = s.left[..n].windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
            crossings as f32 / 2.0 / (n as f32 / 44100.0)
        };
        assert!(pitch(kick_deep) < 200.0, "deep kick: {}", pitch(kick_deep));
        assert!(pitch(kick_boom) < 200.0, "boom kick: {}", pitch(kick_boom));
        assert!(pitch(hat_closed) > 2500.0, "closed hat: {}", pitch(hat_closed));
        assert!(pitch(tom_low) < pitch(tom_high), "toms rise");
    }

    /// The stereo sounds differ between the ears; the others are the same in both.
    #[test]
    fn crash_and_shaker_are_stereo() {
        for make in [crash as fn(u32) -> Arc<Sample>, shaker] {
            let s = make(44100);
            assert_ne!(s.left, s.right);
        }
        let k = kick_deep(44100);
        assert_eq!(k.left, k.right);
    }
}
