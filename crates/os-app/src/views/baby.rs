// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Baby Mode's screen (a user request, "a way easier version of Mango"): the GarageBand-like one
//! flat workspace — track list on the left, bar ruler and clip lanes in the middle, the selected
//! track's instrument and effects on the right, the selected clip's notes along the foot — painted
//! light, over the same project the full studio edits. A tap selects, the ruler seeks, + opens the
//! Add-track sheet; long-press on a clip opens its usual menu.

use os_model::ClipSource;
use os_ui::Rect;

use crate::app::{Action, State, Ui};
use crate::icons::Icon;
use crate::views::fnt;

use crate::baby::{self, FAMILIES};

/// A tap anywhere on the lanes: selects the clip under it (and its pattern for the preview), or
/// clears the selection on the ground; a drag scrolls.
pub const AREA_HINT: &str = "Tap a clip to select it, drag to scroll, long-press for its menu";

pub fn paint(ui: &mut Ui, s: &State, f: &super::FrameInfo) {
    let t = ui.t;
    let work = baby::work(s);
    ui.c.fill_rect(work, t.s(baby::BG));

    lanes_ground(ui, s);
    ruler(ui, s, f);
    clips(ui, s);
    playhead(ui, s, f);
    track_list(ui, s);
    panel(ui, s);
    preview(ui, s);
    if s.baby.add_open {
        add_sheet(ui, s);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Timeline

fn lanes_ground(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let lanes = baby::lanes_rect(s);
    ui.c.fill_rect(lanes, t.s(baby::TIMELINE_BG));
    ui.hit(lanes, Action::BabyArea, AREA_HINT);
    // Alternating row tints under every visible track, mute-dimmed like GarageBand's.
    for row in 0..baby::visible_tracks(s) {
        let r = Rect::new(lanes.x, lanes.y + f32::from(row) * baby::ROW_H, lanes.w, baby::ROW_H);
        let on = crate::playlist::track_on(s, row);
        ui.c.fill_rect(r, t.s(if row % 2 == 0 { baby::LANE_BG } else { baby::TIMELINE_BG }));
        if !on {
            ui.c.fill_rect(r, t.s(0xD5D5DA));
        }
    }
}

fn ruler(ui: &mut Ui, s: &State, f: &super::FrameInfo) {
    let t = ui.t;
    let r = baby::ruler_rect(s);
    ui.c.fill_rect(r, t.s(baby::RULER_BG));
    ui.hit(r, Action::BabySeek, "Song position: tap to move the playhead");
    // The loop region, shaded like GarageBand's yellow cycle band.
    if let Some((a, b)) = s.playlist.range {
        let x0 = baby::x_of_tick(s, a).max(r.x);
        let x1 = baby::x_of_tick(s, b).min(r.right());
        if x1 > x0 {
            ui.c.fill_rect(Rect::new(x0, r.y + 3.0, x1 - x0, r.h - 6.0), t.s(0xF2D24B));
        }
    }
    // Bar numbers, every bar while there is room, else every four.
    let bar = s.project.bar_ticks().max(1) as u32;
    let first = s.baby.scroll_x / bar;
    let last = baby::tick_at(s, r.right()) / bar + 1;
    for b in first..=last {
        let x = baby::x_of_tick(s, b * bar);
        if x < r.x - 2.0 || x > r.right() - 14.0 {
            continue;
        }
        ui.c.text(fnt(10.0), x + 3.0, r.y + 6.0, t.s(baby::RULER_TEXT), &format!("{}", b + 1));
    }
    // The playhead's marker.
    let x = playhead_x(s, f);
    if r.x <= x && x <= r.right() {
        for dy in 0..5i32 {
            let w = 9.0 - f32::from(dy) * 2.0;
            ui.c.fill_rect(Rect::new(x - w / 2.0, r.y + 2.0 + f32::from(dy) * 2.0, w, 2.0), t.s(baby::PLAYHEAD));
        }
    }
}

fn clips(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let lanes = baby::lanes_rect(s);
    ui.c.push_clip(lanes);
    for (i, c) in crate::playlist::clips(s).iter().enumerate() {
        if c.track >= baby::visible_tracks(s) {
            continue;
        }
        let mut r = baby::clip_rect(s, c);
        if r.right() < lanes.x || r.x > lanes.right() {
            continue;
        }
        r.x = r.x.max(lanes.x - 1.0);
        let on = crate::playlist::track_on(s, c.track);
        let sel = s.playlist.sel.contains(&i);
        let (top, bot, edge, pip) = match c.source {
            ClipSource::Pattern(_) => (baby::GREEN_TOP, baby::GREEN_BOT, baby::GREEN_EDGE, 0x3D7527),
            ClipSource::Channel(_) => (baby::BLUE_TOP, baby::BLUE_BOT, baby::BLUE_EDGE, 0x2C5384),
        };
        let (top, bot, edge) = if on { (top, bot, edge) } else { (0xC9C9CE, 0xBDBDC3, 0xA8A8B0) };
        ui.c.fill_rrect(r, 3.0, t.s(bot));
        ui.c.fill_rect(Rect::new(r.x, r.y, r.w, r.h / 2.0), t.s(top));
        // The notes of a pattern clip, as GarageBand's little pips.
        if let ClipSource::Pattern(n) = c.source {
            if let Some(p) = s.project.pattern(n) {
                let (lo, hi) = p.notes.iter().fold((u16::MAX, 0u16), |(a, b), n| (a.min(n.key), b.max(n.key)));
                if lo <= hi {
                    let span = f32::from((hi - lo).max(1));
                    let body = Rect::new(r.x + 2.0, r.y + 13.0, (r.w - 4.0).max(0.0), r.h - 16.0);
                    for n in &p.notes {
                        let nx = body.x + (n.position as f32 / c.length.max(1) as f32) * body.w;
                        if nx > body.right() {
                            continue;
                        }
                        let ny = body.bottom() - (f32::from(n.key - lo) / span) * body.h;
                        ui.c.fill_rect(Rect::new(nx, ny - 1.5, 3.0, 3.0), t.s(pip));
                    }
                }
            }
        }
        // The name on a title strip, like the playlist's.
        ui.c.fill_rect(Rect::new(r.x, r.y, r.w, 11.0), t.s(edge));
        ui.c.text_clip(fnt(9.0), r.x + 3.0, r.y + 1.5, r.w - 6.0, t.s(0xF2F6EE), &crate::playlist::source_name(s, c.source));
        ui.c.stroke_rrect(r, 3.0, if sel { 2.0 } else { 1.0 }, t.s(if sel { baby::SEL_BLUE } else { edge }));
    }
    ui.c.pop_clip();
}

/// Where the playhead sits: playback's tick while the song runs, else the stopped position.
fn playhead_x(s: &State, f: &super::FrameInfo) -> f32 {
    let tick = if f.playing && f.song { f.tick } else { f64::from(s.playlist.pos) };
    baby::x_of_tick(s, tick as u32)
}

fn playhead(ui: &mut Ui, s: &State, f: &super::FrameInfo) {
    let lanes = baby::lanes_rect(s);
    let x = playhead_x(s, f);
    if lanes.x <= x && x <= lanes.right() {
        ui.c.fill_rect(Rect::new(x, lanes.y, 1.5, lanes.h), ui.t.s(baby::PLAYHEAD));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Track list

fn track_list(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let list = baby::list_rect(s);
    ui.c.fill_rect(list, t.s(baby::LIST_BG));
    // + Add Track, GarageBand's plus at the top of the list.
    let add = Rect::new(list.x + 8.0, list.y + 8.0, list.w - 16.0, 34.0);
    ui.hit(add, Action::BabyAdd, "Add a track: pick an instrument");
    ui.c.fill_rrect(add, 6.0, t.s(baby::ADD_BLUE));
    crate::icons::draw(ui.c, Icon::Plus, Rect::new(add.x + 8.0, add.y + 8.0, 18.0, 18.0), t.s(0xFFFFFF));
    ui.c.text(fnt(13.0), add.x + 32.0, add.y + 9.0, t.s(0xFFFFFF), "Add Track");

    let rows_y = add.bottom() + 6.0;
    for row in 0..baby::visible_tracks(s) {
        let r = Rect::new(list.x + 4.0, rows_y + f32::from(row) * baby::ROW_H, list.w - 8.0, baby::ROW_H - 4.0);
        if r.bottom() > list.bottom() {
            break;
        }
        let sel = s.playlist.track == row;
        ui.hit(r, Action::BabyTrack(row), crate::playlist::track_name(s, row));
        ui.c.fill_rrect(r, 5.0, t.s(if sel { baby::ROW_SEL } else { baby::ROW_BG }));
        if sel {
            ui.c.stroke_rrect(r, 5.0, 1.5, t.s(baby::SEL_BLUE));
        }
        // The instrument chip.
        let chip = Rect::new(r.x + 6.0, r.y + 8.0, 24.0, 24.0);
        ui.c.fill_rrect(chip, 5.0, t.s(baby::track_color(s, row)));
        crate::icons::draw(ui.c, Icon::Note, Rect::new(chip.x + 5.0, chip.y + 5.0, 14.0, 14.0), t.s(0xFFFFFF));
        ui.c.text_clip(fnt(12.0), r.x + 38.0, r.y + 12.0, r.w - 66.0, t.s(baby::TEXT), &crate::playlist::track_name(s, row));
        // The mute LED, the playlist's own.
        let led = Rect::new(r.right() - 24.0, r.y + 12.0, 16.0, 16.0);
        ui.hit(led, Action::PlLed(row), if crate::playlist::track_on(s, row) { "Mute track" } else { "Unmute track" });
        ui.c.fill_rrect(led, 3.0, t.s(if crate::playlist::track_on(s, row) { 0x8FC97E } else { 0xB9B9C2 }));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Instrument panel

fn panel(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let p = baby::panel_rect(s);
    ui.c.fill_rect(p, t.s(baby::LIST_BG));
    ui.c.fill_rect(Rect::new(p.x, p.y, 1.0, p.h), t.s(0xD4D4DA));
    let track = s.playlist.track;
    let inner = Rect::new(p.x + 12.0, p.y + 12.0, p.w - 24.0, p.h - 24.0);
    ui.c.text_clip(fnt(14.0), inner.x, inner.y, inner.w, t.s(baby::TEXT), &crate::playlist::track_name(s, track));

    let Some(ch) = baby::track_channel(s, track) else {
        ui.c.text_clip(fnt(12.0), inner.x, inner.y + 40.0, inner.w, t.s(baby::TEXT_DIM), "No instrument yet.");
        ui.c.text_clip(fnt(12.0), inner.x, inner.y + 58.0, inner.w, t.s(baby::TEXT_DIM), "Tap + Add Track.");
        return;
    };
    // The instrument picture: a drawn keyboard on a white card, like GarageBand's.
    let card = Rect::new(inner.x, inner.y + 30.0, inner.w, 104.0);
    ui.c.fill_rrect(card, 8.0, t.s(0xFFFFFF));
    ui.c.stroke_rrect(card, 8.0, 1.0, t.s(0xD8D8DE));
    let keys = Rect::new(card.x + 10.0, card.y + 12.0, card.w - 20.0, 58.0);
    let whites = 10;
    let wk = keys.w / whites as f32;
    for i in 0..whites {
        ui.c.fill_rect(Rect::new(keys.x + i as f32 * wk, keys.y, wk - 1.0, keys.h), t.s(0xFCFCF7));
    }
    for &b in &[0usize, 1, 3, 4, 5] {
        for oct in 0..2 {
            let k = b + oct * 7;
            if k >= whites - 1 {
                continue;
            }
            let x = keys.x + (k + 1) as f32 * wk - wk * 0.3;
            ui.c.fill_rect(Rect::new(x, keys.y, wk * 0.6, keys.h * 0.62), t.s(0x232326));
        }
    }
    ui.c.text_clip(fnt(10.0), card.x + 10.0, keys.bottom() + 6.0, card.w - 20.0, t.s(baby::TEXT_DIM), "Software instrument");

    // Sound: the wrapped plugin's own name, else the channel's.
    let name = baby::instrument_name(s, ch).unwrap_or_else(|| "Instrument".into());
    ui.c.text(fnt(10.0), inner.x, card.bottom() + 10.0, t.s(baby::TEXT_DIM), "SOUND");
    ui.c.text_clip(fnt(13.0), inner.x, card.bottom() + 24.0, inner.w, t.s(baby::TEXT), &name);

    // Effects: the channel's mixer track slots, each with its on/off LED.
    let fx_y = card.bottom() + 52.0;
    ui.c.text(fnt(10.0), inner.x, fx_y, t.s(baby::TEXT_DIM), "EFFECTS");
    let mt = s.project.channel(ch).map(|c| c.mixer_track).unwrap_or(0);
    let (slots, ons) = crate::mixer::track(s, mt).map(|m| (m.slots, m.slot_on)).unwrap_or_default();
    let mut any = false;
    for (i, slot) in slots.iter().enumerate() {
        let Some(slot) = slot else { continue };
        any = true;
        let y = fx_y + 16.0 + i as f32 * 26.0;
        if y > inner.bottom() - 18.0 {
            break;
        }
        let led = Rect::new(inner.x, y, 16.0, 16.0);
        let on = ons[i];
        ui.hit(led, Action::BabyFxLed(mt, i as u8), if on { "Turn the effect off" } else { "Turn the effect on" });
        ui.c.fill_rrect(led, 3.0, t.s(if on { 0x8FC97E } else { 0xB9B9C2 }));
        let label = Rect::new(inner.x + 24.0, y - 2.0, inner.w - 24.0, 18.0);
        let shown = crate::mixer::slot_name(s, mt, i as u8).unwrap_or_else(|| slot.name.clone());
        let hov = ui.hit(label, Action::BabyFxShow(mt, i as u8), "Open the effect");
        ui.c.text_clip(fnt(12.0), label.x, label.y + 2.0, label.w, t.s(if hov { baby::SEL_BLUE } else if on { baby::TEXT } else { baby::TEXT_DIM }), &shown);
    }
    if !any {
        ui.c.text_clip(fnt(12.0), inner.x, fx_y + 16.0, inner.w, t.s(baby::TEXT_DIM), "No effects on this track.");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Note preview (the foot)

fn preview(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let Some(p) = baby::preview_rect(s) else { return };
    ui.c.fill_rect(p, t.s(baby::LIST_BG));
    ui.c.fill_rect(Rect::new(p.x, p.y, p.w, 1.0), t.s(0xD4D4DA));
    // The little keyboard.
    ui.c.fill_rect(Rect::new(p.x, p.y, baby::KEYS_W, p.h), t.s(0xFCFCF7));
    let rows = 14;
    let rh = p.h / rows as f32;
    for i in 0..rows {
        let y = p.y + i as f32 * rh;
        if [1, 3, 6, 8, 10].contains(&i) {
            ui.c.fill_rect(Rect::new(p.x, y, baby::KEYS_W, rh), t.s(0x232326));
        } else {
            ui.c.fill_rect(Rect::new(p.x, y, baby::KEYS_W, 1.0), t.s(0xD0D0D6));
        }
    }
    let grid = Rect::new(p.x + baby::KEYS_W, p.y, p.w - baby::KEYS_W, p.h);
    let notes: &[os_model::Note] = s.project.pattern(s.pattern).map(|p| p.notes.as_slice()).unwrap_or(&[]);
    ui.hit(p, Action::BabyEditNotes, "Edit these notes in the Piano Roll");
    if notes.is_empty() {
        ui.c.text(fnt(12.0), grid.x + 14.0, p.y + 12.0, t.s(baby::TEXT_DIM), "Tap a clip to see its notes here.");
        return;
    }
    let (lo, hi) = notes.iter().fold((u16::MAX, 0u16), |(a, b), n| (a.min(n.key), b.max(n.key)));
    let span = f32::from((hi - lo).max(1));
    let len = s.project.pattern_length(s.pattern).max(1) as f32;
    ui.c.push_clip(grid);
    for n in notes {
        let nx = grid.x + (n.position as f32 / len) * grid.w;
        let nw = (n.length.max(1) as f32 / len * grid.w).clamp(3.0, grid.w);
        let ny = grid.bottom() - 6.0 - (f32::from(n.key - lo) / span) * (grid.h - 12.0);
        ui.c.fill_rrect(Rect::new(nx, ny - 3.0, nw, 6.0), 2.0, t.s(baby::GREEN_BOT));
    }
    ui.c.pop_clip();
}

// ---------------------------------------------------------------------------------------------------------------
// The Add-track sheet

fn add_sheet(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let work = baby::work(s);
    let (sw, sh) = baby::SHEET;
    let card = Rect::new(work.cx() - sw / 2.0, work.cy() - sh / 2.0, sw, sh);
    ui.hit(work, Action::Swallow, "");
    ui.c.fill_rrect(card, 10.0, t.s(0xFFFFFF));
    ui.c.stroke_rrect(card, 10.0, 1.5, t.s(0xC8C8D0));
    ui.c.text(fnt(14.0), card.x + 16.0, card.y + 14.0, t.s(baby::TEXT), "Add a track");
    let chips = [baby::DRUM_COL, baby::KEYS_COL, baby::BASS_COL, baby::SYN_COL, baby::SAMP_COL];
    let subs = ["A drum kit with a ready beat", "Piano keys with a ready riff", "A bass with a ready line", "A wavetable synth with a ready riff", "Drop a sample in and play it"];
    for (i, name) in FAMILIES.iter().enumerate() {
        let r = Rect::new(card.x + 14.0, card.y + 42.0 + i as f32 * 54.0, card.w - 28.0, 48.0);
        let hov = ui.hit(r, Action::BabyAddFamily(i), format!("Add a {name} track"));
        ui.c.fill_rrect(r, 7.0, t.s(if hov { baby::ROW_SEL } else { baby::LIST_BG }));
        ui.c.stroke_rrect(r, 7.0, 1.0, t.s(0xD8D8DE));
        let chip = Rect::new(r.x + 8.0, r.y + 10.0, 28.0, 28.0);
        ui.c.fill_rrect(chip, 6.0, t.s(chips[i]));
        crate::icons::draw(ui.c, Icon::Note, Rect::new(chip.x + 7.0, chip.y + 7.0, 14.0, 14.0), t.s(0xFFFFFF));
        ui.c.text(fnt(13.0), r.x + 46.0, r.y + 8.0, t.s(baby::TEXT), name);
        ui.c.text(fnt(10.0), r.x + 46.0, r.y + 27.0, t.s(baby::TEXT_DIM), subs[i]);
    }
    let cancel = Rect::new(card.x + 14.0, card.bottom() - 40.0, card.w - 28.0, 30.0);
    ui.hit(cancel, Action::BabyAdd, "Close");
    ui.c.text(fnt(12.0), card.cx() - ui.c.text_width(fnt(12.0), "Cancel") / 2.0, cancel.y + 7.0, t.s(baby::TEXT_DIM), "Cancel");
}
