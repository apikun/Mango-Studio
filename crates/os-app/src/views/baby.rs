// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Baby Mode's screen (a user request, "a way easier version of Mango"): the GarageBand-like one
//! flat workspace — track list on the left, bar ruler and clip lanes in the middle, the selected
//! track's instrument and effects on the right, the selected clip's notes along the foot and a
//! transport band under it all — painted in GarageBand's own dark look, taken from real GarageBand
//! screenshots: the dark gray track headers with a working volume slider each, the near-black
//! timeline with big rounded green and blue regions (a name strip and note pips inside, a yellow
//! outline when selected), the white playhead, the note editor with a piano at its left and the
//! silver control bar at the foot with dark glyphs, the red record dot and the dark LCD. A tap
//! selects, the ruler seeks, + opens the Add-track sheet; long-press on a clip opens its usual menu.

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
    let work = baby::work(s);
    ui.c.fill_rect(work, ui.t.s(baby::BG));

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
// Helpers

/// A colour nudged `d` per channel (up or down, clamped): the shades without alpha blending.
fn lite(c: u32, d: i32) -> u32 {
    let ch = |v: u32| ((v as i32 + d).clamp(0, 255)) as u32;
    (ch((c >> 16) & 0xFF) << 16) | (ch((c >> 8) & 0xFF) << 8) | ch(c & 0xFF)
}

/// A brushed-silver band: the base fill with fine pinstripes, a lit top edge and a shaded foot —
/// the control bar the transport draws on, like GarageBand's.
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

/// A vertical-gradient capsule: the two-tone fill over a darker rim (the sheet's Cancel button).
fn pill(ui: &mut Ui, r: Rect, rad: f32, top: u32, bot: u32, edge: u32) {
    let t = ui.t;
    ui.c.fill_rrect(r, rad, t.s(edge));
    ui.c.fill_rect_gradient(r.inset(1.0), t.s(top), t.s(bot), (rad - 1.0).max(0.0));
}

/// A small glossy ball (the effects' on/off lights): a lit crown, the base colour's foot, a rim.
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

/// A round flat button on the silver band (GarageBand's toolbar buttons): a lit face, a rim, the
/// glyph over it — blue while the thing it toggles is on.
fn flat_button(ui: &mut Ui, r: Rect, on: bool, icon: Icon) {
    let t = ui.t;
    let rad = r.w / 2.0;
    let (face, rim, glyph) = if on {
        (baby::AQUA_BOT, baby::AQUA_EDGE, 0xFFFFFF)
    } else {
        (baby::METAL_LIGHT, baby::METAL_EDGE, baby::GLYPH)
    };
    ui.c.fill_circle(r.cx(), r.cy(), rad - 1.0, t.s(face));
    if !on {
        ui.c.fill_circle(r.cx(), r.cy() - rad * 0.35, rad * 0.55, t.s(lite(face, 14)));
    }
    ui.c.stroke_circle(r.cx(), r.cy(), rad - 0.5, 1.0, t.s(rim));
    crate::icons::draw(ui.c, icon, r.inset(r.w * 0.24), t.s(glyph));
}

/// The icon a track's chip wears: the family its name (or its instrument's) names, the note as the
/// fallback — GarageBand's track headers all show their instrument's picture.
fn track_icon(s: &State, t: u16) -> Icon {
    let n = crate::playlist::track_name(s, t).to_lowercase();
    let plugin = baby::track_channel(s, t)
        .and_then(|ch| s.project.channel(ch))
        .and_then(|c| crate::project_plugins::plugin_title(c).or_else(|| c.name.clone()))
        .unwrap_or_default()
        .to_lowercase();
    let has = |p: &str| n.contains(p) || plugin.contains(p);
    if has("drum") {
        Icon::Kick
    } else if has("key") || has("piano") || has("organ") || has("electric") {
        Icon::PianoRoll
    } else if has("bass") || has("guitar") {
        Icon::Wave
    } else if has("sampler") || has("sample") || has("audio") {
        Icon::FileAudio
    } else {
        Icon::Note
    }
}

/// Whether a list row is a real track (named or carrying clips) — the spare slot under the last
/// track stays empty space, like GarageBand's, instead of a fake "Track N" row.
fn real_track(s: &State, row: u16) -> bool {
    let named = crate::playlist::track_info(s, row).is_some_and(|t| t.name.as_deref().is_some_and(|n| !n.is_empty()));
    named || crate::playlist::clips(s).iter().any(|c| c.track == row)
}

/// The pattern the note preview shows: the selected clip's, else the selected track's first clip's,
/// else the pattern rack's own selection — the editor always explains the region in front of you.
fn preview_pattern(s: &State) -> u16 {
    let clips = crate::playlist::clips(s);
    let sel = clips
        .iter()
        .enumerate()
        .find(|(i, _)| s.playlist.sel.contains(i))
        .and_then(|(_, c)| match c.source {
            ClipSource::Pattern(n) => Some(n),
            _ => None,
        });
    let on_track = clips.iter().find(|c| c.track == s.playlist.track).and_then(|c| match c.source {
        ClipSource::Pattern(n) => Some(n),
        _ => None,
    });
    sel.or(on_track).unwrap_or(s.pattern)
}

// ---------------------------------------------------------------------------------------------------------------
// Timeline: the near-black lanes, the dark ruler and GarageBand's regions

fn lanes_ground(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let lanes = baby::lanes_rect(s);
    ui.c.fill_rect(lanes, t.s(baby::BG));
    ui.hit(lanes, Action::BabyArea, AREA_HINT);
    // The rows, the hairlines under them and the beat grid, mute-dimmed like GarageBand's.
    for row in 0..baby::visible_tracks(s) {
        let r = Rect::new(lanes.x, lanes.y + f32::from(row) * baby::ROW_H, lanes.w, baby::ROW_H);
        ui.c.fill_rect(r, t.s(if row % 2 == 0 { baby::TIMELINE_BG } else { baby::LANE_ALT }));
        if !crate::playlist::track_on(s, row) {
            ui.c.fill_rect(r, t.s(baby::MUTED));
        }
        ui.c.fill_rect(Rect::new(lanes.x, r.bottom() - 1.0, lanes.w, 1.0), t.s(baby::LANE_LINE));
    }
    // The beat grid: a quiet line per beat, a firmer one per bar.
    let bar = s.project.bar_ticks().max(1) as u32;
    let beats = u32::from(s.project.signature().map(|x| x.1).unwrap_or(4)).max(1);
    let beat = (bar / beats).max(1);
    let first = s.baby.scroll_x / beat * beat;
    let mut b = first;
    while baby::x_of_tick(s, b) <= lanes.right() + 1.0 {
        let x = baby::x_of_tick(s, b);
        if x >= lanes.x {
            let barline = b % bar == 0;
            ui.c.fill_rect(Rect::new(x, lanes.y, 1.0, lanes.h), t.s(if barline { baby::LANE_LINE } else { baby::GRID_LINE }));
        }
        b += beat;
    }
}

fn ruler(ui: &mut Ui, s: &State, f: &super::FrameInfo) {
    let t = ui.t;
    let r = baby::ruler_rect(s);
    ui.c.fill_rect(r, t.s(baby::RULER_BG));
    ui.hit(r, Action::BabySeek, "Song position: tap to move the playhead");
    ui.c.fill_rect(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), t.s(0x17171A));
    // The loop region, shaded like GarageBand's yellow cycle band.
    if let Some((a, b)) = s.playlist.range {
        let x0 = baby::x_of_tick(s, a).max(r.x);
        let x1 = baby::x_of_tick(s, b).min(r.right());
        if x1 > x0 {
            ui.c.fill_rect(Rect::new(x0, r.y + 3.0, x1 - x0, r.h - 7.0), t.s(baby::SEL_EDGE));
        }
    }
    // The beat ticks and the bar numbers, light on the dark strip like GarageBand's ruler.
    let bar = s.project.bar_ticks().max(1) as u32;
    let beats = u32::from(s.project.signature().map(|x| x.1).unwrap_or(4)).max(1);
    let beat = (bar / beats).max(1);
    let mut b = s.baby.scroll_x / beat * beat;
    while baby::x_of_tick(s, b) <= r.right() + 1.0 {
        let x = baby::x_of_tick(s, b);
        if x >= r.x && b % bar != 0 {
            ui.c.fill_rect(Rect::new(x, r.bottom() - 6.0, 1.0, 5.0), t.s(0x4A4A52));
        }
        b += beat;
    }
    let first = s.baby.scroll_x / bar;
    let last = baby::tick_at(s, r.right()) / bar + 1;
    for b in first..=last {
        let x = baby::x_of_tick(s, b * bar);
        if x < r.x - 2.0 || x > r.right() - 14.0 {
            continue;
        }
        ui.c.text_clip(fnt(10.0), x + 4.0, r.y + 5.0, 26.0, t.s(baby::RULER_TEXT), &format!("{}", b + 1));
    }
    // The playhead's grip: GarageBand's white vane over the strip.
    let x = playhead_x(s, f);
    if r.x <= x && x <= r.right() {
        ui.c.fill_polygon(&[(x - 5.0, r.y + 2.0), (x + 5.0, r.y + 2.0), (x, r.y + 10.0)], t.s(baby::PLAYHEAD));
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
            ClipSource::Pattern(_) => (baby::GREEN_TOP, baby::GREEN_BOT, baby::GREEN_EDGE, baby::GREEN_PIP),
            ClipSource::Channel(_) => (baby::BLUE_TOP, baby::BLUE_BOT, baby::BLUE_EDGE, baby::BLUE_PIP),
        };
        let (top, bot, edge) = if on { (top, bot, edge) } else { (0x5A5A60, 0x505056, 0x44444A) };
        // The region: the rounded fill, the name strip on top and the notes as pips under it —
        // GarageBand's regions carry their own little preview.
        ui.c.fill_rrect(r, 5.0, t.s(bot));
        ui.c.fill_rect(Rect::new(r.x, r.y, r.w, r.h / 2.0), t.s(top));
        if let ClipSource::Pattern(n) = c.source {
            if let Some(p) = s.project.pattern(n) {
                let (lo, hi) = p.notes.iter().fold((u16::MAX, 0u16), |(a, b), n| (a.min(n.key), b.max(n.key)));
                if lo <= hi {
                    let span = f32::from((hi - lo).max(1));
                    let body = Rect::new(r.x + 3.0, r.y + 14.0, (r.w - 6.0).max(0.0), r.h - 17.0);
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
        // The name strip, darker than the fill, GarageBand's region title.
        ui.c.fill_rrect(Rect::new(r.x, r.y, r.w, 12.0), 5.0, t.s(edge));
        ui.c.fill_rect(Rect::new(r.x, r.y + 6.0, r.w, 6.0), t.s(edge));
        ui.c.text_clip(fnt(9.0), r.x + 4.0, r.y + 2.0, r.w - 8.0, t.s(0xF2F6EE), &crate::playlist::source_name(s, c.source));
        // Selected: GarageBand's yellow outline; otherwise the quiet edge.
        ui.c.stroke_rrect(r, 5.0, if sel { 2.0 } else { 1.0 }, t.s(if sel { baby::SEL_EDGE } else { edge }));
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
// Track list: the dark header column, the traffic lights, the working volume sliders

fn track_list(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let list = baby::list_rect(s);
    ui.c.fill_rect(list, t.s(baby::LIST_BG));
    ui.c.fill_rect(Rect::new(list.right() - 1.0, list.y, 1.0, list.h), t.s(0x17171A));
    // The window chrome: the traffic lights are the real window controls now (the full studio's
    // band hides while Baby Mode is on), and the mango at the right goes back to it.
    let cy = list.y + baby::HEAD_H / 2.0;
    for (i, (c, act, hint)) in [
        (baby::TL_RED, Action::Close, "Close the window"),
        (baby::TL_YELLOW, Action::Minimize, "Minimize the window"),
        (baby::TL_GREEN, Action::Maximize, "Maximize or restore the window"),
    ]
    .into_iter()
    .enumerate()
    {
        let cx = list.x + 18.0 + i as f32 * 17.0;
        ui.hit(Rect::new(cx - 8.0, cy - 8.0, 16.0, 16.0), act, hint);
        ui.c.fill_circle(cx, cy, 6.0, t.s(c));
        ui.c.fill_circle(cx, cy - 2.2, 3.2, t.s(lite(c, 40)));
        ui.c.stroke_circle(cx, cy, 6.0, 1.0, t.s(lite(c, -70)));
    }
    let back = Rect::new(list.right() - 32.0, cy - 12.0, 26.0, 24.0);
    ui.hit(back, Action::Baby, "Back to the full studio");
    crate::icons::logo(ui.c, back.cx(), back.cy(), 8.5, t.s(0xF0B442));
    if list.w >= 150.0 {
        let title = "Tracks";
        let tw = ui.c.text_width(fnt(12.0), title);
        ui.c.text_clip(fnt(12.0), list.cx() - tw / 2.0, list.y + 10.0, tw + 2.0, t.s(baby::TEXT_DIM), title);
    }

    // + Add Track: the dark bar where GarageBand's + sits, labelled for the ones new to this.
    let add = baby::add_button(s);
    ui.hit(add, Action::BabyAdd, "Add a track: pick an instrument");
    ui.c.fill_rrect(add, 6.0, t.s(0x45454C));
    ui.c.fill_rect(Rect::new(add.x, add.y, add.w, add.h / 2.0), t.s(0x50505A));
    ui.c.stroke_rrect(add, 6.0, 1.0, t.s(0x5E5E68));
    crate::icons::draw(ui.c, Icon::Plus, Rect::new(add.x + 9.0, add.y + 8.0, 16.0, 16.0), t.s(baby::TEXT));
    ui.c.text(fnt(12.0), add.x + 32.0, add.y + 8.0, t.s(baby::TEXT), "Add Track");

    // The rows: GarageBand's dark headers — the instrument's chip, the name, a volume slider that
    // sets the instrument's volume and the round mute button.
    for row in 0..baby::visible_tracks(s) {
        let r = baby::row_rect(s, row);
        if r.bottom() > list.bottom() {
            break;
        }
        if !real_track(s, row) {
            continue;
        }
        let sel = s.playlist.track == row;
        let on = crate::playlist::track_on(s, row);
        ui.hit(r, Action::BabyTrack(row), crate::playlist::track_name(s, row));
        if sel {
            ui.c.fill_rrect(r, 5.0, t.s(baby::ROW_SEL));
        }
        // The instrument chip: the track's colour, dimmed with the row when muted.
        let col = if on { baby::track_color(s, row) } else { 0x55555C };
        let chip = Rect::new(r.x + 8.0, r.y + 6.0, 24.0, 24.0);
        ui.c.fill_rect_gradient(chip, t.s(lite(col, 30)), t.s(col), 5.0);
        ui.c.stroke_rrect(chip, 5.0, 1.0, t.s(lite(col, -45)));
        crate::icons::draw(ui.c, track_icon(s, row), chip.inset(5.0), t.s(0xFFFFFF));
        // The name.
        ui.c.text_clip(
            fnt(12.0),
            r.x + 40.0,
            r.y + 4.0,
            r.w - 48.0,
            t.s(if on { baby::TEXT } else { baby::TEXT_DIM }),
            &crate::playlist::track_name(s, row),
        );
        // The volume slider: the dark groove, the filled share and the light knob; a click on it
        // sets the instrument's volume.
        if let Some(g) = baby::vol_slider(s, row) {
            let v = baby::track_channel(s, row)
                .and_then(|ch| s.project.channel(ch))
                .map(|c| c.volume)
                .unwrap_or(0.78) as f32;
            let mut hot = g.inset(-4.0);
            hot.y = r.y + 12.0;
            hot.h = r.h - 12.0;
            ui.hit(hot, Action::BabyVol(row), "Click to set the track's volume");
            ui.c.fill_rrect(g, 4.5, t.s(0x17171A));
            if v > 0.01 {
                ui.c.fill_rrect(Rect::new(g.x, g.y + 2.0, (g.w * v).max(6.0), g.h - 4.0), 2.5, t.s(0x8A8A94));
            }
            let kx = g.x + g.w * v;
            ui.c.fill_circle(kx, g.cy(), 6.0, t.s(0xDADADF));
            ui.c.stroke_circle(kx, g.cy(), 6.0, 1.0, t.s(0x5E5E66));
        }
        // The round mute button: the dot lights while the track sounds.
        let led = Rect::new(r.right() - 24.0, r.y + 18.0, 16.0, 16.0);
        ui.hit(led, Action::PlLed(row), if on { "Mute track" } else { "Unmute track" });
        ui.c.fill_circle(led.cx(), led.cy(), 7.5, t.s(0x45454C));
        ui.c.stroke_circle(led.cx(), led.cy(), 7.5, 1.0, t.s(0x5E5E68));
        ui.c.fill_circle(led.cx(), led.cy(), 3.5, t.s(if on { 0x8AE07A } else { 0x6A6A72 }));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Instrument panel: the dark card with the sound and the effects

fn panel(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let p = baby::panel_rect(s);
    ui.c.fill_rect(p, t.s(0x2B2B30));
    ui.c.fill_rect(Rect::new(p.x, p.y, 1.0, p.h), t.s(0x17171A));
    let track = s.playlist.track;
    let inner = Rect::new(p.x + 12.0, p.y + 12.0, p.w - 24.0, p.h - 24.0);
    ui.c.text_clip(fnt(14.0), inner.x, inner.y, inner.w, t.s(baby::TEXT), &crate::playlist::track_name(s, track));

    let Some(ch) = baby::track_channel(s, track) else {
        ui.c.text_clip(fnt(12.0), inner.x, inner.y + 40.0, inner.w, t.s(baby::TEXT_DIM), "No instrument yet.");
        ui.c.text_clip(fnt(12.0), inner.x, inner.y + 58.0, inner.w, t.s(baby::TEXT_DIM), "Tap + Add Track.");
        return;
    };
    // The instrument's chip, big: the track's colour and its icon, like the list's.
    let col = baby::track_color(s, track);
    let chip = Rect::new(inner.x, inner.y + 30.0, 44.0, 44.0);
    ui.c.fill_rect_gradient(chip, t.s(lite(col, 30)), t.s(col), 8.0);
    ui.c.stroke_rrect(chip, 8.0, 1.0, t.s(lite(col, -45)));
    crate::icons::draw(ui.c, track_icon(s, track), chip.inset(9.0), t.s(0xFFFFFF));

    // Sound: the wrapped plugin's own name, else the channel's.
    let name = baby::instrument_name(s, ch).unwrap_or_else(|| "Instrument".into());
    ui.c.text(light(8.0), inner.x + 54.0, chip.y + 2.0, t.s(baby::TEXT_DIM), "SOUND");
    ui.c.text_clip(fnt(13.0), inner.x + 54.0, chip.y + 16.0, inner.w - 54.0, t.s(baby::TEXT), &name);
    ui.c.text_clip(fnt(10.0), inner.x, chip.bottom() + 8.0, inner.w, t.s(baby::TEXT_DIM), "Software instrument");

    // Effects: the channel's mixer track slots, each with its on/off ball.
    let fx_y = chip.bottom() + 30.0;
    ui.c.text(light(8.0), inner.x, fx_y, t.s(baby::TEXT_DIM), "EFFECTS");
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
        ball(ui, led, if on { 0x9FE08A } else { 0x55555C }, if on { 0x4CA83C } else { 0x45454B }, if on { 0x2F7A22 } else { 0x2A2A2E });
        let label = Rect::new(inner.x + 24.0, y - 2.0, inner.w - 24.0, 18.0);
        let hov = ui.hit(label, Action::BabyFxShow(mt, i as u8), "Open the effect");
        ui.c.text_clip(fnt(12.0), label.x, label.y + 2.0, label.w, t.s(if hov { 0x9CC4F0 } else if on { baby::TEXT } else { baby::TEXT_DIM }), &{
            crate::mixer::slot_name(s, mt, i as u8).unwrap_or_else(|| slot.name.clone())
        });
    }
    if !any {
        ui.c.text_clip(fnt(12.0), inner.x, fx_y + 16.0, inner.w, t.s(baby::TEXT_DIM), "No effects on this track.");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Note preview (the stage's foot, above the transport): the little editor with its piano

fn preview(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let Some(p) = baby::preview_rect(s) else { return };
    ui.c.fill_rect(p, t.s(0x19191C));
    ui.c.fill_rect(Rect::new(p.x, p.y, p.w, 1.0), t.s(baby::LANE_LINE));
    let pat = preview_pattern(s);
    let notes: &[os_model::Note] = s.project.pattern(pat).map(|p| p.notes.as_slice()).unwrap_or(&[]);
    ui.hit(p, Action::BabyEditNotes, "Edit these notes in the Piano Roll");

    // The piano: a row per key in the notes' span, the black keys dark, the C's labelled.
    let (lo, hi) = notes.iter().fold((u16::MAX, 0u16), |(a, b), n| (a.min(n.key), b.max(n.key)));
    if lo > hi {
        // No notes: the empty keyboard and the hint, so the editor still reads as the editor.
        for i in 0..14 {
            let y = p.y + i as f32 * (p.h / 14.0);
            ui.c.fill_rect(Rect::new(p.x, y, baby::KEYS_W, p.h / 14.0), t.s(if [1, 3, 6, 8, 10].contains(&i) { 0x242428 } else { 0x2A2A2E }));
        }
        ui.c.text(fnt(12.0), p.x + baby::KEYS_W + 14.0, p.y + 12.0, t.s(baby::TEXT_DIM), "Tap a clip to see its notes here.");
        return;
    }
    let rows = u32::from(hi - lo) + 1;
    let rh = (p.h / rows as f32).max(2.5);
    let top = hi;
    for i in 0..rows {
        let key = top - i as u16;
        let y = p.y + i as f32 * rh;
        let black = matches!(key % 12, 1 | 3 | 6 | 8 | 10);
        ui.c.fill_rect(Rect::new(p.x, y, baby::KEYS_W, rh), t.s(if black { 0x242428 } else { 0xE6E6E2 }));
        if key % 12 == 0 {
            ui.c.text(fnt(8.0), p.x + 4.0, y + (rh - 9.0).max(0.0) / 2.0, t.s(if black { 0x9A9AA0 } else { 0x7A7A80 }), &format!("C{}", key / 12 - 1));
        }
        ui.c.fill_rect(Rect::new(p.x, y + rh - 0.5, baby::KEYS_W, 0.5), t.s(0x151518));
    }
    // The grid and the notes: green bars on their key's row, like GarageBand's editor.
    let grid = Rect::new(p.x + baby::KEYS_W, p.y, p.w - baby::KEYS_W, p.h);
    ui.c.fill_rect(grid, t.s(0x1D1D21));
    let len = s.project.pattern_length(pat).max(1) as f32;
    let bar = s.project.bar_ticks().max(1) as u32;
    let beats = u32::from(s.project.signature().map(|x| x.1).unwrap_or(4)).max(1);
    let beat = (bar / beats).max(1) as f32;
    let mut bt = 0.0;
    while bt <= len {
        let x = grid.x + bt / len * grid.w;
        ui.c.fill_rect(Rect::new(x, grid.y, 1.0, grid.h), t.s(if bt % bar as f32 == 0.0 { baby::LANE_LINE } else { baby::GRID_LINE }));
        bt += beat;
    }
    ui.c.push_clip(grid);
    for n in notes {
        if n.key < lo || n.key > top {
            continue;
        }
        let y = p.y + f32::from(top - n.key) * rh;
        let nx = grid.x + (n.position as f32 / len) * grid.w;
        let nw = ((n.length.max(1) as f32 / len) * grid.w).clamp(3.0, grid.w);
        ui.c.fill_rrect(Rect::new(nx + 1.0, y + 1.0, (nw - 2.0).max(2.0), (rh - 2.0).max(2.0)), 1.5, t.s(baby::NOTE_GREEN));
        ui.c.stroke_rrect(Rect::new(nx + 1.0, y + 1.0, (nw - 2.0).max(2.0), (rh - 2.0).max(2.0)), 1.5, 1.0, t.s(0x2F7A22));
    }
    ui.c.pop_clip();
}

// ---------------------------------------------------------------------------------------------------------------
// The transport (the foot): the silver control bar, the dark glyphs, the red dot and the dark LCD

fn transport(ui: &mut Ui, s: &State, f: &super::FrameInfo) {
    let t = ui.t;
    let tr = baby::transport_rect(s);
    metal(ui, tr);
    let tc = baby::transport_controls(s);

    // The metronome, the band's left edge: blue while it clicks, GarageBand's active blue.
    let on = crate::metronome::on();
    ui.hit(tc.metro, Action::Metronome, "Metronome (Ctrl+M)");
    flat_button(ui, tc.metro, on, Icon::Metronome);

    // Record: the red dot, filling its face while armed.
    ui.hit(tc.rec, Action::Record, crate::record::hint());
    let dot = if s.rec.armed { baby::REC_RED } else { lite(baby::REC_RED, -25) };
    let dr = tc.rec.w / 2.0 - 8.0;
    ui.c.fill_circle(tc.rec.cx(), tc.rec.cy(), dr, t.s(dot));
    ui.c.fill_circle(tc.rec.cx(), tc.rec.cy() - dr * 0.3, dr * 0.55, t.s(lite(dot, 35)));
    ui.c.stroke_circle(tc.rec.cx(), tc.rec.cy(), dr, 1.0, t.s(lite(dot, -70)));

    // Stop and play: the dark glyphs on the silver, GarageBand's.
    ui.hit(tc.stop, Action::Stop, "Stop (Space)");
    ui.c.fill_rrect(tc.stop.inset(11.0), 3.0, t.s(baby::GLYPH));
    ui.hit(tc.play, Action::Play, "Play / pause (Space)");
    if f.playing {
        let b = tc.play.inset(15.0);
        let bw = b.w * 0.3;
        ui.c.fill_rrect(Rect::new(b.x, b.y, bw, b.h), 2.5, t.s(baby::GLYPH));
        ui.c.fill_rrect(Rect::new(b.right() - bw, b.y, bw, b.h), 2.5, t.s(baby::GLYPH));
    } else {
        let x = tc.play.x + tc.play.w * 0.35;
        let y = tc.play.y + tc.play.h * 0.27;
        let h = tc.play.h * 0.46;
        let w = tc.play.w * 0.31;
        ui.c.fill_polygon(&[(x, y), (x, y + h), (x + w, y + h / 2.0)], t.s(baby::GLYPH));
    }

    // The LCD: the dark well with GarageBand's read-out — the bar.beat over its caption, then the
    // tempo and the signature over theirs, hairline dividers between.
    ui.c.fill_rrect(tc.lcd, 7.0, t.s(baby::LCD_EDGE));
    ui.c.fill_rrect(tc.lcd.inset(2.0), 5.5, t.s(baby::LCD_BG));
    super::display_edge(ui.c, t, tc.lcd, 7.0);
    let ppq = f64::from(s.project.ppq.max(1));
    let ticks = if f.playing && f.song { f.tick } else { f64::from(s.playlist.pos) };
    let sig = s.project.signature().unwrap_or((s.project.bar().0, 4));
    let (bar, beat, _tick) = crate::clock::bbt(ticks * 96.0 / ppq, sig.0, sig.1);
    let big = light(17.0);
    let cap = light(8.0);
    let mut secs: Vec<(String, &str, u32)> = vec![(format!("{bar:03}.{beat:02}"), "BAR.BEAT", baby::LCD_DIGIT)];
    if tc.lcd.w > 170.0 {
        secs.push((format!("{}", f.tempo.round() as i64), "TEMPO", baby::LCD_DIGIT));
        secs.push((format!("{}/{}", sig.0, sig.1), "SIG", baby::LCD_DIGIT));
    }
    let gaps = (secs.len() - 1) as f32 * 13.0;
    let total: f32 = secs.iter().map(|(n, c, _)| ui.c.text_width(big, n).max(ui.c.text_width(cap, c))).sum::<f32>() + gaps;
    let mut x = tc.lcd.cx() - total / 2.0;
    for (i, (num, capt, col)) in secs.iter().enumerate() {
        if i > 0 {
            let d = x - 6.5;
            ui.c.fill_rect(Rect::new(d, tc.lcd.y + 7.0, 1.0, tc.lcd.h - 14.0), t.s(0x3A3C45));
        }
        let (nw, cw) = (ui.c.text_width(big, num), ui.c.text_width(cap, capt));
        let w = nw.max(cw);
        ui.c.text(big, x + (w - nw) / 2.0, tc.lcd.y + 4.0, t.s(*col), num);
        ui.c.text(cap, x + (w - cw) / 2.0, tc.lcd.y + 25.0, t.s(baby::LCD_DIM), capt);
        x += w + 13.0;
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
    ui.c.fill_rrect(card, 10.0, t.s(0x2B2B30));
    ui.c.stroke_rrect(card, 10.0, 1.5, t.s(0x4E4E56));
    ui.c.text(fnt(14.0), card.x + 16.0, card.y + 14.0, t.s(baby::TEXT), "Add a track");
    let chips = [baby::DRUM_COL, baby::KEYS_COL, baby::BASS_COL, baby::SYN_COL, baby::SAMP_COL];
    let subs = ["A drum kit with a ready beat", "Piano keys with a ready riff", "A bass with a ready line", "A wavetable synth with a ready riff", "Drop a sample in and play it"];
    let icons = [Icon::Kick, Icon::PianoRoll, Icon::Wave, Icon::Note, Icon::FileAudio];
    for (i, name) in FAMILIES.iter().enumerate() {
        let r = Rect::new(card.x + 14.0, card.y + 42.0 + i as f32 * 54.0, card.w - 28.0, 48.0);
        let hov = ui.hit(r, Action::BabyAddFamily(i), format!("Add a {name} track"));
        ui.c.fill_rrect(r, 7.0, t.s(if hov { 0x43434B } else { 0x36363C }));
        ui.c.stroke_rrect(r, 7.0, 1.0, t.s(0x1A1A1D));
        let chip = Rect::new(r.x + 8.0, r.y + 10.0, 28.0, 28.0);
        ui.c.fill_rect_gradient(chip, t.s(lite(chips[i], 30)), t.s(chips[i]), 6.0);
        ui.c.stroke_rrect(chip, 6.0, 1.0, t.s(lite(chips[i], -45)));
        crate::icons::draw(ui.c, icons[i], chip.inset(6.0), t.s(0xFFFFFF));
        ui.c.text(fnt(13.0), r.x + 46.0, r.y + 8.0, t.s(baby::TEXT), name);
        ui.c.text(fnt(10.0), r.x + 46.0, r.y + 27.0, t.s(baby::TEXT_DIM), subs[i]);
    }
    let cancel = Rect::new(card.x + 14.0, card.bottom() - 40.0, card.w - 28.0, 30.0);
    ui.hit(cancel, Action::BabyAdd, "Close");
    pill(ui, cancel, 15.0, 0x4A4A52, 0x3A3A41, 0x5A5A62);
    let cw = ui.c.text_width(fnt(12.0), "Cancel");
    ui.c.text(fnt(12.0), card.cx() - cw / 2.0, cancel.y + 7.0, t.s(baby::TEXT), "Cancel");
}
