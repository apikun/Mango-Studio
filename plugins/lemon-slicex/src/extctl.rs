// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The editor's controls for what Slicex adds (`ext`): which control there is, what it shows and how a knob's 0..1
//! position reads and writes the articulators' values.
//!
// spec: plugins/lemon-slicex.md §5

use crate::ext::{Ext, Source, Target, ENV_MAX_MS};

/// What the envelope panel and the articulator row show: the articulator, and the target and source tabs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sel {
    pub art: usize,
    pub tgt: Target,
    pub src: Source,
}

impl Default for Sel {
    fn default() -> Self {
        Sel { art: 0, tgt: Target::Vol, src: Source::Env }
    }
}

/// A control of the added panels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ctl {
    /// Articulator button `n` (0-based): it is shown, and the chosen slice plays through it.
    Art(usize),
    /// A filter mode button and a slope button (1..3).
    Mode(u8),
    Slope(u8),
    /// The filter's knobs: cutoff, resonance, and how much the envelope moves the cutoff.
    Cut,
    Res,
    CutEnv,
    /// A tab of the targets row and of the sources row.
    Tgt(usize),
    Src(usize),
    /// The knobs of the chosen target and source.
    Amt,
    Att,
    Dec,
    Sus,
    Rel,
    /// Times in beats instead of ms (envelope), cycles in beats instead of Hz (LFO).
    Tempo,
    LfoShape(u8),
    LfoRate,
    /// The modulation pad and its smoothing switch.
    Pad,
    Smooth,
    /// Layering: the mode, SAME DECK and the crossfade curve (0..4).
    Layer(u8),
    SameDeck,
    Crossfade(u8),
    /// AUTO-DUMP, and the round options button.
    AutoDump,
    Options,
    /// Opens a loop into deck A (0) or B (1).
    LoadDeck(u8),
    /// The master volume fader, and the pad's two axes as knobs.
    Master,
    RandLvl,
    LfoLvl,
    PadX,
    PadY,
    /// The toolbar: play the chosen slice, stop it.
    PlaySlice,
    Stop,
    /// A key of the keyboard.
    Key(u8),
    /// A deck's sound on or off (A = 0, B = 1).
    DeckToggle(u8),
    /// The view buttons: what the window shows (0..3), and the keyboard.
    View(u8),
    Keys,
    /// The toolbar: the loop button, the menu buttons (0..8), the sample edit functions (0..10) and the info strip's
    /// switches (0..4).
    Loop,
    Menu(u8),
    Edit(u8),
    Info(u8),
    /// The row under the wave: the left icons (0..5) and the right switches (6..11).
    Row(u8),
    /// The wave's scroll arrows: left (-1) or right (1).
    Scroll(i8),
}

impl Ctl {
    /// Whether it is a knob (a vertical drag moves it).
    pub fn is_knob(self) -> bool {
        matches!(self, Ctl::Cut | Ctl::Res | Ctl::CutEnv | Ctl::Amt | Ctl::Att | Ctl::Dec | Ctl::Sus | Ctl::Rel | Ctl::LfoRate | Ctl::Master | Ctl::RandLvl | Ctl::LfoLvl | Ctl::PadX | Ctl::PadY)
    }

    /// Whether a click flips it or picks it (a button).
    pub fn caption(self) -> &'static str {
        match self {
            Ctl::Cut => "CUT",
            Ctl::Res => "RES",
            Ctl::CutEnv => "ENV",
            Ctl::Amt => "AMOUNT",
            Ctl::Att => "ATT",
            Ctl::Dec => "DEC",
            Ctl::Sus => "SUS",
            Ctl::Rel => "REL",
            Ctl::LfoRate => "SPEED",
            Ctl::Master => "VOLUME",
            Ctl::RandLvl => "RANDOMNESS",
            Ctl::LfoLvl => "LFO",
            Ctl::PadX => "Pad X",
            Ctl::PadY => "Pad Y",
            _ => "",
        }
    }
}

fn bipolar(v: f32) -> f32 {
    (v + 1.0) / 2.0
}

fn time_norm(ms: f32) -> f32 {
    (ms / ENV_MAX_MS).clamp(0.0, 1.0).sqrt()
}

fn time_ms(n: f32) -> f32 {
    n.clamp(0.0, 1.0).powi(2) * ENV_MAX_MS
}

/// The LFO's speed: 0.05..20 Hz, or 1/4..16 beats a cycle (the knob turned up = faster in both).
fn rate_of(n: f32, tempo: bool) -> f32 {
    if tempo { 16.0 * 64f32.powf(-n) } else { 0.05 * 400f32.powf(n) }
}

fn rate_norm(r: f32, tempo: bool) -> f32 {
    let n = if tempo { (16.0 / r.max(0.01)).ln() / 64f32.ln() } else { (r.max(0.01) / 0.05).ln() / 400f32.ln() };
    n.clamp(0.0, 1.0)
}

/// The knob's position (0..1) for control `c`.
pub fn get(x: &Ext, s: &Sel, c: Ctl) -> f32 {
    let a = &x.artic[s.art];
    let (t, e) = (s.tgt.index(), &a.env[s.tgt.index()]);
    match c {
        Ctl::Cut => a.cut,
        Ctl::Res => a.res,
        Ctl::Master => x.master / 1.25,
        Ctl::RandLvl => x.rand_level,
        Ctl::LfoLvl => x.lfo_level,
        Ctl::PadX => x.mod_x,
        Ctl::PadY => x.mod_y,
        Ctl::CutEnv => bipolar(a.amount(Target::Cut, Source::Env)),
        Ctl::Amt => bipolar(a.amount(s.tgt, s.src)),
        Ctl::Att => time_norm(e.att),
        Ctl::Dec => time_norm(e.dec),
        Ctl::Sus => e.sus,
        Ctl::Rel => time_norm(e.rel),
        Ctl::LfoRate => rate_norm(a.lfo[t].rate, a.lfo[t].tempo),
        _ => 0.0,
    }
}

/// Sets control `c` from the knob's position.
pub fn set(x: &mut Ext, s: &Sel, c: Ctl, n: f32) {
    let n = n.clamp(0.0, 1.0);
    let a = &mut x.artic[s.art];
    let t = s.tgt.index();
    match c {
        Ctl::Cut => a.cut = n,
        Ctl::Res => a.res = n,
        Ctl::Master => x.master = n * 1.25,
        Ctl::RandLvl => x.rand_level = n,
        Ctl::LfoLvl => x.lfo_level = n,
        Ctl::PadX => x.mod_x = n,
        Ctl::PadY => x.mod_y = n,
        Ctl::CutEnv => a.set_amount(Target::Cut, Source::Env, n * 2.0 - 1.0),
        Ctl::Amt => a.set_amount(s.tgt, s.src, n * 2.0 - 1.0),
        Ctl::Att => a.env[t].att = time_ms(n),
        Ctl::Dec => a.env[t].dec = time_ms(n),
        Ctl::Sus => a.env[t].sus = n,
        Ctl::Rel => a.env[t].rel = time_ms(n),
        Ctl::LfoRate => a.lfo[t].rate = rate_of(n, a.lfo[t].tempo),
        _ => {}
    }
}

/// The control's value as text.
pub fn text(x: &Ext, s: &Sel, c: Ctl) -> String {
    let a = &x.artic[s.art];
    let t = s.tgt.index();
    let ms = |v: f32| if a.env[t].tempo { format!("{:.2} beats", v / 1000.0 * 4.0) } else { format!("{v:.0} ms") };
    let pct = |v: f32| format!("{:.0}%", v * 100.0);
    match c {
        Ctl::Cut => pct(a.cut),
        Ctl::Res => pct(a.res),
        Ctl::Master => pct(x.master),
        Ctl::RandLvl => pct(x.rand_level),
        Ctl::LfoLvl => pct(x.lfo_level),
        Ctl::PadX => pct(x.mod_x),
        Ctl::PadY => pct(x.mod_y),
        Ctl::CutEnv => pct(a.amount(Target::Cut, Source::Env)),
        Ctl::Amt => pct(a.amount(s.tgt, s.src)),
        Ctl::Att => ms(a.env[t].att),
        Ctl::Dec => ms(a.env[t].dec),
        Ctl::Sus => pct(a.env[t].sus),
        Ctl::Rel => ms(a.env[t].rel),
        Ctl::LfoRate => {
            let l = a.lfo[t];
            if l.tempo { format!("{:.2} beats a cycle", l.rate) } else { format!("{:.2} Hz", l.rate) }
        }
        _ => String::new(),
    }
}

/// The default position of a knob (a middle click or alt-click puts it back).
pub fn default_norm(c: Ctl, s: &Sel) -> f32 {
    let d = crate::ext::Artic::default();
    let e = d.env[0];
    match c {
        Ctl::Cut => d.cut,
        Ctl::Res => d.res,
        Ctl::Master => 0.8,
        Ctl::RandLvl | Ctl::LfoLvl => 1.0,
        Ctl::PadX | Ctl::PadY => 0.5,
        Ctl::CutEnv | Ctl::Amt => {
            let v = if c == Ctl::Amt { d.amount(s.tgt, s.src) } else { 0.0 };
            bipolar(v)
        }
        Ctl::Att => time_norm(e.att),
        Ctl::Dec => time_norm(e.dec),
        Ctl::Sus => e.sus,
        Ctl::Rel => time_norm(e.rel),
        Ctl::LfoRate => rate_norm(crate::ext::Lfo::default().rate, false),
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knobs_read_back_what_they_wrote() {
        let mut x = Ext::default();
        let s = Sel { art: 2, tgt: Target::Cut, src: Source::Lfo };
        for c in [Ctl::Cut, Ctl::Res, Ctl::CutEnv, Ctl::Amt, Ctl::Att, Ctl::Dec, Ctl::Sus, Ctl::Rel, Ctl::LfoRate] {
            set(&mut x, &s, c, 0.3);
            assert!((get(&x, &s, c) - 0.3).abs() < 1e-3, "{c:?}: {}", get(&x, &s, c));
        }
        // Another articulator is untouched.
        assert_eq!(x.artic[0], crate::ext::Artic::default());
        assert_eq!(text(&x, &s, Ctl::Cut), "30%");
        assert_eq!(text(&x, &s, Ctl::Amt), "-40%");
        // A bipolar knob's middle is zero.
        set(&mut x, &s, Ctl::Amt, 0.5);
        assert_eq!(x.artic[2].amount(Target::Cut, Source::Lfo), 0.0);
    }

    #[test]
    fn the_lfo_speed_follows_tempo_mode() {
        let mut x = Ext::default();
        let s = Sel { art: 0, tgt: Target::Pan, src: Source::Lfo };
        set(&mut x, &s, Ctl::LfoRate, 1.0);
        assert!((x.artic[0].lfo[0].rate - 20.0).abs() < 1e-2);
        x.artic[0].lfo[0].tempo = true;
        set(&mut x, &s, Ctl::LfoRate, 1.0);
        assert!((x.artic[0].lfo[0].rate - 0.25).abs() < 1e-3);
        assert!((get(&x, &s, Ctl::LfoRate) - 1.0).abs() < 1e-3);
    }
}
