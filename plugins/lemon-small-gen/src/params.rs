// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The parameters and saved state of a small effect.
//!
//! The settings come in the original's order (setting k = int k of its saved state, unless the description maps them:
//! [`GenSpec::fields`]) with the original's editor ranges. A copy of the whole saved state is persisted next to them (`state`): stored values outside a setting's
//! range keep their bytes and play as stored (the originals use stored values as they are).
//!
// spec: plugins/lemon-small-generators.md (§4 magnets and typed values, §5 parameters and state), plugins/lemon-kick.md

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use nih_plug::prelude::*;
use os_plugkit::live::LiveEdits;

use crate::{GenSpec, MAX};

/// How near (a share of the range) a drag has to come to a magnet to land on it.
pub const MAGNET_ZONE: f32 = 0.015;

pub struct GenParams {
    pub spec: &'static GenSpec,
    /// The settings in the original's order.
    pub p: Vec<IntParam>,
    /// The original's saved state the parameters came from.
    pub raw: RwLock<Vec<u8>>,
    /// Bumped on every `raw` change.
    pub raw_version: AtomicU64,
    /// Editor edits the host has not confirmed yet (the sound hears them at once).
    pub live: Arc<LiveEdits>,
}

impl GenParams {
    pub fn new(spec: &'static GenSpec) -> GenParams {
        let p = spec
            .settings
            .iter()
            .enumerate()
            .map(|(k, s)| {
                let (text, parse) = (spec.text, spec.parse);
                IntParam::new(
                    s.name,
                    s.start,
                    IntRange::Linear {
                        min: s.range.0,
                        max: s.range.1,
                    },
                )
                .with_value_to_string(Arc::new(move |v| text(k, v)))
                .with_string_to_value(Arc::new(move |t| parse(k, t)))
            })
            .collect();
        GenParams {
            spec,
            p,
            raw: RwLock::new(default_state(spec)),
            raw_version: AtomicU64::new(0),
            live: Arc::default(),
        }
    }

    /// Setting `k`'s value for the sound (an editor edit the host has not confirmed yet wins).
    pub fn v(&self, k: usize) -> i32 {
        self.live.value(&self.p[k])
    }

    /// The settings the sound plays (the first `n` of `MAX`): settings whose stored value still means the same keep
    /// it (values outside a setting's range are used as they are).
    pub fn settings(&self, raw: &[u8]) -> [i32; MAX] {
        std::array::from_fn(|k| {
            if k >= self.p.len() {
                return 0;
            }
            let v = self.v(k);
            let s = setting(self.spec, raw, k);
            if decode(self.spec, k, s) == v { s } else { v }
        })
    }

    pub fn set_raw(&self, b: Vec<u8>) {
        if let Ok(mut r) = self.raw.write() {
            *r = b;
        }
        self.raw_version.fetch_add(1, Ordering::AcqRel);
    }

    pub fn raw_copy(&self) -> Vec<u8> {
        self.raw
            .read()
            .map(|b| b.clone())
            .unwrap_or_else(|_| default_state(self.spec))
    }

    /// The original's saved state of the current settings.
    pub fn fl_state(&self) -> Vec<u8> {
        let vals: Vec<i32> = self.p.iter().map(|p| p.value()).collect();
        to_fl(self.spec, &vals, &self.raw_copy())
    }
}

/// The persisted field holding `raw`.
pub const STATE_FIELD: &str = "state";

unsafe impl Params for GenParams {
    fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
        self.p
            .iter()
            .zip(self.spec.settings)
            .map(|(p, s)| (s.id.to_string(), p.as_ptr(), String::new()))
            .collect()
    }

    fn serialize_fields(&self) -> BTreeMap<String, String> {
        let hex = self.raw.read().map(|b| hex(&b)).unwrap_or_default();
        BTreeMap::from([(STATE_FIELD.to_string(), format!("\"{hex}\""))])
    }

    fn deserialize_fields(&self, serialized: &BTreeMap<String, String>) {
        if let Some(bytes) = serialized
            .get(STATE_FIELD)
            .and_then(|s| from_hex(s.trim_matches('"')))
        {
            self.set_raw(normalized(self.spec, bytes));
        }
    }
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    (s.len() % 2 == 0).then_some(())?;
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The int the first setting is (1 after a version field, else 0).
pub fn first(spec: &GenSpec) -> usize {
    usize::from(spec.version.is_some())
}

/// The int (after the version field) setting `k` is.
pub fn int_of(spec: &GenSpec, k: usize) -> usize {
    spec.fields.get(k).copied().unwrap_or(k)
}

/// The ints the saved state has for its settings (after the version field): one per setting, and those the
/// description leaves out.
pub fn ints(spec: &GenSpec) -> usize {
    spec.fields
        .iter()
        .map(|&i| i + 1)
        .max()
        .unwrap_or(0)
        .max(spec.settings.len())
}

/// The shortest readable saved state: the version field, if any, and the settings' ints.
pub fn state_len(spec: &GenSpec) -> usize {
    4 * (first(spec) + ints(spec))
}

/// A readable saved state as it is (longer ones keep their tail); a short one, or one of a later version (unless every
/// version is read), gives the start.
pub fn normalized(spec: &GenSpec, b: Vec<u8>) -> Vec<u8> {
    let version_read = spec.any_version || spec.version.is_none_or(|v| field(&b, 0) <= v);
    if b.len() >= state_len(spec) && version_read {
        b
    } else {
        default_state(spec)
    }
}

/// Setting `k`'s stored value.
pub fn setting(spec: &GenSpec, b: &[u8], k: usize) -> i32 {
    field(b, first(spec) + int_of(spec, k))
}

pub fn set_setting(spec: &GenSpec, b: &mut [u8], k: usize, v: i32) {
    set_field(b, first(spec) + int_of(spec, k), v);
}

/// Little-endian int32 field `k`.
pub fn field(b: &[u8], k: usize) -> i32 {
    b.get(4 * k..4 * k + 4)
        .map_or(0, |s| i32::from_le_bytes(s.try_into().unwrap()))
}

pub fn set_field(b: &mut [u8], k: usize, v: i32) {
    if let Some(s) = b.get_mut(4 * k..4 * k + 4) {
        s.copy_from_slice(&v.to_le_bytes());
    }
}

/// A new instance's saved state (the version, every setting at its start, an int left out at 0, the tail).
pub fn default_state(spec: &GenSpec) -> Vec<u8> {
    let start = |i: usize| {
        (0..spec.settings.len())
            .find(|&k| int_of(spec, k) == i)
            .map_or(0, |k| spec.settings[k].start)
    };
    let starts = (0..ints(spec)).flat_map(|i| start(i).to_le_bytes());
    spec.version
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .chain(starts)
        .chain(spec.tail.iter().copied())
        .collect()
}

/// Setting `k`'s parameter value for a stored field value.
pub fn decode(spec: &GenSpec, k: usize, s: i32) -> i32 {
    let (min, max) = spec.settings[k].range;
    s.clamp(min, max)
}

/// The original's saved state with values `vals`: fields whose value still means the same keep their stored bytes
/// (out-of-range values), the rest are written from the parameters.
pub fn to_fl(spec: &GenSpec, vals: &[i32], raw: &[u8]) -> Vec<u8> {
    let mut b = normalized(spec, raw.to_vec());
    for (k, &v) in vals.iter().enumerate().take(spec.settings.len()) {
        if decode(spec, k, setting(spec, &b, k)) != v {
            set_setting(spec, &mut b, k, v);
        }
    }
    b
}

/// Parameter values of a saved state.
pub fn values_of(spec: &GenSpec, fl: &[u8]) -> Vec<i32> {
    (0..spec.settings.len())
        .map(|k| decode(spec, k, setting(spec, fl, k)))
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------
// Snapping (Lemon Studio choice, spec §4 of each plugin page)

/// Where a drag at the stored value `plain` lands: on the setting's magnet when within `MAGNET_ZONE` of its range;
/// `None` = where the drag is.
pub fn snap(spec: &GenSpec, k: usize, plain: f32) -> Option<i32> {
    let s = &spec.settings[k];
    let m = s.magnet?;
    ((plain - m as f32).abs() <= MAGNET_ZONE * (s.range.1 - s.range.0) as f32).then_some(m)
}

/// The wheel's next stored value from `v` given the plain step `next`: a setting with a magnet stops on it when the
/// step would pass it.
pub fn snap_step(spec: &GenSpec, k: usize, v: i32, next: i32) -> i32 {
    match spec.settings[k].magnet {
        Some(m) if (v < m && m <= next) || (next <= m && m < v) => m,
        _ => next,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Value texts shared by the effects (Lemon Studio choices, spec §3 of each plugin page)

/// A gain as `<dB>dB` with one decimal (`-INFdB` for silence).
pub fn db_text(gain: f32) -> String {
    if gain <= 0.0 {
        "-INFdB".into()
    } else {
        format!("{:.1}dB", 20.0 * f64::from(gain).log10())
    }
}

/// The number a typed text starts with.
pub fn leading_number(s: &str) -> Option<f64> {
    let lead: String = s
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        .collect();
    lead.parse().ok()
}

/// The value in `range` whose number (`None` = it has none) is nearest to `x`.
pub fn nearest(range: (i32, i32), x: f64, number: impl Fn(i32) -> Option<f64>) -> Option<i32> {
    (range.0..=range.1)
        .filter_map(|v| number(v).map(|n| (v, (n - x).abs())))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(v, _)| v)
}

/// The value in `range` whose number is nearest to `x`, for a `number` that never falls as the value rises (a binary
/// search: for the long ranges).
pub fn nearest_rising(range: (i32, i32), x: f64, number: impl Fn(i32) -> f64) -> i32 {
    let (mut lo, mut hi) = range;
    // The first value whose number reaches x.
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if number(mid) < x {
            lo = mid + 1
        } else {
            hi = mid
        }
    }
    if lo > range.0 && (x - number(lo - 1)).abs() <= (number(lo) - x).abs() {
        lo - 1
    } else {
        lo
    }
}

/// A typed gain: `-inf` (or `off`) gives the value of silence, a number of dB the nearest value.
pub fn parse_db(range: (i32, i32), s: &str, gain: impl Fn(i32) -> f32) -> Option<i32> {
    let s = s.trim().to_ascii_lowercase();
    if s.contains("inf") || s == "off" {
        return (range.0..=range.1).find(|&v| gain(v) <= 0.0);
    }
    let x = leading_number(&s)?;
    nearest(range, x, |v| {
        let g = gain(v);
        (g > 0.0).then(|| 20.0 * f64::from(g).log10())
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Plugin state <-> the original's saved state

/// The plugin's own saved state (nih-plug JSON with the u64 length prefix of its CLAP state) for a slot's saved state.
pub fn plugin_state_from_fl(spec: &GenSpec, fl: &[u8]) -> Vec<u8> {
    let b = normalized(spec, fl.to_vec());
    let params: Vec<String> = values_of(spec, &b)
        .iter()
        .zip(spec.settings)
        .map(|(v, s)| format!("\"{}\":{{\"i32\":{v}}}", s.id))
        .collect();
    let state = format!(
        "{{\"version\":\"{}\",\"params\":{{{}}},\"fields\":{{\"{STATE_FIELD}\":\"\\\"{}\\\"\"}}}}",
        env!("CARGO_PKG_VERSION"),
        params.join(","),
        hex(&b)
    );
    let mut blob = (state.len() as u64).to_le_bytes().to_vec();
    blob.extend_from_slice(state.as_bytes());
    blob
}

/// The original's saved state for a plugin state blob (see [`plugin_state_from_fl`]); `None` when it is not one.
pub fn fl_from_plugin_state(spec: &GenSpec, blob: &[u8]) -> Option<Vec<u8>> {
    let json = if blob.len() > 8 && blob[0] != b'{' {
        &blob[8..]
    } else {
        blob
    };
    let v: serde_json::Value = serde_json::from_str(std::str::from_utf8(json).ok()?).ok()?;
    let raw = normalized(
        spec,
        from_hex(
            v.get("fields")?
                .get(STATE_FIELD)?
                .as_str()?
                .trim_matches('"'),
        )?,
    );
    let params = v.get("params")?;
    let mut vals = values_of(spec, &raw);
    for (k, s) in spec.settings.iter().enumerate() {
        if let Some(x) = params
            .get(s.id)
            .and_then(|p| p.get("i32"))
            .and_then(|x| x.as_i64())
        {
            vals[k] = x as i32;
        }
    }
    Some(to_fl(spec, &vals, &raw))
}
