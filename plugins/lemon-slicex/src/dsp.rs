// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The voices: a note plays its key's slice from its start to the next slice's start (or the sample's end), read with
//! linear interpolation at the sample's own speed times the pitch shift, with the attack / decay fades, the declick
//! fades and a short fade when the note ends. The slice's articulator filters it and moves its pan, volume, cutoff,
//! resonance, speed and start by its sources (`ext`).
//!
// spec: plugins/lemon-slicex.md §2

use std::sync::Arc;

use os_engine::sample::Sample;

use crate::ext::{Artic, Ext, Source, Target, ARTICS, FILTER_OFF};
use crate::state::State;

/// Voices at most; the oldest gives way.
pub const VOICES: usize = 32;
/// The fade when a note ends, and the declick fades (s).
const RELEASE_S: f64 = 0.005;
const DECLICK_S: f64 = 0.002;

/// One slice as the voices play it: frames `start..end` of the sample (`next` = where the next slice starts).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub start: usize,
    pub next: usize,
    pub reversed: bool,
}

/// What the voices play: the sample, its slices and which slice each key plays.
#[derive(Clone, Debug)]
pub struct Kit {
    pub sample: Arc<Sample>,
    pub spans: Vec<Span>,
    pub keys: [Option<u16>; 128],
    /// The articulator of each slice, and the articulators.
    pub art: Vec<u8>,
    pub ext: Arc<Ext>,
    /// The second deck: its loop and slices.
    pub b: Option<Deck>,
}

/// A deck's loop as the voices play it.
#[derive(Clone, Debug)]
pub struct Deck {
    pub sample: Arc<Sample>,
    pub spans: Vec<Span>,
    pub keys: [Option<u16>; 128],
}

impl Deck {
    pub fn new(sample: Arc<Sample>, st: &State) -> Self {
        let frames = sample.frames();
        let n = st.slices.len();
        let spans = (0..n)
            .map(|i| {
                let start = (st.slices[i].start as usize).min(frames);
                let next = st.slices.get(i + 1).map_or(frames, |s| (s.start as usize).min(frames)).max(start);
                Span { start, next, reversed: st.slices[i].reversed() }
            })
            .collect();
        let mut keys = [None; 128];
        for (k, slot) in keys.iter_mut().enumerate() {
            *slot = st.slice_for(k as i32).map(|i| i as u16);
        }
        Deck { sample, spans, keys }
    }
}

impl Kit {
    /// The kit with a second deck.
    pub fn with_b(mut self, deck: Option<Deck>) -> Self {
        self.b = deck;
        self
    }

    /// Deck `d`'s sample and slices (`None`: no such deck).
    fn deck(&self, d: u8) -> Option<(&Arc<Sample>, &[Span])> {
        if d == 0 { Some((&self.sample, &self.spans)) } else { self.b.as_ref().map(|b| (&b.sample, b.spans.as_slice())) }
    }

    pub fn new(sample: Arc<Sample>, st: &State, ext: Arc<Ext>) -> Self {
        let frames = sample.frames();
        let n = st.slices.len();
        let spans = (0..n)
            .map(|i| {
                let start = (st.slices[i].start as usize).min(frames);
                let next = st.slices.get(i + 1).map_or(frames, |s| (s.start as usize).min(frames)).max(start);
                Span { start, next, reversed: st.slices[i].reversed() }
            })
            .collect();
        let mut keys = [None; 128];
        for (k, slot) in keys.iter_mut().enumerate() {
            *slot = st.slice_for(k as i32).map(|i| i as u16);
        }
        let art = (0..n).map(|i| ext.art_of(i) as u8).collect();
        Kit { sample, spans, keys, art, ext, b: None }
    }
}

/// The settings a block plays with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    /// Pitch shift, cents.
    pub cents: f64,
    pub attack_ms: f64,
    pub decay_ms: f64,
    pub declick: bool,
    /// PLAY TO END: a slice plays on to the sample's end.
    pub to_end: bool,
    /// The modulation pad (0..1), whether it moves smoothly, and a beat's length in seconds.
    pub mod_x: f32,
    pub mod_y: f32,
    pub smooth: bool,
    pub beat: f64,
    /// The master volume as a gain, and the master levels of the randomness and the LFOs.
    pub master: f32,
    pub rand_level: f32,
    pub lfo_level: f32,
}

impl Default for Block {
    fn default() -> Self {
        Block { cents: 0.0, attack_ms: 0.0, decay_ms: 4.0, declick: true, to_end: false, mod_x: 0.5, mod_y: 0.5, smooth: false, beat: 0.5, master: 1.0, rand_level: 1.0, lfo_level: 1.0 }
    }
}

/// Control values are worked out every this many output samples.
const CTRL: usize = 16;

#[derive(Clone, Debug)]
struct Voice {
    key: u8,
    id: Option<i32>,
    slice: usize,
    /// The deck the slice is of: 0 = A, 1 = B, and the gain its layering gives it.
    deck: u8,
    layer_gain: f32,
    art: usize,
    /// Frames into the slice (sample frames).
    pos: f64,
    gain: f32,
    /// A number picked for the note, 0..1.
    rand: f32,
    /// Seconds since the note began.
    t: f64,
    /// The filter's state: per channel and stage.
    filt: [[[f32; 2]; 3]; 2],
    /// Output samples left of the end fade, and its length.
    release: Option<(u32, u32)>,
    age: u64,
}

/// What an articulator's sources make of a voice's targets at one moment.
#[derive(Clone, Copy, Debug)]
struct Mods {
    pan: f32,
    gain: f32,
    cut: f32,
    res: f32,
    /// Octaves of speed on top of the pitch shift.
    speed: f32,
}

/// The sources' values for a voice at `t` seconds.
fn source(a: &Artic, target: Target, s: Source, t: f64, v: &Voice, b: &Block, mx: f32, my: f32) -> f32 {
    match s {
        Source::Env => a.env[target.index()].level(t, b.beat),
        Source::Lfo => a.lfo[target.index()].value(t, b.beat, v.rand),
        Source::Vel => v.gain,
        Source::ModX => mx,
        Source::ModY => my,
        Source::Rand => v.rand,
    }
}

fn modulate(a: &Artic, t: f64, v: &Voice, b: &Block, mx: f32, my: f32) -> Mods {
    // The master levels of the randomness and the LFOs scale what those sources add.
    let level = |s: Source| match s {
        Source::Lfo => b.lfo_level,
        Source::Rand => b.rand_level,
        _ => 1.0,
    };
    // A source's value as it moves a target: ENV, VEL, MOD and RAND run 0..1 (the pad's and RAND's from the middle),
    // the LFO -1..1.
    let signed = |tg: Target, s: Source| -> f32 {
        let x = source(a, tg, s, t, v, b, mx, my);
        match s {
            Source::Env | Source::Vel => x,
            Source::Lfo => x,
            Source::ModX | Source::ModY | Source::Rand => (x - 0.5) * 2.0,
        }
    };
    let sum = |tg: Target| -> f32 { crate::ext::ALL_SOURCES.iter().map(|&s| a.amount(tg, s) * level(s) * signed(tg, s)).sum() };
    // The volume scales: each source with an amount takes that share of the volume (its value) and leaves the rest.
    let mut gain = 1.0f32;
    for &s in &crate::ext::ALL_SOURCES {
        let amt = a.amount(Target::Vol, s) * level(s);
        if amt == 0.0 {
            continue;
        }
        let x = source(a, Target::Vol, s, t, v, b, mx, my);
        let x = if s == Source::Lfo { (x + 1.0) / 2.0 } else { x };
        gain *= 1.0 - amt.abs() + amt.abs() * if amt > 0.0 { x } else { 1.0 - x };
    }
    Mods {
        pan: (a.base[Target::Pan.index()] + sum(Target::Pan)).clamp(-1.0, 1.0),
        gain,
        cut: (a.cut + sum(Target::Cut)).clamp(0.0, 1.0),
        res: (a.res + sum(Target::Res)).clamp(0.0, 1.0),
        speed: sum(Target::Speed),
    }
}

/// One stage of a state-variable filter (the trapezoidal form): mode `mode` of `x` at cutoff `g` and damping `k`.
fn svf(st: &mut [f32; 2], x: f32, g: f32, k: f32, mode: u8) -> f32 {
    let a1 = 1.0 / (1.0 + g * (g + k));
    let a2 = g * a1;
    let a3 = g * a2;
    let v3 = x - st[1];
    let v1 = a1 * st[0] + a2 * v3;
    let v2 = st[1] + a2 * st[0] + a3 * v3;
    st[0] = 2.0 * v1 - st[0];
    st[1] = 2.0 * v2 - st[1];
    match mode {
        0 => v2,
        1 => v1,
        2 => x - k * v1,
        _ => x - k * v1 - v2,
    }
}

pub struct Engine {
    rate: f32,
    voices: Vec<Voice>,
    clock: u64,
    ended: Vec<(u8, Option<i32>)>,
    /// The pad's position as the voices hear it (it moves there smoothly).
    mx: f32,
    my: f32,
}

impl Engine {
    pub fn new(rate: f32) -> Self {
        Engine { rate: rate.max(1.0), voices: Vec::with_capacity(VOICES), clock: 0, ended: Vec::new(), mx: 0.5, my: 0.5 }
    }

    pub fn set_rate(&mut self, rate: f32) {
        self.rate = rate.max(1.0);
    }

    pub fn reset(&mut self) {
        self.voices.clear();
    }

    pub fn has_voices(&self) -> bool {
        !self.voices.is_empty()
    }

    pub fn sounding(&self) -> impl Iterator<Item = u8> + '_ {
        self.voices.iter().filter(|v| v.release.is_none()).map(|v| v.key)
    }

    /// Voices that ended since the last call (key, host voice id).
    pub fn take_ended(&mut self) -> Vec<(u8, Option<i32>)> {
        std::mem::take(&mut self.ended)
    }

    /// Starts the voices for `key`: the slice it plays on each deck that sounds; with both, the layering mode and the
    /// crossfade curve give each its gain. Keys without a slice are silent.
    pub fn note_on(&mut self, kit: Option<&Kit>, key: u8, velocity: f32, id: Option<i32>) {
        let Some(kit) = kit else { return };
        let ext = &kit.ext;
        let a = kit.keys.get(usize::from(key)).copied().flatten();
        let b = kit.b.as_ref().and_then(|d| {
            let s = if ext.same_deck { a } else { d.keys.get(usize::from(key)).copied().flatten() };
            s.filter(|&i| usize::from(i) < d.spans.len())
        });
        let (a, b) = (a.filter(|_| ext.a_on), b.filter(|_| ext.b_on));
        self.clock += 1;
        // A number for the note: a hash of the note's count.
        let h = (self.clock as u32).wrapping_mul(0x9e37_79b9) ^ u32::from(key).wrapping_mul(0x85eb_ca6b);
        let rand = ((h.wrapping_mul(0x2c1b_3c6d) >> 8) as f32) / (1u32 << 24) as f32;
        let (ga, gb) = if a.is_some() && b.is_some() {
            let x = match ext.layer {
                2 => velocity,
                3 => self.mx,
                4 => self.my,
                5 => rand,
                6 => (self.clock % 2) as f32,
                _ => 0.5,
            };
            ext.layer_gains(x)
        } else {
            (1.0, 1.0)
        };
        for (deck, slice, g) in [(0u8, a, ga), (1u8, b, gb)] {
            if let Some(slice) = slice {
                self.start(kit, deck, key, slice, velocity, id, rand, g);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn start(&mut self, kit: &Kit, deck: u8, key: u8, slice: u16, velocity: f32, id: Option<i32>, rand: f32, layer_gain: f32) {
        if self.voices.len() >= VOICES
            && let Some(i) = (0..self.voices.len()).min_by_key(|&i| self.voices[i].age)
        {
            let v = self.voices.remove(i);
            self.ended.push((v.key, v.id));
        }
        let art = usize::from(kit.ext.art_of(usize::from(slice))).min(ARTICS - 1);
        self.voices.push(Voice { key, id, slice: usize::from(slice), deck, layer_gain, art, pos: 0.0, gain: velocity.clamp(0.0, 1.0), rand, t: 0.0, filt: [[[0.0; 2]; 3]; 2], release: None, age: self.clock });
    }

    /// The note ends: its voices fade out (over the volume envelope's release when it shapes the volume).
    pub fn note_off(&mut self, kit: Option<&Kit>, key: u8, id: Option<i32>, b: &Block) {
        for v in &mut self.voices {
            if v.key == key && v.release.is_none() && (id.is_none() || v.id.is_none() || v.id == id) {
                let a = kit.map(|k| &k.ext.artic[v.art.min(ARTICS - 1)]);
                let rel = a.filter(|a| a.amount(Target::Vol, Source::Env) != 0.0).map_or(0.0, |a| a.env[Target::Vol.index()].release(b.beat));
                let n = ((rel.max(RELEASE_S) + if b.declick { DECLICK_S } else { 0.0 }) * f64::from(self.rate)).round().max(1.0) as u32;
                v.release = Some((n, n));
            }
        }
    }

    /// Every voice stops at once.
    pub fn all_off(&mut self) {
        for v in self.voices.drain(..) {
            self.ended.push((v.key, v.id));
        }
    }

    /// Adds the voices into `left` / `right`.
    pub fn render(&mut self, kit: Option<&Kit>, left: &mut [f32], right: &mut [f32], b: &Block) {
        let Some(kit) = kit else {
            self.all_off();
            return;
        };
        let out_rate = f64::from(self.rate);
        let n = left.len().min(right.len());
        // The pad as the voices hear it.
        if b.smooth {
            self.mx += (b.mod_x - self.mx) * 0.25;
            self.my += (b.mod_y - self.my) * 0.25;
        } else {
            (self.mx, self.my) = (b.mod_x, b.mod_y);
        }
        let (mx, my) = (self.mx, self.my);
        let mut i = 0;
        while i < self.voices.len() {
            let v = &mut self.voices[i];
            let Some((s, spans)) = kit.deck(v.deck) else {
                v.pos = f64::MAX;
                i += 1;
                continue;
            };
            let Some(span) = spans.get(v.slice).copied() else {
                v.pos = f64::MAX;
                i += 1;
                continue;
            };
            let sr = f64::from(s.rate.max(1));
            let base_step = sr / out_rate * (b.cents / 1200.0).exp2();
            let (att, dec) = (b.attack_ms / 1000.0 * sr, b.decay_ms / 1000.0 * sr);
            let click = if b.declick { DECLICK_S * sr } else { 0.0 };
            let frames = s.frames();
            let fade = v.layer_gain;
            let art = &kit.ext.artic[v.art.min(ARTICS - 1)];
            let end = if b.to_end { frames } else { span.next };
            let len = end.saturating_sub(span.start) as f64;
            let filtered = art.mode < FILTER_OFF;
            let mut at = 0;
            'voice: while at < n {
                let m = modulate(art, v.t, v, b, mx, my);
                let step = base_step * f64::from(m.speed).exp2();
                // The filter's cutoff (20 Hz .. 20 kHz) and damping for the next stretch.
                let fc = (20.0 * 1000f32.powf(m.cut)).min(out_rate as f32 * 0.45);
                let g = (std::f32::consts::PI * fc / out_rate as f32).tan();
                let k = 2.0 - 1.95 * m.res;
                let (pl, pr) = ((1.0 - m.pan).min(1.0), (1.0 + m.pan).min(1.0));
                for j in at..(at + CTRL).min(n) {
                    if v.pos >= len - 1.0 {
                        v.pos = len;
                        break 'voice;
                    }
                    let mut g_out = m.gain * fade * b.master;
                    let left_of = len - v.pos;
                    for (fade, x) in [(att, v.pos), (dec, left_of), (click, v.pos), (click, left_of)] {
                        if fade > 0.0 && x < fade {
                            g_out *= (x / fade) as f32;
                        }
                    }
                    if let Some((n, total)) = v.release.as_mut() {
                        if *n == 0 {
                            v.pos = len;
                            break 'voice;
                        }
                        g_out *= *n as f32 / *total as f32;
                        *n -= 1;
                    }
                    // Reversed: from the slice's last frame back to its first.
                    let x = if span.reversed { (end as f64 - 1.0 - v.pos).max(span.start as f64) } else { span.start as f64 + v.pos };
                    let k0 = x.floor() as usize;
                    let f = (x - k0 as f64) as f32;
                    let at_ = |ch: &[f32], q: usize| ch.get(q.min(end.saturating_sub(1))).copied().unwrap_or(0.0);
                    let (l0, r0) = (at_(&s.left, k0), at_(&s.right, k0));
                    let (l1, r1) = (at_(&s.left, k0 + 1), at_(&s.right, k0 + 1));
                    let (mut sl, mut sr_) = (l0 + (l1 - l0) * f, r0 + (r1 - r0) * f);
                    if filtered {
                        for stage in 0..usize::from(art.slope) {
                            sl = svf(&mut v.filt[0][stage], sl, g, k, art.mode);
                            sr_ = svf(&mut v.filt[1][stage], sr_, g, k, art.mode);
                        }
                    }
                    left[j] += g_out * pl * sl;
                    right[j] += g_out * pr * sr_;
                    v.pos += step;
                    v.t += 1.0 / out_rate;
                }
                at += CTRL;
            }
            i += 1;
        }
        let ended = &mut self.ended;
        self.voices.retain(|v| {
            let len = kit.deck(v.deck).and_then(|(s, spans)| spans.get(v.slice).map(|sp| ((if b.to_end { s.frames() } else { sp.next }).saturating_sub(sp.start)) as f64)).unwrap_or(0.0);
            let done = v.pos >= len - 1.0 || v.release.is_some_and(|(n, _)| n == 0);
            if done {
                ended.push((v.key, v.id));
            }
            !done
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Slice;

    /// A sample whose frame `i` is `i / 1000` (left) and `-i / 1000` (right), at 1000 Hz, cut at 0, 100, 300.
    fn kit() -> Kit {
        let left: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let right = left.iter().map(|v| -v).collect();
        let mut st = State::default();
        st.slices = vec![Slice::at(0), Slice::at(100), Slice::at(300)];
        st.slices[2].flags = 1;
        Kit::new(Arc::new(Sample { rate: 1000, left, right }), &st, Arc::new(Ext::default()))
    }

    fn plain() -> Block {
        Block { cents: 0.0, attack_ms: 0.0, decay_ms: 0.0, declick: false, to_end: false, ..Block::default() }
    }

    fn play(e: &mut Engine, k: &Kit, n: usize, b: &Block) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        e.render(Some(k), &mut l, &mut r, b);
        l
    }

    #[test]
    fn a_key_plays_its_slice_to_the_next_start() {
        let k = kit();
        assert_eq!((k.keys[60], k.keys[61], k.keys[62], k.keys[63]), (Some(0), Some(1), Some(2), None));
        let mut e = Engine::new(1000.0);
        e.note_on(Some(&k), 61, 1.0, None);
        let out = play(&mut e, &k, 250, &plain());
        assert!((out[0] - 0.1).abs() < 1e-6 && (out[10] - 0.11).abs() < 1e-6);
        // Frames 100..300, the last one excepted (the interpolation needs the next frame): 199 output samples.
        assert!(out[198] > 0.29 && out[199] == 0.0, "{} {}", out[198], out[199]);
        assert!(!e.has_voices());
        assert_eq!(e.take_ended(), vec![(61, None)]);
        // Keys without a slice are silent.
        e.note_on(Some(&k), 63, 1.0, None);
        assert!(!e.has_voices());
    }

    #[test]
    fn reversed_slices_play_backwards_and_pitch_changes_the_speed() {
        let k = kit();
        let mut e = Engine::new(1000.0);
        e.note_on(Some(&k), 62, 0.5, None);
        let out = play(&mut e, &k, 4, &plain());
        assert!((out[0] - 0.5 * 0.999).abs() < 1e-6 && (out[1] - 0.5 * 0.998).abs() < 1e-6, "{out:?}");
        let mut e = Engine::new(1000.0);
        e.note_on(Some(&k), 60, 1.0, None);
        let out = play(&mut e, &k, 4, &Block { cents: 1200.0, ..plain() });
        assert!((out[1] - 0.002).abs() < 1e-6 && (out[2] - 0.004).abs() < 1e-6, "an octave up reads two frames a sample");
    }

    #[test]
    fn fades_and_note_ends() {
        let k = kit();
        let mut e = Engine::new(1000.0);
        e.note_on(Some(&k), 61, 1.0, None);
        // Attack 10 ms: half way up after 5 frames.
        let out = play(&mut e, &k, 6, &Block { attack_ms: 10.0, ..plain() });
        assert!((out[5] - 0.105 * 0.5).abs() < 1e-6, "{}", out[5]);
        // The note ends: 5 ms (5 samples at 1 kHz) to silence.
        e.note_off(Some(&k), 61, None, &plain());
        let out = play(&mut e, &k, 8, &plain());
        assert!(out[0] > 0.0 && out[5] == 0.0, "{out:?}");
        assert!(!e.has_voices());
        // PLAY TO END: slice 0 plays on past 100.
        e.note_on(Some(&k), 60, 1.0, None);
        let out = play(&mut e, &k, 400, &Block { to_end: true, ..plain() });
        assert!(out[350] > 0.34);
    }

    #[test]
    fn the_oldest_voice_gives_way() {
        let k = kit();
        let mut e = Engine::new(1000.0);
        for _ in 0..VOICES + 1 {
            e.note_on(Some(&k), 60, 1.0, None);
        }
        assert_eq!(e.voices.len(), VOICES);
        assert_eq!(e.take_ended().len(), 1);
    }

    /// A kit of one slice of a square-ish wave, played through articulator 0 as `set` makes it.
    fn art_kit(set: impl Fn(&mut Artic)) -> Kit {
        let left: Vec<f32> = (0..2000).map(|i| if i % 2 == 0 { 0.8 } else { -0.8 }).collect();
        let right = left.clone();
        let mut st = State::default();
        st.slices = vec![Slice::at(0)];
        let mut ext = Ext::default();
        set(&mut ext.artic[0]);
        Kit::new(Arc::new(Sample { rate: 1000, left, right }), &st, Arc::new(ext))
    }

    fn peak(v: &[f32]) -> f32 {
        v.iter().fold(0.0f32, |m, x| m.max(x.abs()))
    }

    /// The articulator's filter takes a fast wave away (low pass), keeps it (high pass) or does nothing (off).
    #[test]
    fn the_filter_passes_what_its_mode_says() {
        let run = |mode: u8, cut: f32| {
            let k = art_kit(|a| {
                a.mode = mode;
                a.cut = cut;
            });
            let mut e = Engine::new(1000.0);
            e.note_on(Some(&k), 60, 1.0, None);
            let o = play(&mut e, &k, 600, &plain());
            peak(&o[300..])
        };
        assert!(run(0, 0.05) < 0.05, "a low pass at 25 Hz stops a 500 Hz wave");
        assert!(run(3, 0.05) > 0.5, "a high pass keeps it");
        assert!(run(4, 0.05) > 0.7, "off keeps it all");
    }

    /// Pan, the volume's sources and the pad each move what they should.
    #[test]
    fn sources_move_the_targets() {
        let both = |k: &Kit, b: &Block| {
            let mut e = Engine::new(1000.0);
            e.note_on(Some(k), 60, 1.0, None);
            let (mut l, mut r) = (vec![0.0; 100], vec![0.0; 100]);
            e.render(Some(k), &mut l, &mut r, b);
            (peak(&l[10..]), peak(&r[10..]))
        };
        let k = art_kit(|a| a.base[Target::Pan.index()] = 1.0);
        let (l, r) = both(&k, &plain());
        assert!(l < 1e-6 && r > 0.7, "panned right: {l} {r}");
        // The pad's X lowers the volume: amount 1 on MOD X scales by the pad's value.
        let k = art_kit(|a| a.set_amount(Target::Vol, Source::ModX, 1.0));
        let (l0, _) = both(&k, &Block { mod_x: 0.0, ..plain() });
        let (l1, _) = both(&k, &Block { mod_x: 1.0, ..plain() });
        assert!(l0 < 1e-6 && l1 > 0.7, "{l0} {l1}");
        // A volume envelope with an attack starts silent.
        let k = art_kit(|a| a.env[Target::Vol.index()].att = 50.0);
        let mut e = Engine::new(1000.0);
        e.note_on(Some(&k), 60, 1.0, None);
        let o = play(&mut e, &k, 100, &plain());
        assert!(o[0].abs() < 0.05 && peak(&o[60..]) > 0.7);
    }
}
