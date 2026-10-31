// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Beat's sound: a tape-style time section and a gate, both driven by the envelope grids.
//!
//! The time section writes the input into a ring and reads it back at the rate the time grid asks for: the read
//! head falls behind at slow rates, catches up at fast ones and walks backwards under the grid's centre, so
//! half-time, double-time, tape-stop, reverse and scratch are all one mechanism. The volume section multiplies the
//! sound by the volume grid, smoothed a half millisecond so gates do not click. With no grid in a section the
//! section is transparent, so a fresh Mango Beat passes the sound through untouched.
//!
// spec: plugins/mango-beat.md (§3 sound)

use crate::grid::{GridState, Mode, Pt};

/// The history's length per channel, in frames: a power of two (2^20, about 21 s at 44.1 kHz; 8 MB of ring).
const HISTORY: usize = 1 << 20;
/// The read head keeps this far behind the write head at the least (the interpolator reads one older frame) and
/// the most (past it the sound has run out of past and stalls until the rate picks up).
const MIN_D: f64 = 1.0;
const MAX_D: f64 = (HISTORY - 2) as f64;
/// The gate's de-click: the smoothing time in seconds (a half millisecond a gate edge takes to close).
const SMOOTH_S: f32 = 0.0005;

/// The transport position a block starts at (`playing` and the song's beats); `None` plays nothing new.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportStep {
    pub playing: bool,
    pub beats: f64,
}

/// The settings the sound takes from the parameters and the grids, all at once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// The time section's mix (0..1): dry sound against the re-read.
    pub time_mix: f32,
    /// The volume section's mix (0..1): the sound against the gated sound.
    pub vol_mix: f32,
    /// The time slot (0..4) and the volume slot (0..4) playing.
    pub time_slot: usize,
    pub vol_slot: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { time_mix: 1.0, vol_mix: 1.0, time_slot: 0, vol_slot: 0 }
    }
}

/// One Mango Beat voice of sound.
pub struct Beat {
    rate: u32,
    bpm: f64,
    settings: Settings,
    grids: GridState,
    /// The ring: the past of the sound.
    hist_l: Vec<f32>,
    hist_r: Vec<f32>,
    /// The next write position in the ring.
    write: usize,
    /// The read head's distance behind the write head, in frames.
    d: f64,
    /// Where in the beat the envelopes are (0..1).
    phase: f64,
    /// The transport: playing, and the beats the next block should start at (jumps reposition the envelopes).
    playing: bool,
    next_beats: Option<f64>,
    /// The smoothed gate gain.
    gain: f32,
    /// The smoothing coefficient from the rate (made in `set_sample_rate`).
    smooth_k: f32,
}

impl Default for Beat {
    fn default() -> Self {
        Beat::new(44100)
    }
}

impl Beat {
    pub fn new(rate: u32) -> Beat {
        let mut b = Beat {
            rate: 44100,
            bpm: 130.0,
            settings: Settings::default(),
            grids: GridState::default(),
            hist_l: vec![0.0; HISTORY],
            hist_r: vec![0.0; HISTORY],
            write: 0,
            d: MIN_D,
            phase: 0.0,
            playing: false,
            next_beats: None,
            gain: 1.0,
            smooth_k: 0.0,
        };
        b.set_sample_rate(rate);
        b
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate.max(1);
        self.smooth_k = 1.0 - f32::exp(-1.0 / (SMOOTH_S * self.rate as f32));
    }

    pub fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 {
            self.bpm = bpm;
        }
    }

    pub fn set(&mut self, s: &Settings) {
        self.settings = *s;
    }

    /// The grids moved (the editor edited, a preset or a project loaded): the sound plays them from the next block.
    pub fn set_grids(&mut self, g: GridState) {
        self.grids = g;
    }

    /// Empties the history and the heads: the next block starts from nothing.
    pub fn reset(&mut self) {
        self.hist_l.fill(0.0);
        self.hist_r.fill(0.0);
        self.write = 0;
        self.d = MIN_D;
        self.phase = 0.0;
        self.next_beats = None;
        self.gain = 1.0;
    }

    /// The rate the time grid asks for at `phase`: an empty grid is the sound's own speed.
    fn rate_at(&self, phase: f64) -> f64 {
        match self.grids.time[self.settings.time_slot.min(3)].value_at(phase) {
            Some(y) => f64::from(GridState::rate_of_y(y)),
            None => 1.0,
        }
    }

    /// The gain the volume grid asks for at `phase`: an empty grid is wide open.
    pub(crate) fn gain_at(&self, phase: f64) -> f32 {
        match self.grids.volume[self.settings.vol_slot.min(3)].value_at(phase) {
            Some(y) => y.clamp(0.0, 1.0),
            None => 1.0,
        }
    }

    /// One block, in place, planar stereo. `transport` is the host's position at the block's start (`None`: the
    /// host says nothing, the envelopes free-run).
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], transport: Option<TransportStep>) {
        let frames = l.len().min(r.len());
        let Some(ts) = transport else { return self.process_free(l, r, frames) };
        let bps = if self.bpm > 0.0 { self.bpm / 60.0 / f64::from(self.rate) } else { 0.0 };
        if ts.playing {
            // A start or a jump puts the read head back at the present (the envelopes follow the host's beats).
            let jumped = !self.playing || self.next_beats.is_none_or(|n| (n - ts.beats).abs() > 1e-3);
            if jumped {
                self.d = MIN_D;
            }
            self.playing = true;
            self.next_beats = Some(ts.beats + frames as f64 * bps);
            self.run(l, r, frames, ts.beats.rem_euclid(1.0), bps);
        } else {
            self.playing = false;
            self.next_beats = None;
            self.run(l, r, frames, self.phase, bps);
        }
    }

    /// Without a transport word the envelopes keep running on their own (the tail after a stop).
    fn process_free(&mut self, l: &mut [f32], r: &mut [f32], frames: usize) {
        let bps = if self.bpm > 0.0 { self.bpm / 60.0 / f64::from(self.rate) } else { 0.0 };
        self.run(l, r, frames, self.phase, bps);
    }

    fn run(&mut self, l: &mut [f32], r: &mut [f32], frames: usize, mut phase: f64, bps: f64) {
        let mask = HISTORY - 1;
        let (tm, vm) = (self.settings.time_mix.clamp(0.0, 1.0), self.settings.vol_mix.clamp(0.0, 1.0));
        let time_on = !self.grids.time[self.settings.time_slot.min(3)].pts.is_empty() && tm > 0.0;
        let vol_on = !self.grids.volume[self.settings.vol_slot.min(3)].pts.is_empty() && vm > 0.0;
        for k in 0..frames {
            // The envelopes at this frame.
            if bps > 0.0 {
                phase += bps;
                phase = phase.rem_euclid(1.0);
            }
            let rate = if time_on { self.rate_at(phase) } else { 1.0 };
            let target = if vol_on { self.gain_at(phase) } else { 1.0 };
            // The gate glides to its target so steps do not click.
            self.gain += (target - self.gain) * self.smooth_k;
            let (il, ir) = (l[k], r[k]);
            // The past keeps growing whatever the sections do, so switching a section on mid-sound has material.
            self.hist_l[self.write] = il;
            self.hist_r[self.write] = ir;
            self.write = (self.write + 1) & mask;
            // The read head: slower rates fall behind, faster catch up, backwards walk further back.
            if time_on {
                self.d += 1.0 - rate;
                self.d = self.d.clamp(MIN_D, MAX_D);
            } else {
                self.d = MIN_D;
            }
            let (mut out_l, mut out_r) = if time_on {
                let j = self.d.floor() as usize;
                let f = (self.d - j as f64) as f32;
                let i0 = (self.write.wrapping_sub(j)) & mask;
                let i1 = (i0 + HISTORY - 1) & mask;
                let w0 = 1.0 - f;
                (
                    self.hist_l[i0] * w0 + self.hist_l[i1] * f,
                    self.hist_r[i0] * w0 + self.hist_r[i1] * f,
                )
            } else {
                (il, ir)
            };
            if vol_on {
                out_l *= self.gain;
                out_r *= self.gain;
            }
            // The sections' mixes.
            l[k] = il + (out_l - il) * tm;
            r[k] = ir + (out_r - ir) * vm;
        }
        self.phase = phase;
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Presets' grids (shared with the factory preset generator)

/// A time grid that plays at one rate everywhere (`rate` in x, 0.375 = half).
pub fn constant_time(rate: f32) -> crate::grid::Curve {
    crate::grid::Curve::constant(GridState::y_of_rate(rate))
}

/// A gate grid of `steps` equal on-off steps per beat (1 = an eighth-note gate at steps 2).
pub fn gate(steps: usize) -> crate::grid::Curve {
    let hold = Mode::Hold as u8;
    let mut pts = Vec::new();
    let step = 1.0 / steps.max(1) as f32;
    for k in 0..steps {
        pts.push(Pt::new(step * k as f32, 1.0, 0.0, hold));
        pts.push(Pt::new(step * (k as f32 + 0.5), 0.0, 0.0, hold));
    }
    crate::grid::Curve { pts }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Curve, GridState};

    const RATE: u32 = 48000;

    fn beat() -> Beat {
        let mut b = Beat::new(RATE);
        b.set_tempo(120.0);
        b
    }

    /// A fresh Mango Beat with no grids is the sound itself, sample for sample.
    #[test]
    fn empty_grids_pass_the_sound_through() {
        let mut b = beat();
        let mut l: Vec<f32> = (0..256).map(|i| (i as f32 * 0.013).sin()).collect();
        let mut r: Vec<f32> = (0..256).map(|i| (i as f32 * 0.029).sin()).collect();
        let (il, ir) = (l.clone(), r.clone());
        b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats: 0.0 }));
        assert_eq!(l, il);
        assert_eq!(r, ir);
    }

    /// Half time plays the past at half speed: a rising ramp comes out rising half as fast (each input frame lands
    /// across two output frames, within the linear interpolation's reach).
    #[test]
    fn half_time_plays_half_as_fast() {
        let mut b = beat();
        let mut g = GridState::default();
        g.time[0] = constant_time(0.5);
        b.set_grids(g);
        let n = 512;
        let mut l: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();
        let mut r = vec![0.0; n];
        b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats: 0.0 }));
        // After a moment the output rises at about half the input's slope.
        let slope = (l[400] - l[300]) / 100.0;
        assert!(slope > 0.0006 && slope < 0.0014, "slope {slope}");
    }

    /// Double time catches up with the past at twice the speed: after a slow passage has built the head up, a
    /// fast grid drains it at twice the input's rate (a live sound itself cannot be overtaken, so the test first
    /// runs half time to fill the past).
    #[test]
    fn double_time_plays_twice_as_fast() {
        let mut b = beat();
        let mut g = GridState::default();
        g.time[0] = constant_time(0.5);
        b.set_grids(g);
        let n = 512;
        let mut l: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();
        let mut r = vec![0.0; n];
        b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats: 0.0 }));
        // Now the fast grid: the head runs down its backlog at twice the input's rate.
        let mut g = GridState::default();
        g.time[0] = constant_time(2.0);
        b.set_grids(g);
        let mut l: Vec<f32> = (n..2 * n).map(|i| i as f32 / n as f32).collect();
        let mut r = vec![0.0; n];
        // The beats continue exactly where the first block said (a jump would reposition the head).
        let bps = 1.0 / 24000.0;
        b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats: n as f64 * bps }));
        let slope = (l[250] - l[50]) / 200.0;
        assert!(slope > 0.0030 && slope < 0.0050, "slope {slope}");
    }

    /// A backwards rate walks the read head back through what just played: after half a beat at the sound's own
    /// speed, the reverse half of the grid plays it descending.
    #[test]
    fn backwards_rates_play_the_past_in_reverse() {
        let mut b = beat();
        let mut g = GridState::default();
        let rev = GridState::y_of_rate(-1.0);
        g.time[0] = Curve {
            pts: vec![Pt::new(0.0, 0.5, 0.0, Mode::Single as u8), Pt::new(0.5, rev, 0.0, Mode::Hold as u8), Pt::new(1.0, rev, 0.0, Mode::Hold as u8)],
        };
        b.set_grids(g);
        let n = 24000; // a beat and a quarter at 120 bpm and 48 kHz
        let mut l: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();
        let mut r = vec![0.0; n];
        b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats: 0.0 }));
        // Half a beat in, the grid turns around: the output descends through the recent past.
        let slope = (l[14000] - l[12500]) / 1500.0;
        assert!(slope < -0.00003, "slope {slope}");
    }

    /// A gate closes and opens where its points say, smoothly (no step wider than the smoothing).
    #[test]
    fn the_gate_closes_where_its_points_say() {
        let mut b = beat();
        let mut g = GridState::default();
        g.volume[0] = Curve { pts: vec![Pt::new(0.0, 1.0, 0.0, Mode::Hold as u8), Pt::new(0.5, 0.0, 0.0, Mode::Hold as u8)] };
        b.set_grids(g);
        let n = 1024; // 120 bpm at 48 kHz: one beat is 24000 frames; 1024 frames is 4.3 % of the beat
        let mut l = vec![1.0; n];
        let mut r = vec![1.0; n];
        b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats: 0.0 }));
        // The whole block sits in the open half: nothing was gated yet.
        assert!(l.iter().all(|v| *v > 0.99));
        // From beat 0.5 on the gate is closed: start a block there.
        let mut l = vec![1.0; n];
        let mut r = vec![1.0; n];
        b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats: 0.5 }));
        assert!(*l.last().unwrap() < 0.01, "closed by the end: {}", l.last().unwrap());
    }

    /// A jump puts the read head back at the present: the first frames after the jump read what just played, not
    /// the deep past the slow rate had run into.
    #[test]
    fn a_jump_repositions_the_read_head() {
        let mut b = beat();
        let mut g = GridState::default();
        g.time[0] = constant_time(0.5);
        b.set_grids(g);
        let n = 480;
        let mut l: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();
        let mut r = vec![0.0; n];
        b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats: 0.0 }));
        assert!(b.d > 50.0, "half time ran the head into the past: {}", b.d);
        // The jump: from a fresh position the first output frame is the sound of a moment ago.
        let mut l = vec![0.5; n];
        let mut r = vec![0.0; n];
        b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats: 10.0 }));
        // With the reset the head reads the present (between the last of the old sound and the new one); deep in
        // the past it would sit near 0.49.
        assert!(l[0] > 0.6, "the head reset to the present: {}", l[0]);
    }

    /// The tail free-runs without a transport word and stays finite (a stopped host still drains the ring).
    #[test]
    fn the_tail_free_runs_and_stays_finite() {
        let mut b = beat();
        let mut g = GridState::default();
        g.time[0] = constant_time(0.5);
        g.volume[0] = gate(2);
        b.set_grids(g);
        let n = 256;
        let mut l: Vec<f32> = vec![0.5; n];
        let mut r = vec![0.0; n];
        b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats: 0.0 }));
        for _ in 0..8 {
            let mut l = vec![0.0; n];
            let mut r = vec![0.0; n];
            b.process(&mut l, &mut r, None);
            assert!(l.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        }
    }

    /// Random grids at random phases never leave the output range or make a NaN.
    #[test]
    fn random_grids_stay_finite() {
        let mut b = beat();
        let mut state: u64 = 0x9E3779B97F4A7C15;
        let mut rand = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 40) as f32) / (1u32 << 24) as f32
        };
        let mut g = GridState::default();
        for slot in 0..4 {
            for (sec, hi) in [(0, 1.0f32), (1, 1.0)] {
                let n = 1 + (rand() * 5.0) as usize;
                let pts: Vec<Pt> = (0..n).map(|k| Pt::new(k as f32 / n as f32, rand() * hi, rand() * 2.0 - 1.0, (rand() * 13.0) as u8)).collect();
                let mut c = Curve { pts };
                c.normalize();
                if sec == 0 {
                    g.time[slot] = c;
                } else {
                    g.volume[slot] = c;
                }
            }
        }
        b.set_grids(g);
        let n = 2048;
        let mut l: Vec<f32> = (0..n).map(|_| rand() * 2.0 - 1.0).collect();
        let mut r = l.clone();
        for k in 0..4 {
            let beats = f64::from(k) * 0.37 + 0.11;
            b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats }));
            assert!(l.iter().all(|v| v.is_finite() && v.abs() <= 1.001), "frame range at {k}");
            assert!(r.iter().all(|v| v.is_finite()));
        }
    }

    /// The mixes: a time mix of 0 leaves the timing alone even with a grid; a volume mix of 0 leaves the level.
    #[test]
    fn the_mixes_fade_their_sections_out() {
        let mut b = beat();
        let mut g = GridState::default();
        g.time[0] = constant_time(0.5);
        g.volume[0] = gate(2);
        b.set_grids(g);
        b.set(&Settings { time_mix: 0.0, vol_mix: 0.0, ..Settings::default() });
        let n = 256;
        let mut l: Vec<f32> = vec![0.6; n];
        let mut r = vec![0.0; n];
        let (il, ir) = (l.clone(), r.clone());
        b.process(&mut l, &mut r, Some(TransportStep { playing: true, beats: 0.0 }));
        assert_eq!(l, il);
        assert_eq!(r, ir);
    }

    /// A gate of two equal steps makes the eighth-note pattern the classic trance gate rides on.
    #[test]
    fn gate_steps_alternate() {
        let g = gate(2);
        assert_eq!(g.pts.len(), 4);
        assert!((g.value_at(0.1).unwrap() - 1.0).abs() < 1e-6, "open on the first half of the step");
        assert_eq!(g.value_at(0.3), Some(0.0), "closed on the second half");
    }
}

