// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

use os_theme::design::lemon as brand;
use os_ui::kit::lemon::plug;
use os_ui::{Canvas, Color, Font, Rect};

pub const WIDTH: u32 = 620;
pub const HEIGHT: u32 = 520;

const LABEL: Font = Font::new(12);
const SMALL: Font = Font::new(10);
const TITLE: Font = Font::new(20).bold();
const SECTION: Font = Font::new(11).bold();
const RACK_PANEL: Color = Color::rgb(0x141414);
pub const VOLUME_TRACK: Rect = Rect::new(100.0, 192.0, 445.0, 8.0);
const ENV_TRACK: Rect = Rect::new(0.0, 358.0, 38.0, 91.0);
const ENV_X: [f32; 4] = [62.0, 126.0, 190.0, 254.0];
const FX_CENTERS: [(f32, f32); 2] = [(165.0, 270.0), (455.0, 270.0)];
const LFO_CENTERS: [(f32, f32); 3] = [(370.0, 366.0), (455.0, 366.0), (540.0, 366.0)];
const MISC_CENTERS: [(f32, f32); 2] = [(410.0, 453.0), (520.0, 453.0)];

#[derive(Clone, Debug, Default)]
pub struct Control {
    pub norm: f32,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Display {
    pub path: String,
    pub preset: String,
    pub bank: i32,
    pub index: usize,
    pub count: usize,
    pub status: String,
    pub volume: f32,
    pub controls: [Control; 11],
    pub mouse: Option<(f32, f32)>,
}

pub fn control_region(index: usize) -> Rect {
    match index {
        0 | 1 => {
            let (x, y) = FX_CENTERS[index];
            Rect::new(x - 26.0, y - 26.0, 52.0, 52.0)
        }
        2..=5 => Rect::new(ENV_X[index - 2] - 19.0, ENV_TRACK.y, ENV_TRACK.w, ENV_TRACK.h),
        6..=8 => {
            let (x, y) = LFO_CENTERS[index - 6];
            Rect::new(x - 23.0, y - 23.0, 46.0, 46.0)
        }
        9..=10 => {
            let (x, y) = MISC_CENTERS[index - 9];
            Rect::new(x - 20.0, y - 20.0, 40.0, 40.0)
        }
        _ => Rect::default(),
    }
}

pub fn control_norm_at(index: usize, y: f32) -> f32 {
    let region = control_region(index);
    (1.0 - (y - region.y) / region.h).clamp(0.0, 1.0)
}

pub fn volume_norm_at(x: f32) -> f32 {
    ((x - VOLUME_TRACK.x) / VOLUME_TRACK.w).clamp(0.0, 1.0)
}

fn centered(c: &mut Canvas, font: Font, x: f32, y: f32, color: Color, text: &str) {
    let width = c.text_width(font, text);
    let height = c.line_height(font);
    c.text(font, (x - width / 2.0).round(), (y - height / 2.0).round(), color, text);
}

fn card(c: &mut Canvas, rect: Rect, title: &str) {
    c.fill_rect(rect, RACK_PANEL);
    c.stroke_rrect(rect, 0.0, 1.0, Color::rgb(0x3c3c3c));
    c.text(SECTION, rect.x + 10.0, rect.y + 7.0, plug::DIM, title);
}

fn button(c: &mut Canvas, r: Rect, label: &str, hover: bool) {
    let fill = if hover { Color::rgb(0x484848) } else { Color::rgb(0x303030) };
    c.fill_rrect(r, 5.0, fill);
    c.stroke_rrect(r, 5.0, 1.0, Color::rgb(0x555555));
    centered(c, LABEL, r.cx(), r.cy(), brand::TEXT, label);
}

fn knob(c: &mut Canvas, index: usize, d: &Display, caption: &str, x: f32, y: f32, radius: f32) {
    let control = &d.controls[index];
    let hover = d.mouse.is_some_and(|(mx, my)| control_region(index).contains(mx, my));
    let face = Color::rgb(if hover { 0x393939 } else { 0x2b2b2b });
    c.fill_circle(x, y, radius, face);
    c.stroke_circle(x, y, radius, 1.5, Color::rgb(0x555555));
    let angle = std::f32::consts::PI * (0.75 + 1.5 * control.norm.clamp(0.0, 1.0));
    let (sin, cos) = angle.sin_cos();
    c.line(x + cos * 5.0, y + sin * 5.0, x + cos * (radius - 4.0), y + sin * (radius - 4.0), 2.0, brand::MAIN);
    centered(c, SMALL, x, y - radius - 14.0, brand::TEXT, caption);
    centered(c, SMALL, x, y + radius + 12.0, plug::DIM, &control.text);
}

fn fader(c: &mut Canvas, index: usize, d: &Display, caption: &str) {
    let region = control_region(index);
    let control = &d.controls[index];
    let hover = d.mouse.is_some_and(|(x, y)| region.contains(x, y));
    let track = Rect::new(region.x + 12.0, region.y, 14.0, region.h);
    c.fill_rrect(track, 4.0, Color::rgb(0x202020));
    let fill_height = track.h * control.norm.clamp(0.0, 1.0);
    c.fill_rrect(Rect::new(track.x, track.bottom() - fill_height, track.w, fill_height.max(2.0)), 4.0, brand::MAIN);
    let cap_y = track.bottom() - fill_height;
    c.fill_rrect(Rect::new(region.x + 3.0, cap_y - 4.0, region.w - 6.0, 8.0), 3.0, if hover { brand::TEXT } else { Color::rgb(0x8c8c8c) });
    centered(c, SMALL, region.cx(), region.y - 11.0, brand::TEXT, caption);
    centered(c, SMALL, region.cx(), region.bottom() + 10.0, plug::DIM, &control.text);
}

pub fn paint(c: &mut Canvas, d: &Display) {
    c.begin_frame(None);
    c.clear(plug::BG);
    let panel = Rect::new(12.0, 12.0, 596.0, 496.0);
    c.fill_rect(panel, RACK_PANEL);
    c.stroke_rrect(panel, 0.0, 1.0, Color::rgb(0x3c3c3c));

    c.text(TITLE, 28.0, 22.0, brand::MAIN, "MANGO FONT");
    c.text(SMALL, 30.0, 48.0, plug::DIM, "SOUNDFONT PLAYER");

    c.text(SMALL, 30.0, 66.0, plug::DIM, "SOUNDFONT");
    let path = if d.path.is_empty() { "No SoundFont loaded".to_string() } else { d.path.rsplit(['/', '\\']).next().unwrap_or(&d.path).to_string() };
    c.fill_rrect(Rect::new(26.0, 82.0, 470.0, 34.0), 4.0, Color::rgb(0x1e1e1e));
    c.stroke_rrect(Rect::new(26.0, 82.0, 470.0, 34.0), 4.0, 1.0, Color::rgb(0x444444));
    c.text_clip(LABEL, 36.0, 92.0, 450.0, brand::TEXT, &path);
    let load = Rect::new(510.0, 82.0, 78.0, 34.0);
    button(c, load, "LOAD SF2", d.mouse.is_some_and(|(x, y)| load.contains(x, y)));

    c.text(SMALL, 30.0, 126.0, plug::DIM, "BANK / PRESET");
    let (bank_text, preset_text) = if d.count == 0 {
        ("—".to_string(), "Choose a SoundFont to begin".to_string())
    } else {
        (d.bank.to_string(), format!("{}  ({}/{})", d.preset, d.index + 1, d.count))
    };
    c.fill_rrect(Rect::new(26.0, 142.0, 74.0, 36.0), 4.0, Color::rgb(0x1e1e1e));
    c.fill_rrect(Rect::new(108.0, 142.0, 388.0, 36.0), 4.0, Color::rgb(0x1e1e1e));
    c.stroke_rrect(Rect::new(26.0, 142.0, 74.0, 36.0), 4.0, 1.0, Color::rgb(0x444444));
    c.stroke_rrect(Rect::new(108.0, 142.0, 388.0, 36.0), 4.0, 1.0, Color::rgb(0x444444));
    centered(c, LABEL, 63.0, 160.0, brand::TEXT, &bank_text);
    c.text_clip(LABEL, 118.0, 153.0, 368.0, brand::TEXT, &preset_text);
    let prev = Rect::new(510.0, 142.0, 36.0, 36.0);
    let next = Rect::new(552.0, 142.0, 36.0, 36.0);
    button(c, prev, "<", d.mouse.is_some_and(|(x, y)| prev.contains(x, y)));
    button(c, next, ">", d.mouse.is_some_and(|(x, y)| next.contains(x, y)));

    c.text(SMALL, 30.0, 184.0, plug::DIM, "VOLUME");
    c.fill_rrect(VOLUME_TRACK, 4.0, Color::rgb(0x202020));
    let fill = VOLUME_TRACK.w * d.volume.clamp(0.0, 1.0);
    c.fill_rrect(Rect::new(VOLUME_TRACK.x, VOLUME_TRACK.y, fill.max(3.0), VOLUME_TRACK.h), 4.0, brand::MAIN);
    c.fill_circle(VOLUME_TRACK.x + fill, VOLUME_TRACK.cy(), 8.0, brand::TEXT);
    let pct = format!("{:.0}%", d.volume.clamp(0.0, 1.0) * 100.0);
    c.text(SMALL, 562.0, 184.0, brand::TEXT, &pct);

    let reverb = Rect::new(24.0, 212.0, 283.0, 96.0);
    let chorus = Rect::new(313.0, 212.0, 283.0, 96.0);
    card(c, reverb, "REVERB");
    card(c, chorus, "CHORUS");
    knob(c, 0, d, "SEND", FX_CENTERS[0].0, FX_CENTERS[0].1, 23.0);
    knob(c, 1, d, "SEND", FX_CENTERS[1].0, FX_CENTERS[1].1, 23.0);

    let envelope = Rect::new(24.0, 318.0, 283.0, 162.0);
    let lfo = Rect::new(313.0, 318.0, 283.0, 91.0);
    let misc = Rect::new(313.0, 419.0, 283.0, 61.0);
    card(c, envelope, "ENVELOPE");
    card(c, lfo, "LFO");
    card(c, misc, "MISC");
    for (i, caption) in ["A", "D", "S", "R"].into_iter().enumerate() {
        fader(c, i + 2, d, caption);
    }
    for (i, caption) in ["DEL", "AMT", "SPD"].into_iter().enumerate() {
        let (x, y) = LFO_CENTERS[i];
        knob(c, i + 6, d, caption, x, y, 18.0);
    }
    for (i, caption) in ["CUT", "MOD"].into_iter().enumerate() {
        let (x, y) = MISC_CENTERS[i];
        knob(c, i + 9, d, caption, x, y, 15.0);
    }

    let status = if d.status.is_empty() { "Choose an SF2 file, then use the preset arrows.".to_string() } else { d.status.clone() };
    c.text_clip(SMALL, 30.0, 494.0, 560.0, plug::DIM, &status);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_hit_regions_map_top_to_maximum_and_bottom_to_minimum() {
        for index in 0..11 {
            let region = control_region(index);
            assert_eq!(control_norm_at(index, region.y), 1.0);
            assert_eq!(control_norm_at(index, region.bottom()), 0.0);
        }
    }

    #[test]
    fn editor_paints_at_its_declared_size() {
        let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0).expect("editor canvas");
        paint(&mut canvas, &Display::default());
        assert_eq!(canvas.pixmap.width(), WIDTH);
        assert_eq!(canvas.pixmap.height(), HEIGHT);
    }
}
