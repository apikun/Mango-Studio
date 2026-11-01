// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The voices: a note starts every zone whose key range holds it and whose velocity range holds its velocity, each
//! through Lemon Sampler's own voices (`lemon_sampler::dsp::Engine`: the resampler, the envelope, the filter, the
//! declicking) at the zone's gain, pan and tune, the sample at the note's key against the zone's root. A zone's
//! loop plays while the key is down; the master's envelope and filter every zone plays through. Keys without a zone
//! are silent, and their note ends at once.
//!
// spec: plugins/mango-directwave.md

use std::f64::consts::{PI, SQRT_2};
use std::sync::Arc;

use lemon_sampler::dsp::{Block, Engine as Voices, Target, VOL};
use os_engine::sample::Sample;

use crate::program::{Master, Program};

/// Room made per zone for voices (more grow when needed).
const ROOM: usize = 4;

/// The master's gain: square law, unity at 256.
pub fn master_gain(v: i32) -> f32 {
    (v.clamp(0, 512) as f32 / 256.0).powi(2)
}

/// A zone's gain: 0..256 as a share of unity (128 = unity).
pub fn zone_gain(v: i32) -> f32 {
    (v.clamp(0, 256) as f32 / 128.0).min(2.0)
}

/// A zone's pan (-64..64) as -1..1.
pub fn pan_of(v: i32) -> f64 {
    (f64::from(v.clamp(-64, 64)) / 64.0).clamp(-1.0, 1.0)
}

/// A pan's split: constant power, unity at the centre.
pub fn split(pan: f64) -> (f64, f64) {
    let a = (pan.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
    (SQRT_2 * a.cos(), SQRT_2 * a.sin())
}

/// The master's envelope, from the knobs (times on the shared seconds law, `100` = none).
pub fn master_env(m: &Master) -> Option<os_engine::envelope::Params> {
    os_engine::envelope::Params::new(&os_model::Envelope {
        enabled: true,
        delay: 0,
        attack: m.attack * 512,
        hold: 0,
        decay: m.decay * 512,
        sustain: m.sustain.clamp(0, 128),
        release: m.release * 512,
        amount: 0,
        attack_tension: 0,
        decay_tension: 0,
        release_tension: 0,
        flags: 0,
        lfo_delay: 0,
        lfo_attack: 0,
        lfo_amount: 0,
        lfo_speed: 0,
        lfo_shape: 0,
    })
}

/// One zone as the voices play it.
#[derive(Clone, Debug)]
pub struct ZoneKit {
    pub sample: Option<Arc<Sample>>,
    pub lo: i32,
    pub hi: i32,
    pub vel_lo: i32,
    pub vel_hi: i32,
    pub root: i32,
    pub gain: f32,
    pub pan: f64,
    pub cents: f64,
    pub loop_on: bool,
}

impl ZoneKit {
    /// Whether the zone holds key `k` at velocity `v` (the host's 0..128).
    pub fn holds(&self, k: i32, v: i32) -> bool {
        self.sample.is_some()
            && k >= self.lo
            && k <= self.hi
            && v >= self.vel_lo
            && (v < self.vel_hi || self.vel_hi >= 128)
    }
}

/// What the voices play: the zones, and the zones on each key.
#[derive(Clone, Debug, Default)]
pub struct Kit {
    pub zones: Vec<ZoneKit>,
    pub by_key: Vec<Vec<u16>>,
    pub master_gain: f32,
    pub env: Option<os_engine::envelope::Params>,
    pub cut: f32,
    pub res: f32,
}

impl Kit {
    /// The kit of a program; `open(i, path)` gives a zone's sample (`None`: silent).
    pub fn new(p: &Program, open: &mut dyn FnMut(usize, &str) -> Option<Arc<Sample>>) -> Kit {
        let master = p.master.clone();
        let zones: Vec<ZoneKit> = p
            .zones()
            .iter()
            .enumerate()
            .map(|(i, z)| ZoneKit {
                sample: if z.path.is_empty() { None } else { open(i, &z.path) },
                lo: z.lo,
                hi: z.hi,
                vel_lo: z.vel_lo,
                vel_hi: z.vel_hi,
                root: z.root,
                gain: zone_gain(z.gain),
                pan: pan_of(z.pan),
                cents: f64::from(z.tune.clamp(-100, 100)),
                loop_on: z.loop_on,
            })
            .collect();
        let mut by_key = vec![Vec::new(); 128];
        for (i, z) in zones.iter().enumerate() {
            if z.sample.is_none() {
                continue;
            }
            for k in z.lo.clamp(0, 127)..=z.hi.clamp(0, 127) {
                by_key[k as usize].push(i as u16);
            }
        }
        Kit {
            master_gain: master_gain(master.gain),
            env: master_env(&master),
            cut: master.cut.clamp(0, 128) as f32 / 128.0,
            res: master.res.clamp(0, 128) as f32 / 128.0,
            zones,
            by_key,
        }
    }

    /// The zones that sound on `key` at velocity `v`.
    pub fn on(&self, key: u8, v: i32) -> Vec<usize> {
        self.by_key
            .get(usize::from(key))
            .map_or(&[][..], |z| z.as_slice())
            .iter()
            .filter(|&&i| self.zones[i as usize].holds(i32::from(key), v))
            .map(|&i| i as usize)
            .collect()
    }

    /// A zone's block: the note plays against the zone's root, through the master's envelope and filter.
    fn block(&self, z: &ZoneKit) -> Block {
        let mut targets = [Target::default(); 5];
        targets[VOL] = Target { env: self.env.clone(), amount: 1.0, lfo: Default::default() };
        Block {
            root: z.root,
            cents: z.cents,
            targets,
            f_type: 0,
            f_cut: self.cut,
            f_res: self.res,
            loop_on: z.loop_on,
            ..Block::default()
        }
    }
}

/// A host note still sounding: its own voice id, its key and host id, and the zones still playing it.
#[derive(Clone, Copy, Debug)]
struct Live {
    id: i32,
    key: u8,
    host: Option<i32>,
    left: u32,
}

pub struct Engine {
    rate: f32,
    kit: Option<Arc<Kit>>,
    /// A voice engine and a block per zone.
    voices: Vec<Voices>,
    blocks: Vec<Block>,
    live: Vec<Live>,
    next_id: i32,
    ended: Vec<(u8, Option<i32>)>,
}

impl Engine {
    pub fn new(rate: f32) -> Self {
        Engine { rate: rate.max(1.0), kit: None, voices: Vec::new(), blocks: Vec::new(), live: Vec::new(), next_id: 0, ended: Vec::new() }
    }

    pub fn set_rate(&mut self, rate: f32) {
        self.rate = rate.max(1.0);
        let kit = self.kit.take();
        self.voices.clear();
        self.blocks.clear();
        self.set_kit(kit);
    }

    /// Plays `kit` from now on: zones keeping their sample keep their voices; the others start silent.
    pub fn set_kit(&mut self, kit: Option<Arc<Kit>>) {
        let n = kit.as_ref().map_or(0, |k| k.zones.len());
        self.voices.resize_with(n, || Voices::with_room(self.rate, ROOM));
        self.blocks.resize(n, Block::default());
        if let Some(k) = &kit {
            for (i, z) in k.zones.iter().enumerate() {
                let same = match (&self.voices[i].sample, &z.sample) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                };
                if !same {
                    let mut v = Voices::with_room(self.rate, ROOM);
                    v.sample = z.sample.clone();
                    self.voices[i] = v;
                }
                self.blocks[i] = k.block(z);
            }
        }
        self.kit = kit;
    }

    pub fn reset(&mut self) {
        for v in &mut self.voices {
            v.reset();
        }
        self.live.clear();
    }

    pub fn has_voices(&self) -> bool {
        self.voices.iter().any(|v| v.has_voices())
    }

    /// Host voices that ended since the last call (key, host voice id).
    pub fn take_ended(&mut self) -> Vec<(u8, Option<i32>)> {
        std::mem::take(&mut self.ended)
    }

    /// A note: every zone on `key` plays; keys without a zone are silent (and their note ends at once).
    pub fn note_on(&mut self, key: u8, velocity: f32, host: Option<i32>) {
        let Some(kit) = self.kit.clone() else {
            self.ended.push((key, host));
            return;
        };
        let v128 = (velocity.clamp(0.0, 1.0) * 128.0).round() as i32;
        let zones = kit.on(key, v128);
        if zones.is_empty() {
            self.ended.push((key, host));
            return;
        }
        let n = zones.len();
        self.next_id = self.next_id.wrapping_add(1) & 0x3fff_ffff;
        let id = self.next_id;
        for i in zones {
            let z = &kit.zones[i];
            let (v, b) = (&mut self.voices[i], &self.blocks[i]);
            let (gl, gr) = split(z.pan);
            let m = ((gl * gl + gr * gr) / 2.0).sqrt();
            v.note_on_id(key, velocity, Some(id), b);
            v.volume(Some(id), key, kit.master_gain * z.gain * m as f32);
            v.pan(Some(id), key, z.pan as f32);
        }
        self.live.push(Live { id, key, host, left: n as u32 });
    }

    /// The key's zones release (a looping voice closes, an enveloped one falls to its release).
    pub fn note_off(&mut self, key: u8) {
        for (v, b) in self.voices.iter_mut().zip(self.blocks.iter()) {
            v.note_off(key, b);
        }
    }

    /// Every voice stops with a short fade.
    pub fn all_off(&mut self) {
        for v in &mut self.voices {
            v.all_off();
        }
    }

    /// Adds the voices into `left` / `right`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (i, v) in self.voices.iter_mut().enumerate() {
            if v.has_voices() {
                let b = self.blocks[i].clone();
                v.render(left, right, &b);
            }
            for (_, id) in v.take_ended() {
                let Some(id) = id else { continue };
                if let Some(p) = self.live.iter().position(|n| n.id == id) {
                    self.live[p].left -= 1;
                    if self.live[p].left == 0 {
                        let n = self.live.remove(p);
                        self.ended.push((n.key, n.host));
                    }
                }
            }
        }
    }
}
