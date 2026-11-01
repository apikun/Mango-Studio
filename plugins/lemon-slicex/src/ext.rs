// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! What Mango Slicex adds to the slicer's saved state: eight articulators (a filter and, for six targets, the sources
//! that move them), which articulator each slice plays through, the modulation pad, and the two decks' layering. It is
//! saved next to the slicer's own state (`params::EXT_FIELD`) in a small binary form that ignores bytes it does not
//! know, so later versions can add to it.
//!
// spec: plugins/lemon-slicex.md §1

/// Articulators, targets and sources.
pub const ARTICS: usize = 8;
pub const TARGETS: usize = 6;
pub const SOURCES: usize = 6;

/// A target the sources move: the tabs along the top of the envelope panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Pan,
    Vol,
    Cut,
    Res,
    Speed,
    Start,
}

pub const ALL_TARGETS: [Target; TARGETS] = [Target::Pan, Target::Vol, Target::Cut, Target::Res, Target::Speed, Target::Start];

impl Target {
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        ["PAN", "VOL", "CUT", "RES", "SPEED", "START"][self.index()]
    }
}

/// What moves a target: the second row of tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A time curve from the note's start (attack, decay, sustain; the volume one also sets the release).
    Env,
    Lfo,
    /// The note's velocity.
    Vel,
    /// The modulation pad's two axes.
    ModX,
    ModY,
    /// A number picked again at every note.
    Rand,
}

pub const ALL_SOURCES: [Source; SOURCES] = [Source::Env, Source::Lfo, Source::Vel, Source::ModX, Source::ModY, Source::Rand];

impl Source {
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        ["ENV", "LFO", "VEL", "MOD X", "MOD Y", "RAND"][self.index()]
    }
}

/// The filter's modes, in the order of its buttons (OFF last, as the original's).
pub const FILTER_MODES: [&str; 5] = ["LP", "BP", "BS", "HP", "OFF"];
pub const FILTER_OFF: u8 = 4;

/// An envelope: attack, decay (ms), sustain (0..1) and release (ms); `tempo` reads the times as fractions of a beat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Env {
    pub att: f32,
    pub dec: f32,
    pub sus: f32,
    pub rel: f32,
    pub tempo: bool,
}

pub const ENV_MAX_MS: f32 = 5000.0;

impl Default for Env {
    fn default() -> Self {
        Env { att: 0.0, dec: 1500.0, sus: 0.0, rel: 5.0, tempo: false }
    }
}

impl Env {
    /// The envelope's value `t` seconds after the note began (while it is held); `beat` is a beat's length in
    /// seconds.
    pub fn level(&self, t: f64, beat: f64) -> f32 {
        let k = if self.tempo { beat / 1000.0 * 4.0 } else { 0.001 };
        let (att, dec) = (f64::from(self.att) * k, f64::from(self.dec) * k);
        if t < att {
            return (t / att.max(1e-9)) as f32;
        }
        let p = ((t - att) / dec.max(1e-9)).min(1.0) as f32;
        // The decay bends: fast at first, as the curve in the original's panel.
        self.sus + (1.0 - self.sus) * (1.0 - p) * (1.0 - p)
    }

    /// The attack, decay and release in seconds.
    pub fn seconds(&self, beat: f64) -> (f64, f64, f64) {
        let k = if self.tempo { beat / 1000.0 * 4.0 } else { 0.001 };
        (f64::from(self.att) * k, f64::from(self.dec) * k, f64::from(self.rel) * k)
    }

    /// The release in seconds.
    pub fn release(&self, beat: f64) -> f64 {
        f64::from(self.rel) * if self.tempo { beat / 1000.0 * 4.0 } else { 0.001 }
    }
}

/// Low-frequency oscillator shapes.
pub const LFO_SHAPES: [&str; 5] = ["SINE", "TRI", "SAW", "SQUARE", "RANDOM"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lfo {
    pub shape: u8,
    /// Hz, or beats per cycle when `tempo`.
    pub rate: f32,
    pub tempo: bool,
}

impl Default for Lfo {
    fn default() -> Self {
        Lfo { shape: 0, rate: 2.0, tempo: false }
    }
}

impl Lfo {
    /// The value, -1..1, `t` seconds in; `rand` is a number picked for the note, 0..1.
    pub fn value(&self, t: f64, beat: f64, rand: f32) -> f32 {
        let hz = if self.tempo { 1.0 / (f64::from(self.rate.max(0.01)) * beat.max(1e-3)) } else { f64::from(self.rate) };
        let ph = (t * hz).fract() as f32;
        match self.shape {
            0 => (ph * std::f32::consts::TAU).sin(),
            1 => 1.0 - 4.0 * (ph - 0.5).abs(),
            2 => 2.0 * ph - 1.0,
            3 => if ph < 0.5 { 1.0 } else { -1.0 },
            _ => {
                // A value per cycle, a hash of the note's number and the cycle.
                let n = (t * hz).floor() as u32;
                let h = (rand.to_bits() ^ n.wrapping_mul(0x9e37_79b9)).wrapping_mul(0x85eb_ca6b);
                ((h >> 8) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0
            }
        }
    }
}

/// One articulator: the filter and, per target, its base value, the amount each source adds and the envelope and LFO
/// that target's sources use.
#[derive(Clone, Debug, PartialEq)]
pub struct Artic {
    pub mode: u8,
    /// Filter slope: 1, 2 or 3 stages (12, 24, 36 dB).
    pub slope: u8,
    /// The cutoff and resonance knobs, 0..1.
    pub cut: f32,
    pub res: f32,
    /// Per target: the value without any source. Pan -1..1, the others 0..1.
    pub base: [f32; TARGETS],
    /// Per target and source: how much the source adds, -1..1.
    pub amount: [[f32; SOURCES]; TARGETS],
    pub env: [Env; TARGETS],
    pub lfo: [Lfo; TARGETS],
}

impl Default for Artic {
    fn default() -> Self {
        let mut amount = [[0.0; SOURCES]; TARGETS];
        // The volume follows its envelope (held at full by default) and the note's velocity.
        amount[Target::Vol.index()][Source::Env.index()] = 1.0;
        amount[Target::Vol.index()][Source::Vel.index()] = 1.0;
        Artic { mode: FILTER_OFF, slope: 1, cut: 1.0, res: 0.0, base: [0.0; TARGETS], amount, env: [Env::default(); TARGETS], lfo: [Lfo::default(); TARGETS] }
    }
}

impl Artic {
    pub fn amount(&self, t: Target, s: Source) -> f32 {
        self.amount[t.index()][s.index()]
    }
    pub fn set_amount(&mut self, t: Target, s: Source, v: f32) {
        self.amount[t.index()][s.index()] = v.clamp(-1.0, 1.0);
    }
}

/// How the two decks' regions on one key layer: the layering modes (the original's list).
pub const LAYERS: [&str; 7] = ["All", "All (gain compensated)", "Velocity-mapped", "Modulation X-mapped", "Modulation Y-mapped", "Random", "Cycle"];

/// The crossfade curves between layered regions, in the order of their buttons.
pub const FADES: [&str; 5] = ["None", "Linear", "Balanced curve", "Unbalanced curve", "Adding"];

impl Ext {
    /// The gains of deck A's and deck B's regions of a key when both play, for position `x` (0..1) along the mode's
    /// source (velocity, the pad, a random number) and the crossfade curve.
    pub fn layer_gains(&self, x: f32) -> (f32, f32) {
        let x = x.clamp(0.0, 1.0);
        match self.layer {
            0 => (1.0, 1.0),
            1 => (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2),
            _ => match self.fade {
                0 => if x < 0.5 { (1.0, 0.0) } else { (0.0, 1.0) },
                1 => (1.0 - x, x),
                2 => ((x * std::f32::consts::FRAC_PI_2).cos(), (x * std::f32::consts::FRAC_PI_2).sin()),
                3 => ((1.0 - x).powi(3), x.powi(3)),
                _ => ((2.0 * (1.0 - x)).min(1.0), (2.0 * x).min(1.0)),
            },
        }
    }
}

/// The whole addition to the saved state.
#[derive(Clone, Debug, PartialEq)]
pub struct Ext {
    pub artic: [Artic; ARTICS],
    /// The articulator of each slice (missing = the first).
    pub slice_art: Vec<u8>,
    /// The modulation pad, 0..1 each way, and whether it moves smoothly to a new position.
    pub mod_x: f32,
    pub mod_y: f32,
    pub smooth: bool,
    /// Layering: the mode (`LAYERS`), whether deck B plays the slice with deck A's number (SAME DECK), the crossfade
    /// curve (`FADES`), and which decks sound.
    pub layer: u8,
    pub same_deck: bool,
    pub fade: u8,
    pub a_on: bool,
    pub b_on: bool,
    /// The second deck's saved slicer state and sample path (empty = no second loop).
    pub deck_b: Vec<u8>,
    /// The master volume as a gain, 0..1.25 (1 = as recorded), and the master levels of the randomness and the LFOs.
    pub master: f32,
    pub rand_level: f32,
    pub lfo_level: f32,
    /// A loop loaded into deck A is dumped to the piano roll at once.
    pub auto_dump: bool,
    /// What the window shows (`view::arrange`): 0 everything, 1 without the audio editor, 2 without the region
    /// settings, 3 only the master panel; and whether the keyboard shows.
    pub view: u8,
    pub keys: bool,
}

impl Default for Ext {
    fn default() -> Self {
        Ext {
            artic: std::array::from_fn(|_| Artic::default()),
            slice_art: Vec::new(),
            mod_x: 0.5,
            mod_y: 0.5,
            smooth: true,
            layer: 0,
            same_deck: true,
            fade: 0,
            a_on: true,
            b_on: true,
            deck_b: Vec::new(),
            master: 1.0,
            rand_level: 1.0,
            lfo_level: 1.0,
            auto_dump: false,
            view: 0,
            keys: true,
        }
    }
}

const MAGIC: &[u8; 4] = b"MSX1";

struct Out(Vec<u8>);

impl Out {
    fn f(&mut self, v: f32) {
        self.0.extend(v.to_le_bytes());
    }
    fn b(&mut self, v: u8) {
        self.0.push(v);
    }
    fn n(&mut self, v: usize) {
        self.0.extend((v as u32).to_le_bytes());
    }
}

struct In<'a>(&'a [u8], usize);

impl In<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.0.get(self.1..self.1 + n)?;
        self.1 += n;
        Some(s)
    }
    fn f(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn b(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn n(&mut self) -> Option<usize> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?) as usize)
    }
}

impl Ext {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut o = Out(MAGIC.to_vec());
        o.f(self.mod_x);
        o.f(self.mod_y);
        o.b(u8::from(self.smooth));
        o.b(self.layer);
        o.b(u8::from(self.same_deck));
        o.b(self.fade);
        for a in &self.artic {
            o.b(a.mode);
            o.b(a.slope);
            o.f(a.cut);
            o.f(a.res);
            for t in 0..TARGETS {
                o.f(a.base[t]);
                for s in 0..SOURCES {
                    o.f(a.amount[t][s]);
                }
                let e = a.env[t];
                for v in [e.att, e.dec, e.sus, e.rel] {
                    o.f(v);
                }
                o.b(u8::from(e.tempo));
                let l = a.lfo[t];
                o.b(l.shape);
                o.f(l.rate);
                o.b(u8::from(l.tempo));
            }
        }
        o.n(self.slice_art.len());
        o.0.extend(&self.slice_art);
        o.n(self.deck_b.len());
        o.0.extend(&self.deck_b);
        o.f(self.master);
        o.b(self.view);
        o.b(u8::from(self.keys));
        o.f(self.rand_level);
        o.f(self.lfo_level);
        o.b(u8::from(self.auto_dump));
        o.b(u8::from(self.a_on));
        o.b(u8::from(self.b_on));
        o.0
    }

    /// Parses saved bytes; `None` when they are not this format or cut short.
    pub fn parse(b: &[u8]) -> Option<Ext> {
        if b.get(..4)? != MAGIC {
            return None;
        }
        let mut i = In(b, 4);
        let mut x = Ext { mod_x: i.f()?, mod_y: i.f()?, smooth: i.b()? != 0, layer: i.b()?.min(6), same_deck: i.b()? != 0, fade: i.b()?.min(4), ..Ext::default() };
        for a in &mut x.artic {
            a.mode = i.b()?.min(FILTER_OFF);
            a.slope = i.b()?.clamp(1, 3);
            a.cut = i.f()?;
            a.res = i.f()?;
            for t in 0..TARGETS {
                a.base[t] = i.f()?;
                for s in 0..SOURCES {
                    a.amount[t][s] = i.f()?;
                }
                a.env[t] = Env { att: i.f()?, dec: i.f()?, sus: i.f()?, rel: i.f()?, tempo: i.b()? != 0 };
                a.lfo[t] = Lfo { shape: i.b()?.min(4), rate: i.f()?, tempo: i.b()? != 0 };
            }
        }
        let n = i.n()?;
        x.slice_art = i.take(n)?.to_vec();
        let n = i.n()?;
        x.deck_b = i.take(n)?.to_vec();
        // Added later: saved states without it keep the default.
        x.master = i.f().map_or(1.0, |m| m.clamp(0.0, 1.25));
        x.view = i.b().map_or(0, |v| v.min(3));
        x.keys = i.b().is_none_or(|k| k != 0);
        x.rand_level = i.f().map_or(1.0, |v| v.clamp(0.0, 1.0));
        x.lfo_level = i.f().map_or(1.0, |v| v.clamp(0.0, 1.0));
        x.auto_dump = i.b().is_some_and(|b| b != 0);
        x.a_on = i.b().is_none_or(|b| b != 0);
        x.b_on = i.b().is_none_or(|b| b != 0);
        Some(x)
    }

    /// The articulator slice `i` plays through.
    pub fn art_of(&self, i: usize) -> usize {
        self.slice_art.get(i).map_or(0, |&a| usize::from(a).min(ARTICS - 1))
    }

    pub fn set_art(&mut self, slice: usize, art: usize) {
        if self.slice_art.len() <= slice {
            self.slice_art.resize(slice + 1, 0);
        }
        self.slice_art[slice] = art.min(ARTICS - 1) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_saved_bytes_come_back() {
        let mut x = Ext::default();
        x.mod_x = 0.25;
        x.layer = 2;
        x.fade = 3;
        x.b_on = false;
        x.auto_dump = true;
        x.lfo_level = 0.25;
        x.artic[3].mode = 1;
        x.artic[3].slope = 3;
        x.artic[3].cut = 0.4;
        x.artic[3].set_amount(Target::Cut, Source::Lfo, -0.5);
        x.artic[3].env[Target::Vol.index()] = Env { att: 12.0, dec: 300.0, sus: 0.5, rel: 40.0, tempo: true };
        x.artic[3].lfo[Target::Pan.index()] = Lfo { shape: 3, rate: 0.5, tempo: true };
        x.set_art(5, 3);
        x.deck_b = vec![1, 2, 3];
        x.master = 0.5;
        x.view = 2;
        x.keys = false;
        let b = x.to_bytes();
        assert_eq!(Ext::parse(&b), Some(x.clone()));
        assert_eq!(x.art_of(5), 3);
        assert_eq!(x.art_of(2), 0);
        assert!(Ext::parse(&b[..b.len() - 17]).is_some_and(|e| e.master == 1.0), "an older state has no master volume");
        assert!(Ext::parse(&b[..b.len() - 22]).is_none(), "cut short");
        assert!(Ext::parse(b"nope").is_none());
    }

    #[test]
    fn envelopes_rise_decay_and_sustain() {
        let e = Env { att: 100.0, dec: 200.0, sus: 0.25, rel: 10.0, tempo: false };
        assert!((e.level(0.05, 0.5) - 0.5).abs() < 1e-6, "half way up");
        assert!((e.level(0.1, 0.5) - 1.0).abs() < 1e-6, "the peak");
        assert!(e.level(0.15, 0.5) < 0.7, "decaying");
        assert!((e.level(5.0, 0.5) - 0.25).abs() < 1e-6, "sustain");
        // In tempo mode a time of 100 is 0.1 of four beats.
        let t = Env { tempo: true, ..e };
        assert!((t.release(0.5) - 0.02).abs() < 1e-9);
    }

    #[test]
    fn lfos_swing_between_minus_one_and_one() {
        let l = Lfo { shape: 0, rate: 1.0, tempo: false };
        assert!(l.value(0.25, 0.5, 0.0) > 0.99 && l.value(0.75, 0.5, 0.0) < -0.99);
        let sq = Lfo { shape: 3, ..l };
        assert_eq!((sq.value(0.1, 0.5, 0.0), sq.value(0.6, 0.5, 0.0)), (1.0, -1.0));
        let r = Lfo { shape: 4, ..l };
        assert!((-1.0..=1.0).contains(&r.value(3.2, 0.5, 0.3)));
    }
}
