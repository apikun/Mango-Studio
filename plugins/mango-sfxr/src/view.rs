// Copyright (C) 2026 mikobuntu
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Sfxr's editor picture: sfxr's own layout, drawn the Mango way — the waveform buttons and
//! the seven generators down the left edge, and the sliders grouped into section cards on the right
//! (Envelope, Tone, Vibrato, Change, Square, Repeat, Phaser, Filter). All painting, and the
//! geometry the editor's hit tests ask for.

use os_theme::design::lemon as brand;
use os_ui::kit::lemon::{plug, plug_button, plug_panel};
use os_ui::kit::State;
use os_ui::{Canvas, Color, Font, Rect};

pub const WIDTH: u32 = 720;
pub const HEIGHT: u32 = 500;

const TITLE: Font = Font::new(20).bold();
const SECTION: Font = Font::new(11).bold();
const LABEL: Font = Font::new(12);
const SMALL: Font = Font::new(10);
const TINY: Font = Font::new(9);

/// The key the editor's Play and generator buttons sound (A4: an effect at its natural speed).
pub const PREVIEW_KEY: u8 = 69;

/// The wave buttons' captions (sfxr's four waveforms).
pub const WAVE_CAPTIONS: [&str; 4] = ["SQR", "SAW", "SIN", "NOI"];

/// The generator buttons' captions.
pub const PRESET_LABELS: [&str; 7] = ["PICKUP/COIN", "LASER/SHOOT", "EXPLOSION", "POWER UP", "HIT/HURT", "JUMP", "BLIT/SELECT"];

/// The section cards: a caption and the sliders each holds (ids of `params::SLIDER_IDS`).
const CARDS: [(&str, &[&str]); 8] = [
    ("ENVELOPE", &["att", "hold", "sus", "dec"]),
    ("TONE", &["start_freq", "min_freq", "slide", "dslide"]),
    ("VIBRATO", &["vib_depth", "vib_speed"]),
    ("CHANGE", &["change_amt", "change_speed"]),
    ("SQUARE", &["sqr_duty", "sqr_sweep"]),
    ("REPEAT & VOLUME", &["repeat", "volume"]),
    ("PHASER", &["phaser_offset", "phaser_sweep"]),
    ("FILTER", &["lp_cut", "lp_sweep", "lp_reso", "hp_cut", "hp_sweep"]),
];

/// Each card's place: three rows of three, the filter card stretching over two columns.
const CARD_RECTS: [Rect; 8] = [
    Rect::new(188.0, 66.0, 158.0, 124.0),
    Rect::new(354.0, 66.0, 158.0, 124.0),
    Rect::new(520.0, 66.0, 176.0, 124.0),
    Rect::new(188.0, 202.0, 158.0, 124.0),
    Rect::new(354.0, 202.0, 158.0, 124.0),
    Rect::new(520.0, 202.0, 176.0, 124.0),
    Rect::new(188.0, 338.0, 158.0, 124.0),
    Rect::new(354.0, 338.0, 342.0, 124.0),
];

/// The waveform buttons (sfxr keeps them at the top of its left edge).
pub const WAVE_RECTS: [Rect; 4] = [
    Rect::new(28.0, 66.0, 34.0, 26.0),
    Rect::new(66.0, 66.0, 34.0, 26.0),
    Rect::new(104.0, 66.0, 34.0, 26.0),
    Rect::new(142.0, 66.0, 34.0, 26.0),
];

/// The seven generator buttons.
pub const PRESET_RECTS: [Rect; 7] = [
    Rect::new(28.0, 108.0, 148.0, 27.0),
    Rect::new(28.0, 140.0, 148.0, 27.0),
    Rect::new(28.0, 172.0, 148.0, 27.0),
    Rect::new(28.0, 204.0, 148.0, 27.0),
    Rect::new(28.0, 236.0, 148.0, 27.0),
    Rect::new(28.0, 268.0, 148.0, 27.0),
    Rect::new(28.0, 300.0, 148.0, 27.0),
];

/// The play button.
pub const PLAY_RECT: Rect = Rect::new(28.0, 340.0, 148.0, 34.0);

/// The sliders' track inside a card, relative to it: top and height.
const TRACK_TOP: f32 = 30.0;
const TRACK_H: f32 = 64.0;

/// What one slider shows: its value (normalised) and the parameter's own text for it.
#[derive(Clone, Debug, Default)]
pub struct Control {
    pub norm: f32,
    pub text: String,
}

/// What the editor paints.
#[derive(Clone, Debug, Default)]
pub struct Display {
    /// The waveform, normalised (its index over 3).
    pub wave: f32,
    /// One control per slider, in `params::SLIDER_IDS` order.
    pub controls: [Control; 23],
    pub mouse: Option<(f32, f32)>,
    /// The line under the cards (what the last click rolled, else the hint).
    pub status: String,
}

/// The slot rect of slider `id` (its card, and its column inside the card): the hit region a drag
/// starts in (the hit tests ask `param_at` instead; the tests use this one).
#[cfg_attr(not(test), allow(dead_code))]
pub fn param_region(id: &str) -> Rect {
    for (index, (_, ids)) in CARDS.iter().enumerate() {
        if let Some(column) = ids.iter().position(|candidate| *candidate == id) {
            let card = CARD_RECTS[index];
            let slot_w = (card.w - 16.0) / ids.len() as f32;
            return Rect::new(card.x + 8.0 + slot_w * column as f32, card.y, slot_w, card.h);
        }
    }
    Rect::new(0.0, 0.0, 0.0, 0.0)
}

/// The value a drag at window point `y` gives slider `id` (the track's top is the maximum).
pub fn param_norm_at(id: &str, y: f32) -> f32 {
    let Some(index) = CARDS.iter().position(|(_, ids)| ids.contains(&id)) else { return 0.0 };
    let track_y = CARD_RECTS[index].y + TRACK_TOP;
    (1.0 - (y - track_y) / TRACK_H).clamp(0.0, 1.0)
}

/// The slider (if any) under window point `(x, y)`.
pub fn param_at(x: f32, y: f32) -> Option<&'static str> {
    for (index, (_, ids)) in CARDS.iter().enumerate() {
        let card = CARD_RECTS[index];
        if !card.contains(x, y) {
            continue;
        }
        let slot_w = (card.w - 16.0) / ids.len() as f32;
        let column = ((x - card.x - 8.0) / slot_w).clamp(0.0, ids.len() as f32 - 0.001) as usize;
        return Some(ids[column]);
    }
    None
}

/// A slider's short caption (they fit the card's slot).
fn caption(id: &str) -> &'static str {
    match id {
        "att" => "ATTACK",
        "hold" => "SUSTAIN",
        "sus" => "PUNCH",
        "dec" => "DECAY",
        "start_freq" => "FREQ",
        "min_freq" => "MIN",
        "slide" => "SLIDE",
        "dslide" => "D-SLIDE",
        "vib_depth" => "DEPTH",
        "vib_speed" => "SPEED",
        "change_amt" => "AMOUNT",
        "change_speed" => "SPEED",
        "sqr_duty" => "DUTY",
        "sqr_sweep" => "SWEEP",
        "repeat" => "REPEAT",
        "phaser_offset" => "OFFSET",
        "phaser_sweep" => "SWEEP",
        "lp_cut" => "LP CUT",
        "lp_sweep" => "LP SWP",
        "lp_reso" => "RESO",
        "hp_cut" => "HP CUT",
        "hp_sweep" => "HP SWP",
        _ => "VOLUME",
    }
}

fn centered(c: &mut Canvas, font: Font, x: f32, y: f32, color: Color, text: &str) {
    let width = c.text_width(font, text);
    let height = c.line_height(font);
    c.text(font, (x - width / 2.0).round(), (y - height / 2.0).round(), color, text);
}

fn slider(c: &mut Canvas, card: Rect, column: usize, columns: usize, control: &Control, mouse: Option<(f32, f32)>) {
    let slot_w = (card.w - 16.0) / columns as f32;
    let cx = card.x + 8.0 + slot_w * (column as f32 + 0.5);
    let top = card.y + TRACK_TOP;
    let track = Rect::new(cx - 3.0, top, 6.0, TRACK_H);
    c.fill_rrect(track, 3.0, plug::WELL);
    c.stroke_rrect(track, 3.0, 1.0, plug::WELL_LINE);
    // The mango fill under the knob, and the knob itself.
    let knob_y = top + (1.0 - control.norm.clamp(0.0, 1.0)) * TRACK_H;
    if knob_y < top + TRACK_H {
        c.fill_rrect(Rect::new(cx - 3.0, knob_y, 6.0, top + TRACK_H - knob_y), 2.0, brand::DEEP);
    }
    let hover = mouse.is_some_and(|(mx, my)| Rect::new(cx - slot_w / 2.0, card.y, slot_w, card.h).contains(mx, my));
    let knob = Rect::new(cx - 11.0, knob_y - 5.0, 22.0, 10.0);
    c.fill_rrect(knob, 4.0, if hover { Color::rgb(0x3a3a3a) } else { plug::FACE });
    c.stroke_rrect(knob, 4.0, 1.0, plug::FACE_LINE);
    c.line(cx - 6.0, knob_y, cx + 6.0, knob_y, 2.0, brand::MAIN);
}

pub fn paint(c: &mut Canvas, d: &Display) {
    c.begin_frame(None);
    c.clear(plug::BG);
    plug_panel(c, Rect::new(12.0, 12.0, 696.0, 476.0));

    c.text(TITLE, 28.0, 22.0, brand::MAIN, "MANGO SFXR");
    let title_w = c.text_width(TITLE, "MANGO SFXR");
    c.text(SMALL, 30.0 + title_w, 28.0, plug::DIM, "RETRO SFX GENERATOR");

    // The waveform buttons: the current one lit.
    for (i, r) in WAVE_RECTS.into_iter().enumerate() {
        let on = ((d.wave * 3.0).round() as usize).clamp(0, 3) == i;
        let hover = d.mouse.is_some_and(|(x, y)| r.contains(x, y));
        let ink = plug_button(c, r, 6.0, State { hover, on, ..State::NORMAL });
        centered(c, SMALL, r.cx(), r.cy(), ink, WAVE_CAPTIONS[i]);
    }

    // The generator buttons, and Play under them.
    for (i, r) in PRESET_RECTS.into_iter().enumerate() {
        let hover = d.mouse.is_some_and(|(x, y)| r.contains(x, y));
        let ink = plug_button(c, r, 6.0, State::hover(hover));
        centered(c, SMALL, r.cx(), r.cy(), ink, PRESET_LABELS[i]);
    }
    let play_hover = d.mouse.is_some_and(|(x, y)| PLAY_RECT.contains(x, y));
    let play_ink = plug_button(c, PLAY_RECT, 6.0, State { hover: play_hover, on: true, ..State::NORMAL });
    centered(c, LABEL, PLAY_RECT.cx(), PLAY_RECT.cy(), play_ink, "PLAY");

    // The section cards with their sliders.
    for (index, (title, ids)) in CARDS.iter().enumerate() {
        let card = CARD_RECTS[index];
        plug_panel(c, card);
        c.text(SECTION, card.x + 10.0, card.y + 9.0, plug::DIM, title);
        let columns = ids.len();
        let slot_w = (card.w - 16.0) / columns as f32;
        for (column, id) in ids.iter().enumerate() {
            let control_index = crate::params::SLIDER_IDS.iter().position(|candidate| candidate == id).unwrap_or(0);
            let control = &d.controls[control_index];
            slider(c, card, column, columns, control, d.mouse);
            let cx = card.x + 8.0 + slot_w * (column as f32 + 0.5);
            centered(c, TINY, cx, card.y + 102.0, plug::FAINT, caption(id));
            centered(c, TINY, cx, card.y + 114.0, plug::DIM, &control.text);
        }
    }

    // The status line under the cards.
    let status =
        if d.status.is_empty() { "ROLL A GENERATOR, THEN PLAY NOTES - SPACE PLAYS, RIGHT-CLICK RESETS A SLIDER" } else { &d.status };
    c.text_clip(SMALL, 28.0, 474.0, 664.0, plug::DIM, status);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::SLIDER_IDS;

    #[test]
    fn every_slider_has_a_slot_caption_and_full_track_range() {
        for id in SLIDER_IDS {
            let region = param_region(id);
            assert!(region.w > 10.0, "{id}: no slot");
            assert!(!caption(id).is_empty(), "{id}: no caption");
            let mid = CARD_RECTS[CARDS.iter().position(|(_, ids)| ids.contains(&id)).unwrap()].y + TRACK_TOP;
            assert_eq!(param_norm_at(id, mid), 1.0, "{id}: the track's top must be the maximum");
            assert_eq!(param_norm_at(id, mid + TRACK_H), 0.0, "{id}: the track's bottom must be the minimum");
            // The middle of the track answers to the slider itself.
            let (x, y) = (region.x + region.w / 2.0, mid + TRACK_H / 2.0);
            assert_eq!(param_at(x, y), Some(id), "{id}: its own track must hit it");
        }
    }

    #[test]
    fn the_window_holds_its_controls_without_overlap() {
        // The left column and the cards stay inside the panel.
        for r in WAVE_RECTS.into_iter().chain(PRESET_RECTS).chain([PLAY_RECT]) {
            assert!(r.x + r.w <= 176.1, "the left column runs past {:?}", r);
        }
        for r in CARD_RECTS {
            assert!(r.x >= 188.0 && r.x + r.w <= 696.0 && r.y + r.h <= 462.0, "a card runs past {:?}", r);
        }
    }

    #[test]
    fn the_editor_paints_at_its_declared_size() {
        let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0).expect("editor canvas");
        paint(&mut canvas, &Display::default());
        assert_eq!(canvas.pixmap.width(), WIDTH);
        assert_eq!(canvas.pixmap.height(), HEIGHT);
    }

    #[test]
    fn a_rolled_display_paints_too() {
        let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0).expect("editor canvas");
        let mut d = Display::default();
        d.wave = 1.0 / 3.0;
        for (i, control) in d.controls.iter_mut().enumerate() {
            control.norm = (i % 4) as f32 / 3.0;
            control.text = format!("{:.3}", i as f32 / 23.0);
        }
        d.status = "ROLLED EXPLOSION".to_string();
        d.mouse = Some((60.0, 120.0));
        paint(&mut canvas, &d);
        assert_eq!(canvas.pixmap.width(), WIDTH);
    }
}
