// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The grain engine: a note keeps a playhead walking the sample at the note's pitch (wrapping around, so a held key
//! loops), and every `spacing` spawns a grain — a short read from (near) the playhead with a trapezoid envelope of
//! its own and a pan of its own — until the key comes up, when the live grains fade over the release and the voice
//! ends once they are done. The grain length, spacing, random start window, pan spread and hold come from the
//! parameters; the grain starts are a deterministic xorshift, so the same notes render the same sound.
//!
// spec: plugins/lemon-granulizer.md §2

use std::sync::Arc;

use os_engine::sample::Sample;

use crate::params::Values;

/// A voice's grains at most (the cloud's density cap: the spacing knob can ask for far more).
const MAX_GRAINS: usize = 128;
/// Voices at most; a note over the cap takes the oldest one's place.
const MAX_VOICES: usize = 24;
/// The declick when the release is off: grains fade over this many frames at a key up (all off too).
const DECLICK: usize = 64;

/// What the voices play: the sample and the key the note plays against.
#[derive(Clone, Debug, Default)]
pub struct Kit {
    pub sample: Option<Arc<Sample>>,
    pub root: i32,
}

impl Kit {
    /// Whether a key plays (a sample is loaded).
    pub fn plays(&self) -> bool {
        self.sample.as_ref().is_some_and(|s| s.frames() > 0)
    }
}

/// A block's settings, from the parameters and the rates (`Engine::block`).
#[derive(Clone, Debug)]
pub struct Block {
    /// Sample frames an output frame moves the read (signed: reverse plays backwards).
    pub speed: f64,
    /// The grain's length and the frames between grain onsets (output frames, at least 1).
    pub grain: usize,
    pub spacing: usize,
    /// The grain start's random window (sample frames).
    pub random: usize,
    /// The pan spread (0..1; a grain pans anywhere in ± spread).
    pub pan: f64,
    /// The grain envelope's flat share (the ramps take the rest, 0.02..0.5 each side).
    pub ramp: f64,
    /// The pitch shift's cents (the note's key plays against the kit's root on top of it).
    pub cents: f64,
    pub attack: usize,
    pub release: usize,
}

impl Default for Block {
    fn default() -> Self {
        Block { speed: 1.0, grain: 1, spacing: 1, random: 0, pan: 0.0, ramp: 0.11, cents: 0.0, attack: 0, release: 0 }
    }
}

/// A linear read of `s` at `pos` (sample frames, wraps around the loop).
fn read(s: &Sample, pos: f64) -> (f32, f32) {
    let n = s.frames();
    if n == 0 {
        return (0.0, 0.0);
    }
    let p = pos.rem_euclid(n as f64);
    let i0 = p as usize;
    let i1 = (i0 + 1) % n;
    let f = (p - i0 as f64) as f32;
    let lerp = |a: f32, b: f32| a + (b - a) * f;
    (lerp(s.left[i0], s.left[i1]), lerp(s.right[i0], s.right[i1]))
}

/// A pan's split: constant power, unity at the centre.
fn split(pan: f64) -> (f64, f64) {
    use std::f64::consts::{PI, SQRT_2};
    let a = (pan.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
    (SQRT_2 * a.cos(), SQRT_2 * a.sin())
}

/// One grain: a read from `start` at `speed`, its envelope and its cut (a key up's fade).
#[derive(Clone, Copy, Debug)]
struct Grain {
    start: f64,
    speed: f64,
    played: usize,
    len: usize,
    ramp: f64,
    pan: (f64, f64),
    /// Where a cut fade starts (output frames into the grain) and how long it takes.
    cut: Option<(usize, usize)>,
}

impl Grain {
    /// The grain's envelope at `played`: the trapezoid's ramps, then the cut's linear fall.
    fn env(&self) -> f32 {
        let t = self.played as f64 / self.len.max(1) as f64;
        let mut e = if t < self.ramp {
            t / self.ramp
        } else if t > 1.0 - self.ramp {
            ((1.0 - t) / self.ramp).max(0.0)
        } else {
            1.0
        };
        if let Some((c0, cf)) = self.cut
            && self.played > c0
        {
            e *= (1.0 - (self.played - c0) as f64 / cf.max(1) as f64).clamp(0.0, 1.0);
        }
        e as f32
    }
}

/// A note still sounding: its playhead, its grain cloud, its attack ramp and its randoms.
struct Voice {
    key: u8,
    host: Option<i32>,
    speed: f64,
    playhead: f64,
    since: usize,
    /// The attack ramp's progress (0..1) and the note's level (the velocity).
    gain: f64,
    vel: f64,
    releasing: bool,
    grains: Vec<Grain>,
    rng: u64,
}

impl Voice {
    /// The next random 0..1 (a deterministic xorshift: the same notes render the same sound).
    fn next(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Spawns the grains the spacing asks for from the block.
    fn spawn(&mut self, blk: &Block) {
        let window = blk.random as f64;
        while self.since >= blk.spacing {
            self.since -= blk.spacing;
            if self.grains.len() >= MAX_GRAINS {
                break;
            }
            let r = self.next();
            let start = if blk.speed < 0.0 { self.playhead + r * window } else { self.playhead - r * window };
            let pan = if blk.pan > 0.0 { (self.next() * 2.0 - 1.0) * blk.pan } else { 0.0 };
            self.grains.push(Grain { start, speed: blk.speed, played: 0, len: blk.grain, ramp: blk.ramp, pan: split(pan), cut: None });
        }
    }
}

pub struct Engine {
    rate: f32,
    kit: Option<Arc<Kit>>,
    voices: Vec<Voice>,
    next_id: u64,
    ended: Vec<(u8, Option<i32>)>,
}

impl Engine {
    pub fn new(rate: f32) -> Self {
        Engine { rate: rate.max(1.0), kit: None, voices: Vec::new(), next_id: 0, ended: Vec::new() }
    }

    pub fn set_rate(&mut self, rate: f32) {
        self.rate = rate.max(1.0);
        // A rate change moves every read: the voices start over rather than jump.
        self.voices.clear();
    }

    pub fn set_kit(&mut self, kit: Option<Arc<Kit>>) {
        self.kit = kit;
    }

    pub fn reset(&mut self) {
        self.voices.clear();
    }

    pub fn has_voices(&self) -> bool {
        !self.voices.is_empty()
    }

    /// Host voices that ended since the last call (key, host voice id).
    pub fn take_ended(&mut self) -> Vec<(u8, Option<i32>)> {
        std::mem::take(&mut self.ended)
    }

    /// The block of the parameters at the engine's rate and the kit's sample.
    pub fn block(&self, v: &Values) -> Block {
        let (rate, sr, frames) = match &self.kit {
            Some(k) if k.plays() => (self.rate, f64::from(k.sample.as_ref().unwrap().rate), k.sample.as_ref().unwrap().frames()),
            _ => (self.rate, f64::from(self.rate as u32), 0),
        };
        let speed = if frames == 0 { 1.0 } else { sr / f64::from(rate) };
        Block {
            speed: if v.reverse { -speed } else { speed },
            grain: ((f64::from(v.grain.clamp(5, 2000)) * f64::from(rate) / 1000.0).round() as usize).max(1),
            spacing: ((f64::from(v.spacing.clamp(5, 2000)) * f64::from(rate) / 1000.0).round() as usize).max(1),
            random: (v.random.clamp(0, 100) as u64 * frames as u64 / 100) as usize,
            pan: f64::from(v.pan.clamp(0, 100)) / 100.0,
            ramp: (0.5 * (1.0 - f64::from(v.hold.clamp(0, 100)) / 100.0) + 0.02).min(0.5),
            cents: f64::from(v.pitch.clamp(-1200, 1200)),
            attack: (f64::from(v.attack.clamp(0, 500)) * f64::from(rate) / 1000.0).round() as usize,
            release: (f64::from(v.release.clamp(0, 500)) * f64::from(rate) / 1000.0).round() as usize,
        }
    }

    /// A note: a voice walks the sample from its start (its end when reversed), at the note's pitch against the kit's
    /// root. A key without a sample is silent (and its note ends at once).
    pub fn note_on(&mut self, key: u8, velocity: f32, host: Option<i32>, blk: &Block) {
        let Some(kit) = self.kit.clone() else {
            self.ended.push((key, host));
            return;
        };
        if !kit.plays() {
            self.ended.push((key, host));
            return;
        }
        if self.voices.len() >= MAX_VOICES {
            let old = self.voices.remove(0);
            self.ended.push((old.key, old.host));
        }
        self.next_id = self.next_id.wrapping_add(1);
        let cents = blk.cents + f64::from(key as i32 - kit.root) * 100.0;
        let pitch = 2f64.powf(cents / 1200.0);
        let speed = blk.speed * pitch;
        let seed = 0x9E37_79B9_7F4A_7C15u64 ^ (u64::from(key) << 32) ^ self.next_id;
        let n = kit.sample.as_ref().unwrap().frames() as f64;
        let playhead = if speed < 0.0 { n } else { 0.0 };
        // `since` starts at the spacing so the first grain spawns on the note's first frame.
        self.voices.push(Voice { key, host, speed, playhead, since: blk.spacing, gain: if blk.attack == 0 { 1.0 } else { 0.0 }, vel: f64::from(velocity.clamp(0.0, 1.0)), releasing: false, grains: Vec::new(), rng: seed });
    }

    /// The key's voices stop spawning grains; the live ones fade over the release (a short declick when it is off).
    pub fn note_off(&mut self, key: u8, blk: &Block) {
        let fade = if blk.release > 0 { blk.release } else { DECLICK };
        for v in self.voices.iter_mut().filter(|v| v.key == key) {
            v.releasing = true;
            for g in v.grains.iter_mut() {
                g.cut = Some((g.played, fade));
            }
        }
    }

    /// Every voice stops with a short fade.
    pub fn all_off(&mut self) {
        for v in self.voices.iter_mut() {
            v.releasing = true;
            for g in v.grains.iter_mut() {
                g.cut = Some((g.played, DECLICK));
            }
        }
    }

    /// Adds the voices into `left` / `right`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32], blk: &Block) {
        let Some(kit) = self.kit.clone() else {
            self.voices.clear();
            return;
        };
        let Some(s) = kit.sample.clone() else {
            self.voices.clear();
            return;
        };
        for vi in (0..self.voices.len()).rev() {
            let v = &mut self.voices[vi];
            for i in 0..left.len().min(right.len()) {
                if !v.releasing && v.gain < 1.0 {
                    v.gain = (v.gain + 1.0 / blk.attack.max(1) as f64).min(1.0);
                }
                if !v.releasing {
                    v.since += 1;
                    v.spawn(blk);
                }
                v.playhead += v.speed;
                let (mut l, mut r) = (0.0f64, 0.0f64);
                for g in v.grains.iter_mut() {
                    let (sl, sr) = read(&s, g.start + g.played as f64 * g.speed);
                    let e = g.env() as f64;
                    l += sl as f64 * e * g.pan.0;
                    r += sr as f64 * e * g.pan.1;
                    g.played += 1;
                }
                v.grains.retain(|g| g.played < g.len);
                left[i] += (l * v.gain * v.vel) as f32;
                right[i] += (r * v.gain * v.vel) as f32;
                if v.releasing && v.grains.is_empty() {
                    self.ended.push((v.key, v.host));
                    self.voices.remove(vi);
                    break;
                }
            }
        }
    }
}
