// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango DirectWave's program: the master's settings and the eight zones (a saved state, JSON). A zone holds one
//! sample and where it plays: its key range and the key it is tuned to, its velocity range, and its gain, pan, tune
//! and loop switch. The master holds the amp envelope and the filter every zone plays through.
//!
// spec: plugins/mango-directwave.md

/// How many zones a program holds.
pub const ZONES: usize = 8;

/// A zone: one sample and where it plays.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Zone {
    /// The sample file's path ("" = empty).
    #[serde(default)]
    pub path: String,
    /// The keys the zone covers, lowest and highest.
    #[serde(default = "d_lo")]
    pub lo: i32,
    #[serde(default = "d_hi")]
    pub hi: i32,
    /// The key the sample is tuned to.
    #[serde(default = "d_root")]
    pub root: i32,
    /// The velocities the zone covers, lowest and highest (the host's 0..128).
    #[serde(default)]
    pub vel_lo: i32,
    #[serde(default = "d_vel_hi")]
    pub vel_hi: i32,
    /// The gain, 0..256 as a percent (128 = unity).
    #[serde(default = "d_gain")]
    pub gain: i32,
    /// The pan, -64..64.
    #[serde(default)]
    pub pan: i32,
    /// The fine tune, -100..100 cents.
    #[serde(default)]
    pub tune: i32,
    /// Whether the sample's loop points play.
    #[serde(default)]
    pub loop_on: bool,
}

fn d_lo() -> i32 {
    0
}
fn d_hi() -> i32 {
    127
}
fn d_root() -> i32 {
    60
}
fn d_vel_hi() -> i32 {
    128
}
fn d_gain() -> i32 {
    128
}

impl Default for Zone {
    fn default() -> Self {
        Zone {
            path: String::new(),
            lo: d_lo(),
            hi: d_hi(),
            root: d_root(),
            vel_lo: 0,
            vel_hi: d_vel_hi(),
            gain: d_gain(),
            pan: 0,
            tune: 0,
            loop_on: false,
        }
    }
}

impl Zone {
    /// Whether the zone holds a sample.
    pub fn loaded(&self) -> bool {
        !self.path.is_empty()
    }

    /// Whether the zone plays key `k` at velocity `v` (the host's 0..128): the key inside, the velocity in its
    /// range's low end and under its high (a high end of 128 holds everything).
    pub fn holds(&self, k: i32, v: i32) -> bool {
        self.loaded() && k >= self.lo && k <= self.hi && v >= self.vel_lo && v < self.vel_hi
    }
}

/// The master's settings: the envelope and the filter every zone plays through.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Master {
    /// The gain, 0..512, unity at 256 (the shared square law).
    pub gain: i32,
    /// The filter's knob positions: the cutoff 0..128 (128 wide open) and the resonance 0..128.
    pub cut: i32,
    pub res: i32,
    /// The envelope's times (0..128, the shared seconds law) and its held level (0..128).
    pub attack: i32,
    pub decay: i32,
    pub sustain: i32,
    pub release: i32,
}

impl Default for Master {
    fn default() -> Self {
        Master { gain: 256, cut: 128, res: 0, attack: 0, decay: 0, sustain: 128, release: 16 }
    }
}

/// The whole program: the master and the eight zones.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Program {
    pub master: Master,
    pub zones: Vec<Zone>,
}

impl Program {
    /// The zones, padded to `ZONES` (a short program's missing zones are empty).
    pub fn zones(&self) -> Vec<Zone> {
        let mut v = self.zones.clone();
        v.resize(ZONES, Zone::default());
        v
    }

    /// The zone `i`, padded.
    pub fn zone(&self, i: usize) -> Zone {
        self.zones.get(i).cloned().unwrap_or_default()
    }

    /// Lays the loaded zones over the keyboard in order: the first from C1 (12), the last to B7 (119), the key
    /// ranges meeting between them, every zone tuned to its range's bottom.
    pub fn spread(&mut self) {
        let n = self.zones.iter().filter(|z| z.loaded()).count();
        if n == 0 {
            return;
        }
        let (a, b) = (12i32, 119i32);
        let span = (b - a + 1) / n as i32;
        let mut at = a;
        for i in 0..self.zones.len() {
            if !self.zones[i].loaded() {
                continue;
            }
            let more = self.zones.iter().skip(i + 1).any(|z| z.loaded());
            let hi = if more { (at + span - 1).min(b) } else { b };
            let z = &mut self.zones[i];
            z.lo = at;
            z.hi = hi;
            z.root = at;
            at = hi + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program with `n` loaded zones named `n.wav`.
    fn prog(n: usize) -> Program {
        let mut p = Program::default();
        p.zones = (0..n).map(|i| Zone { path: format!("{i}.wav"), ..Zone::default() }).collect();
        p
    }

    #[test]
    fn zones_hold_their_key_and_velocity() {
        let mut p = prog(1);
        p.zones[0].lo = 36;
        p.zones[0].hi = 48;
        p.zones[0].vel_lo = 64;
        p.zones[0].vel_hi = 128;
        assert!(p.zones[0].holds(36, 64));
        assert!(p.zones[0].holds(48, 127));
        assert!(!p.zones[0].holds(35, 64), "under the range");
        assert!(!p.zones[0].holds(49, 64), "over the range");
        assert!(!p.zones[0].holds(40, 63), "under the velocity");
        // An empty zone holds nothing.
        assert!(!p.zone(1).holds(40, 100));
    }

    #[test]
    fn the_program_round_trips_through_its_json() {
        let mut p = prog(2);
        p.master.cut = 96;
        p.master.release = 40;
        p.zones[1].root = 48;
        p.zones[1].loop_on = true;
        let text = serde_json::to_string(&p).unwrap();
        let back: Program = serde_json::from_str(&text).unwrap();
        assert_eq!(back, p);
        // A short program pads to eight zones; a fresh one's zones are empty.
        assert_eq!(p.zones().len(), ZONES);
        assert_eq!(Program::default().zone(7), Zone::default());
        let bare: Program = serde_json::from_str("{}").unwrap();
        assert_eq!(bare.master, Master::default());
    }

    #[test]
    fn spread_lays_the_zones_over_the_keyboard_in_order() {
        let mut p = prog(3);
        p.spread();
        let z = |i| p.zone(i);
        assert_eq!((z(0).lo, z(0).hi, z(0).root), (12, 47, 12));
        assert_eq!((z(1).lo, z(1).hi, z(1).root), (48, 83, 48));
        assert_eq!((z(2).lo, z(2).hi, z(2).root), (84, 119, 84));
        // One zone takes the whole keyboard; none: nothing moves.
        let mut one = prog(1);
        one.spread();
        assert_eq!((one.zone(0).lo, one.zone(0).hi), (12, 119));
        let mut none = Program::default();
        none.spread();
        assert_eq!(none, Program::default());
    }
}
