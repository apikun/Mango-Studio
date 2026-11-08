// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Renders every small generator's editor offscreen (no window, no audio) to PNG files, so a UI can be looked at
//! without the app: `cargo run -p lemon-small-gen --example shots -- <out-dir>` writes `<out-dir>/<plugin>.png`.

use lemon_small_gen::view::{self, Model, ViewState};
use os_plugkit::palette;

/// A model at every setting's start value.
struct Start<'a>(&'a lemon_small_gen::GenSpec);

impl Model for Start<'_> {
    fn norm(&self, id: &str) -> f32 {
        match self.0.settings.iter().find(|s| s.id == id) {
            Some(s) => {
                let (min, max) = s.range;
                (s.start - min) as f32 / (max - min) as f32
            }
            None => 0.0,
        }
    }

    fn plain(&self, id: &str) -> f32 {
        match self.0.settings.iter().find(|s| s.id == id) {
            Some(s) => s.start as f32,
            None => 0.0,
        }
    }

    fn text(&self, id: &str) -> String {
        match self.0.settings.iter().enumerate().find(|(_, s)| s.id == id) {
            Some((k, s)) => (self.0.text)(k, s.start),
            None => String::new(),
        }
    }
}

/// The plugins in the module order.
const SPECS: &[&str] = &[
    "acid",
    "monk",
    "surface",
    "flex",
    "boobass",
    "fruit_kick",
    "lemon_boom",
    "autogun",
    "drums",
    "dx",
    "keys",
    "mini",
    "pluck",
    "slayer",
    "dashboard",
];

fn spec(name: &str) -> &'static lemon_small_gen::GenSpec {
    match name {
        "acid" => lemon_small_gen::acid::SPEC,
        "monk" => lemon_small_gen::monk::SPEC,
        "surface" => lemon_small_gen::surface::SPEC,
        "flex" => lemon_small_gen::flex::SPEC,
        "boobass" => lemon_small_gen::boobass::SPEC,
        "fruit_kick" => lemon_small_gen::fruit_kick::SPEC,
        "lemon_boom" => lemon_small_gen::lemon_boom::SPEC,
        "autogun" => lemon_small_gen::autogun::SPEC,
        "drums" => lemon_small_gen::drums::SPEC,
        "dx" => lemon_small_gen::dx::SPEC,
        "keys" => lemon_small_gen::keys::SPEC,
        "mini" => lemon_small_gen::mini::SPEC,
        "pluck" => lemon_small_gen::pluck::SPEC,
        "slayer" => lemon_small_gen::slayer::SPEC,
        "dashboard" => lemon_small_gen::dashboard::SPEC,
        _ => panic!("unknown plugin {name}"),
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
