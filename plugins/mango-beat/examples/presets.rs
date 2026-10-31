// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Writes the factory presets (`crates/os-app/factory-presets/Mango Beat/*.ospreset`): one Mango Beat state each,
//! the grids of a classic shape and the parameters at their defaults. Run from the repository root:
//!
//! ```text
//! cargo run -p mango-beat --example presets
//! ```

use std::path::Path;

use mango_beat::dsp;
use mango_beat::grid::{Curve, GridState, Pt};

/// The presets: (name, grids).
fn presets() -> Vec<(&'static str, GridState)> {
    let const_time = |rate: f32| {
        let mut g = GridState::default();
        g.time[0] = Curve::constant(GridState::y_of_rate(rate));
        g
    };
    let mut_with_time = |c: Curve| {
        let mut g = GridState::default();
        g.time[0] = c;
        g
    };
    let mut_with_vol = |c: Curve| {
        let mut g = GridState::default();
        g.volume[0] = c;
        g
    };
    // The scratch: eight straight runs, four times forwards, four times backwards.
    let mut scratch = Curve::default();
    for k in 0..=8 {
        let y = if k % 2 == 0 { 0.75 } else { 0.125 };
        scratch.pts.push(Pt::new(k as f32 / 8.0, y, 0.0, 0));
    }
    // The pump: a quick dip, a smooth climb back to the top.
    let pump = Curve {
        pts: vec![Pt::new(0.0, 1.0, -0.4, 0), Pt::new(0.3, 0.45, 0.0, 9), Pt::new(1.0, 1.0, 0.0, 9)],
    };
    let tape_stop = Curve { pts: vec![Pt::new(0.0, 0.5, 0.0, 0), Pt::new(1.0, 0.26, 0.7, 0)] };
    vec![
        ("Default", GridState::default()),
        ("Half Time", const_time(0.5)),
        ("Double Time", const_time(2.0)),
        ("Tape Stop", mut_with_time(tape_stop)),
        ("Reverse", const_time(-1.0)),
        ("Scratch", mut_with_time(scratch)),
        ("Trance Gate 1-8", mut_with_vol(dsp::gate(2))),
        ("Trance Gate 1-16", mut_with_vol(dsp::gate(4))),
        ("Triplet Chop", mut_with_vol(dsp::gate(3))),
        ("Sidechain Pump", mut_with_vol(pump)),
        ("Half Time Gate", {
            let mut g = const_time(0.5);
            g.volume[0] = dsp::gate(2);
            g
        }),
    ]
}

/// The plugin's saved state for `grids`: the parameters at their defaults and the grids in the state's field.
fn state(grids: &GridState) -> Vec<u8> {
    let json = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "params": {
            "time_mix": { "i32": 10000 },
            "vol_mix": { "i32": 10000 },
            "time_slot": { "i32": 0 },
            "vol_slot": { "i32": 0 },
        },
        "fields": { "grid": grids.to_json() },
    })
    .to_string();
    // The CLAP state form: the length in front of the JSON.
    let mut blob = (json.len() as u64).to_le_bytes().to_vec();
    blob.extend_from_slice(json.as_bytes());
    blob
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/os-app/factory-presets/Mango Beat");
    std::fs::create_dir_all(&dir).expect("preset folder");
    for (name, grids) in presets() {
        let header = format!("plugin=Mango Beat\tid=clap:org.openstudio.mango-beat\tformat=clap\n");
        let mut file = b"OSPRESET 1\n".to_vec();
        file.extend_from_slice(header.as_bytes());
        file.extend_from_slice(&state(&grids));
        let path = dir.join(format!("{name}.ospreset"));
        std::fs::write(&path, &file).expect("preset file");
        println!("wrote {}", path.display());
    }
}
