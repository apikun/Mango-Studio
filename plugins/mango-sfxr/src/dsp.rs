// Copyright (C) 2026 mikobuntu
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Sfxr's synthesis: a Rust port of sfxr, drPetter's retro sound effect generator, following
//! LMMS's port of it (the Sfxr instrument, Wong Cho Ching) for the parameter scalings and the preset
//! generators.
//!
//! sfxr, the algorithm this file ports, is by Tomas "DrPetter" Pettersson
//! (http://www.drpetter.se/project_sfxr.html), MIT licensed. Its notice, which its license asks to
//! keep with substantial portions of it, follows:
//!
//! ```text
//! Copyright (c) 2007 Tomas Pettersson
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
//! associated documentation files (the "Software"), to deal in the Software without restriction,
//! including without limitation the rights to use, copy, modify, merge, publish, distribute,
//! sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all copies or
//! substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
//! NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
//! NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
//! DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//! ```
//!
//! The LMMS port (plugins/Sfxr) is GPL-2.0-or-later, so its contributions fold into this
//! GPL-3.0-or-later file.
//!
//! How the sound works (the sfxr readme tells it best): a phase walks a period (the pitch); each
//! period break may refill the noise table. The waveform (square with a swept duty cycle, saw,
//! half-cycle sine, or noise) runs through a resonant lowpass and a highpass, a phaser mixes a
//! delayed copy back in, and a three stage envelope (attack, sustain with punch, decay) shapes the
//! volume. [`Synth::next`] is that whole pipeline for one virtual 44.1 kHz sample; [`Engine`] plays
//! several of them at once, each reading its virtual samples at the played key's rate, the way LMMS's
//! pitched playback does (a 440 Hz key plays the effect at its natural speed).

use std::time::{SystemTime, UNIX_EPOCH};

/// The sample rate the effect is synthesized at, whatever the host's is (as LMMS's virtual clock).
pub const VIRTUAL_RATE: f32 = 44_100.0;

/// The key that plays an effect at its natural speed (A4, LMMS's base note).
pub const REFERENCE_HZ: f32 = 440.0;

/// A patch: the raw sfxr slider values (`0..1`, or `-1..1` where the slider goes negative), plus the
/// waveform and the output volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patch {
    pub wave: u8,
    pub att: f32,
    pub hold: f32,
    pub sus: f32,
    pub dec: f32,
    pub start_freq: f32,
    pub min_freq: f32,
    pub slide: f32,
    pub dslide: f32,
    pub vib_depth: f32,
    pub vib_speed: f32,
    pub change_amt: f32,
    pub change_speed: f32,
    pub sqr_duty: f32,
    pub sqr_sweep: f32,
    pub repeat: f32,
    pub phaser_offset: f32,
    pub phaser_sweep: f32,
    pub lp_cut: f32,
    pub lp_sweep: f32,
    pub lp_reso: f32,
    pub hp_cut: f32,
    pub hp_sweep: f32,
    pub volume: f32,
}

impl Default for Patch {
    /// The patch a fresh plugin sounds (LMMS's model defaults; LMMS has no volume control).
    fn default() -> Self {
        Patch {
            wave: 0,
            att: 0.0,
            hold: 0.3,
            sus: 0.0,
            dec: 0.4,
            start_freq: 0.3,
            min_freq: 0.0,
            slide: 0.0,
            dslide: 0.0,
            vib_depth: 0.0,
            vib_speed: 0.0,
            change_amt: 0.0,
            change_speed: 0.0,
            sqr_duty: 0.0,
            sqr_sweep: 0.0,
            repeat: 0.0,
            phaser_offset: 0.0,
            phaser_sweep: 0.0,
            lp_cut: 1.0,
            lp_sweep: 0.0,
            lp_reso: 0.0,
            hp_cut: 0.0,
            hp_sweep: 0.0,
            volume: 1.0,
        }
    }
}

impl Patch {
    /// The patch's value of slider `id` (the ids of [`crate::params::SLIDER_IDS`]; wave is not one).
    pub fn get(&self, id: &str) -> f32 {
        match id {
            "att" => self.att,
            "hold" => self.hold,
            "sus" => self.sus,
            "dec" => self.dec,
            "start_freq" => self.start_freq,
            "min_freq" => self.min_freq,
            "slide" => self.slide,
            "dslide" => self.dslide,
            "vib_depth" => self.vib_depth,
            "vib_speed" => self.vib_speed,
            "change_amt" => self.change_amt,
            "change_speed" => self.change_speed,
            "sqr_duty" => self.sqr_duty,
            "sqr_sweep" => self.sqr_sweep,
            "repeat" => self.repeat,
            "phaser_offset" => self.phaser_offset,
            "phaser_sweep" => self.phaser_sweep,
            "lp_cut" => self.lp_cut,
            "lp_sweep" => self.lp_sweep,
            "lp_reso" => self.lp_reso,
            "hp_cut" => self.hp_cut,
            "hp_sweep" => self.hp_sweep,
            "volume" => self.volume,
            _ => 0.0,
        }
    }
}

/// LMMS's `fastRand` (a plain linear congruential generator), so the port rolls like the original.
///
/// `inc(from, to)` is `fastRandInc`: uniform `from..=to`, ends included (its scale divides by 32767).
/// `one_in(n)` is `oneIn`: true with probability one in n. A generator rolls the same sequence from
/// the same state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng(u64);

impl Rng {
    pub fn new(state: u64) -> Self {
        Rng(state)
    }

    /// A seed from the clock (a fresh roll per note does not want yesterday's numbers).
    pub fn from_clock() -> Self {
        let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x2545_f491_4f6c_dd1d);
        Rng(n ^ (n >> 17))
    }

    /// LMMS's `fastRand`: `s = s * 1103515245 + 12345`, then `s / 65536 % 32768`.
    pub fn fast_rand(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1103515245).wrapping_add(12345);
        ((self.0 / 65536) % 32768) as u32
    }

    /// `fastRandInc(from, to)`: uniform `from..=to`, ends included.
    pub fn inc(&mut self, from: f32, to: f32) -> f32 {
        from + (to - from) * self.fast_rand() as f32 / 32767.0
    }

    /// `oneIn(n)`: true one time in n.
    pub fn one_in(&mut self, n: u32) -> bool {
        self.fast_rand() % n == 0
    }

    /// The integer `fastRand()` (the presets use it for `& 1` wave picks).
    pub fn word(&mut self) -> u32 {
        self.fast_rand()
    }
}

/// The seven preset generators (the buttons on sfxr's left edge, LMMS's generator row). Each one
/// resets the patch to its defaults, then rolls the values its button describes; they are the LMMS
/// preset functions (`genPickup` and friends) one to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Pickup,
    Laser,
    Explosion,
    PowerUp,
    Hit,
    Jump,
    Blit,
}

pub const PRESETS: [Preset; 7] =
    [Preset::Pickup, Preset::Laser, Preset::Explosion, Preset::PowerUp, Preset::Hit, Preset::Jump, Preset::Blit];

impl Preset {
    pub fn name(self) -> &'static str {
        match self {
            Preset::Pickup => "Pickup/Coin",
            Preset::Laser => "Laser/Shoot",
            Preset::Explosion => "Explosion",
            Preset::PowerUp => "PowerUp",
            Preset::Hit => "Hit/Hurt",
            Preset::Jump => "Jump",
            Preset::Blit => "Blit/Select",
        }
    }

    /// Rolls a patch of this preset (LMMS's `setValue(fastRandInc(...))` calls, in order).
    pub fn roll(self, r: &mut Rng) -> Patch {
        // LMMS rounds a float into its int wave model (`castValue<int>` rounds), so
        // `fastRandInc(2)` picks square, saw or sine in equal shares, like sfxr's `rnd(2)`.
        let mut p = Patch::default();
        match self {
            Preset::Pickup => {
                p.start_freq = r.inc(0.4, 0.9);
                p.att = 0.0;
                p.hold = r.inc(0.0, 0.1);
                p.dec = r.inc(0.1, 0.5);
                p.sus = r.inc(0.3, 0.6);
                if r.one_in(2) {
                    p.change_speed = r.inc(0.5, 0.7);
                    p.change_amt = r.inc(0.2, 0.6);
                }
            }
            Preset::Laser => {
                p.wave = r.inc(0.0, 2.0).round() as u8;
                if p.wave == 2 && r.one_in(2) {
                    p.wave = (r.word() & 1) as u8;
                }
                p.start_freq = r.inc(0.5, 1.0);
                p.min_freq = (p.start_freq - r.inc(0.2, 0.8)).max(0.2);
                p.slide = r.inc(-0.15, -0.35);
                if r.one_in(3) {
                    p.start_freq = r.inc(0.3, 0.9);
                    p.min_freq = r.inc(0.0, 0.1);
                    p.slide = r.inc(-0.65, -0.35);
                }
                if r.one_in(2) {
                    p.sqr_duty = r.inc(0.0, 0.5);
                    p.sqr_sweep = 0.2;
                } else {
                    p.sqr_duty = r.inc(0.4, 0.9);
                    p.sqr_sweep = r.inc(-0.7, 0.0);
                }
                p.att = 0.0;
                p.hold = r.inc(0.1, 0.3);
                p.dec = r.inc(0.0, 0.4);
                if r.one_in(2) {
                    p.sus = r.inc(0.0, 0.3);
                }
                if r.one_in(3) {
                    p.phaser_offset = r.inc(0.0, 0.2);
                    p.phaser_sweep = r.inc(-0.2, 0.0);
                }
                if r.one_in(2) {
                    p.hp_cut = r.inc(0.0, 0.3);
                }
            }
            Preset::Explosion => {
                p.wave = 3;
                if r.one_in(2) {
                    p.start_freq = r.inc(0.1, 0.5);
                    p.slide = r.inc(-0.1, 0.3);
                } else {
                    p.start_freq = r.inc(0.2, 0.9);
                    p.slide = r.inc(-0.4, -0.2);
                }
                p.start_freq *= p.start_freq;
                if r.one_in(5) {
                    p.slide = 0.0;
                }
                if r.one_in(3) {
                    p.repeat = r.inc(0.3, 0.8);
                }
                p.att = 0.0;
                p.hold = r.inc(0.1, 0.4);
                p.dec = 0.5;
                if r.one_in(2) {
                    p.phaser_offset = r.inc(-0.3, 0.6);
                    p.phaser_sweep = r.inc(-0.3, 0.0);
                }
                p.sus = r.inc(0.2, 0.8);
                if r.one_in(2) {
                    p.vib_depth = r.inc(0.0, 0.7);
                    p.vib_speed = r.inc(0.0, 0.6);
                }
                if r.one_in(3) {
                    p.change_speed = r.inc(0.6, 0.9);
                    p.change_amt = r.inc(-0.8, 0.8);
                }
            }
            Preset::PowerUp => {
                if r.one_in(2) {
                    p.wave = 1;
                } else {
                    p.sqr_duty = r.inc(0.0, 0.6);
                }
                if r.one_in(2) {
                    p.start_freq = r.inc(0.2, 0.5);
                    p.slide = r.inc(0.1, 0.5);
                    p.repeat = r.inc(0.4, 0.8);
                } else {
                    p.start_freq = r.inc(0.2, 0.5);
                    p.slide = r.inc(0.05, 0.25);
                    if r.one_in(2) {
                        p.vib_depth = r.inc(0.0, 0.7);
                        p.vib_speed = r.inc(0.0, 0.6);
                    }
                }
                p.att = 0.0;
                p.hold = r.inc(0.0, 0.4);
                p.dec = r.inc(0.1, 0.5);
            }
            Preset::Hit => {
                p.wave = r.inc(0.0, 2.0).round() as u8;
                if p.wave == 2 {
                    p.wave = 3;
                }
                if p.wave == 0 {
                    p.sqr_duty = r.inc(0.0, 0.6);
                }
                p.start_freq = r.inc(0.2, 0.8);
                p.slide = r.inc(-0.7, -0.3);
                p.att = 0.0;
                p.hold = r.inc(0.0, 0.1);
                p.dec = r.inc(0.1, 0.3);
                if r.one_in(2) {
                    p.hp_cut = r.inc(0.0, 0.3);
                }
            }
            Preset::Jump => {
                p.wave = 0;
                p.sqr_duty = r.inc(0.0, 0.6);
                p.start_freq = r.inc(0.3, 0.6);
                p.slide = r.inc(0.1, 0.3);
                p.att = 0.0;
                p.hold = r.inc(0.1, 0.4);
                p.dec = r.inc(0.1, 0.3);
                if r.one_in(2) {
                    p.hp_cut = r.inc(0.0, 0.3);
                }
                if r.one_in(2) {
                    p.lp_cut = r.inc(0.4, 1.0);
                }
            }
            Preset::Blit => {
                p.wave = (r.word() & 1) as u8;
                if p.wave == 0 {
                    p.sqr_duty = r.inc(0.0, 0.6);
                }
                p.start_freq = r.inc(0.2, 0.6);
                p.att = 0.0;
                p.hold = r.inc(0.1, 0.2);
                p.dec = r.inc(0.0, 0.2);
                p.hp_cut = 0.1;
            }
        }
        p
    }
}

/// One sounding effect: the whole sfxr synthesis, one virtual 44.1 kHz sample per [`Synth::next`]
/// call (LMMS's `SfxrSynth`, filter states included).
#[derive(Debug, Clone)]
pub struct Synth {
    /// The raw patch the synth was built from (its sliders are read again on every repeat reset).
    patch: Patch,
    playing: bool,
    phase: i32,
    fperiod: f64,
    fmaxperiod: f64,
    fslide: f64,
    fdslide: f64,
    period: i32,
    square_duty: f32,
    square_slide: f32,
    env_stage: i32,
    env_time: i32,
    env_length: [i32; 3],
    env_vol: f32,
    fphase: f32,
    fdphase: f32,
    iphase: i32,
    phaser_buffer: [f32; 1024],
    ipp: i32,
    noise_buffer: [f32; 32],
    fltp: f32,
    fltdp: f32,
    fltw: f32,
    fltw_d: f32,
    fltdmp: f32,
    fltphp: f32,
    flthp: f32,
    flthp_d: f32,
    vib_phase: f32,
    vib_speed: f32,
    vib_amp: f32,
    rep_time: i32,
    rep_limit: i32,
    arp_time: i32,
    arp_limit: i32,
    arp_mod: f64,
    /// How often the repeat speed restarted the pitch (a test's window into it).
    pub restarts: u32,
}

impl Synth {
    pub fn new(patch: &Patch, r: &mut Rng) -> Self {
        let mut s = Synth {
            patch: *patch,
            playing: true,
            phase: 0,
            fperiod: 0.0,
            fmaxperiod: 0.0,
            fslide: 0.0,
            fdslide: 0.0,
            period: 0,
            square_duty: 0.0,
            square_slide: 0.0,
            env_stage: 0,
            env_time: 0,
            env_length: [0; 3],
            env_vol: 0.0,
            fphase: 0.0,
            fdphase: 0.0,
            iphase: 0,
            phaser_buffer: [0.0; 1024],
            ipp: 0,
            noise_buffer: [0.0; 32],
            fltp: 0.0,
            fltdp: 0.0,
            fltw: 0.0,
            fltw_d: 0.0,
            fltdmp: 0.0,
            fltphp: 0.0,
            flthp: 0.0,
            flthp_d: 0.0,
            vib_phase: 0.0,
            vib_speed: 0.0,
            vib_amp: 0.0,
            rep_time: 0,
            rep_limit: 0,
            arp_time: 0,
            arp_limit: 0,
            arp_mod: 0.0,
            restarts: 0,
        };
        s.reset(false, r);
        s
    }

    pub fn is_done(&self) -> bool {
        !self.playing
    }

    /// LMMS's `resetSample`: the frequency, duty, jump and repeat settings reset on every repeat hit
    /// (`restart`); the filters, vibrato, envelope, phaser and noise table reset only on a fresh
    /// note, so a repeat pulses the pitch over an unbroken envelope, as sfxr's readme describes.
    /// (LMMS computes the repeat limit once per note, which turns its repeats into a single restart;
    /// this port recomputes it, so a repeat speed keeps pulsing for the sound's whole life.)
    fn reset(&mut self, restart: bool, r: &mut Rng) {
        let p = self.patch;
        if !restart {
            self.phase = 0;
        }
        self.fperiod = 100.0 / (f64::from(p.start_freq) * f64::from(p.start_freq) + 0.001);
        self.period = self.fperiod as i32;
        self.fmaxperiod = 100.0 / (f64::from(p.min_freq) * f64::from(p.min_freq) + 0.001);
        self.fslide = 1.0 - f64::from(p.slide) * f64::from(p.slide) * f64::from(p.slide) * 0.01;
        self.fdslide = -f64::from(p.dslide) * f64::from(p.dslide) * f64::from(p.dslide) * 0.000001;
        self.square_duty = 0.5 - p.sqr_duty * 0.5;
        self.square_slide = -p.sqr_sweep * 0.00005;
        // The abrupt pitch change ("Change Amount"/"Change Speed" in LMMS).
        self.arp_mod = if p.change_amt >= 0.0 {
            1.0 - f64::from(p.change_amt) * f64::from(p.change_amt) * 0.9
        } else {
            1.0 + f64::from(p.change_amt) * f64::from(p.change_amt) * 10.0
        };
        self.arp_time = 0;
        let chs = 1.0 - p.change_speed;
        self.arp_limit = if chs == 0.0 { 0 } else { (chs * chs * 20000.0 + 32.0) as i32 };
        // The repeat speed's interval (recomputed on every restart, see above).
        self.rep_time = 0;
        self.rep_limit = ((1.0 - p.repeat) * (1.0 - p.repeat) * 20000.0 + 32.0) as i32;
        if p.repeat == 0.0 {
            self.rep_limit = 0;
        }
        if !restart {
            // Reset filter.
            self.fltp = 0.0;
            self.fltdp = 0.0;
            self.fltw = p.lp_cut * p.lp_cut * p.lp_cut * 0.1;
            self.fltw_d = 1.0 + p.lp_sweep * 0.0001;
            self.fltdmp = 5.0 / (1.0 + p.lp_reso * p.lp_reso * 20.0) * (0.01 + self.fltw);
            if self.fltdmp > 0.8 {
                self.fltdmp = 0.8;
            }
            self.fltphp = 0.0;
            self.flthp = p.hp_cut * p.hp_cut * 0.1;
            self.flthp_d = 1.0 + p.hp_sweep * 0.0003;
            // Reset vibrato.
            self.vib_phase = 0.0;
            self.vib_speed = p.vib_speed * p.vib_speed * 0.01;
            self.vib_amp = p.vib_depth * 0.5;
            // Reset envelope.
            self.env_vol = 0.0;
            self.env_stage = 0;
            self.env_time = 0;
            self.env_length =
                [(p.att * p.att * 99999.0) as i32 + 1, (p.hold * p.hold * 99999.0) as i32 + 1, (p.dec * p.dec * 99999.0) as i32 + 1];
            // Reset phaser.
            self.fphase = p.phaser_offset * p.phaser_offset * 1020.0;
            if p.phaser_offset < 0.0 {
                self.fphase = -self.fphase;
            }
            self.fdphase = p.phaser_sweep * p.phaser_sweep;
            if p.phaser_sweep < 0.0 {
                self.fdphase = -self.fdphase;
            }
            self.iphase = self.fphase as i32;
            if self.iphase < 0 {
                self.iphase = -self.iphase;
            }
            self.ipp = 0;
            self.phaser_buffer = [0.0; 1024];
            for noise in self.noise_buffer.iter_mut() {
                *noise = r.inc(-1.0, 1.0);
            }
        }
    }

    /// The next virtual 44.1 kHz sample of the effect, or `None` once it has finished: its envelope
    /// ran out, or a downward slide reached a non-zero minimum frequency.
    pub fn next(&mut self, r: &mut Rng) -> Option<f32> {
        if !self.playing {
            return None;
        }
        // The repeat speed restarts the pitch and duty.
        if self.rep_limit != 0 {
            self.rep_time += 1;
            if self.rep_time >= self.rep_limit {
                self.restarts += 1;
                self.reset(true, r);
            }
        }
        // The pitch jump fires once, after its wait.
        self.arp_time += 1;
        if self.arp_limit != 0 && self.arp_time >= self.arp_limit {
            self.arp_limit = 0;
            self.fperiod *= self.arp_mod;
        }
        // The slide and its delta, and the minimum frequency cutoff.
        self.fslide += self.fdslide;
        self.fperiod *= self.fslide;
        if self.fperiod > self.fmaxperiod {
            self.fperiod = self.fmaxperiod;
            if self.patch.min_freq > 0.0 {
                self.playing = false;
            }
        }
        // Vibrato wobbles the period.
        let mut rfperiod = self.fperiod;
        if self.vib_amp > 0.0 {
            self.vib_phase += self.vib_speed;
            rfperiod = self.fperiod * (1.0 + self.vib_phase.sin() as f64 * f64::from(self.vib_amp));
        }
        self.period = (rfperiod as i32).max(8);
        // The square's duty sweeps, held inside its bounds.
        self.square_duty += self.square_slide;
        if self.square_duty < 0.0 {
            self.square_duty = 0.0;
        }
        if self.square_duty > 0.5 {
            self.square_duty = 0.5;
        }
        // The volume envelope: attack, sustain (with punch falling through it), decay.
        self.env_time += 1;
        if self.env_time > self.env_length[self.env_stage as usize] {
            self.env_time = 0;
            self.env_stage += 1;
            if self.env_stage == 3 {
                self.playing = false;
            }
        }
        match self.env_stage {
            0 => self.env_vol = self.env_time as f32 / self.env_length[0] as f32,
            1 => self.env_vol = 1.0 + (1.0 - self.env_time as f32 / self.env_length[1] as f32) * 2.0 * self.patch.sus,
            2 => self.env_vol = 1.0 - self.env_time as f32 / self.env_length[2] as f32,
            _ => {}
        }
        // The phaser's delay, and the highpass's cutoff drift.
        self.fphase += self.fdphase;
        self.iphase = self.fphase as i32;
        if self.iphase < 0 {
            self.iphase = -self.iphase;
        }
        if self.iphase > 1023 {
            self.iphase = 1023;
        }
        if self.flthp_d != 0.0 {
            self.flthp *= self.flthp_d;
            if self.flthp < 0.00001 {
                self.flthp = 0.00001;
            }
            if self.flthp > 0.1 {
                self.flthp = 0.1;
            }
        }
        // Eight supersamples per output sample, as sfxr antialiases its waves.
        let mut ssample = 0.0f32;
        for _ in 0..8 {
            let mut sample;
            self.phase += 1;
            if self.phase >= self.period {
                self.phase %= self.period;
                if self.patch.wave == 3 {
                    for noise in self.noise_buffer.iter_mut() {
                        *noise = r.inc(-1.0, 1.0);
                    }
                }
            }
            let fp = self.phase as f32 / self.period as f32;
            match self.patch.wave {
                0 => sample = if fp < self.square_duty { 0.5 } else { -0.5 },
                1 => sample = 1.0 - 2.0 * self.phase as f32 / self.period as f32,
                2 => sample = (std::f32::consts::PI * self.phase as f32 / self.period as f32).sin(),
                _ => sample = self.noise_buffer[(self.phase * 32 / self.period) as usize & 31],
            }
            // The resonant lowpass (a plain one when the cutoff sits at its maximum).
            let pp = self.fltp;
            self.fltw *= self.fltw_d;
            if self.fltw < 0.0 {
                self.fltw = 0.0;
            }
            if self.fltw > 0.1 {
                self.fltw = 0.1;
            }
            if self.patch.lp_cut != 1.0 {
                self.fltdp += (sample - self.fltp) * self.fltw;
                self.fltdp -= self.fltdp * self.fltdmp;
            } else {
                self.fltp = sample;
                self.fltdp = 0.0;
            }
            self.fltp += self.fltdp;
            // The highpass.
            self.fltphp += self.fltp - pp;
            self.fltphp -= self.fltphp * self.flthp;
            sample = self.fltphp;
            // The phaser: the sample plus its delayed copy.
            self.phaser_buffer[(self.ipp & 1023) as usize] = sample;
            sample += self.phaser_buffer[((self.ipp - self.iphase + 1024) & 1023) as usize];
            self.ipp = (self.ipp + 1) & 1023;
            ssample += sample * self.env_vol;
        }
        // sfxr's original output stage: half a master volume, twice a half sound volume.
        ssample = ssample / 8.0 * 0.5;
        ssample *= 2.0 * 0.5;
        Some(ssample.clamp(-1.0, 1.0))
    }
}

/// One playing note: a [`Synth`] read at the played key's rate. The virtual samples it generates are
/// kept a little ahead of the read position (linear interpolation over them) and dropped behind it,
/// so the pool of voices never holds more than a sound needs.
#[derive(Debug)]
struct Voice {
    synth: Synth,
    rng: Rng,
    gain: f32,
    /// The read position in virtual samples.
    pos: f64,
    /// Virtual samples per output sample: the key's pitch over the reference, on the virtual clock.
    step: f64,
    /// The generated virtual samples, from index `gen_start` on.
    buf: Vec<f32>,
    gen_start: usize,
    done: bool,
}

impl Voice {
    fn new(patch: &Patch, rng: Rng, hz: f32, rate: f32) -> Self {
        let mut rng = rng;
        let synth = Synth::new(patch, &mut rng);
        Voice {
            synth,
            rng,
            gain: patch.volume.max(0.0),
            pos: 0.0,
            step: f64::from(hz / REFERENCE_HZ) * f64::from(VIRTUAL_RATE / rate),
            buf: Vec::new(),
            gen_start: 0,
            done: false,
        }
    }

    /// Adds this voice's sound to a stereo pair (the same mono sound in both, as LMMS's does).
    fn fill(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.done {
            return;
        }
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.done {
                break;
            }
            let want = self.pos.floor() as usize;
            // Generate every virtual sample this output sample spans (a high key reads several).
            while !self.synth.is_done() && self.gen_start + self.buf.len() <= want + 1 {
                match self.synth.next(&mut self.rng) {
                    Some(s) => self.buf.push(s),
                    None => break,
                }
            }
            let have = self.gen_start + self.buf.len();
            if want >= have {
                if self.synth.is_done() {
                    self.done = true;
                    break;
                }
                continue;
            }
            let a = self.buf[want - self.gen_start];
            let b = self.buf.get(want + 1 - self.gen_start).copied().unwrap_or(a);
            let frac = (self.pos - want as f64) as f32;
            let s = a + (b - a) * frac;
            *l += s * self.gain;
            *r += s * self.gain;
            self.pos += self.step;
        }
        // Drop the generated prefix the read position left behind (keeping two samples of it).
        let keep_from = (self.pos.floor() as usize).saturating_sub(2);
        if keep_from > self.gen_start + 8192 {
            self.buf.drain(..keep_from - self.gen_start);
            self.gen_start = keep_from;
        }
    }
}

/// The pool of playing effects: a note rolls a fresh [`Synth`] (a fresh sound every key press, as
/// LMMS's Sfxr makes one), and up to [`VOICES`] of them sound at once, the oldest cut when full.
pub struct Engine {
    rate: f32,
    seed: u64,
    voices: Vec<Voice>,
}

/// How many effects may sound at once (sfxr sounds are one-shots; eight covers chords and runs).
pub const VOICES: usize = 8;

impl Engine {
    pub fn new(rate: f32) -> Self {
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x9e37_79b9_7f4a_7c15);
        Engine { rate, seed, voices: Vec::new() }
    }

    pub fn set_rate(&mut self, rate: f32) {
        self.rate = rate;
    }

    /// Starts a fresh effect of `patch` at key pitch `hz` (a fresh roll: the noise table, the preset
    /// rolls and the repeats take a new random path each note).
    pub fn note_on(&mut self, patch: &Patch, hz: f32) {
        self.seed = self.seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let rng = Rng::new(self.seed);
        let voice = Voice::new(patch, rng, hz, self.rate);
        if self.voices.len() >= VOICES {
            self.voices.remove(0);
        }
        self.voices.push(voice);
    }

    /// Stops every sounding effect (an all-notes-off from the host).
    pub fn all_off(&mut self) {
        self.voices.clear();
    }

    pub fn has_voices(&self) -> bool {
        self.voices.iter().any(|v| !v.done)
    }

    /// Mixes every sounding effect into a stereo pair.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        for voice in self.voices.iter_mut() {
            voice.fill(left, right);
        }
        self.voices.retain(|v| !v.done);
    }

    #[cfg(test)]
    pub fn voices(&self) -> usize {
        self.voices.len()
    }
}

/// The hertz of MIDI key `note`.
pub fn key_hz(note: u8) -> f32 {
    440.0 * 2.0f32.powf((f32::from(note) - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rng_rolls_like_lmmss_fastrand() {
        let mut r = Rng::new(1);
        // LMMS's LCG: s = s * 1103515245 + 12345, s / 65536 % 32768.
        let s: u64 = 1 * 1103515245 + 12345;
        assert_eq!(r.fast_rand(), (s / 65536 % 32768) as u32);
        for _ in 0..500 {
            let x = r.inc(0.3, 0.7);
            assert!((0.3..=0.7).contains(&x), "{x} outside 0.3..=0.7");
            let n = r.inc(-0.5, -0.1);
            assert!((-0.5..=-0.1).contains(&n), "{n} outside -0.5..=-0.1");
        }
        let hits = (0..2000).filter(|_| r.one_in(2)).count();
        assert!((700..1300).contains(&hits), "oneIn(2) hit {hits}/2000");
    }

    #[test]
    fn every_preset_rolls_a_loud_enough_sound_that_ends() {
        for preset in PRESETS {
            for seed in 1..12u64 {
                let mut r = Rng::new(seed * 7919);
                let patch = preset.roll(&mut r);
                let mut synth = Synth::new(&patch, &mut r);
                let mut loud = 0u32;
                let mut ended = false;
                for _ in 0..(VIRTUAL_RATE as i32) {
                    match synth.next(&mut r) {
                        Some(s) => {
                            if s.abs() > 0.01 {
                                loud += 1;
                            }
                            assert!(s.is_finite() && s.abs() <= 1.0, "preset {preset:?}: bad sample {s}");
                        }
                        None => {
                            ended = true;
                            break;
                        }
                    }
                }
                assert!(ended, "preset {preset:?} seed {seed} never ended within 1 s");
                assert!(loud > 100, "preset {preset:?} seed {seed} is silent (loud samples: {loud})");
            }
        }
    }

    #[test]
    fn a_repeat_speed_restarts_the_pitch_more_than_once() {
        let mut r = Rng::new(42);
        let mut patch = Patch::default();
        patch.repeat = 0.8;
        patch.att = 0.5;
        patch.hold = 0.5;
        patch.dec = 0.5;
        let mut synth = Synth::new(&patch, &mut r);
        for _ in 0..(VIRTUAL_RATE as i32 * 2) {
            if synth.next(&mut r).is_none() {
                break;
            }
        }
        assert!(synth.restarts > 1, "repeat speed restarts only {} times", synth.restarts);
    }

    #[test]
    fn the_lowpass_brightens_and_darkens_like_sfxr() {
        // A noise burst through the open lowpass is louder than through a closed one.
        let loud = |cut: f32| {
            let mut r = Rng::new(7);
            let mut patch = Patch::default();
            patch.wave = 3;
            patch.lp_cut = cut;
            patch.hold = 0.5;
            patch.dec = 0.1;
            let mut synth = Synth::new(&patch, &mut r);
            let mut sum = 0.0f32;
            for _ in 0..20_000 {
                sum += synth.next(&mut r).unwrap_or(0.0).abs();
            }
            sum
        };
        assert!(loud(1.0) > loud(0.05) * 2.0, "open {} vs closed {}", loud(1.0), loud(0.05));
    }

    #[test]
    fn an_engine_note_sounds_and_stops() {
        let mut engine = Engine::new(48_000.0);
        let patch = Preset::Pickup.roll(&mut Rng::new(3));
        engine.note_on(&patch, key_hz(69));
        assert!(engine.has_voices());
        let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
        let mut energy = 0.0f32;
        for _ in 0..20 {
            for x in l.iter_mut() {
                *x = 0.0;
            }
            for x in r.iter_mut() {
                *x = 0.0;
            }
            engine.render(&mut l, &mut r);
            energy += l.iter().map(|x| x.abs()).sum::<f32>();
        }
        assert!(energy > 0.5, "the note is silent (energy {energy})");
        engine.all_off();
        assert!(!engine.has_voices());
    }

    #[test]
    fn a_high_key_plays_faster_than_a_low_one() {
        // The same patch, played two octaves apart: the high one finishes within far fewer blocks.
        let finish = |note: u8| {
            let mut engine = Engine::new(44_100.0);
            let patch = Preset::Jump.roll(&mut Rng::new(11));
            engine.note_on(&patch, key_hz(note));
            let mut blocks = 0usize;
            while engine.has_voices() && blocks < 10_000 {
                let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
                engine.render(&mut l, &mut r);
                blocks += 1;
            }
            blocks
        };
        let high = finish(93);
        let low = finish(69);
        assert!(high * 3 < low, "high {high} blocks vs low {low}");
    }

    #[test]
    fn an_eighth_note_steals_the_oldest_voice() {
        let mut engine = Engine::new(44_100.0);
        let patch = Patch::default();
        for _ in 0..(VOICES + 1) {
            engine.note_on(&patch, key_hz(69));
        }
        assert_eq!(engine.voices(), VOICES);
    }

    #[test]
    fn keys_map_to_hertz() {
        assert!((key_hz(69) - 440.0).abs() < 0.01);
        assert!((key_hz(81) - 880.0).abs() < 0.01);
        assert!((key_hz(57) - 220.0).abs() < 0.01);
    }
}
