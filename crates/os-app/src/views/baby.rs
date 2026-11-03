// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Baby Mode's screen (a user request, "a way easier version of Mango"): the GarageBand-like one
//! flat workspace — track list on the left, bar ruler and clip lanes in the middle, the selected
//! track's instrument and effects on the right, the selected clip's notes along the foot and a
//! transport band under it all — painted in Mac OS X Aqua, as the user asked: brushed-metal bands,
//! glossy round buttons, traffic lights and a dark LCD read-out. A tap selects, the ruler seeks, +
//! opens the Add-track sheet; long-press on a clip opens its usual menu.

use os_model::ClipSource;
use os_ui::Rect;

use crate::app::{Action, State, Ui};
use crate::icons::Icon;
use crate::views::{fnt, light};

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
    transport(ui, s, f);
    if s.baby.add_open {
        add_sheet(ui, s);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Aqua helpers: the brushed metal, the glossy fills and the embossed labels

/// A colour nudged `d` per channel (up or down, clamped): Aqua's shades without alpha blending.
fn lite(c: u32, d: i32) -> u32 {
    let ch = |v: u32| ((v as i32 + d).clamp(0, 255)) as u32;
    (ch((c >> 16) & 0xFF) << 16) | (ch((c >> 8) & 0xFF) << 8) | ch(c & 0xFF)
}

/// A brushed-metal band: the base fill with GarageBand's fine pinstripes, a lit top edge and a
/// shaded foot — the chrome the list, the ruler, the panel and the transport all share.
fn metal(ui: &mut Ui, r: Rect) {
    let t = ui.t;
    ui.c.fill_rect(r, t.s(baby::METAL));
    let mut y = r.y + 2.0;
    while y < r.bottom() - 1.0 {
        ui.c.fill_rect(Rect::new(r.x, y, r.w, 1.0), t.s(baby::METAL_DARK));
        y += 3.0;
    }
    ui.c.fill_rect(Rect::new(r.x, r.y, r.w, 1.0), t.s(baby::METAL_LIGHT));
    ui.c.fill_rect(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), t.s(baby::METAL_EDGE));
}

/// An embossed label on metal: Aqua's light shadow under dark type, clipped to `w`.
fn emboss(ui: &mut Ui, x: f32, y: f32, w: f32, size: f32, color: u32, text: &str) {
    let t = ui.t;
    ui.c.text_clip(fnt(size), x, y + 1.0, w, t.s(baby::METAL_LIGHT), text);
    ui.c.text_clip(fnt(size), x, y, w, t.s(color), text);
}

/// A vertical-gradient capsule: Aqua's two-tone fill over a darker rim.
fn pill(ui: &mut Ui, r: Rect, rad: f32, top: u32, bot: u32, edge: u32) {
    let t = ui.t;
    ui.c.fill_rrect(r, rad, t.s(edge));
    ui.c.fill_rect_gradient(r.inset(1.0), t.s(top), t.s(bot), (rad - 1.0).max(0.0));
}

/// A round Aqua button: the glossy ball — a lit crown, the base colour's foot and a darker rim.
fn ball(ui: &mut Ui, r: Rect, top: u32, bot: u32, edge: u32) {
    let t = ui.t;
    let rad = r.w / 2.0;
    ui.c.fill_circle(r.cx(), r.cy(), rad - 1.0, t.s(bot));
    if r.h >= 14.0 {
        ui.c.fill_circle(r.cx(), r.cy() - rad * 0.4, rad * 0.62, t.s(top));
        // The foot back to the base colour: the gloss sits in the top half only.
        ui.c.push_clip(Rect::new(r.x, r.cy(), r.w, r.h / 2.0 + 1.0));
        ui.c.fill_circle(r.cx(), r.cy(), rad - 1.0, t.s(bot));
        ui.c.pop_clip();
    }
    ui.c.stroke_circle(r.cx(), r.cy(), rad - 0.5, 1.0, t.s(edge));
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
        // The hairline under the row, GarageBand's lane separators.
        ui.c.fill_rect(Rect::new(lanes.x, r.bottom() - 1.0, lanes.w, 1.0), t.s(0xFFFFFF));
    }
}

fn ruler(ui: &mut Ui, s: &State, f: &super::FrameInfo) {
    let t = ui.t;
    let r = baby::ruler_rect(s);
    metal(ui, r);
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
        emboss(ui, x + 3.0, r.y + 6.0, 24.0, 10.0, baby::RULER_TEXT, &format!("{}", b + 1));
    }
    // The playhead's marker.
    let x = playhead_x(s, f);
    if r.x <= x && x <= r.right() {
        for dy in 0..5i32 {
            let w = 9.0 - dy as f32 * 2.0;
            ui.c.fill_rect(Rect::new(x - w / 2.0, r.y + 2.0 + dy as f32 * 2.0, w, 2.0), t.s(baby::PLAYHEAD));
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
        ui.c.fill_rrect(r, 4.0, t.s(bot));
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
        ui.c.stroke_rrect(r, 4.0, if sel { 2.0 } else { 1.0 }, t.s(if sel { baby::SEL_BLUE } else { edge }));
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
// Track list: the brushed metal sidebar, the traffic lights and the aqua selection

fn track_list(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let list = baby::list_rect(s);
    metal(ui, list);
    // The window chrome: GarageBand's traffic lights over the metal, then the "Tracks" title.
    let head_h = 34.0;
    for (i, c) in [baby::TL_RED, baby::TL_YELLOW, baby::TL_GREEN].into_iter().enumerate() {
        let cx = list.x + 18.0 + i as f32 * 17.0;
        let cy = list.y + head_h / 2.0;
        ui.c.fill_circle(cx, cy, 6.0, t.s(c));
        ui.c.fill_circle(cx, cy - 2.2, 3.2, t.s(lite(c, 40)));
        ui.c.stroke_circle(cx, cy, 6.0, 1.0, t.s(baby::METAL_EDGE));
    }
    let title = "Tracks";
    let tw = ui.c.text_width(fnt(13.0), title);
    emboss(ui, list.cx() - tw / 2.0, list.y + 10.0, tw + 2.0, 13.0, baby::TEXT, title);
    ui.c.fill_rect(Rect::new(list.x, list.y + head_h, list.w, 1.0), t.s(baby::METAL_EDGE));

    // + Add Track: Aqua's glossy blue capsule.
    let add = Rect::new(list.x + 8.0, list.y + head_h + 8.0, list.w - 16.0, 34.0);
    ui.hit(add, Action::BabyAdd, "Add a track: pick an instrument");
    pill(ui, add, 8.0, baby::AQUA_TOP, baby::AQUA_BOT, baby::AQUA_EDGE);
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
        // The selected row is Aqua's blue gradient with white type, like GarageBand's.
        if sel {
            ui.c.fill_rect_gradient(r, t.s(baby::AQUA_TOP), t.s(baby::AQUA_BOT), 5.0);
            ui.c.stroke_rrect(r, 5.0, 1.0, t.s(baby::AQUA_EDGE));
        } else {
            ui.c.fill_rrect(r, 5.0, t.s(baby::ROW_BG));
            ui.c.stroke_rrect(r, 5.0, 1.0, t.s(0xDCDCE1));
        }
        // The instrument chip: the track's colour, glossed.
        let col = baby::track_color(s, row);
        let chip = Rect::new(r.x + 6.0, r.y + 8.0, 24.0, 24.0);
        ui.c.fill_rect_gradient(chip, t.s(lite(col, 36)), t.s(col), 5.0);
        ui.c.stroke_rrect(chip, 5.0, 1.0, t.s(lite(col, -34)));
        crate::icons::draw(ui.c, Icon::Note, Rect::new(chip.x + 5.0, chip.y + 5.0, 14.0, 14.0), t.s(0xFFFFFF));
        let label = Rect::new(r.x + 38.0, r.y + 12.0, r.w - 66.0, 16.0);
        ui.c.text_clip(fnt(12.0), label.x, label.y, label.w, t.s(if sel { 0xFFFFFF } else { baby::TEXT }), &crate::playlist::track_name(s, row));
        // The mute LED: a small round Aqua button, green while the track sounds.
        let on = crate::playlist::track_on(s, row);
        let led = Rect::new(r.right() - 26.0, r.y + 11.0, 18.0, 18.0);
        ui.hit(led, Action::PlLed(row), if on { "Mute track" } else { "Unmute track" });
        ball(ui, led, if on { 0x9FE08A } else { baby::METAL_LIGHT }, if on { 0x4CA83C } else { 0xC2C3C8 }, if on { 0x2F7A22 } else { baby::METAL_EDGE });
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Instrument panel

fn panel(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let p = baby::panel_rect(s);
    metal(ui, p);
    ui.c.fill_rect(Rect::new(p.x, p.y, 1.0, p.h), t.s(baby::METAL_EDGE));
    let track = s.playlist.track;
    let inner = Rect::new(p.x + 12.0, p.y + 12.0, p.w - 24.0, p.h - 24.0);
    emboss(ui, inner.x, inner.y, inner.w, 14.0, baby::TEXT, &crate::playlist::track_name(s, track));

    let Some(ch) = baby::track_channel(s, track) else {
        ui.c.text_clip(fnt(12.0), inner.x, inner.y + 40.0, inner.w, t.s(baby::TEXT_DIM), "No instrument yet.");
        ui.c.text_clip(fnt(12.0), inner.x, inner.y + 58.0, inner.w, t.s(baby::TEXT_DIM), "Tap + Add Track.");
        return;
    };
    // The instrument picture: a drawn keyboard on a white card, like GarageBand's.
    let card = Rect::new(inner.x, inner.y + 30.0, inner.w, 104.0);
    ui.c.fill_rrect(card, 8.0, t.s(0xFFFFFF));
    ui.c.stroke_rrect(card, 8.0, 1.0, t.s(0xC9CAD0));
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

    // Effects: the channel's mixer track slots, each with its on/off ball.
    let fx_y = card.bottom() + 52.0;
    ui.c.text(fnt(10.0), inner.x, fx_y, t.s(baby::TEXT_DIM), "EFFECTS");
    let mt = s.project.channel(ch).map(|c| c.mixer_track).unwrap_or(0);
    let (slots, ons) = crate::mixer::track(s, mt).map(|m| (m.slots.clone(), m.slot_on)).unwrap_or_default();
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
        ball(ui, led, if on { 0x9FE08A } else { baby::METAL_LIGHT }, if on { 0x4CA83C } else { 0xC2C3C8 }, if on { 0x2F7A22 } else { baby::METAL_EDGE });
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
// Note preview (the stage's foot, above the transport)

fn preview(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let Some(p) = baby::preview_rect(s) else { return };
    ui.c.fill_rect(p, t.s(0xF4F4F6));
    ui.c.fill_rect(Rect::new(p.x, p.y, p.w, 1.0), t.s(baby::METAL_EDGE));
    // The little keyboard.
    ui.c.fill_rect(Rect::new(p.x, p.y, baby::KEYS_W, p.h), t.s(0xFCFCF7));
    let rows = 14;
    let rh = p.h / rows as f32;
    for i in 0..rows {
        let y = p.y + i as f32 * rh;
        if [1, 3, 6, 8, 10].contains(&i) {
            ui.c.fill_rect(Rect::new(p.x, y, baby::KEYS_W, rh), t.s(0x232326));
        } else {
            ui.c.fill_rect(Rect::new(p.x, y, baby::KEYS_W, 1.0), t.s(baby::METAL_DARK));
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
        ui.c.fill_rect_gradient(Rect::new(nx, ny - 3.0, nw, 6.0), t.s(baby::GREEN_TOP), t.s(baby::GREEN_BOT), 2.0);
    }
    ui.c.pop_clip();
}

// ---------------------------------------------------------------------------------------------------------------
// The transport (the foot): GarageBand's brushed band, the glossy round buttons and the LCD

fn transport(ui: &mut Ui, s: &State, f: &super::FrameInfo) {
    let t = ui.t;
    let tr = baby::transport_rect(s);
    metal(ui, tr);
    ui.c.fill_rect(Rect::new(tr.x, tr.y, tr.w, 1.0), t.s(0xFFFFFF));
    let tc = baby::transport_controls(s);
    let (metal_top, metal_bot) = (baby::METAL_LIGHT, 0xC2C3C8);

    // The metronome's LED and Add Track, the band's left cluster.
    let on = crate::metronome::on();
    ui.hit(tc.metro, Action::Metronome, "Metronome (Ctrl+M)");
    ball(ui, tc.metro, if on { baby::AQUA_TOP } else { metal_top }, if on { baby::AQUA_BOT } else { metal_bot }, if on { baby::AQUA_EDGE } else { baby::METAL_EDGE });
    crate::icons::draw(ui.c, Icon::Metronome, tc.metro.inset(8.0), t.s(if on { 0xFFFFFF } else { 0x4A4B52 }));
    ui.hit(tc.add, Action::BabyAdd, "Add a track: pick an instrument");
    ball(ui, tc.add, baby::AQUA_TOP, baby::AQUA_BOT, baby::AQUA_EDGE);
    crate::icons::draw(ui.c, Icon::Plus, tc.add.inset(8.0), t.s(0xFFFFFF));

    // Record: a red dot in a metal ball, the dot filling the face while armed.
    ui.hit(tc.rec, Action::Record, crate::record::hint());
    ball(ui, tc.rec, metal_top, metal_bot, baby::METAL_EDGE);
    let dot = if s.rec.armed { baby::REC_RED } else { lite(baby::REC_RED, -20) };
    let dr = tc.rec.w / 2.0 - 7.0;
    ui.c.fill_circle(tc.rec.cx(), tc.rec.cy(), dr, t.s(dot));
    ui.c.stroke_circle(tc.rec.cx(), tc.rec.cy(), dr, 1.0, t.s(lite(dot, -60)));

    // Stop: a dark square on its metal ball.
    ui.hit(tc.stop, Action::Stop, "Stop (Space)");
    ball(ui, tc.stop, metal_top, metal_bot, baby::METAL_EDGE);
    ui.c.fill_rrect(tc.stop.inset(11.0), 2.0, t.s(0x4A4B52));

    // Play / pause: the big glossy blue ball, GarageBand's.
    ui.hit(tc.play, Action::Play, "Play / pause (Space)");
    ball(ui, tc.play, baby::AQUA_TOP, baby::AQUA_BOT, baby::AQUA_EDGE);
    if f.playing {
        let b = tc.play.inset(14.0);
        let bw = b.w * 0.32;
        ui.c.fill_rrect(Rect::new(b.x, b.y, bw, b.h), 2.0, t.s(0xFFFFFF));
        ui.c.fill_rrect(Rect::new(b.right() - bw, b.y, bw, b.h), 2.0, t.s(0xFFFFFF));
    } else {
        let x = tc.play.x + tc.play.w * 0.34;
        let y = tc.play.y + tc.play.h * 0.26;
        let h = tc.play.h * 0.48;
        let w = tc.play.w * 0.32;
        ui.c.fill_polygon(&[(x, y), (x, y + h), (x + w, y + h / 2.0)], t.s(0xFFFFFF));
    }

    // The LCD: the dark well with the bar.beat.tick read-out and the tempo at its right.
    ui.c.fill_rrect(tc.lcd, 7.0, t.s(baby::LCD_EDGE));
    ui.c.fill_rrect(tc.lcd.inset(2.0), 5.5, t.s(baby::LCD_BG));
    super::display_edge(ui.c, t, tc.lcd, 7.0);
    let ppq = f64::from(s.project.ppq.max(1));
    let ticks = if f.playing && f.song { f.tick } else { f64::from(s.playlist.pos) };
    let sig = s.project.signature().unwrap_or((s.project.bar().0, 4));
    let (bar, beat, tick) = crate::clock::bbt(ticks * 96.0 / ppq, sig.0, sig.1);
    let big = light(20.0);
    let small = light(13.0);
    let segs: [(String, u32); 5] = [
        (format!("{bar:03}"), baby::LCD_DIGIT),
        (".".into(), baby::LCD_DIM),
        (format!("{beat:02}"), baby::LCD_DIGIT),
        (".".into(), baby::LCD_DIM),
        (format!("{tick:02}"), baby::LCD_AMBER),
    ];
    let mut wsum = 0.0;
    for (txt, _) in &segs {
        wsum += ui.c.text_width(big, txt);
    }
    let tempo = format!("{}", f.tempo.round() as i64);
    let twide = if tc.lcd.w > 160.0 { ui.c.text_width(small, &tempo) + 10.0 } else { 0.0 };
    let mut x = tc.lcd.cx() - (wsum + twide) / 2.0;
    let y = tc.lcd.y + 9.0;
    for (txt, col) in &segs {
        ui.c.text(big, x, y, t.s(*col), txt.as_str());
        x += ui.c.text_width(big, txt);
    }
    if twide > 0.0 {
        ui.c.text(small, x + 6.0, y + 5.0, t.s(baby::LCD_AMBER), &tempo);
    }
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
    ui.c.stroke_rrect(card, 10.0, 1.5, t.s(baby::METAL_EDGE));
    emboss(ui, card.x + 16.0, card.y + 14.0, card.w - 32.0, 14.0, baby::TEXT, "Add a track");
    let chips = [baby::DRUM_COL, baby::KEYS_COL, baby::BASS_COL, baby::SYN_COL, baby::SAMP_COL];
    let subs = ["A drum kit with a ready beat", "Piano keys with a ready riff", "A bass with a ready line", "A wavetable synth with a ready riff", "Drop a sample in and play it"];
    for (i, name) in FAMILIES.iter().enumerate() {
        let r = Rect::new(card.x + 14.0, card.y + 42.0 + i as f32 * 54.0, card.w - 28.0, 48.0);
        let hov = ui.hit(r, Action::BabyAddFamily(i), format!("Add a {name} track"));
        ui.c.fill_rrect(r, 7.0, t.s(if hov { baby::ROW_SEL } else { baby::LIST_BG }));
        ui.c.stroke_rrect(r, 7.0, 1.0, t.s(0xC9CAD0));
        let chip = Rect::new(r.x + 8.0, r.y + 10.0, 28.0, 28.0);
        ui.c.fill_rect_gradient(chip, t.s(lite(chips[i], 36)), t.s(chips[i]), 6.0);
        ui.c.stroke_rrect(chip, 6.0, 1.0, t.s(lite(chips[i], -34)));
        crate::icons::draw(ui.c, Icon::Note, Rect::new(chip.x + 7.0, chip.y + 7.0, 14.0, 14.0), t.s(0xFFFFFF));
        ui.c.text(fnt(13.0), r.x + 46.0, r.y + 8.0, t.s(baby::TEXT), name);
        ui.c.text(fnt(10.0), r.x + 46.0, r.y + 27.0, t.s(baby::TEXT_DIM), subs[i]);
    }
    let cancel = Rect::new(card.x + 14.0, card.bottom() - 40.0, card.w - 28.0, 30.0);
    ui.hit(cancel, Action::BabyAdd, "Close");
    pill(ui, cancel, 15.0, 0xF2F2F5, 0xD8D9DE, 0xAFAFB6);
    let cw = ui.c.text_width(fnt(12.0), "Cancel");
    ui.c.text(fnt(12.0), card.cx() - cw / 2.0, cancel.y + 7.0, t.s(baby::TEXT), "Cancel");
}
