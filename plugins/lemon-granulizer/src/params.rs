// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Granulizer's parameters and saved state.
//!
//! Nine parameters: the grain cloud's grain length, spacing, random start window, pan spread and hold, the pitch
//! shift, the note's attack and release, and the reverse switch; they are the settings a host automates and the
//! editor's knobs turn. The saved state holds what parameters do not: the sample file and the root key, as a JSON
//! object (`state`), the way DirectWave keeps its program.
//!
// spec: plugins/lemon-granulizer.md §1

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use nih_plug::prelude::*;
use os_plugkit::live::LiveEdits;
use serde::{Deserialize, Serialize};

pub const GRAIN_ID: &str = "grain";
pub const SPACING_ID: &str = "spacing";
pub const RANDOM_ID: &str = "random";
pub const PAN_ID: &str = "pan";
pub const HOLD_ID: &str = "hold";
pub const PITCH_ID: &str = "pitch";
pub const ATTACK_ID: &str = "att";
pub const RELEASE_ID: &str = "rel";
pub const REVERSE_ID: &str = "rev";

/// The parameters in the host's order: (id, name).
pub const NAMES: [(&str, &str); 9] = [
    (GRAIN_ID, "Grain"),
    (SPACING_ID, "Spacing"),
    (RANDOM_ID, "Random"),
    (PAN_ID, "Pan spread"),
    (HOLD_ID, "Hold"),
    (PITCH_ID, "Pitch shift"),
    (ATTACK_ID, "Attack"),
    (RELEASE_ID, "Release"),
    (REVERSE_ID, "Reverse"),
];

pub const GRAIN_RANGE: (i32, i32) = (5, 2000);
pub const SPACING_RANGE: (i32, i32) = (5, 2000);
pub const SHARE_RANGE: (i32, i32) = (0, 100);
pub const PITCH_RANGE: (i32, i32) = (-1200, 1200);
pub const FADE_RANGE: (i32, i32) = (0, 500);

/// The text of a time (ms, seconds past one).
pub fn ms_text(v: i32) -> String {
    if v >= 1000 { format!("{:.2}s", f64::from(v) / 1000.0) } else { format!("{v}ms") }
}

/// The text of a share (%).
pub fn share_text(v: i32) -> String {
    format!("{v}%")
}

/// The text of a pitch shift (cents).
pub fn pitch_text(v: i32) -> String {
    if v == 0 { "0 cents".into() } else { format!("{v:+} cents") }
}

/// A typed number (its unit, if any, ignored), clamped to the range.
pub fn parse_number(s: &str, (lo, hi): (i32, i32)) -> Option<i32> {
    let num: String = s.trim().chars().take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.')).collect();
    let x: f64 = num.parse().ok()?;
    Some((x.round() as i32).clamp(lo, hi))
}

/// The parameters' values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Values {
    pub grain: i32,
    pub spacing: i32,
    pub random: i32,
    pub pan: i32,
    pub hold: i32,
    pub pitch: i32,
    pub attack: i32,
    pub release: i32,
    pub reverse: bool,
}

/// The saved state: the sample file and the root key the note plays against.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub root: i32,
}

impl State {
    /// A new instance's: no sample yet, the root on C5.
    pub fn fresh() -> State {
        State { path: String::new(), root: 60 }
    }
}

/// The persisted field of the state JSON.
pub const STATE_FIELD: &str = "state";

pub struct GranParams {
    pub grain: IntParam,
    pub spacing: IntParam,
    pub random: IntParam,
    pub pan: IntParam,
    pub hold: IntParam,
    pub pitch: IntParam,
    pub attack: IntParam,
    pub release: IntParam,
    pub reverse: BoolParam,
    /// The saved state (the sample file and the root key).
    pub raw: RwLock<String>,
    /// Bumped on every `raw` change.
    pub raw_version: AtomicU64,
    /// Editor edits the host has not confirmed yet (the sound hears them at once).
    pub live: Arc<LiveEdits>,
}

impl Default for GranParams {
    fn default() -> Self {
        let st = State::fresh();
        let int = |name: &'static str, v: i32, range: (i32, i32), text: fn(i32) -> String| {
            IntParam::new(name, v, IntRange::Linear { min: range.0, max: range.1 })
                .with_value_to_string(Arc::new(move |v| text(v)))
                .with_string_to_value(Arc::new(move |s| parse_number(s, range)))
        };
        GranParams {
            grain: int(NAMES[0].1, 50, GRAIN_RANGE, ms_text),
            spacing: int(NAMES[1].1, 25, SPACING_RANGE, ms_text),
            random: int(NAMES[2].1, 0, SHARE_RANGE, share_text),
            pan: int(NAMES[3].1, 0, SHARE_RANGE, share_text),
            hold: int(NAMES[4].1, 80, SHARE_RANGE, share_text),
            pitch: int(NAMES[5].1, 0, PITCH_RANGE, pitch_text),
            attack: int(NAMES[6].1, 0, FADE_RANGE, ms_text),
            release: int(NAMES[7].1, 0, FADE_RANGE, ms_text),
            reverse: BoolParam::new(NAMES[8].1, false),
            raw: RwLock::new(serde_json::to_string(&st).unwrap_or_default()),
            raw_version: AtomicU64::new(0),
            live: Arc::default(),
        }
    }
}

impl GranParams {
    /// The values the sound plays (an editor edit the host has not confirmed yet wins).
    pub fn values(&self) -> Values {
        let l = &self.live;
        Values {
            grain: l.value(&self.grain),
            spacing: l.value(&self.spacing),
            random: l.value(&self.random),
            pan: l.value(&self.pan),
            hold: l.value(&self.hold),
            pitch: l.value(&self.pitch),
            attack: l.value(&self.attack),
            release: l.value(&self.release),
            reverse: l.value(&self.reverse),
        }
    }

    /// Replaces the saved state.
    pub fn set_state(&self, st: &State) {
        if let Ok(mut r) = self.raw.write() {
            *r = serde_json::to_string(st).unwrap_or_default();
        }
        self.raw_version.fetch_add(1, Ordering::AcqRel);
    }

    /// The saved state parsed (a new instance's when it cannot be read).
    pub fn state(&self) -> State {
        let raw = self.raw.read().map(|r| r.clone()).unwrap_or_default();
        serde_json::from_str(&raw).unwrap_or_else(|_| State::fresh())
    }
}

unsafe impl Params for GranParams {
    fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
        let ptrs = [
            self.grain.as_ptr(),
            self.spacing.as_ptr(),
            self.random.as_ptr(),
            self.pan.as_ptr(),
            self.hold.as_ptr(),
            self.pitch.as_ptr(),
            self.attack.as_ptr(),
            self.release.as_ptr(),
            self.reverse.as_ptr(),
        ];
        NAMES.iter().zip(ptrs).map(|((id, _), p)| (id.to_string(), p, String::new())).collect()
    }

    fn serialize_fields(&self) -> BTreeMap<String, String> {
        let raw = self.raw.read().map(|r| r.clone()).unwrap_or_default();
        BTreeMap::from([(STATE_FIELD.to_string(), format!("\"{raw}\""))])
    }

    fn deserialize_fields(&self, serialized: &BTreeMap<String, String>) {
        if let Some(s) = serialized.get(STATE_FIELD) {
            let s = s.trim_matches('"');
            if !s.is_empty()
                && let Ok(st) = serde_json::from_str::<State>(s)
            {
                self.set_state(&st);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_survives_the_plugin_state() {
        let params = GranParams::default();
        let st = State { path: "a.wav".into(), root: 48 };
        params.set_state(&st);
        let fields = params.serialize_fields();
        // The field is a JSON string with quoted quotes, the way nih-plug stores strings.
        let s = &fields[STATE_FIELD];
        assert!(s.starts_with('"') && s.contains("{\"path\""), "{s}");
        let fresh = GranParams::default();
        fresh.deserialize_fields(&fields);
        assert_eq!(fresh.state(), st);
        assert!(fresh.raw_version.load(Ordering::Acquire) > 0);
        // Junk is a fresh state, not a panic.
        let junk = GranParams::default();
        let mut bad = BTreeMap::new();
        bad.insert(STATE_FIELD.to_string(), "\"{not json\"".to_string());
        junk.deserialize_fields(&bad);
        assert_eq!(junk.state(), State::fresh());
    }

    #[test]
    fn the_parameters_text_and_parse_their_values() {
        assert_eq!(NAMES.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [GRAIN_ID, SPACING_ID, RANDOM_ID, PAN_ID, HOLD_ID, PITCH_ID, ATTACK_ID, RELEASE_ID, REVERSE_ID]);
        assert_eq!((ms_text(50), ms_text(1500)), ("50ms".into(), "1.50s".into()));
        assert_eq!((share_text(0), share_text(100)), ("0%".into(), "100%".into()));
        assert_eq!(pitch_text(25), "+25 cents");
        let r = GRAIN_RANGE;
        assert_eq!((parse_number("120ms", r), parse_number("9900ms", r), parse_number("x", r)), (Some(120), Some(2000), None));
        let v = GranParams::default().values();
        assert_eq!(
            (v.grain, v.spacing, v.random, v.pan, v.hold, v.pitch, v.attack, v.release, v.reverse),
            (50, 25, 0, 0, 80, 0, 0, 0, false)
        );
    }
}
