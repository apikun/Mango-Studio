// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Baby Mode: the simple studio (a user request, "a way easier version of Mango"). One flat,
//! GarageBand-like screen over the same project the full studio edits — a track list on the left, a
//! clip timeline in the middle, the selected track's instrument and effects on the right and a note
//! preview along the foot, over a transport band — so nothing new to learn and nothing is lost
//! switching back. The look is Mac OS X Aqua, as the user asked: brushed-metal bands, glossy round
//! buttons and a dark LCD read-out. This module holds the state, the layout maths and the model
//! edits (add a track, a starter riff, the demo song); `views::baby` paints it.

use os_model::{Clip, ClipSource, Note, Pattern, PlaylistTrack, Rgb};
use os_ui::Rect;

use crate::app::State;

/// The switch (settings.txt `baby_mode`): on, the workspace is the Baby layout; off, the full
/// studio. Declared with the other checks so the settings page and the toolbar button agree, and so
/// the choice survives a restart.
pub use crate::prefs::BABY_MODE;

/// Whether Baby Mode paints the workspace (read live at paint time, like high visibility).
pub fn on() -> bool {
    crate::prefs::check(&BABY_MODE)
}

/// The instrument families the Add-track sheet offers, in order.
pub const FAMILIES: [&str; 5] = ["Drums", "Keys", "Bass", "Synth", "Sampler"];
/// What a family's plugin is called in the app's own list (lowercase substring match).
const FAMILY_MATCH: [&str; 5] = ["drum", "keys", "bass", "wavetable", "sampler"];

/// Baby Mode's own UI state (session-only; never saved with the project).
#[derive(Default)]
pub struct BabyState {
    /// The Add-track sheet is open.
    pub add_open: bool,
    /// The timeline's horizontal scroll, in ticks from the song's start.
    pub scroll_x: u32,
}

// ---------------------------------------------------------------------------------------------------------------
// Layout (shared by the view, the click handlers and the tests)

/// Width of the track list on the left, on a screen wide enough for it.
pub const LIST_W: f32 = 190.0;
/// Height of a track row (list and timeline lanes are the same rows).
pub const ROW_H: f32 = 40.0;
/// Height of the bar-number ruler above the lanes.
pub const RULER_H: f32 = 26.0;
/// Height of the note preview along the foot.
pub const PREVIEW_H: f32 = 170.0;
/// Width of the preview's little keyboard.
pub const KEYS_W: f32 = 52.0;
/// Timeline width of one bar.
pub const BAR_PX: f32 = 64.0;
/// The add-track sheet's size.
pub const SHEET: (f32, f32) = (330.0, 372.0);
/// Height of the transport band along the foot (the round buttons and the LCD).
pub const TRANSPORT_H: f32 = 64.0;

/// The workspace Baby Mode paints over (under the toolbar band and the rolling strip).
pub fn work(s: &State) -> Rect {
    let top = crate::views::work_y(s);
    Rect::new(0.0, top, s.screen.0, s.screen.1 - top)
}

/// Width of the track list: the full 190 on desktop, down to 120 on a phone.
pub fn list_w(s: &State) -> f32 {
    LIST_W.min((s.screen.0 * 0.34).max(120.0))
}

/// The stage: the workspace above the transport — the list, the ruler, the lanes, the panel and the
/// preview share it; the transport band takes the foot.
pub fn stage(s: &State) -> Rect {
    let w = work(s);
    Rect::new(w.x, w.y, w.w, (w.h - TRANSPORT_H).max(2.0 * ROW_H))
}

/// The transport band along the foot: GarageBand's brushed metal with the glossy round buttons and
/// the dark LCD read-out. Never taller than the workspace itself.
pub fn transport_rect(s: &State) -> Rect {
    let w = work(s);
    let y = (w.bottom() - TRANSPORT_H).max(stage(s).bottom());
    Rect::new(w.x, y, w.w, w.bottom() - y)
}

pub fn list_rect(s: &State) -> Rect {
    let w = stage(s);
    Rect::new(0.0, w.y, list_w(s), w.h)
}

/// The bar-number ruler (between the list and the instrument panel).
pub fn ruler_rect(s: &State) -> Rect {
    let w = stage(s);
    Rect::new(list_w(s), w.y, w.w - list_w(s) - panel_w(s), RULER_H)
}

pub fn lanes_rect(s: &State) -> Rect {
    let r = ruler_rect(s);
    Rect::new(LIST_W, r.bottom(), r.w, lanes_h(s))
}

/// The instrument panel on the right, the whole stage tall.
pub fn panel_rect(s: &State) -> Rect {
    let w = stage(s);
    Rect::new(w.right() - panel_w(s), w.y, panel_w(s), w.h)
}

/// The note preview along the foot of the stage (under the list and the lanes, left of the panel,
/// above the transport). `None` on a screen too short for the lanes to keep two rows.
pub fn preview_rect(s: &State) -> Option<Rect> {
    let w = stage(s);
    let lanes = lanes_rect(s);
    if lanes.bottom() + PREVIEW_H > w.bottom() + 1.0 {
        return None;
    }
    Some(Rect::new(0.0, lanes.bottom(), w.w - panel_w(s), PREVIEW_H))
}

fn lanes_h(s: &State) -> f32 {
    let h = stage(s).h - RULER_H;
    // The lanes keep at least two rows; the preview gives up its height first.
    (h - PREVIEW_H).max((h * 0.55).max(2.0 * ROW_H))
}

/// The transport's controls, laid out inside `transport_rect` (the view draws them, the tests check
/// them): the LCD at the right, the play / stop / record cluster left of it and the metronome and
/// add-track buttons at the band's left edge. A narrow band shrinks the LCD and the buttons rather
/// than spilling out of it.
pub struct Transport {
    /// The dark LCD well (bar.beat.tick, the tempo at its right on a wide band).
    pub lcd: Rect,
    /// Play / pause: the big glossy blue ball.
    pub play: Rect,
    pub stop: Rect,
    /// The record button (a red dot in a metal ball).
    pub rec: Rect,
    /// The metronome's LED ball.
    pub metro: Rect,
    /// The add-track ball.
    pub add: Rect,
}

/// The transport's control rects, all inside `transport_rect`.
pub fn transport_controls(s: &State) -> Transport {
    let tr = transport_rect(s);
    let cy = tr.cy();
    let wide = tr.w >= 460.0;
    let (small, big, lcd_w) = if wide { (36.0, 46.0, 210.0) } else { (30.0, 38.0, 124.0) };
    let right_of = |right: f32, size: f32| Rect::new(right - size, cy - size / 2.0, size, size);
    let lcd = Rect::new(tr.right() - 12.0 - lcd_w, cy - 20.0, lcd_w, 40.0);
    let play = right_of(lcd.x - 12.0, big);
    let stop = right_of(play.x - 10.0, small);
    let rec = right_of(stop.x - 10.0, small);
    let add = Rect::new(tr.x + 12.0, cy - small / 2.0, small, small);
    let metro = Rect::new(add.right() + 8.0, cy - small / 2.0, small, small);
    Transport { lcd, play, stop, rec, metro, add }
}

/// Width of the instrument panel: a quarter of the screen, 190..300. A narrow phone keeps its width
/// for the list and the timeline (GarageBand's library slides away there too); the panel comes back
/// on landscape and on desktop.
pub fn panel_w(s: &State) -> f32 {
    if s.screen.0 < 620.0 {
        return 0.0;
    }
    (s.screen.0 * 0.25).clamp(190.0, 300.0)
}

/// Pixels per tick at the timeline's zoom.
pub fn px_per_tick(s: &State) -> f32 {
    let bar = s.project.bar_ticks().max(1) as f32;
    BAR_PX / bar
}

/// The tick under timeline x (the scroll included).
pub fn tick_at(s: &State, x: f32) -> u32 {
    let lanes = lanes_rect(s);
    (s.baby.scroll_x as f32 + (x - lanes.x) / px_per_tick(s).max(0.01)).max(0.0) as u32
}

/// The left edge of `tick` on screen (clipped back into the lanes' span for painting).
pub fn x_of_tick(s: &State, t: u32) -> f32 {
    let lanes = lanes_rect(s);
    lanes.x + (t as f32 - s.baby.scroll_x as f32) * px_per_tick(s)
}

/// The screen rect of clip `c` (unclipped: the painter clips it against the lanes).
pub fn clip_rect(s: &State, c: &Clip) -> Rect {
    let lanes = lanes_rect(s);
    let x = x_of_tick(s, c.position);
    Rect::new(x, lanes.y + f32::from(c.track) * ROW_H + 3.0, (c.length.max(1) as f32) * px_per_tick(s), ROW_H - 6.0)
}

/// The clip under (`x`, `y`), topmost last drawn wins; `None` off the lanes or on the ground.
pub fn clip_at(s: &State, x: f32, y: f32) -> Option<usize> {
    let lanes = lanes_rect(s);
    if !lanes.contains(x, y) {
        return None;
    }
    let clips = crate::playlist::clips(s);
    (0..clips.len()).rev().find(|&i| clip_rect(s, &clips[i]).contains(x, y))
}

/// How many track rows the list and the lanes show: every track with a name or a clip, one spare
/// row under them, at least four (the playlist pads its track list to 500; the padding stays hidden).
pub fn visible_tracks(s: &State) -> u16 {
    let mut last = 0u16;
    if let Some(a) = s.project.arrangement() {
        for c in &a.clips {
            last = last.max(c.track + 1);
        }
        for (i, t) in a.tracks.iter().enumerate() {
            if t.name.as_deref().is_some_and(|n| !n.is_empty()) {
                last = last.max(i as u16 + 1);
            }
        }
    }
    (last + 1).max(4)
}

// ---------------------------------------------------------------------------------------------------------------
// Colours: the light, GarageBand-like look of the Baby screen (the rest of the app stays dark)

pub const BG: u32 = 0xE9E9EC;
pub const LIST_BG: u32 = 0xF1F1F4;
pub const ROW_BG: u32 = 0xFBFBFD;
pub const ROW_SEL: u32 = 0xD9E4FB;
pub const TIMELINE_BG: u32 = 0xE6E6E9;
pub const LANE_BG: u32 = 0xF3F3F5;
pub const RULER_TEXT: u32 = 0x5A5A60;
pub const TEXT: u32 = 0x3C3C42;
pub const TEXT_DIM: u32 = 0x8A8A92;
pub const SEL_BLUE: u32 = 0x2F6BD8;
pub const PLAYHEAD: u32 = 0xE04040;
// Aqua metal: the brushed bands (list, ruler, panel, transport), GarageBand's chrome.
pub const METAL: u32 = 0xD8D9DD;
pub const METAL_LIGHT: u32 = 0xE9EAEE;
pub const METAL_DARK: u32 = 0xC7C8CD;
pub const METAL_EDGE: u32 = 0x96979D;
/// Aqua's glossy blue: the selection rows, the Add Track capsule and the play ball.
pub const AQUA_TOP: u32 = 0xA6C8F5;
pub const AQUA_BOT: u32 = 0x3670D6;
pub const AQUA_EDGE: u32 = 0x25539F;
/// The LCD's dark well and its digits.
pub const LCD_BG: u32 = 0x2E2F33;
pub const LCD_EDGE: u32 = 0x1B1C1E;
pub const LCD_DIGIT: u32 = 0xEFF0EA;
pub const LCD_DIM: u32 = 0x85878C;
pub const LCD_AMBER: u32 = 0xE8B33C;
/// The traffic lights and the record dot.
pub const TL_RED: u32 = 0xF26057;
pub const TL_YELLOW: u32 = 0xF6BD2F;
pub const TL_GREEN: u32 = 0x61C554;
pub const REC_RED: u32 = 0xD93A2B;
/// Pattern (note) clips: GarageBand's greens.
pub const GREEN_TOP: u32 = 0x8CC973;
pub const GREEN_BOT: u32 = 0x64A94A;
pub const GREEN_EDGE: u32 = 0x4E8F33;
/// Audio clips: GarageBand's blues.
pub const BLUE_TOP: u32 = 0x74A3DC;
pub const BLUE_BOT: u32 = 0x4F80C4;
pub const BLUE_EDGE: u32 = 0x3A669F;
/// The starter riffs' pattern colours.
pub const DRUM_COL: u32 = 0xE07050;
pub const KEYS_COL: u32 = 0x67B862;
pub const BASS_COL: u32 = 0x5B8FD9;
pub const SYN_COL: u32 = 0xB090D0;
pub const SAMP_COL: u32 = 0x8AA0B8;

/// The colour of a track's chip and clips: the track's own, else a pastel of its name.
pub fn track_color(s: &State, t: u16) -> u32 {
    let named = crate::playlist::track_info(s, t).and_then(|t| t.color);
    match named {
        Some(c) => (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b),
        None => {
            let p = crate::playlist::pastel(&crate::playlist::track_name(s, t));
            (u32::from(p.r) << 16) | (u32::from(p.g) << 8) | u32::from(p.b)
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Model edits

/// The instrument a playlist track plays: its first audio clip's channel, else the first note's
/// channel of its first pattern clip.
pub fn track_channel(s: &State, t: u16) -> Option<u16> {
    let a = s.project.arrangement()?;
    for c in &a.clips {
        if c.track != t {
            continue;
        }
        match c.source {
            ClipSource::Channel(ch) => return Some(ch),
            ClipSource::Pattern(n) => {
                if let Some(note) = s.project.pattern(n).and_then(|p| p.notes.first()) {
                    return Some(note.channel);
                }
            }
        }
    }
    None
}

/// The name the instrument panel shows for a channel: the wrapped plugin's own name, else the
/// channel's, else the sample's file stem.
pub fn instrument_name(s: &State, ch: u16) -> Option<String> {
    let c = s.project.channel(ch)?;
    Some(
        crate::project_plugins::plugin_title(c)
            .or_else(|| c.name.clone())
            .unwrap_or_else(|| "Instrument".into()),
    )
}

/// Names the first track without one and without clips (the playlist's padding gives the slot) and
/// colours it; returns the track index.
pub fn ensure_track(s: &mut State, name: &str, color: Option<Rgb>) -> u16 {
    let used: Vec<u16> = crate::playlist::clips(s).iter().map(|c| c.track).collect();
    let a = crate::playlist::arr_mut(s);
    for (i, t) in a.tracks.iter_mut().enumerate() {
        let i = i as u16;
        if t.name.is_none() && !used.contains(&i) {
            t.name = Some(name.into());
            t.color = color;
            return i;
        }
    }
    let mut t = PlaylistTrack::new(a.tracks.len() as u32 + 1);
    t.name = Some(name.into());
    t.color = color;
    a.tracks.push(t);
    (a.tracks.len() - 1) as u16
}

/// The first pattern number no pattern has.
fn next_pattern(s: &State) -> u16 {
    s.project.patterns.iter().map(|p| p.number).max().unwrap_or(0) + 1
}

fn note(ch: u16, position: u32, key: u16, length: u32) -> Note {
    let mut n = Note::step(ch, position);
    n.key = key;
    n.length = length;
    n
}

/// A one-bar starter riff for the family `tile` on channel `ch`: the pattern is created, named and
/// coloured, and its number returned. The Sampler gets an empty pattern (a sample is the user's).
fn riff(s: &mut State, tile: usize, ch: u16) -> u16 {
    let n = next_pattern(s);
    let bar = s.project.bar_ticks();
    let beat = bar / u32::from(s.project.bar().0.max(1));
    let half = (beat / 2).max(1);
    let (name, color, notes): (&str, u32, Vec<Note>) = match tile {
        0 => {
            // Kick on the beats, a snare on the backbeats, hats on the eighths: a plain, useful beat.
            let mut ns = vec![note(ch, 0, 60, 1), note(ch, 2 * beat, 60, 1), note(ch, beat, 62, 1), note(ch, 3 * beat, 62, 1)];
            ns.extend((0..bar).step_by(half as usize).map(|p| note(ch, p, 64, 1)));
            ("Drums", DRUM_COL, ns)
        }
        1 => {
            // A rising C-major arpeggio, one note per half beat.
            let ns = [48u16, 52, 55, 60, 64, 60, 55, 52].iter().enumerate().map(|(i, &k)| note(ch, i as u32 * half, k, half)).collect();
            ("Keys", KEYS_COL, ns)
        }
        2 => {
            let ns = vec![note(ch, 0, 36, 2 * beat), note(ch, 2 * beat, 43, 2 * beat)];
            ("Bass", BASS_COL, ns)
        }
        3 => {
            // A soft chord stab on every beat: three notes, an octave under the keys riff.
            let ns = [0usize, 1, 2, 3].iter().flat_map(|&i| [48u16, 51, 55].map(|k| note(ch, i as u32 * beat, k, half))).collect();
            ("Synth", SYN_COL, ns)
        }
        _ => (FAMILIES.get(tile).copied().unwrap_or("Sampler"), SAMP_COL, Vec::new()),
    };
    let p = Pattern { number: n, name: Some(name.into()), color: Some(Rgb::new((color >> 16) as u8, (color >> 8) as u8, color as u8)), notes, length: Some(bar), ..Default::default() };
    s.project.patterns.push(p);
    n
}

/// Lays `count` one-bar clips of pattern `n` on track `t` from the song's start.
fn lay_clips(s: &mut State, t: u16, n: u16, count: u32) {
    let bar = s.project.bar_ticks();
    let a = crate::playlist::arr_mut(s);
    for i in 0..count {
        a.clips.push(Clip::new(i * bar, bar, t, ClipSource::Pattern(n)));
    }
}

/// Adds the instrument `index` (into `plugins.available`) as a new channel and a Baby track for it,
/// with family `tile`'s starter riff on the track so it makes sound at once. The channel hosts on
/// the next tick, as every add does.
pub fn add_family_at(app: &mut crate::app::App, tile: usize, index: usize) {
    let before = app.state.project.channels.len();
    crate::plugin_menu::add_generator(app, index, None);
    if app.state.project.channels.len() == before {
        return;
    }
    // The new channel is the last one and is selected (add_generator does both).
    let (ch, ch_name) = {
        let s = &app.state;
        match s.project.channels.last() {
            Some(c) => (c.index, crate::project_plugins::plugin_title(c).or_else(|| c.name.clone()).unwrap_or_else(|| "Instrument".into())),
            None => return,
        }
    };
    let color = app.state.project.channel(ch).and_then(|c| c.color);
    let t = ensure_track(&mut app.state, &ch_name, color);
    let n = riff(&mut app.state, tile, ch);
    lay_clips(&mut app.state, t, n, 4);
    app.state.playlist.track = t;
    app.state.pattern = n;
    app.state.message = Some(format!("{ch_name} added — a ready riff is on its track"));
}

/// Adds the family `tile`'s instrument (the sheet's rows are `FAMILIES` in order).
pub fn add_family(app: &mut crate::app::App, tile: usize) {
    let want = FAMILY_MATCH.get(tile).copied().unwrap_or("");
    let index = app
        .plugins
        .available
        .iter()
        .position(|a| a.desc.name.to_lowercase().contains(want))
        .unwrap_or(0);
    add_family_at(app, tile, index);
}

/// A first song for an empty project, the first time Baby Mode comes on: a drum beat, keys and bass,
/// each as its own track with a ready riff — the screen never opens empty.
pub fn seed_demo(app: &mut crate::app::App) {
    if app.state.project.arrangement().is_some_and(|a| !a.clips.is_empty()) {
        return;
    }
    for tile in 0..3 {
        add_family(app, tile);
    }
    // Give the first clip back the selection the adds took turns setting.
    app.state.playlist.track = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    fn app() -> App {
        let mut a = App::new(None);
        a.plugins.available = crate::plugins::builtin();
        a
    }

    /// Layout: the lanes keep two rows before the preview gives way, and the panel never swallows
    /// the timeline on a narrow screen.
    #[test]
    fn the_layout_fits_small_screens() {
        let mut a = app();
        a.state.screen = (360.0, 640.0);
        let s = &a.state;
        assert_eq!(panel_w(s), 0.0, "a phone keeps its width for the list and the timeline");
        assert!(lanes_rect(s).w > 200.0, "the timeline takes the panel's share");
        assert!(list_w(s) >= 120.0, "the list never collapses to nothing");
        let p = preview_rect(s).expect("the lanes yield, the preview still fits");
        assert!(p.h == PREVIEW_H);
        assert!(lanes_rect(s).h >= 2.0 * ROW_H, "two lanes at least even on a phone");
        a.state.screen = (1280.0, 800.0);
        let s = &a.state;
        let p = preview_rect(s).expect("a tall screen shows the preview");
        assert_eq!(p.h, PREVIEW_H);
        assert!(lanes_rect(s).h >= 2.0 * ROW_H);
        assert!(panel_rect(s).right() <= s.screen.0 + 1.0);
    }

    /// Add Track: a channel is created, a named track appears for it and four one-bar clips of a
    /// riff pattern sit on the track, selected.
    #[test]
    fn add_family_builds_a_track_with_a_riff() {
        let mut a = app();
        let channels = a.state.project.channels.len();
        add_family(&mut a, 1); // Keys
        assert_eq!(a.state.project.channels.len(), channels + 1, "the instrument channel is added");
        let t = a.state.playlist.track;
        assert!(crate::playlist::track_name(&a.state, t) != format!("Track {}", t + 1), "the track is named");
        let clips = crate::playlist::clips(&a.state).iter().filter(|c| c.track == t).count();
        assert_eq!(clips, 4, "a ready riff is laid out, four bars");
        let n = a.state.pattern;
        let pat = a.state.project.pattern(n).expect("the riff pattern exists");
        assert!(!pat.notes.is_empty(), "the keys riff has notes");
        assert_eq!(pat.notes[0].channel, a.state.project.channels.last().unwrap().index, "the riff plays the new channel");
    }

    /// Seed demo: an empty project gains three tracks with clips; a project that already has clips
    /// is left alone.
    #[test]
    fn seed_demo_fills_an_empty_project() {
        let mut a = app();
        seed_demo(&mut a);
        assert!(visible_tracks(&a.state) >= 3, "three tracks at least");
        assert!(!crate::playlist::clips(&a.state).is_empty());
        let clips = crate::playlist::clips(&a.state).len();
        seed_demo(&mut a);
        assert_eq!(crate::playlist::clips(&a.state).len(), clips, "a started project is left alone");
    }

    /// The transport bands the foot: every control stays inside it, play is the big ball, the LCD
    /// never crosses the band's right edge and the preview yields to the band.
    #[test]
    fn the_transport_bands_the_foot() {
        let mut a = app();
        a.state.screen = (1280.0, 800.0);
        let s = &a.state;
        let tr = transport_rect(s);
        assert_eq!(tr.h, TRANSPORT_H);
        assert!(stage(s).bottom() <= tr.y + 1.0, "the stage ends where the transport starts");
        let tc = transport_controls(s);
        for (name, r) in [("lcd", tc.lcd), ("play", tc.play), ("stop", tc.stop), ("rec", tc.rec), ("metro", tc.metro), ("add", tc.add)] {
            assert!(tr.x <= r.x && r.right() <= tr.right() + 1.0 && tr.y <= r.y && r.bottom() <= tr.bottom() + 1.0, "{name} leaves the band");
        }
        assert!(tc.play.w > tc.stop.w && tc.play.h > tc.stop.h, "play is the big ball");
        assert!(preview_rect(s).is_none_or(|p| p.bottom() <= tr.y + 1.0), "the preview yields to the transport");
        // A phone: the LCD and the buttons shrink; the clusters never cross.
        a.state.screen = (360.0, 640.0);
        let s = &a.state;
        let tr = transport_rect(s);
        let tc = transport_controls(s);
        assert!(tc.lcd.right() <= tr.right() + 1.0, "the LCD stays in the band");
        assert!(tc.rec.x >= tc.metro.right(), "the left and centre clusters never cross");
    }

    /// Geometry round-trips: a clip lands where its tick says, and `clip_at` finds it again.
    #[test]
    fn timeline_geometry_round_trips() {
        let mut a = app();
        a.state.screen = (1280.0, 800.0);
        let bar = a.state.project.bar_ticks();
        crate::playlist::arr_mut(&mut a.state).clips.push(Clip::new(bar, 2 * bar, 1, ClipSource::Pattern(1)));
        let s = &a.state;
        let c = &crate::playlist::clips(s)[0];
        let r = clip_rect(s, c);
        assert!(r.x > lanes_rect(s).x, "bar 1 sits right of the lanes' start");
        assert_eq!(clip_at(s, r.x + 4.0, r.y + 4.0), Some(0));
        assert_eq!(clip_at(s, lanes_rect(s).x + 2.0, r.y + 4.0), None, "the ground before it is empty");
        // Seek maths: the ruler's x gives the tick back.
        assert_eq!(tick_at(s, x_of_tick(s, 3 * bar)), 3 * bar);
    }
}
