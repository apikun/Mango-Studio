// Copyright (C) 2026 mikobuntu
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The rolling sampler's strip (issue #82), docked between the toolbar and the playlist: the newest master output as
//! a scrolling wave; a selection on it drops onto the playlist as an audio clip. Audio ▸ Rolling sampler turns the
//! sampler on and sets the tape's length; the arrow at the left collapses the strip.
//!
//! `App::tick` pulls the picture (`pull`), the paint draws it and registers the hits; the interactions live on
//! `App` (`Action::Rolling`, `Drag::RollSel`).

use std::time::Instant;

use os_ui::Rect;

use super::FrameInfo;
use crate::app::{Action, App, State, Ui};
use crate::views::fnt;

/// The collapse arrow's column.
pub const ARROW_W: f32 = 18.0;
/// The strip's height with the wave shown.
const TALL: f32 = 52.0;
/// The strip's height collapsed.
const SHORT: f32 = 14.0;
/// The smallest view, in seconds, that Shift+wheel zooms into.
pub const MIN_ZOOM: f32 = 0.1;

/// What the strip's hits do.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// The wave: a press draws a selection; a press on one begins the drag out to the playlist.
    Wave,
    /// The arrow at the left: collapse or expand the strip.
    Collapse,
}

/// The strip's height in the window's px: none while the setting is off, one row collapsed, the wave tall.
pub fn strip_h(s: &State) -> f32 {
    if !crate::prefs::check(&crate::prefs::ROLLING_SAMPLER) {
        0.0
    } else if s.roll.collapsed {
        SHORT
    } else {
        TALL
    }
}

/// The strip's rect over a window `w` px wide: the band under the toolbar's, over the workspace's top.
pub fn rect(s: &State, w: f32) -> Rect {
    let h = strip_h(s);
    Rect::new(0.0, super::work_y(s) - h, w, h)
}

/// The seconds of tape the view shows; 0 means the whole tape.
pub fn shown(s: &State) -> f32 {
    let cap = crate::prefs::rolling_secs();
    if s.roll.zoom > 0.0 {
        s.roll.zoom.min(cap.max(MIN_ZOOM))
    } else {
        cap
    }
}

/// The frame under `x` in the wave, clamped to the view's edges.
pub fn frame_at(s: &State, x: f32) -> u64 {
    let Some(w) = s.roll.wave else { return s.roll.from };
    let k = ((x - w.x) / w.w.max(1.0)).clamp(0.0, 1.0) as f64;
    s.roll.from + (s.roll.visible as f64 * k) as u64
}

/// The x of `frame` in the wave; frames off the view clamp to its edges.
pub fn x_of(s: &State, frame: u64) -> f32 {
    let Some(w) = s.roll.wave else { return 0.0 };
    let k = (frame as f64 - s.roll.from as f64) / s.roll.visible.max(1) as f64;
    w.x + k.clamp(0.0, 1.0) as f32 * w.w
}

/// Pulls the strip's picture from the tape (in `App::tick`, and after every zoom, pan or collapse): the view's
/// frames, the newest frame heard, the wave's rect and the per-column peaks of the wave's width.
pub fn pull(app: &mut App) {
    let w = app.state.screen.0;
    let h = strip_h(&app.state);
    let (oldest, heard, rate) = app.transport.rolling().window(Instant::now());
    let s = &mut app.state;
    s.roll.heard = heard;
    s.roll.rate = rate;
    s.roll.paused = app.transport.rolling().paused();
    s.roll.have = (oldest, heard);
    if h <= SHORT + 0.5 {
        s.roll.wave = None;
        return;
    }
    let wave = Rect::new(ARROW_W + 1.0, super::work_y(s) - h + 1.0, (w - ARROW_W - 2.0).max(1.0), h - 2.0);
    s.roll.wave = Some(wave);
    let cols = wave.w.round().max(1.0) as usize;
    if s.roll.cols.len() != cols {
        s.roll.cols.resize(cols, (0.0, 0.0));
    }
    if rate == 0 {
        s.roll.cols.fill((0.0, 0.0));
        s.roll.visible = 0;
        s.roll.from = 0;
        return;
    }
    let visible = ((shown(s) as f64 * f64::from(rate)) as u64).max(1);
    let from = match s.roll.hold {
        Some(held) => held,
        None => heard.saturating_sub(visible + s.roll.pan),
    };
    s.roll.from = from;
    s.roll.visible = visible;
    app.transport.rolling().scan(from, visible, &mut s.roll.cols);
}

/// Paints the strip over the window's width.
pub fn paint(ui: &mut Ui, s: &State, _f: &FrameInfo) {
    let (w, _) = ui.c.size();
    let r = rect(s, w);
    if r.h <= 0.0 {
        return;
    }
    let (edge, fill, ink) = (ui.t.s(0x1d2022), ui.t.sunken, ui.t.tx(0xb9bfc3));
    ui.c.fill_rect(r, edge);
    ui.c.fill_rect(Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0), fill);

    // The collapse arrow's column, a chevron in its middle.
    let ar = Rect::new(r.x, r.y, ARROW_W, r.h);
    let chev = if s.roll.collapsed { ">" } else { "<" };
    let cw = ui.c.text_width(fnt(12.0), chev);
    ui.c.text(fnt(12.0), ar.x + (ar.w - cw) / 2.0, r.y + (r.h - 12.0) / 2.0, ink, chev);
    ui.hit(ar, Action::Rolling(Act::Collapse), if s.roll.collapsed { "Show the rolling sampler" } else { "Hide the rolling sampler" });
    ui.c.fill_rect(Rect::new(ar.x + ar.w, r.y, 1.0, r.h), edge);
    if s.roll.collapsed {
        return;
    }

    let Some(wv) = s.roll.wave else { return };
    // The wave: a column a device pixel, min/max of its frames about the middle. A column of nothing outside the
    // tape (before its oldest frame, past the newest heard) draws nothing: it is not silence, it is no tape.
    let mid = wv.y + wv.h / 2.0;
    let amp = wv.h / 2.0 - 1.0;
    let ink = crate::prefs::rolling_wave_color().map_or_else(|| ui.t.tx(0xb9bfc3), os_ui::Color::rgb);

    // Time marks back from the newest frame, a step that keeps its labels apart. The lines go down behind
    // the wave — a ruler on the strip's back wall, not a dark hairline cut through it — and only marks
    // inside the view are taken: they used to pile up clamped on the left edge (`x_of` clamps, so the old
    // off-view break could not fire).
    let mut marks: Vec<(f32, String)> = Vec::new();
    if s.roll.rate > 0 && s.roll.visible > 0 {
        let per_px = shown(s) / wv.w.max(1.0);
        let step = [1.0f64, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0]
            .into_iter()
            .find(|&t| t / f64::from(per_px) >= 70.0)
            .unwrap_or(600.0);
        let tape_secs = s.roll.heard as f64 / f64::from(s.roll.rate.max(1));
        let mut t = step;
        while t < tape_secs {
            let frame = s.roll.heard.saturating_sub((t * f64::from(s.roll.rate)) as u64);
            if frame < s.roll.from {
                break; // past the view's left edge, and the rest only older
            }
            let x = x_of(s, frame);
            if x < wv.x + wv.w {
                let label = if t >= 60.0 && t % 60.0 == 0.0 { format!("-{}:00", t as u64 / 60) } else { format!("-0:{:02}", t as u64) };
                marks.push((x, label));
            }
            t += step;
        }
    }
    for &(x, _) in &marks {
        ui.c.fill_rect(Rect::new(x, wv.y, 1.0, wv.h), edge);
    }

    for (c, &(lo, hi)) in s.roll.cols.iter().enumerate() {
        let at = s.roll.from + (s.roll.visible as f64 * (c as f64 + 0.5) / s.roll.cols.len().max(1) as f64) as u64;
        if (lo, hi) == (0.0, 0.0) && !(s.roll.have.0..s.roll.have.1).contains(&at) {
            continue;
        }
        let (lo, hi) = (lo.clamp(-1.0, 1.0), hi.clamp(-1.0, 1.0));
        let y0 = mid - hi * amp;
        let y1 = mid - lo * amp;
        ui.c.fill_cell(Rect::new(wv.x + c as f32, y0.min(y1), 1.0, (y1 - y0).max(1.0)), ink);
    }

    // Paused (a Ctrl+click on the wave): the newest audio is not taken while it stands.
    if s.roll.paused {
        let text = "Paused";
        let tw = ui.c.text_width(fnt(11.0), text);
        let b = Rect::new(wv.right() - tw - 10.0, wv.y + 3.0, tw + 8.0, 15.0);
        ui.c.fill_rrect(b, 3.0, ui.t.accent);
        ui.c.text(fnt(11.0), b.x + 4.0, b.y + 2.0, ui.t.s(0x101214), text);
    }

    // The marks' labels, over the wave.
    for (x, label) in &marks {
        let lw = ui.c.text_width(fnt(11.0), label);
        ui.c.text(fnt(11.0), (x + 2.0).min(wv.x + wv.w - lw - 2.0), wv.y + 2.0, ink, label);
    }

    // The selection, its edges accented.
    if let Some((a, b)) = s.roll.sel {
        let (x0, x1) = (x_of(s, a), x_of(s, b));
        let sel = Rect::new(x0, wv.y, (x1 - x0).max(2.0), wv.h);
        ui.c.fill_rect(sel, ui.t.accent.with_alpha(56));
        ui.c.fill_rect(Rect::new(x0, wv.y, 1.0, wv.h), ui.t.accent);
        ui.c.fill_rect(Rect::new(x1 - 1.0, wv.y, 1.0, wv.h), ui.t.accent);
    }

    let hint = if s.roll.paused {
        "Rolling sampler paused — Ctrl+click to keep taking the newest output"
    } else if s.roll.sel.is_some() {
        "Rolling sampler — drag the selection onto a playlist track, click beside it to select again, Ctrl+click to pause"
    } else {
        "Rolling sampler — drag to select a piece of the recent output, then drag it onto a playlist track; Ctrl+click pauses"
    };
    ui.hit(wv, Action::Rolling(Act::Wave), hint);
}
