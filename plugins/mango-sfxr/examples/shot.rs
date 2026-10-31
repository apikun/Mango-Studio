// Copyright (C) 2026 mikobuntu
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Renders the Mango Sfxr editor offscreen:
//! `cargo run -p mango-sfxr --example shot -- out.png [roll] [x,y]`.
//! `roll` rolls the Explosion preset onto the sliders first; `x,y` = mouse position (hover looks).

use std::sync::Arc;

use mango_sfxr::dsp;
use mango_sfxr::editor::Logic;
use mango_sfxr::params::SfxrParams;
use mango_sfxr::{view, Shared};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| args.get(i).map(String::as_str).filter(|a| !a.is_empty() && *a != "-");
    let out = arg(1).unwrap_or("mango-sfxr.png");
    let mut l = Logic::new(Arc::new(SfxrParams::default()), Arc::new(Shared::default()), None);
    if arg(2) == Some("roll") {
        l.generate(dsp::Preset::Explosion);
    }
    let scale: f32 = std::env::var("OS_SHOT_SCALE").ok().and_then(|v| v.parse().ok()).filter(|k: &f32| *k > 0.0).unwrap_or(1.0);
    let s = |v: u32| (v as f32 * scale).round() as u32;
    let mut c = os_ui::Canvas::new(s(view::WIDTH), s(view::HEIGHT), scale).unwrap();
    let paint = |l: &mut Logic, c: &mut os_ui::Canvas| view::paint(c, &l.display());
    paint(&mut l, &mut c);
    if let Some((x, y)) = arg(3).and_then(|m| m.split_once(',')).and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))) {
        l.moved(x, y);
        paint(&mut l, &mut c);
    }
    let mut buf = vec![0u32; (s(view::WIDTH) * s(view::HEIGHT)) as usize];
    c.copy_to_xrgb(&mut buf);
    let mut pm = tiny_skia::Pixmap::new(s(view::WIDTH), s(view::HEIGHT)).unwrap();
    for (i, px) in pm.pixels_mut().iter_mut().enumerate() {
        let v = buf[i];
        *px = tiny_skia::PremultipliedColorU8::from_rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, 255).unwrap();
    }
    pm.save_png(out).unwrap();
}
