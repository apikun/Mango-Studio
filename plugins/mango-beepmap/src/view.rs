// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

use os_theme::design::lemon as brand;
use os_ui::kit::lemon::plug;
use os_ui::{Canvas, Color, Font, Rect};

pub const WIDTH: u32 = 620;
pub const HEIGHT: u32 = 340;

const LABEL: Font = Font::new(12);
const SMALL: Font = Font::new(10);
const TITLE: Font = Font::new(20).bold();
const SECTION: Font = Font::new(11).bold();
const RACK_PANEL: Color = Color::rgb(0x141414);
/// The picture the editor shows, and the load button beside it.
pub const PICTURE: Rect = Rect::new(26.0, 64.0, 340.0, 240.0);
pub const LOAD: Rect = Rect::new(380.0, 64.0, 96.0, 34.0);
/// The six knobs: two rows of three.
const KNOB_CENTERS: [(f32, f32); 6] = [
    (418.0, 158.0),
    (498.0, 158.0),
    (578.0, 158.0),
    (418.0, 262.0),
    (498.0, 262.0),
    (578.0, 262.0),
];
const KNOB_R: f32 = 26.0;
const KNOB_CAPTIONS: [&str; 6] = ["TIME", "OCT", "BRIGHT", "GATE", "TONE", "VOL"];

#[derive(Clone, Debug, Default)]
pub struct Control {
    pub norm: f32,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Display {
    /// The image the editor shows: `(w, h, RGB pixels)`.
    pub picture: Option<(usize, usize, std::sync::Arc<Vec<u32>>)>,
    pub path: String,
    pub status: String,
    pub controls: [Control; 6],
    pub mouse: Option<(f32, f32)>,
}

pub fn knob_region(index: usize) -> Rect {
    let (cx, cy) = KNOB_CENTERS[index];
    Rect::new(cx - KNOB_R - 4.0, cy - KNOB_R - 16.0, (KNOB_R + 4.0) * 2.0, (KNOB_R + 4.0) * 2.0 + 14.0)
}

pub fn knob_norm_at(index: usize, y: f32) -> f32 {
    let r = knob_region(index);
    (1.0 - (y - (r.y + 16.0)) / (KNOB_R * 2.0)).clamp(0.0, 1.0)
}

fn centered(c: &mut Canvas, font: Font, x: f32, y: f32, color: Color, text: &str) {
    let width = c.text_width(font, text);
    let height = c.line_height(font);
    c.text(font, (x - width / 2.0).round(), (y - height / 2.0).round(), color, text);
}

fn button(c: &mut Canvas, r: Rect, label: &str, hover: bool) {
    let fill = if hover { Color::rgb(0x484848) } else { Color::rgb(0x303030) };
    c.fill_rrect(r, 5.0, fill);
    c.stroke_rrect(r, 5.0, 1.0, Color::rgb(0x555555));
    centered(c, LABEL, r.cx(), r.cy(), brand::TEXT, label);
}

fn knob(c: &mut Canvas, index: usize, d: &Display) {
    let control = &d.controls[index];
    let hover = d.mouse.is_some_and(|(mx, my)| knob_region(index).contains(mx, my));
    let (cx, cy) = KNOB_CENTERS[index];
    let face = Color::rgb(if hover { 0x393939 } else { 0x2b2b2b });
    c.fill_circle(cx, cy, KNOB_R, face);
    c.stroke_circle(cx, cy, KNOB_R, 1.5, Color::rgb(0x555555));
    let angle = std::f32::consts::PI * (0.75 + 1.5 * control.norm.clamp(0.0, 1.0));
    let (sin, cos) = angle.sin_cos();
    c.line(cx + cos * 6.0, cy + sin * 6.0, cx + cos * (KNOB_R - 4.0), cy + sin * (KNOB_R - 4.0), 2.0, brand::MAIN);
    centered(c, SMALL, cx, cy - KNOB_R - 9.0, brand::TEXT, KNOB_CAPTIONS[index]);
    centered(c, SMALL, cx, cy + KNOB_R + 11.0, plug::DIM, &control.text);
}

pub fn paint(c: &mut Canvas, d: &Display) {
    c.begin_frame(None);
    c.clear(plug::BG);
    let panel = Rect::new(12.0, 12.0, 596.0, 316.0);
    c.fill_rect(panel, RACK_PANEL);
    c.stroke_rrect(panel, 0.0, 1.0, Color::rgb(0x3c3c3c));

    c.text(TITLE, 28.0, 22.0, brand::MAIN, "MANGO BEEPMAP");
    c.text(SMALL, 220.0, 26.0, plug::DIM, "IMAGE SYNTHESIZER");

    // The picture, or the well that asks for one.
    c.fill_rrect(PICTURE, 4.0, Color::rgb(0x1e1e1e));
    c.stroke_rrect(PICTURE, 4.0, 1.0, Color::rgb(0x444444));
    match &d.picture {
        Some((w, h, px)) => c.image(PICTURE, *w, *h, px),
        None => centered(c, SECTION, PICTURE.cx(), PICTURE.cy(), plug::DIM, "LOAD AN IMAGE TO SING IT"),
    }
    button(c, LOAD, "LOAD IMAGE", d.mouse.is_some_and(|(x, y)| LOAD.contains(x, y)));

    for i in 0..6 {
        knob(c, i, d);
    }

    let name = if d.path.is_empty() { "No image loaded".to_string() } else { d.path.rsplit(['/', '\\']).next().unwrap_or(&d.path).to_string() };
    let status = if d.status.is_empty() {
        if d.path.is_empty() { "A picture sings: its columns are time, its rows are pitch, a pixel's brightness is loudness.".into() } else { name }
    } else {
        d.status.clone()
    };
    c.text_clip(SMALL, 26.0, 314.0, 560.0, plug::DIM, &status);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knob_hit_regions_map_top_to_maximum_and_bottom_to_minimum() {
        for index in 0..6 {
            let region = knob_region(index);
            let mid = region.y + 16.0;
            assert_eq!(knob_norm_at(index, mid), 1.0);
            assert_eq!(knob_norm_at(index, mid + region.h - 16.0), 0.0);
        }
    }

    #[test]
    fn editor_paints_at_its_declared_size() {
        let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0).expect("editor canvas");
        paint(&mut canvas, &Display::default());
        assert_eq!(canvas.pixmap.width(), WIDTH);
        assert_eq!(canvas.pixmap.height(), HEIGHT);
    }

    /// A picture shows: the well fills with the image's pixels stretched over it.
    #[test]
    fn the_picture_paints() {
        let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0).expect("editor canvas");
        let px = vec![0xffa33au32; 4 * 4];
        let d = Display { picture: Some((4, 4, std::sync::Arc::new(px))), ..Display::default() };
        paint(&mut canvas, &d);
        assert_eq!(canvas.pixmap.width(), WIDTH);
    }
}
