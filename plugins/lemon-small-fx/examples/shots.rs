// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Renders every small effect's editor offscreen to PNG files:
//! `cargo run -p lemon-small-fx --example shots -- <out-dir>`.

use lemon_small_fx::view::{self, Model, ViewState};
use os_plugkit::palette;

/// A model at every setting's start value.
struct Start<'a>(&'a lemon_small_fx::Spec);

impl Model for Start<'_> {
    fn norm(&self, id: &str) -> f32 {
        match self.0.index(id) {
            Some(k) => {
                let (min, max) = self.0.settings[k].range;
                (self.0.settings[k].start - min) as f32 / (max - min) as f32
            }
            None => 0.0,
        }
    }

    fn plain(&self, id: &str) -> f32 {
        match self.0.index(id) {
            Some(k) => self.0.settings[k].start as f32,
            None => 0.0,
        }
    }

    fn text(&self, id: &str) -> String {
        match self.0.index(id) {
            Some(k) => (self.0.text)(k, self.0.settings[k].start),
            None => String::new(),
        }
    }
}

const SPECS: &[&str] = &[
    "soft_clipper",
    "fast_distortion",
    "balance",
    "stereo_enhancer",
    "transient_processor",
    "chorus",
    "stereo_shaper",
    "filter",
    "flanger",
    "phaser",
    "crusher",
    "vocoder",
    "twelve",
    "oneknob",
    "otter",
    "peak_controller",
    "formula_controller",
];

fn spec(name: &str) -> &'static lemon_small_fx::Spec {
    match name {
        "soft_clipper" => lemon_small_fx::soft_clipper::SPEC,
        "fast_distortion" => lemon_small_fx::fast_distortion::SPEC,
        "balance" => lemon_small_fx::balance::SPEC,
        "stereo_enhancer" => lemon_small_fx::stereo_enhancer::SPEC,
        "transient_processor" => lemon_small_fx::transient_processor::SPEC,
        "chorus" => lemon_small_fx::chorus::SPEC,
        "stereo_shaper" => lemon_small_fx::stereo_shaper::SPEC,
        "filter" => lemon_small_fx::filter::SPEC,
        "flanger" => lemon_small_fx::flanger::SPEC,
        "phaser" => lemon_small_fx::phaser::SPEC,
        "crusher" => lemon_small_fx::crusher::SPEC,
        "vocoder" => lemon_small_fx::vocoder::SPEC,
        "twelve" => lemon_small_fx::twelve::SPEC,
        "oneknob" => lemon_small_fx::oneknob::SPEC,
        "otter" => lemon_small_fx::otter::SPEC,
        "peak_controller" => lemon_small_fx::peak_controller::SPEC,
        "formula_controller" => lemon_small_fx::formula_controller::SPEC,
        _ => panic!("unknown effect {name}"),
    }
}

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: shots <out-dir>");
        return;
    };
    std::fs::create_dir_all(&dir).expect("out dir");
    let t = palette();
    for name in SPECS {
        let s = spec(name);
        let mut c = os_ui::Canvas::new(s.size.0, s.size.1, 1.0).expect("canvas");
        view::paint(s, &mut c, &t, &Start(s), &ViewState::default(), "");
        let png = c.pixmap.encode_png().expect("encode");
        let out = format!("{dir}/{name}.png");
        std::fs::write(&out, png).expect("write");
        println!("{}", out);
    }
}
