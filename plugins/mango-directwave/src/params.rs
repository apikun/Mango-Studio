// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango DirectWave's saved state: the program (the master and the zones) as a JSON string. No automatable
//! parameters: the settings live in the program, the way the drum sampler's live in its state.
//!
// spec: plugins/mango-directwave.md

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use nih_plug::prelude::*;

use crate::program::Program;

pub const PROGRAM_FIELD: &str = "program";

pub struct WaveParams {
    /// The program (the master and the zones), as JSON.
    pub raw: RwLock<String>,
    /// Bumped on every `raw` change.
    pub raw_version: AtomicU64,
}

impl Default for WaveParams {
    fn default() -> Self {
        WaveParams { raw: RwLock::new(String::new()), raw_version: AtomicU64::new(0) }
    }
}

unsafe impl Params for WaveParams {
    fn param_map(&self) -> Vec<(String, ParamPtr, String)> {
        Vec::new()
    }

    fn serialize_fields(&self) -> BTreeMap<String, String> {
        let raw = self.raw.read().map(|r| r.clone()).unwrap_or_default();
        BTreeMap::from([(PROGRAM_FIELD.to_string(), format!("\"{raw}\""))])
    }

    fn deserialize_fields(&self, serialized: &BTreeMap<String, String>) {
        if let Some(s) = serialized.get(PROGRAM_FIELD) {
            let s = s.trim_matches('"');
            if !s.is_empty()
                && let Ok(mut r) = self.raw.write()
            {
                *r = s.to_string();
                self.raw_version.fetch_add(1, Ordering::AcqRel);
            }
        }
    }
}

impl WaveParams {
    pub fn set_program(&self, p: &Program) {
        if let Ok(mut r) = self.raw.write() {
            *r = serde_json::to_string(p).unwrap_or_default();
        }
        self.raw_version.fetch_add(1, Ordering::AcqRel);
    }

    /// The program parsed (a new instance's when it cannot be read).
    pub fn program(&self) -> Program {
        let raw = self.raw.read().map(|r| r.clone()).unwrap_or_default();
        serde_json::from_str(&raw).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::{Master, Zone};

    #[test]
    fn the_program_survives_the_plugin_state() {
        let p = Program {
            master: Master { gain: 300, cut: 96, ..Master::default() },
            zones: vec![Zone { path: "a.wav".into(), root: 48, ..Zone::default() }],
        };
        let params = WaveParams::default();
        params.set_program(&p);
        let fields = params.serialize_fields();
        // The field is a JSON string with quoted quotes, the way nih-plug stores strings.
        let s = &fields[PROGRAM_FIELD];
        assert!(s.starts_with('"') && s.contains("{\"master\""), "{s}");
        let fresh = WaveParams::default();
        fresh.deserialize_fields(&fields);
        assert_eq!(fresh.program(), p);
        assert!(fresh.raw_version.load(Ordering::Acquire) > 0);
        // Junk is a fresh program, not a panic.
        let junk = WaveParams::default();
        let mut bad = BTreeMap::new();
        bad.insert(PROGRAM_FIELD.to_string(), "\"{not json\"".to_string());
        junk.deserialize_fields(&bad);
        assert_eq!(junk.program(), Program::default());
    }
}
