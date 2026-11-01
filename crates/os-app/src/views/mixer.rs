// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 Artig3nce
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mixer window (strips in the chosen layout plus the selected track's slot panel).
// spec: ui/main-window.md §7 (layout), ui/mixer.md (controls, hints, routing look); fader law: audio/gain-laws.md

use std::f32::consts::PI;

use os_ui::kit::lemon as lk;
use os_ui::{Color, Font, Glyph, Rect};

use super::{led_small, resize_zones, window_body, window_buttons, FrameInfo};
use crate::knobs::KnobId;
use crate::app::{Action, Live, State, Ui, Win, TITLE_H};
use crate::meters;
use crate::mixer::{self as mx, MHit};
use crate::icons::{self, Icon};

pub const STRIP_W: f32 = 44.0;
/// Fader travel: value units per pixel (handle centre 618 at 0 dB = value 0.8, 970 at value 0).
pub const FADER_PER_PX: f64 = 1.0 / 440.0;
/// Fader change per wheel notch: 4 px of drag. Fitted to the one measurement (spec: ui/mixer.md §4): three notches up
/// from unity read `103%  0.8dB  1.09`, which puts a notch between 0.0088 and 0.0092.
pub const FADER_NOTCH: f64 = 4.0 * FADER_PER_PX;

// Vertical layout at the default 1080p window (spec §7): absolute y for a window top of 83;
// bottom-anchored values are offsets from the window bottom.
const HEAD_Y0: f32 = 107.0; // header numbers row
const HEAD_Y1: f32 = 127.0;
const METER_Y0: f32 = 130.0;
const METER_Y1: f32 = 453.0;
const PANEL_Y0: f32 = 128.0; // dark head panel behind meter, name, LED, pan
const PANEL_Y1: f32 = 508.0;
const LED_Y: f32 = 467.0;
const PAN_Y: f32 = 496.5;
const SEP_Y: f32 = 527.0;
const SWAP_Y: f32 = 545.0;
const RING_Y: f32 = 563.0;
const TRACK_Y0: f32 = 525.0;
const HANDLE_0DB: f32 = 618.0; // handle centre at 0 dB (value 0.8)

/// dB marks of the level scale and their measured y positions (number centres).
const SCALE: [(&str, f32); 17] = [
    ("3", 181.0), ("0", 241.0), ("3", 301.0), ("6", 360.0), ("9", 419.0), ("12", 479.0), ("15", 535.0), ("18", 588.0),
    ("21", 639.0), ("24", 686.0), ("27", 728.0), ("30", 766.0), ("33", 799.0), ("36", 826.0), ("39", 848.0), ("42", 866.0),
    ("45", 880.0),
];

/// The "C" column's marks as (label, reference y), top down: the default scale's `SCALE`, its first mark the "+3"; with
/// `alt` ("Alternate meter scale") 0, 3 … 30 at the heights the default law puts the levels `meters::ALTERNATE` maps
/// them to, with no +3 (spec: ui/settings-choices.md "Miscellaneous: seven more checks, Default template and Startup
/// project").
pub fn scale_marks(alt: bool, floor: f32) -> Vec<(&'static str, f32)> {
    if !alt {
        return SCALE.to_vec();
    }
    SCALE[1..].iter().zip(meters::ALTERNATE).map(|(&(m, _), (_, d))| (m, meters::law_y(d, floor, false))).collect()
}

/// Height of the mixer window the absolute y values above were measured at (1080p workspace).
const REF_H: f32 = 997.0;
/// Reference y of the window top.
const REF_TOP: f32 = 83.0;
/// Height of the parts that do not scale: header, head-panel controls and the bottom controls.
const FIXED_H: f32 = 218.0;

/// How much the mixer's stretchy parts (meter, level scale, fader) are scaled at window `r`:
/// 1 at the measured height; the rest of the strip keeps its size.
pub fn scale(r: Rect) -> f32 {
    ((r.h - FIXED_H) / (REF_H - FIXED_H)).max(0.2)
}

/// Maps the measured (absolute, window top 83) y values to the window `r`: the meter (130-453) and
/// the fader (525-981) scale, the controls between them and below them keep their size.
struct Ly {
    top: f32,
    k: f32,
}

impl Ly {
    fn new(r: Rect) -> Ly {
        Ly { top: r.y, k: scale(r) }
    }

    fn y(&self, abs: f32) -> f32 {
        let (m0, m1, f0, f1) = (130.0 - REF_TOP, 453.0 - REF_TOP, 525.0 - REF_TOP, 981.0 - REF_TOP);
        let ry = abs - REF_TOP;
        let k = self.k;
        let y = if ry < m0 {
            ry
        } else if ry < m1 {
            m0 + (ry - m0) * k
        } else if ry < f0 {
            m0 + (m1 - m0) * k + (ry - m1)
        } else if ry < f1 {
            m0 + (m1 - m0) * k + (f0 - m1) + (ry - f0) * k
        } else {
            m0 + (m1 - m0) * k + (f0 - m1) + (f1 - f0) * k + (ry - f1)
        };
        self.top + y
    }
}

pub fn fader_db_text(v: f64) -> String {
    let gain = os_engine::laws::fader_gain(v);
    if gain <= 0.0 { "-inf dB".into() } else { format!("{:+.1} dB", 20.0 * gain.log10()) }
}


pub fn paint(ui: &mut Ui, s: &State, r: Rect, f: &FrameInfo) {
    let panel_w = 221.0;
    window_body(ui, r);
    let outer = r;
    let inner = super::float_frame(ui, s, Win::Mixer, r);
    let r = inner.unwrap_or(r);
    let client = super::window_frame_ex(ui, Win::Mixer, Rect::new(r.x, r.y, r.w - panel_w, r.h), "", 0.0, false, inner.is_some());
    // The window's own rim, like the toolbar readouts and the other windows (theme token `display_edge`).
    super::display_edge(ui.c, ui.t, outer, if super::lemon(ui) { os_ui::kit::lemon::r_panel(ui.t) } else { 0.0 });
    title_tools(ui, s, r);
    if r.h < 2.0 * TITLE_H {
        // Collapsed to the title bar: only the slot panel's title with the window buttons.
        s.cables.borrow_mut().clear();
        slot_panel(ui, s, Rect::new(r.right() - panel_w, r.y, panel_w, r.h));
        return;
    }
    let ly = Ly::new(r);
    // Every layout but Wide is laid out by its `Geo`: the header at the measured height, the rest anchored to it and to
    // the window's bottom.
    let wide = s.mixer.layout == mx::WIDE;
    let g = geo(s);
    let sw = g.w;
    let head0 = if wide { ly.y(HEAD_Y0) } else { r.y + HEAD_Y0 - REF_TOP };
    let head_h = if wide { HEAD_Y1 - HEAD_Y0 } else { g.head_h };

    let strips = Rect::new(r.x, client.y, r.w - panel_w - 7.0, client.h);
    ui.c.fill_rect(Rect::new(r.x, client.y, r.w - panel_w, client.h), ui.t.s(0x232527));
    ui.hit(strips, Action::MixerArea, "Mixer");
    let inserts = mx::inserts(s);
    let bottom = r.bottom();

    // Header row: "C" (scale) and "M" (master) on a lighter band; "Current" and "Master" in Wide 3 and Extra large.
    ui.c.fill_rect(Rect::new(r.x + 1.0, head0, g.region + sw + 4.0, head_h + 1.0), ui.t.s(0x2b2e30));
    let (c_label, m_label) = if g.long { ("Current", "Master") } else { ("C", "M") };
    let (c_rect, m_rect) = if wide {
        (Rect::new(r.x + 12.0, head0, 20.0, 21.0), Rect::new(r.x + 50.0, head0, 22.0, 21.0))
    } else {
        (Rect::new(r.x, head0, g.region, 21.0), Rect::new(r.x + g.region, head0, sw, 21.0))
    };
    let hf = Font::new(if sw < 30.0 { 14 } else if g.long { 15 } else { 17 });
    ui.c.text_center(hf, c_rect, ui.t.s(0x6d7275), c_label);
    ui.c.text_center(hf, m_rect, ui.t.s(0x6d7275), m_label);

    // "C" column: the level scale with the current track's meter (spec: ui/mixer.md §9).
    let col = if wide {
        Rect::new(r.x + 9.0, ly.y(METER_Y0), 28.0, bottom - 156.0 - ly.y(METER_Y0))
    } else {
        let top = head0 + head_h + 6.0;
        Rect::new(r.x + g.col.0, top, g.col.1, (bottom + g.col.2 - top).max(10.0))
    };
    let cfg = &s.mixer.meter_cfg;
    let floor = cfg.v[meters::k::FLOOR_DB];
    // The column is the law's whole range (130-924 at the measured size) squeezed evenly into its height, so marks and
    // bars keep their proportions at any mixer height (the strips' fixed-size controls do not apply here).
    let law_h = meters::LAW_BOTTOM - meters::LAW_TOP;
    let cy = |abs: f32| col.y + (abs - meters::LAW_TOP) / law_h * col.h;
    let alt = crate::prefs::check(&crate::prefs::ALTERNATE_METER_SCALE);
    let hint = if col.contains(ui.mouse.0, ui.mouse.1) {
        // The level at the mouse (the scale is not linear in y: invert it through the law).
        let db = meters::db_at((col.bottom() - ui.mouse.1) / col.h.max(1.0), floor, alt);
        format!("Current  {db:.2}dB  {:.2}", 10f32.powf(db / 20.0))
    } else {
        "Level scale".to_string()
    };
    ui.hit(col, Action::Mix(MHit::Scale), hint);
    let lemon = super::lemon(ui);
    if lemon {
        lk::outlined(ui.c, col, lk::r_btn(ui.t), ui.t.s(0x191d1e), ui.t.s(0x222527));
    } else {
        ui.c.fill_rrect(col, 3.0, ui.t.s(0x191d1e));
        ui.c.fill_rrect(col.inset(1.0), 2.5, ui.t.s(0x222527));
    }
    // Marks closer than a label height are skipped when the window is squished: "0" always shows, "+3" only with room
    // above it, the rest downward from "0". The alternate scale has no "+3".
    let scale = scale_marks(alt, floor);
    let (plus, below) = if alt { (None, &scale[..]) } else { (Some(scale[0]), &scale[1..]) };
    let zero = cy(below[0].1);
    let mut marks: Vec<(&str, f32)> = Vec::new();
    if let Some((m, y)) = plus.map(|(m, y)| (m, cy(y))).filter(|&(_, y)| zero - y >= 14.0) {
        marks.push((m, y));
    }
    marks.push((below[0].0, zero));
    let mut last = zero;
    for &(m, y) in &below[1..] {
        let y = cy(y);
        if y - last >= 14.0 {
            marks.push((m, y));
            last = y;
        }
    }
    let mf = Font::new(if col.w < 20.0 { 10 } else { 12 });
    for (m, y) in marks {
        ui.c.text_center(mf, Rect::new(col.x, y - 8.0, col.w, 15.0), ui.t.s(0x7f8a90), m);
        ui.c.fill_rect(Rect::new(col.x + 4.0, y + 7.0, col.w - 8.0, 1.0), ui.t.s(0x111315));
    }
    // Four unlabelled ticks below 45 (the alternate scale has none below its 30).
    for k in (0..4).filter(|_| !alt) {
        let y = cy(893.0 + 7.5 * k as f32);
        if y - last >= 10.0 {
            ui.c.fill_rect(Rect::new(col.x + 4.0, y, col.w - 8.0, 1.0), ui.t.s(0x111315));
        }
    }
    // The bars cover the labels (measured: two 13 px bars filling the column inside its 1 px border).
    ui.live(Live::Meter, col);
    if let Some(m) = s.mixer.meters.get(usize::from(s.selected_mixer)) {
        let ys = |db: f32| cy(meters::law_y(db, floor, alt)).min(col.bottom() - 1.0);
        let bw = (col.w - 2.0) / 2.0;
        for i in 0..2 {
            meter_bar(ui, col.x + 1.0 + bw * i as f32, bw, col.bottom() - 1.0, m.db[i], m.hold[i], cfg, &ys);
        }
    }

    // Inserts scroll; the master strip and the scale stay put.
    let div = g.region + sw;
    let area = Rect::new(r.x + div + 9.0, client.y, strips.w - div - 9.0, client.h);
    ui.c.fill_rect(Rect::new(r.x + div, client.y, 9.0, client.h), ui.t.s(0x1a1e20));
    let dots = if wide { ly.y(497.0) } else { head0 + head_h + 120.0 };
    for k in 0..3 {
        ui.c.fill_rect(Rect::new(r.x + div + 4.0, dots + 3.0 * k as f32, 1.0, 1.0), ui.t.s(0x3a3e41));
    }
    ui.c.push_clip(area);
    let visible = mx::visible_tracks(s);
    let total_tracks = visible.len();
    let first = (s.mixer_scroll / sw) as usize;
    let count = (area.w / sw).ceil() as usize + 2;
    let draw = |ui: &mut Ui, n: u16, left: f32| {
        if wide {
            strip(ui, s, n, left, bottom, f, &ly);
        } else {
            strip_geo(ui, s, n, left, head0, bottom, f, g);
        }
    };
    for (i, &n) in visible.iter().enumerate().skip(first).take(count) {
        let left = area.x + sw * i as f32 - s.mixer_scroll;
        draw(ui, n, left);
    }
    // "+" after the last strip, at the fader handle's height (spec: ui/mixer.md §1).
    if s.pattern_mixer_filter.is_none() {
        let px = area.x + sw * f32::from(inserts) - s.mixer_scroll + 14.0;
        let py = if wide {
            ly.y(HANDLE_0DB)
        } else {
            let (f0, f1) = (Geo::at(head0, bottom, g.fader.1), Geo::at(head0, bottom, g.fader.2));
            f0 + 5.0 + 0.2 * (f1 - f0 - 10.0)
        };
        let pb = Rect::new(px - 9.0, py - 9.0, 18.0, 18.0);
        if px < area.right() + 10.0 {
            let hov = ui.hit(pb, Action::Mix(MHit::Add), "Add more tracks");
            if hov && lemon {
                ui.c.fill_rrect(pb, lk::r_btn(ui.t), ui.t.button_hi);
            } else if hov {
                ui.c.stroke_rrect(pb, 3.0, 1.0, ui.t.h(0xf09a28));
            }
            icons::draw(ui.c, Icon::Plus, pb.inset(3.0), if hov { ui.t.tx(0xffffff) } else { ui.t.tx(0x9aa1a5) });
        }
    }
    ui.c.pop_clip();
    draw(ui, 0, r.x + g.region);

    // Horizontal scroll bar under the strips.
    let sb = Rect::new(area.x + 13.0, bottom - 14.0, area.w - 20.0, 9.0);
    super::scrollbar(ui, sb, false, s.mixer_scroll, area.w, (total_tracks as f32 + 1.0) * sw, crate::app::Scroll::MixerX);
    chevron(ui, area.x + 5.0, bottom - 9.5, false, ui.t.s(0x5d6366));
    cables(ui, s, strips, area, bottom, &visible, g);

    slot_panel(ui, s, Rect::new(r.right() - panel_w, r.y, panel_w, r.h));
    resize_zones(ui, s, Win::Mixer, outer);
}

/// The routing cables (spec: ui/mixer.md §5): one from the current track's ⬇ into each of its send knobs, sagging
/// and swinging (crate::cable). Sockets on strips scrolled out of view still hold their cable's end.
fn cables(ui: &mut Ui, s: &State, strips: Rect, area: Rect, bottom: f32, visible: &[u16], g: &Geo) {
    let sw = g.w;
    let socket_x = |n: u16| match n {
        0 => strips.x + g.region + sw / 2.0,
        n => {
            let idx = visible.iter().position(|&t| t == n);
            match idx {
                Some(i) => area.x + sw * i as f32 - s.mixer_scroll + sw / 2.0,
                None => -1000.0,
            }
        }
    };
    // Each cable is plugged in under the middle of its ⬇ and its knob, which are drawn again over the cables' ends.
    let (ay, ky) = (bottom - 38.0, bottom - 40.0);
    let cur = s.selected_mixer;
    let from = (socket_x(cur), ay);
    let sends: Vec<(u16, f64)> = (0..=mx::inserts(s)).filter(|&n| n != cur).filter_map(|n| Some((n, mx::send(s, cur, n)?))).collect();
    let mut cables = s.cables.borrow_mut();
    cables.begin(cur);
    for &(n, level) in &sends {
        // A send at zero is dimmed, as its knob.
        let col = if level <= 0.0 { ui.t.accent.mix(ui.t.s(0x26292b), 0.65) } else { ui.t.accent };
        cables.connect(n, from, (socket_x(n), ky), cur == 0 || n == 0, col);
    }
    cables.end();
    // Cables between inserts are clipped where the inserts scroll; those into the master are not.
    for (master, pts, col) in cables.each() {
        ui.c.push_clip(if master { strips } else { area });
        ui.c.polyline(pts, 2.0, col);
        ui.c.pop_clip();
    }
    drop(cables);
    if sends.is_empty() {
        return;
    }
    let clip = |n: u16| if n == 0 { strips } else { area };
    ui.c.push_clip(clip(cur));
    down_arrow(ui, from.0, ay);
    ui.c.pop_clip();
    for (n, level) in sends {
        ui.c.push_clip(clip(n));
        send_knob(ui, s, n, socket_x(n), ky, level, g.send);
        ui.c.pop_clip();
    }
}

fn title_tools(ui: &mut Ui, s: &State, r: Rect) {
    let y = r.y + 11.5;
    let ob = Rect::new(r.x + 3.0, y - 9.0, 16.0, 18.0);
    let hov = ui.hit(ob, Action::Mix(MHit::Options), "Mixer options");
    chevron(ui, r.x + 11.0, y, true, if hov { ui.t.tx(0xffffff) } else { ui.t.tx(0xc4ccd0) });
    // View buttons (multi-touch, waveforms, extra properties, track inspector): not available yet, greyed out (drawn
    // in their colours faded into the title bar, like `Ui::veil`; a veil would show over the bar's gradient). Lemon
    // centres them and the "+" 24 px apart: its inspector sits 10 px further right, the "+" and the combo 16 px.
    let bg = ui.t.window_title;
    let dim = |c: Color| c.mix(bg, f32::from(crate::app::OFF_VEIL) / 255.0);
    for (icon, dx, name) in [(Icon::Touch, 37.0, "Multi-touch control"), (Icon::Plug, 61.0, "View waveforms")] {
        let b = Rect::new(r.x + dx - 9.0, y - 9.0, 18.0, 18.0);
        ui.off(b, name);
        icons::draw(ui.c, icon, b.inset(1.0), dim(ui.t.tx(0xc4ccd0)));
    }
    let (ix, px) = if super::lemon(ui) { (10.0, 16.0) } else { (0.0, 0.0) };
    ui.off(Rect::new(r.x + 76.0, y - 9.0, 16.0, 18.0), "Extra volume / stereo properties");
    ui.off(Rect::new(r.x + 92.0 + ix, y - 9.0, 15.0, 18.0), "Track inspector");
    // Bow-tie (stereo) in blue, layout square in orange, then "+" and the view combo.
    let mut g = Glyph::new();
    let cx = r.x + 85.0;
    g.polygon(&[(cx - 8.0, y - 6.0), (cx - 1.0, y), (cx - 8.0, y + 6.0)]).polygon(&[(cx + 8.0, y - 6.0), (cx + 1.0, y), (cx + 8.0, y + 6.0)]);
    g.rect(cx - 8.5, y - 6.0, 1.5, 12.0);
    ui.c.glyph(dim(Color::rgb(0x5fb6f0)), &g);
    let mut g = Glyph::new();
    g.frame(Rect::new(r.x + 93.0 + ix, y - 6.0, 12.0, 12.0), 1.6);
    ui.c.glyph(dim(ui.t.h(0xf3b234)), &g);
    ui.c.fill_rect(Rect::new(r.x + 101.5 + ix, y - 4.5, 2.0, 9.0), dim(Color::rgb(0xd03a2a)));
    let pb = Rect::new(r.x + 109.0 + px, y - 8.0, 16.0, 16.0);
    let hov = ui.hit(pb, Action::Mix(MHit::AddAfter), "Add a new track after the selected track");
    icons::draw(ui.c, Icon::Plus, Rect::new(r.x + 110.0 + px, y - 7.0, 14.0, 14.0), if hov { ui.t.tx(0xffffff) } else { ui.t.tx(0xc4ccd0) });
    let lb = Rect::new(r.x + 130.0 + px, r.y + 2.0, 50.0, 20.0);
    let hov = ui.hit(lb, Action::Mix(MHit::Layout), "Mixer layout");
    let name = mx::LAYOUTS[usize::from(s.mixer.layout).min(mx::LAYOUTS.len() - 1)];
    ui.c.text(Font::new(17), r.x + 134.0 + px, r.y + 2.0, if hov { ui.t.tx(0xffffff) } else { ui.t.tx(0xb8c4cb) }, name);

    if let Some(pat) = s.pattern_mixer_filter {
        let pat_name = crate::patterns::name_of(s, pat);
        let badge_text = format!("Focus: {pat_name}  ✕");
        let badge_w = (badge_text.len() as f32 * 7.5 + 16.0).max(80.0);
        let badge = Rect::new(r.x + 190.0 + px, r.y + 2.0, badge_w, 20.0);
        let hov = ui.hit(badge, Action::Mix(MHit::ClearPatternFilter), "Click to clear pattern focus and show all mixer tracks");
        let bg_col = if hov { ui.t.s(0x3a4248) } else { ui.t.s(0x283036) };
        ui.c.fill_rrect(badge, 3.0, bg_col);
        ui.c.stroke_rrect(badge, 3.0, 1.0, ui.t.h(0xf09a28));
        ui.c.text_center(Font::new(13), badge, if hov { ui.t.tx(0xffffff) } else { ui.t.h(0xf3b234) }, &badge_text);
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u32;
    Color::rgb(ch(a.r, b.r) << 16 | ch(a.g, b.g) << 8 | ch(a.b, b.b))
}

fn shade(c: Color, k: f32) -> Color {
    mix(Color::rgb(0), c, k)
}

/// One meter bar (spec: ui/mixer.md §9, measured from FL): the colour depends on the height, not on the level: green up
/// to "Yellow from", yellow-orange up to "Red from", red above; each zone is a slight vertical gradient. The outer
/// columns are darker, the next ones a touch lighter; a 1 px dark cap sits on the top and the peak-hold line is a
/// 1 px orange line. `ys` maps a level to y; `bottom` is the bar's foot.
#[allow(clippy::too_many_arguments)]
fn meter_bar(ui: &mut Ui, x: f32, w: f32, bottom: f32, db: f32, hold: f32, cfg: &meters::Settings, ys: &dyn Fn(f32) -> f32) {
    use meters::k;
    let t = ui.t;
    let full_top = ys(12.0);
    let grad = cfg.v[k::GRADIENT];
    let edge = cfg.v[k::EDGE];
    // Zone colours at the zone's top and bottom (measured 0xa0db40 / 0xb6db4d, 0xeec757 / 0xfac050).
    let green = (ui.t.a(0xa0db40), ui.t.a(0xb6db4d));
    let yellow = (ui.t.h(0xeec757), ui.t.h(0xfac050));
    let red = (t.meter_clip, t.meter_clip);
    let y_yel = ys(cfg.v[k::YELLOW_DB]);
    let y_red = ys(cfg.v[k::RED_DB].max(cfg.v[k::YELLOW_DB]));
    if db > meters::SILENT {
        let top = ys(db).min(bottom);
        for (z0, z1, (ct, cb)) in [(y_yel, bottom, green), (y_red, y_yel, yellow), (full_top, y_red, red)] {
            let (a, b) = (top.max(z0), z1.min(bottom));
            if b - a < 0.5 {
                continue;
            }
            // Colours at a and b along the zone's gradient (scaled by the gradient setting).
            let at = |y: f32| mix(ct, mix(ct, cb, grad), (y - z0) / (z1 - z0).max(1.0));
            let (ca, cb2) = (at(a), at(b));
            let r = Rect::new(x, a, w, b - a);
            ui.c.fill_rect_gradient(r, ca, cb2, 0.0);
            if w >= 4.0 {
                let hl = |c: Color| mix(c, Color::rgb(0xffffff), 0.04);
                ui.c.fill_rect_gradient(Rect::new(x + 1.0, a, 1.0, b - a), hl(ca), hl(cb2), 0.0);
                ui.c.fill_rect_gradient(Rect::new(x + w - 2.0, a, 1.0, b - a), hl(ca), hl(cb2), 0.0);
                ui.c.fill_rect_gradient(Rect::new(x, a, 1.0, b - a), shade(ca, edge), shade(cb2, edge), 0.0);
                ui.c.fill_rect_gradient(Rect::new(x + w - 1.0, a, 1.0, b - a), shade(ca, edge), shade(cb2, edge), 0.0);
            }
        }
        if cfg.v[k::CAP] >= 0.5 && top - 1.0 > full_top {
            ui.c.fill_rect(Rect::new(x, top - 1.0, w, 1.0), ui.t.s(0x181c1e));
        }
    }
    if cfg.v[k::HOLD_ON] >= 0.5 && hold > meters::SILENT {
        let y = ys(hold).min(bottom - 1.0);
        let c = ui.t.h(0xe16e2f);
        ui.c.fill_rect(Rect::new(x, y, w, 1.0), c);
        if w >= 4.0 {
            ui.c.fill_rect(Rect::new(x, y, 1.0, 1.0), shade(c, edge));
            ui.c.fill_rect(Rect::new(x + w - 1.0, y, 1.0, 1.0), shade(c, edge));
        }
    }
}

/// One strip; `left` is its left edge (44 px pitch). Columns: knobs at +22, meter / fader at +32,
/// name and small controls at +12 (spec §7).
fn strip(ui: &mut Ui, s: &State, n: u16, left: f32, bottom: f32, f: &FrameInfo, ly: &Ly) {
    let track = s.project.mixer.iter().find(|m| m.index == n);
    let (kx, mx, sx) = (left + 22.0, left + 32.0, left + 12.0);
    let (body, hov) = strip_body(ui, s, n, left, STRIP_W, (ly.y(HEAD_Y0), ly.y(HEAD_Y1), ly.y(PANEL_Y1)), bottom, Font::new(17));

    // Dark head panel (name, LED, pan) with a rounded bottom.
    head_panel(ui, Rect::new(left + 1.0, ly.y(PANEL_Y0), STRIP_W - 2.0, ly.y(PANEL_Y1) - ly.y(PANEL_Y0)), track.and_then(|m| m.color).map(super::rgb));

    // Meter slot with its tick marks; the selected strip gets a green frame.
    let meter = Rect::new(mx - 7.5, ly.y(METER_Y0), 16.0, ly.y(METER_Y1) - ly.y(METER_Y0));
    let ticks: Vec<f32> = [181.0, 230.0, 278.0, 327.0, 375.0, 423.0].iter().map(|&y| ly.y(y)).collect();
    strip_meter(ui, s, n, meter, &ticks);

    // Name, rotated, reading upward from the bottom of the meter.
    let clip = Rect::new(left + 2.0, ly.y(PANEL_Y0), 20.0, ly.y(METER_Y1) - ly.y(PANEL_Y0));
    name_vertical(ui, s, n, clip, sx - 7.0, meter.bottom() - 1.0, Font::new(15), hov);

    // As on FL, the light takes clicks across the strip, from the meter's bottom to the pan knob.
    let (led0, led1) = (ly.y(METER_Y1) + 1.0, (ly.y(PAN_Y) - 14.0).max(ly.y(LED_Y) + 7.0));
    led(ui, track, n, Rect::new(left + 1.0, led0, STRIP_W - 2.0, led1 - led0), kx, ly.y(LED_Y));
    pan_knob(ui, s, n, kx, ly.y(PAN_Y), 10.5, 15.0);

    // Reverse polarity (⇕) and swap left / right (⇔) buttons, lit when on, then the separation knob (§4).
    // They keep their size and spacing below the fader's top at any mixer height (FL's short mixer stacks them the
    // same way), so they are placed from the fader's top, not scaled with it.
    let top = ly.y(TRACK_Y0);
    let (sep_y, swap_y, ring_y) = (top + SEP_Y - TRACK_Y0, top + SWAP_Y - TRACK_Y0, top + RING_Y - TRACK_Y0);
    polarity(ui, track, n, sx, sep_y);
    swap(ui, track, n, sx, swap_y);
    sep_knob(ui, s, n, sx, ring_y, 6.0, 9.5);

    // Fader: the handle at 0 dB at the measured height, scaled with the mixer.
    let track_y1 = bottom - 99.0;
    let ys = |v: f32| ly.y(HANDLE_0DB + (0.8 - v) * 440.0);
    fader(ui, s, n, f, mx, top, track_y1, &ys, true);

    // Input pin and latency clock: not available yet, greyed out. The disk-record button below them arms the track.
    io_icons(ui, n, (sx, bottom - 146.0), (sx, bottom - 117.0), Rect::new(sx - 9.0, bottom - 149.0, 18.0, 38.0), body);
    arm(ui, track, n, sx, bottom - 100.0);
    routing(ui, s, n, kx, bottom - 38.0, 1.0);
}

/// Where a layout other than Wide puts a strip's parts, measured from the owner's FL screenshots of each layout. A y
/// of 0 or more is down from the header's top, a negative one up from the window's bottom (`Geo::at`); x is from the
/// strip's left edge.
#[derive(Clone, Copy)]
pub struct Geo {
    /// Strip width.
    pub w: f32,
    /// Width before the master strip: the "C" column's part.
    region: f32,
    /// The "C" column: x and width in its part, and its bottom.
    col: (f32, f32, f32),
    /// "Current" and "Master" over the first two columns instead of "C" and "M".
    long: bool,
    head_h: f32,
    num_font: u16,
    /// The name: rotated in a box (x of its left edge, top, bottom; it reads upward from the bottom), or `None` for
    /// the header's second row (Extra large).
    name: Option<(f32, f32, f32)>,
    name_font: u16,
    /// Meter: x, width, top, bottom.
    meter: (f32, f32, f32, f32),
    /// Bottom of the dark head panel (none in Extra large).
    panel: Option<f32>,
    led: (f32, f32),
    /// Pan knob: x, y, radius, halo.
    pan: (f32, f32, f32, f32),
    /// Fader: x, top of its travel, bottom.
    fader: (f32, f32, f32),
    pol: (f32, f32),
    swap: (f32, f32),
    sep: (f32, f32, f32, f32),
    pin: (f32, f32),
    clock: (f32, f32),
    dot: (f32, f32),
    /// The send knob's size against Wide's.
    send: f32,
    /// Top of a darker band at the strip's foot (input and routing; Extra large).
    foot: Option<f32>,
}

impl Geo {
    /// A y of the layout in the window: from the header's top, or (negative) up from the bottom.
    fn at(top: f32, bottom: f32, v: f32) -> f32 {
        if v >= 0.0 { top + v } else { bottom + v }
    }
}

const fn geo_compact(w: f32, region: f32, col: (f32, f32, f32), meter: (f32, f32, f32, f32), pan_r: f32) -> Geo {
    let x = w / 2.0;
    Geo {
        w,
        region,
        col,
        long: false,
        head_h: 20.0,
        num_font: 14,
        name: Some((x - 6.0, 26.0, meter.2 - 3.0)),
        name_font: 12,
        meter,
        panel: Some(meter.3 + 46.0),
        led: (x, meter.3 + 11.0),
        pan: (x, meter.3 + 33.0, pan_r, pan_r + 2.0),
        fader: (x, meter.3 + 51.0, -180.0),
        pol: (x, -160.0),
        swap: (x, -145.0),
        sep: (x, -128.0, 6.0, 8.0),
        pin: (x, -108.0),
        clock: (x, -83.0),
        dot: (x, -65.0),
        send: 0.6,
        foot: None,
    }
}

/// The layouts in `mx::LAYOUTS` order; Wide (3) has its own measured strip (`strip`).
pub const GEOS: [Geo; 8] = [
    // Compact.
    geo_compact(18.0, 23.0, (3.0, 15.0, -115.0), (3.0, 12.0, 71.0, 123.0), 6.0),
    // Compact 2.
    geo_compact(20.0, 28.0, (4.0, 16.0, -116.0), (4.0, 12.0, 69.0, 126.0), 7.0),
    // Compact alt: the meter at the top, the name at the foot.
    Geo {
        w: 18.0,
        region: 27.0,
        col: (3.0, 15.0, -173.0),
        long: false,
        head_h: 20.0,
        num_font: 14,
        name: Some((3.0, -100.0, -52.0)),
        name_font: 12,
        meter: (3.0, 12.0, 24.0, 76.0),
        panel: Some(118.0),
        led: (9.0, 86.0),
        pan: (9.0, 108.0, 6.0, 8.0),
        fader: (9.0, 124.0, -236.0),
        pol: (9.0, -218.0),
        swap: (9.0, -201.0),
        sep: (9.0, -185.0, 6.0, 8.0),
        pin: (9.0, -165.0),
        clock: (9.0, -139.0),
        dot: (9.0, -121.0),
        send: 0.6,
        foot: None,
    },
    // Wide (drawn by `strip`; these are only its sizes).
    Geo {
        w: STRIP_W,
        region: 44.0,
        col: (9.0, 28.0, -156.0),
        long: false,
        head_h: 20.0,
        num_font: 17,
        name: None,
        name_font: 15,
        meter: (0.0, 0.0, 0.0, 0.0),
        panel: None,
        led: (0.0, 0.0),
        pan: (0.0, 0.0, 0.0, 0.0),
        fader: (0.0, 0.0, 0.0),
        pol: (0.0, 0.0),
        swap: (0.0, 0.0),
        sep: (0.0, 0.0, 0.0, 0.0),
        pin: (0.0, 0.0),
        clock: (0.0, 0.0),
        dot: (0.0, 0.0),
        send: 1.0,
        foot: None,
    },
    // Wide 2: a short meter beside the name, the pan knob under them, the stereo controls under the fader.
    Geo {
        w: 38.0,
        region: 40.0,
        col: (7.0, 26.0, -104.0),
        long: false,
        head_h: 20.0,
        num_font: 17,
        name: Some((3.0, 26.0, 109.0)),
        name_font: 14,
        meter: (22.0, 12.0, 28.0, 108.0),
        panel: Some(166.0),
        led: (19.0, 122.0),
        pan: (19.0, 150.0, 10.0, 13.0),
        fader: (19.0, 179.0, -166.0),
        pol: (28.0, -141.0),
        swap: (10.0, -141.0),
        sep: (19.0, -119.0, 8.0, 11.0),
        pin: (19.0, -93.0),
        clock: (28.0, -68.0),
        dot: (10.0, -68.0),
        send: 0.85,
        foot: None,
    },
    // Wide 3: Wide 2 a little wider, "Current" and "Master" over the first columns.
    Geo {
        w: 44.0,
        region: 50.0,
        col: (8.0, 32.0, -109.0),
        long: true,
        head_h: 20.0,
        num_font: 17,
        name: Some((5.0, 26.0, 101.0)),
        name_font: 15,
        meter: (25.0, 14.0, 26.0, 100.0),
        panel: Some(162.0),
        led: (22.0, 114.0),
        pan: (22.0, 143.0, 11.0, 15.0),
        fader: (22.0, 174.0, -176.0),
        pol: (32.0, -149.0),
        swap: (12.0, -149.0),
        sep: (22.0, -125.0, 9.0, 12.0),
        pin: (22.0, -100.0),
        clock: (32.0, -72.0),
        dot: (12.0, -72.0),
        send: 1.0,
        foot: None,
    },
    // Wide alt: the meter at the top, the stereo controls beside a long fader, the name low on the left.
    Geo {
        w: 40.0,
        region: 45.0,
        col: (8.0, 28.0, -130.0),
        long: false,
        head_h: 20.0,
        num_font: 17,
        name: Some((4.0, -200.0, -122.0)),
        name_font: 14,
        meter: (14.0, 12.0, 26.0, 115.0),
        panel: Some(168.0),
        led: (20.0, 127.0),
        pan: (20.0, 154.0, 10.0, 13.0),
        fader: (29.0, 180.0, -75.0),
        pol: (10.0, 180.0),
        swap: (10.0, 196.0),
        sep: (10.0, 214.0, 6.0, 8.0),
        pin: (10.0, -117.0),
        clock: (10.0, -92.0),
        dot: (10.0, -76.0),
        send: 0.9,
        foot: None,
    },
    // Extra large: number and name over the strip, the light and pan knob in a row, meter and fader side by side.
    Geo {
        w: 64.0,
        region: 68.0,
        col: (21.0, 26.0, -128.0),
        long: true,
        head_h: 44.0,
        num_font: 15,
        name: None,
        name_font: 15,
        meter: (4.0, 14.0, 85.0, -218.0),
        panel: None,
        led: (11.0, 59.0),
        pan: (42.0, 59.0, 14.5, 18.0),
        fader: (41.0, 92.0, -178.0),
        pol: (11.0, -155.0),
        swap: (11.0, -136.0),
        sep: (42.0, -146.0, 14.5, 18.0),
        pin: (32.0, -80.0),
        clock: (11.0, -202.0),
        dot: (11.0, -182.0),
        send: 1.0,
        foot: Some(-121.0),
    },
];

/// The current layout's sizes.
pub fn geo(s: &State) -> &'static Geo {
    &GEOS[usize::from(s.mixer.layout).min(GEOS.len() - 1)]
}

/// Strip width of the current layout.
pub fn strip_w(s: &State) -> f32 {
    geo(s).w
}

/// Fader change per pixel of drag in mixer window `r`: the handle follows the mouse in every layout.
pub fn fader_per_px(s: &State, r: Rect) -> f64 {
    if s.mixer.layout == mx::WIDE {
        return FADER_PER_PX / f64::from(scale(r));
    }
    let g = geo(s);
    let top = r.y + HEAD_Y0 - REF_TOP;
    let (y0, y1) = (Geo::at(top, r.bottom(), g.fader.1), Geo::at(top, r.bottom(), g.fader.2));
    1.0 / f64::from((y1 - y0 - 10.0).max(20.0))
}

/// A strip of the layouts other than Wide, laid out by `g` between the header's top `top` and the window's `bottom`.
#[allow(clippy::too_many_arguments)]
fn strip_geo(ui: &mut Ui, s: &State, n: u16, left: f32, top: f32, bottom: f32, f: &FrameInfo, g: &Geo) {
    let track = s.project.mixer.iter().find(|m| m.index == n);
    let at = |v: f32| Geo::at(top, bottom, v);
    let w = g.w;
    let hov1 = at(g.panel.unwrap_or(g.meter.3));
    let (body, hov) = strip_body(ui, s, n, left, w, (top, top + g.head_h, hov1), bottom, Font::new(g.num_font));
    if let Some(pb) = g.panel {
        head_panel(ui, Rect::new(left + 1.0, top + g.head_h + 1.0, w - 2.0, at(pb) - top - g.head_h - 1.0), track.and_then(|m| m.color).map(super::rgb));
    }
    if let Some(y) = g.foot {
        let band = Rect::new(left + 3.0, at(y), w - 6.0, (bottom - 6.0 - at(y)).max(1.0));
        let c = if super::lemon(ui) { Color::rgb(0x0c0c0c).with_alpha(150) } else { ui.t.s(0x1f2123) };
        ui.c.fill_rect(band, c);
    }
    let (m0, m1) = (at(g.meter.2), at(g.meter.3));
    let meter = Rect::new(left + g.meter.0, m0, g.meter.1, (m1 - m0).max(4.0));
    let ticks: Vec<f32> = (1..7).map(|k| meter.y + meter.h * k as f32 / 7.0).collect();
    strip_meter(ui, s, n, meter, &ticks);
    match g.name {
        Some((x, y0, y1)) => {
            let clip = Rect::new(left + x - 2.0, at(y0), 20.0, at(y1) - at(y0));
            name_vertical(ui, s, n, clip, left + x, at(y1) - 1.0, Font::new(g.name_font), hov);
        }
        None => {
            // Extra large: the name on the header's second row.
            let (name, c) = name_and_colour(ui, s, n, hov);
            let c = track.and_then(|m| m.color).filter(|_| n != 0).map_or(c, |o| ink_on(super::rgb(o)));
            let r = Rect::new(left + 4.0, top + g.head_h / 2.0, w - 6.0, g.head_h / 2.0);
            ui.c.text_clip(Font::new(14), r.x, r.y + 1.0, r.w, c, &name);
        }
    }
    let (lx, ly_) = (left + g.led.0, at(g.led.1));
    led(ui, track, n, Rect::new(lx - 8.0, ly_ - 8.0, 16.0, 16.0), lx, ly_);
    pan_knob(ui, s, n, left + g.pan.0, at(g.pan.1), g.pan.2, g.pan.3);
    polarity(ui, track, n, left + g.pol.0, at(g.pol.1));
    swap(ui, track, n, left + g.swap.0, at(g.swap.1));
    sep_knob(ui, s, n, left + g.sep.0, at(g.sep.1), g.sep.2, g.sep.3);
    let (f0, f1) = (at(g.fader.1), at(g.fader.2));
    let travel = (f1 - f0 - 10.0).max(1.0);
    let ys = |v: f32| f0 + 5.0 + (1.0 - v) * travel;
    fader(ui, s, n, f, left + g.fader.0, f0, f1, &ys, g.w >= 38.0);
    let (pin, clock) = ((left + g.pin.0, at(g.pin.1) - 5.0), (left + g.clock.0, at(g.clock.1)));
    let veil = Rect::new(pin.0 - 9.0, pin.1 - 3.0, 18.0, 21.0);
    io_icons(ui, n, pin, clock, veil, body);
    if !super::lemon(ui) {
        ui.veil(Rect::new(clock.0 - 9.0, clock.1 - 9.0, 18.0, 18.0), body);
    }
    arm(ui, track, n, left + g.dot.0, at(g.dot.1));
    routing(ui, s, n, left + w / 2.0, bottom - 38.0, g.send);
}

/// The strip's body (a card in the Mango skin), its selecting hit and the header number; the body colour and whether
/// the mouse is over the strip's top.
#[allow(clippy::too_many_arguments)]
fn strip_body(ui: &mut Ui, s: &State, n: u16, left: f32, w: f32, (head0, head1, hov1): (f32, f32, f32), bottom: f32, num: Font) -> (Color, bool) {
    let t = ui.t;
    let track = s.project.mixer.iter().find(|m| m.index == n);
    let selected = s.selected_mixer == n;
    // The current strip's body is a shade lighter (measured: 0x26292b vs 0x232527); an armed one is tinted (0x312236 vs
    // 0x2d1f31; spec: audio/track-io.md §1.5).
    let armed = track.is_some_and(|m| m.armed);
    let body = match (armed, selected) {
        (true, true) => ui.t.s(0x312236),
        (true, false) => ui.t.s(0x2d1f31),
        (false, true) => ui.t.s(0x26292b),
        (false, false) => ui.t.s(0x232527),
    };
    let lemon = super::lemon(ui);
    // A coloured track tints its whole strip, as FL does.
    let own = track.and_then(|m| m.color).map(super::rgb);
    let tint = |c: Color, k: f32| own.map_or(c, |o| c.mix(o, k));
    let body = tint(body, 0.3);
    if lemon {
        // Lemon (spec: ui/lemon-design.md §8): each strip is a rounded card on the dark gap colour. Mango, black: the gap
        // black, the card lit from the top down and rimmed, the selected one lighter with a mango rim, so every strip
        // stands on its own.
        let card = Rect::new(left, head1 + 1.0, w, bottom - head1 - 1.0);
        ui.c.fill_rect(card, Color::rgb(0x000000));
        let face = Rect::new(card.x + 2.0, card.y + 1.0, card.w - 4.0, card.h - 4.0);
        let (top, low, rim) = if selected {
            (Color::rgb(0x2a2a2a), Color::rgb(0x1a1a1a), t.accent.mix(super::MANGO_MIXER_RIM, 0.55))
        } else {
            (Color::rgb(0x202020), Color::rgb(0x121212), super::MANGO_MIXER_RIM)
        };
        let (top, low) = (tint(top, 0.3), tint(low, 0.22));
        let rad = lk::r_panel(t).min(face.w / 2.0);
        ui.c.fill_rrect(face.outset(1.0), rad + 1.0, rim);
        ui.c.fill_rect_gradient(face, top, low, rad);
        // The shine: a light line along the card's top edge.
        ui.c.fill_rrect(Rect::new(face.x + 4.0, face.y + 1.0, (face.w - 8.0).max(1.0), 1.0), 0.5, top.mix(Color::rgb(0xffffff), 0.12));
    } else {
        ui.c.fill_rect(Rect::new(left, head1 + 1.0, w, bottom - head1 - 1.0), body);
        ui.c.fill_rect(Rect::new(left + w - 1.0, head1 + 1.0, 1.0, bottom - head1 - 1.0), ui.t.s(0x1c1f21));
    }
    // The whole strip selects (Ctrl adds, Shift renames); controls register on top of it (spec: ui/mixer.md §2).
    ui.hit(Rect::new(left, head0, w, bottom - head0), Action::Mix(MHit::Strip(n)), mx::name(s, n));
    let hov = ui.hovering(Rect::new(left, head0, w, hov1 - head0));

    // Header number (insert strips only; the master's "M" sits in the fixed header band).
    if n != 0 {
        let hr = Rect::new(left, head0, w, head1 - head0 + 1.0);
        let row = Rect::new(left, head0, w, 21.0);
        ui.c.fill_rect(hr, ui.t.s(0x2b2e30));
        let pill = Rect::new(hr.x + 3.0, hr.y + 2.0, hr.w - 6.0, row.h - 4.0);
        if let Some(col) = own {
            // The header in the track's colour, as FL; the selected one underlined.
            ui.c.fill_rect(hr, col);
            if selected {
                ui.c.fill_rect(Rect::new(hr.x, hr.bottom() - 2.0, hr.w, 2.0), t.accent);
            }
        } else if head1 - head0 > 30.0 {
        } else if let (true, Some(col)) = (lemon, own) {
            // Mango: a coloured track's number sits on a pill of its colour (lit for the black), outlined when selected.
            ui.c.fill_rrect(pill, lk::r_btn(t), readable_on_black(col));
            if selected {
                ui.c.stroke_rrect(pill.outset(1.0), lk::r_btn(t) + 1.0, 2.0, t.accent);
            }
            ui.c.text_center(num, row, Color::rgb(0x141414), &n.to_string());
        } else if selected && lemon {
            lk::lit_in(ui.c, ui.t, pill, lk::r_btn(t), false);
        } else if selected {
            // Hatched header for the selected strip.
            ui.c.push_clip(hr);
            for k in 0..14 {
                let x = hr.x - 20.0 + 5.0 * k as f32;
                ui.c.line(x, hr.bottom(), x + 21.0, hr.y, 1.0, ui.t.s(0x34383a));
            }
            ui.c.pop_clip();
        }
        if head1 - head0 > 30.0 {
            // Extra large: the number at the left of its row, over the name (`strip_geo`), as FL has them.
            let c = if let Some(col) = own { ink_on(col) } else if selected { t.accent } else if hov { ui.t.tx(0xd0d6da) } else { ui.t.tx(0x9aa1a5) };
            ui.c.text(num, left + 5.0, head0 + 1.0, c, &n.to_string());
        } else if let Some(col) = own {
            ui.c.text_center(num, row, ink_on(col), &n.to_string());
        } else {
            let c = if selected && lemon {
                lk::ink_on_lit(ui.t)
            } else if selected || hov {
                ui.t.tx(0x9aa1a5)
            } else if lemon {
                ui.t.tx(0x8c8c8c)
            } else {
                ui.t.s(0x6d7275)
            };
            ui.c.text_center(num, row, c, &n.to_string());
        }
    }
    (body, hov)
}

/// The dark head panel behind the meter, name, light and pan knob, with a rounded bottom. Mango's black skin makes it a
/// soft charcoal, not black: the name and the light on it were hard to see on the near-black the skin maps this grey to.
fn head_panel(ui: &mut Ui, r: Rect, own: Option<Color>) {
    let panel = if super::lemon(ui) { Color::rgb(0x2b2e31) } else { ui.t.s(0x1d1f21) };
    let panel = own.map_or(panel, |o| panel.mix(o, 0.25));
    let rad = 9.0_f32.min(r.w / 2.0);
    ui.c.fill_rrect(r, rad, panel);
    ui.c.fill_rect(Rect::new(r.x, r.y, r.w, 12.0_f32.min(r.h)), panel);
}

/// Meter slot with its tick marks and the track's peak meter; the selected strip's gets a frame.
fn strip_meter(ui: &mut Ui, s: &State, n: u16, meter: Rect, ticks: &[f32]) {
    let t = ui.t;
    let lemon = super::lemon(ui);
    let iw = meter.w - 4.0;
    if lemon {
        ui.c.fill_rrect(meter, lk::r_small(t).min(meter.w / 2.0), ui.t.s(0x191d1e));
        ui.c.fill_rrect(Rect::new(meter.x + 2.0, meter.y + 2.0, iw, meter.h - 4.0), 2.5_f32.min(iw / 2.0), ui.t.s(0x232729));
    } else {
        ui.c.fill_rrect(meter, 2.0, ui.t.s(0x191d1e));
        ui.c.fill_rect(Rect::new(meter.x + 2.0, meter.y + 2.0, iw - 1.0, meter.h - 4.0), ui.t.s(0x232729));
    }
    for &y in ticks {
        ui.c.fill_rect(Rect::new(meter.x + 3.0, y, meter.w - 7.0, 1.0), ui.t.s(0x111315));
    }
    // Per-track peak meter (spec: ui/mixer.md §9): two bars on the same scale as the "C" column.
    ui.live(Live::Meter, meter);
    if let Some(m) = s.mixer.meters.get(usize::from(n)) {
        let cfg = &s.mixer.meter_cfg;
        let (top, bot) = (meter.y + 2.0, meter.bottom() - 2.0);
        let floor = cfg.v[meters::k::FLOOR_DB];
        let alt = crate::prefs::check(&crate::prefs::ALTERNATE_METER_SCALE);
        let ys = |db: f32| bot - meters::frac(db, floor, alt) * (bot - top);
        let bw = iw / 2.0;
        for i in 0..2 {
            meter_bar(ui, meter.x + 2.0 + bw * i as f32, bw, bot, m.db[i], m.hold[i], cfg, &ys);
        }
    }
    if s.selected_mixer == n {
        ui.c.stroke_rrect(Rect::new(meter.x - 1.0, meter.y - 2.0, meter.w + 2.0, meter.h + 4.0), if lemon { lk::r_btn(t) } else { 1.0 }, 2.0, t.accent);
    }
}

/// A track's name and its colour on the strip.
fn name_and_colour(ui: &Ui, s: &State, n: u16, hov: bool) -> (String, Color) {
    let _ = hov;
    let t = ui.t;
    let track = s.project.mixer.iter().find(|m| m.index == n);
    let selected = s.selected_mixer == n;
    let also = !selected && mx::is_selected(s, n);
    let name = track.and_then(|m| m.name.clone()).unwrap_or_else(|| if n == 0 { "Master".into() } else { format!("Insert {n}") });
    // Mango: names near white on the black strips, the selected track's in mango; a track's colour shows on its number's
    // pill at the top instead.
    let c = if super::lemon(ui) {
        if selected { t.accent } else { ui.t.tx(0xe2e2e2) }
    } else {
        track
            .and_then(|m| m.color)
            .map(super::rgb)
            .map_or(if selected { ui.t.tx(0xaab2b7) } else if also { ui.t.tx(0xe4e8ea) } else { ui.t.s(0x7f868a) }, |c| {
                if also { c.mix(ui.t.tx(0xffffff), 0.5) } else { c.mix(ui.t.s(0x7f868a), 0.4) }
            })
    };
    (name, c)
}

/// The name rotated, reading upward from `y_bottom`, inside `clip`.
#[allow(clippy::too_many_arguments)]
fn name_vertical(ui: &mut Ui, s: &State, n: u16, clip: Rect, x: f32, y_bottom: f32, font: Font, hov: bool) {
    let (name, c) = name_and_colour(ui, s, n, hov);
    ui.c.push_clip(clip);
    ui.c.text_vertical(font, x, y_bottom, c, &name);
    ui.c.pop_clip();
}

/// The mute light at (x, y), taking clicks over `hit`.
fn led(ui: &mut Ui, track: Option<&os_model::MixerTrack>, n: u16, hit: Rect, x: f32, y: f32) {
    let enabled = track.is_none_or(|m| m.enabled);
    ui.hit(hit, Action::Mix(MHit::Led(n)), "Mute / solo");
    led_small(ui, x, y, enabled);
}

fn pan_knob(ui: &mut Ui, s: &State, n: u16, x: f32, y: f32, r: f32, halo: f32) {
    let pan = KnobId::MixPan(n);
    let hit = if super::lemon(ui) { Rect::new(x - halo, y - 8.0, 2.0 * halo, 16.0) } else { Rect::new(x - (halo - 1.0), y - (halo - 1.0), 2.0 * (halo - 1.0), 2.0 * (halo - 1.0)) };
    ui.hit(hit, Action::Knob(pan), s.knob_text(pan));
    if super::lemon(ui) {
        centre_bar(ui, x, y, halo, s.knob(pan));
    } else {
        mixer_knob(ui, x, y, r, halo, s.knob(pan), Some(true));
    }
}

/// The Mango skin's pan and separation: a horizontal bar like its faders, a rounded trough `2 * half` wide with the
/// value growing left or right from the centre (0.5) in mango.
fn centre_bar(ui: &mut Ui, x: f32, y: f32, half: f32, value: f32) {
    let h = if half >= 12.0 { 10.0 } else { 8.0 };
    let r = Rect::new(x - half, y - h / 2.0, 2.0 * half, h);
    let hov = ui.hovering(r.outset(4.0));
    ui.c.fill_rrect(r, h / 2.0, ui.t.s(0x141618));
    let v = value.clamp(0.0, 1.0);
    let (a, b) = (x.min(r.x + v * r.w), x.max(r.x + v * r.w));
    let main = ui.t.brand_sel(os_theme::design::lemon::MAIN);
    let col = if hov { main.shade(0.12) } else { main };
    if b - a >= 1.0 {
        ui.c.fill_rrect(Rect::new(a, r.y + 1.0, b - a, h - 2.0), (h - 2.0) / 2.0, col);
    }
    // The centre mark, over the fill.
    ui.c.fill_rect(Rect::new(x - 0.5, r.y - 1.0, 1.0, h + 2.0), ui.t.tx(0x8c8c8c));
}

/// Reverse polarity (⇕), lit when on.
fn polarity(ui: &mut Ui, track: Option<&os_model::MixerTrack>, n: u16, sx: f32, y: f32) {
    let pol = track.is_some_and(|m| m.polarity);
    let pr = Rect::new(sx - 8.0, y - 8.0, 16.0, 16.0);
    let hp = ui.hit(pr, Action::Mix(MHit::Polarity(n)), "Reverse polarity");
    let mut g = Glyph::new();
    g.polygon(&[(sx - 5.0, y - 1.5), (sx, y - 6.0), (sx + 5.0, y - 1.5)]);
    g.polygon(&[(sx - 5.0, y + 1.5), (sx, y + 6.0), (sx + 5.0, y + 1.5)]);
    ui.c.glyph(if pol { ui.t.h(0xf3b234) } else if hp { ui.t.tx(0xffffff) } else { ui.t.tx(0xb7c3ca) }, &g);
    // Under the pointer, a rounded border, in the lit colour when on (FL's look).
    if hp {
        ui.c.stroke_rrect(pr.inset(0.5), 3.0, 1.0, if pol { ui.t.h(0xf3b234) } else { ui.t.tx(0x8d989e) });
    }
}

/// Swap left / right (⇔), lit when on.
fn swap(ui: &mut Ui, track: Option<&os_model::MixerTrack>, n: u16, sx: f32, y: f32) {
    let swp = track.is_some_and(|m| m.swap);
    let wr = Rect::new(sx - 8.0, y - 8.0, 16.0, 16.0);
    let hw = ui.hit(wr, Action::Mix(MHit::Swap(n)), "Swap left & right channels");
    let mut g = Glyph::new();
    g.polygon(&[(sx - 1.0, y - 6.0), (sx - 7.0, y), (sx - 1.0, y + 6.0)]);
    g.polygon(&[(sx + 1.5, y - 6.0), (sx + 7.5, y), (sx + 1.5, y + 6.0)]);
    ui.c.glyph(if swp { Color::rgb(0x5fb6f0) } else if hw { ui.t.tx(0xffffff) } else { ui.t.tx(0xb7c3ca) }, &g);
    ui.c.fill_rect(Rect::new(sx - 0.5, y - 6.0, 1.0, 12.0), ui.t.s(0x5a6266));
    if hw {
        ui.c.stroke_rrect(wr.inset(0.5), 3.0, 1.0, if swp { Color::rgb(0x5fb6f0) } else { ui.t.tx(0x8d989e) });
    }
}

fn sep_knob(ui: &mut Ui, s: &State, n: u16, x: f32, y: f32, r: f32, halo: f32) {
    let sep = KnobId::MixSep(n);
    let hit = if super::lemon(ui) { Rect::new(x - halo, y - 8.0, 2.0 * halo, 16.0) } else { Rect::new(x - halo + 0.5, y - halo + 0.5, 2.0 * halo - 1.0, 2.0 * halo - 1.0) };
    ui.hit(hit, Action::Knob(sep), s.knob_text(sep));
    if super::lemon(ui) {
        centre_bar(ui, x, y, halo, s.knob(sep));
    } else {
        mixer_knob(ui, x, y, r, halo, s.knob(sep), Some(true));
    }
}

/// Fader: track, gain marks, handle (green on the selected strip, with the track lit below it). In the Mango skin the
/// track is a rounded trough and the level below the handle a rounded bar, as the logo's equalizer bars: bright on the
/// selected strip, softer on the others. `ys` maps a value to the handle's y; `marks` draws the gain marks.
#[allow(clippy::too_many_arguments)]
fn fader(ui: &mut Ui, s: &State, n: u16, f: &FrameInfo, mx: f32, y0: f32, track_y1: f32, ys: &dyn Fn(f32) -> f32, marks: bool) {
    let t = ui.t;
    let lemon = super::lemon(ui);
    let track = s.project.mixer.iter().find(|m| m.index == n);
    let selected = s.selected_mixer == n;
    let trough = Rect::new(mx - 7.0, y0, 14.0, track_y1 - y0);
    if lemon {
        ui.c.fill_rrect(trough, 7.0, ui.t.s(0x141618));
    } else {
        ui.c.fill_rect(Rect::new(mx - 1.5, trough.y, 3.0, trough.h), ui.t.s(0x141618));
    }
    if marks {
        fader_marks(ui, mx, track_y1, ys, lemon);
    }
    let v = track.and_then(|m| m.volume).unwrap_or(0.8) as f32;
    // max/min, not clamp: a very short window gives track_y1 < y0 (clamp would panic).
    let hy = ys(v).min(track_y1 - 5.0).max(y0 + 5.0);
    let hov = ui.hit(Rect::new(mx - 9.0, y0 - 5.0, 18.0, track_y1 - y0 + 10.0), Action::Fader(n), mx::level_text("Volume", f64::from(v)));
    if lemon {
        // The bar is the fader: its rounded top, as the logo's bars end, is where it is held; no separate cap.
        let r = 6.0;
        let bar = Rect::new(mx - r, hy - r, 2.0 * r, (track_y1 - 1.0 - (hy - r)).max(2.0 * r));
        let (top, bottom_c) = (t.brand_sel(os_theme::design::lemon::HIGHLIGHT), t.brand_sel(os_theme::design::lemon::MAIN));
        // Full mango on the selected strip and while held; softer on the others, a little brighter under the pointer.
        let held = f.fader_drag == Some(n);
        let (top, bottom_c) = if selected || held { (top, bottom_c) } else { (top.mix(ui.t.s(0x141618), 0.38), bottom_c.mix(ui.t.s(0x141618), 0.32)) };
        let lift = if held { 0.07 } else if hov { 0.12 } else { 0.0 };
        let (top, bottom_c) = (top.shade(lift), bottom_c.shade(lift));
        ui.c.fill_rect_gradient(bar, top, bottom_c, r);
    } else {
        if selected {
            ui.c.fill_rect(Rect::new(mx - 1.0, hy, 2.0, track_y1 - hy), ui.t.a(0x8ec63a));
        }
        fader_handle(ui, mx, hy, selected, hov);
    }
}

/// The input pin (its top at `pin`) and the latency clock (centred on `clock`): not available yet, greyed out under
/// `veil`.
fn io_icons(ui: &mut Ui, n: u16, pin: (f32, f32), clock: (f32, f32), veil: Rect, body: Color) {
    let master = n == 0;
    let (sx, pt) = pin;
    // Mango's strips are shaded cards: a veil in one colour would show as a square on them, so the icons are faded
    // into the card instead.
    let lemon = super::lemon(ui);
    let fade = |c: Color| if lemon { c.mix(Color::rgb(0x161616), f32::from(crate::app::OFF_VEIL) / 255.0) } else { c };
    let pin_c = fade(if master { ui.t.h(0xf09a28) } else { ui.t.s(0x4d5357) });
    let mut g = Glyph::new();
    g.rrect(Rect::new(sx - 4.0, pt, 8.0, 9.0), 3.0);
    g.polygon(&[(sx - 2.5, pt + 8.0), (sx + 2.5, pt + 8.0), (sx, pt + 13.0)]);
    ui.c.glyph(pin_c, &g);
    ui.c.fill_rect(Rect::new(sx - 1.0, pt + 1.0, 2.0, 9.0), fade(if master { ui.t.h(0x7a3c08) } else { ui.t.s(0x23272a) }));
    ui.off(Rect::new(sx - 8.0, pt - 2.0, 16.0, 18.0), "Track audio input");
    let (cx, cy) = clock;
    let clk = fade(if master { ui.t.h(0xf09a28) } else { ui.t.tx(0xc4d4e0) });
    let mut g = Glyph::new();
    g.ring(cx, cy, 6.0, 1.6).rect(cx - 0.7, cy - 4.0, 1.4, 4.5).rect(cx - 0.7, cy - 0.7, 3.5, 1.4);
    ui.c.glyph(clk, &g);
    ui.off(Rect::new(cx - 8.0, cy - 8.0, 16.0, 16.0), "Track latency");
    if !lemon {
        ui.veil(veil, body);
    }
}

/// The disk-record button at (x, y): arms the track.
fn arm(ui: &mut Ui, track: Option<&os_model::MixerTrack>, n: u16, x: f32, y: f32) {
    let armed = track.is_some_and(|m| m.armed);
    let hint = if armed { "Armed: monitoring the input (recording into tracks is not available yet)" } else { "Arm disk recording" };
    let hov = ui.hit(Rect::new(x - 7.0, y - 7.0, 14.0, 14.0), Action::Mix(MHit::Arm(n)), hint);
    if armed {
        // An orange-red ring about 8 px across with a 3 px hole.
        ui.c.stroke_circle(x, y, 2.75, 2.5, ui.t.h(0xff633f));
    } else {
        ui.c.fill_circle(x, y, 2.6, if hov { ui.t.tx(0xffffff) } else { ui.t.tx(0xd8e0e6) });
    }
}

/// Routing (spec: ui/mixer.md §5) centred on `kx`, the arrows on `ay`: the current track's green ⬇ (its cables are
/// drawn over all strips: `cables`), a send knob (`k` its size against Wide's) where the current track sends, else the ▲
/// "route to this track" button; an orange "^" marks tracks that receive a route.
fn routing(ui: &mut Ui, s: &State, n: u16, kx: f32, ay: f32, k: f32) {
    let t = ui.t;
    let master = n == 0;
    let by = ay + 38.0;
    let cur = s.selected_mixer;
    let cur_name = mx::name(s, cur);
    if s.selected_mixer == n {
        down_arrow(ui, kx, ay);
    } else if let Some(level) = mx::send(s, cur, n) {
        // Send knob (fader law) with its arc, the green "^" that removes the route; its cable is drawn with the others.
        let ky = by - 40.0;
        let id = KnobId::Send(cur, n);
        let h = 13.0 * k;
        ui.hit(Rect::new(kx - h, ky - h, 2.0 * h, 2.0 * h), Action::Knob(id), s.knob_text(id));
        send_knob(ui, s, n, kx, ky, level, k);
        let ur = Rect::new(kx - 6.0, by - 61.0, 12.0, 8.0);
        let hov = ui.hit(ur, Action::Mix(MHit::Unroute(n)), format!("Remove the send from {cur_name} to {}", mx::name(s, n)));
        ui.c.fill_polygon(&[(kx - 4.0, by - 55.0), (kx, by - 59.0), (kx + 4.0, by - 55.0)], if hov { ui.t.tx(0xffffff) } else { t.accent });
    } else if !(master && cur == 0) {
        let hint = format!("Enable send from {cur_name} to {}  Ctrl/Shift", mx::name(s, n));
        let hov = ui.hit(Rect::new(kx - 8.0, ay - 8.0, 16.0, 14.0), Action::Mix(MHit::Route(n)), hint);
        let mut g = Glyph::new();
        g.polygon(&[(kx - 6.5, ay + 3.0), (kx, ay - 3.5), (kx + 6.5, ay + 3.0)]);
        ui.c.glyph(if hov { t.accent } else { ui.t.tx(0xd8e0e6) }, &g);
        if !master && mx::receives(s, n) {
            ui.c.fill_polygon(&[(kx - 3.5, ay - 9.0), (kx, ay - 12.5), (kx + 3.5, ay - 9.0)], ui.t.h(0xf09a28));
        }
    }
}

/// The current track's green ⬇ (centred on `ay`): where its cables leave.
fn down_arrow(ui: &mut Ui, kx: f32, ay: f32) {
    let a = [(kx - 3.0, ay - 6.0), (kx + 3.0, ay - 6.0), (kx + 3.0, ay - 1.0), (kx + 7.0, ay - 1.0), (kx, ay + 6.0), (kx - 7.0, ay - 1.0), (kx - 3.0, ay - 1.0)];
    ui.c.fill_polygon(&a, ui.t.accent);
}

/// The send knob from the current track into track `n`, with its level arc (none at zero).
fn send_knob(ui: &mut Ui, s: &State, n: u16, kx: f32, ky: f32, level: f64, k: f32) {
    let id = KnobId::Send(s.selected_mixer, n);
    mixer_knob(ui, kx, ky, 10.0 * k, 13.0 * k, s.knob(id), None);
    if level > 0.0 {
        // From the knob's zero to its pointer (the arc takes a sweep, not an end angle).
        let sweep = (level as f32).min(1.0) * 1.5 * PI;
        ui.c.arc(kx, ky, 12.5 * k, 0.75 * PI, sweep, 2.0, ui.t.accent);
    }
}

/// The dark round mixer knob: a shaded ball inside a darker halo.
/// `arc`: Some(true) draws the red value arc from the top (pan style), Some(false) from the start.
fn mixer_knob(ui: &mut Ui, cx: f32, cy: f32, r: f32, halo: f32, value: f32, arc: Option<bool>) {
    let hov = ui.hovering(Rect::new(cx - halo, cy - halo, 2.0 * halo, 2.0 * halo));
    let look = os_ui::kit::KnobLook::Mixer { halo, arc: arc.is_some() };
    os_ui::kit::knob(ui.c, ui.t, cx, cy, r, value, arc.unwrap_or(false), look, os_ui::kit::State::hover(hov));
}

/// Text that reads on a track's colour: near black on light colours, white on dark ones.
fn ink_on(c: Color) -> Color {
    let luma = 0.299 * f32::from(c.r) + 0.587 * f32::from(c.g) + 0.114 * f32::from(c.b);
    if luma > 150.0 { Color::rgb(0x141414) } else { Color::rgb(0xf4f4f4) }
}

/// A track colour on the Mango skin's black strips (its number's pill): its hue kept, lit to at least 68 % lightness and given some
/// saturation back, so a dark red reads as a clear red; greys stay grey and light colours as they are.
fn readable_on_black(c: Color) -> Color {
    let (h, s, l) = c.hsl();
    let s = if s < 0.08 { s } else { s.max(0.6) };
    Color::from_hsl_pub(h, s, l.clamp(0.68, 0.85), 255)
}

/// Gain marks beside a fader track (spec: ui/mixer.md §5, measured from FL): 6 px ticks on both sides at 0, −6, −12 and
/// −18 dB (2 px clear of the track), and faint 4 px rails between consecutive ticks (1 px clear of each tick). They
/// belong to the track: the handle covers them as it moves. In the Mango skin (`lemon`) only short ticks, clear of its
/// wider trough.
fn fader_marks(ui: &mut Ui, mx: f32, track_y1: f32, ys: &dyn Fn(f32) -> f32, lemon: bool) {
    let ys: Vec<f32> = [0.0, -6.0, -12.0, -18.0]
        .iter()
        .map(|&db| ys(fader_value(db)).floor())
        .filter(|&y| y < track_y1 - 1.0)
        .collect();
    let (tick, rail) = (ui.t.s(0x181a1c), ui.t.s(0x202324));
    if lemon {
        // Short ticks between the trough (7 px either side) and the separation knob on the left, the strip's edge on
        // the right.
        for y in ys {
            ui.c.fill_rect(Rect::new(mx - 10.0, y, 2.5, 1.0), tick);
            ui.c.fill_rect(Rect::new(mx + 7.5, y, 3.0, 1.0), tick);
        }
        return;
    }
    for w in ys.windows(2) {
        let (a, b) = (w[0] + 2.0, w[1] - 1.0);
        if b > a {
            ui.c.fill_rect(Rect::new(mx - 7.5, a, 4.0, b - a), rail);
            ui.c.fill_rect(Rect::new(mx + 3.5, a, 4.0, b - a), rail);
        }
    }
    for y in ys {
        ui.c.fill_rect(Rect::new(mx - 9.5, y, 6.0, 1.0), tick);
        ui.c.fill_rect(Rect::new(mx + 3.5, y, 6.0, 1.0), tick);
    }
}

/// Fader value (0..1, 0.8 = 0 dB) of a gain in dB: the inverse of `laws::fader_gain`.
fn fader_value(db: f32) -> f32 {
    0.8 * (10.0 * 10f32.powf(db / 20.0) + 1.0).ln() / 11f32.ln()
}

/// Fader handle centred on (cx, cy): 14 × 23.
fn fader_handle(ui: &mut Ui, cx: f32, cy: f32, selected: bool, hov: bool) {
    os_ui::kit::fader_handle(ui.c, ui.t, cx, cy, selected, os_ui::kit::State::hover(hov));
}

fn slot_panel(ui: &mut Ui, s: &State, r: Rect) {
    let t = ui.t;
    ui.c.fill_rect(r, ui.t.s(0x2e3133));
    ui.c.fill_rect(Rect::new(r.x - 1.0, r.y, 1.0, r.h), ui.t.s(0x1b1e20));
    ui.hit(r, Action::Swallow, "");
    let n = s.selected_mixer;
    let track = s.project.mixer.iter().find(|m| m.index == n);
    let title = match track.and_then(|m| m.name.clone()) {
        Some(nm) => format!("Mixer - {nm}"),
        None if n == 0 => "Mixer - Master".into(),
        None => format!("Mixer - Insert {n}"),
    };
    ui.c.fill_rect(Rect::new(r.x, r.y, r.w, 23.0), t.window_title);
    ui.c.text_clip(Font::new(17), r.x + 1.0, r.y + 2.0, r.w - 70.0, ui.t.tx(0xd4dade), &title);
    ui.hit(Rect::new(r.x, r.y, r.w, 23.0), Action::WinDrag(Win::Mixer), "Drag to move");
    window_buttons(ui, Win::Mixer, r.right() - 58.0, r.y, ui.t.tx(0xd4dade), ui.t.tx(0xffffff));
    if r.h < 2.0 * TITLE_H {
        return;
    }
    // Squish: the slot rows and the equalizer graph shrink with the height (the knob rows under
    // the graph are dropped when they no longer fit); the top and bottom rows keep their size.
    let room = r.h - 125.0;
    let kp = ((room - 15.0) / 503.0).clamp(0.3, 1.0);
    let knobs = room >= 579.0;
    let q = kp.max(0.55);
    // Input row: the record-source icon with its label, the input combo, the input latency clock (not available yet:
    // greyed out). Spec: audio/track-io.md §1.1, §2.3.
    let input_bg = ui.t.s(0x2b2e30);
    ui.c.fill_rect(Rect::new(r.x, r.y + 23.0, r.w, 36.0), input_bg);
    let io = track.map_or((-1, os_model::REC_POST_EQ), |m| (m.audio_in, m.rec_source));
    let icon = Rect::new(r.x + 3.0, r.y + 28.0, 21.0, 23.0);
    let hov = ui.hit(icon, Action::Mix(MHit::RecSource), "Select what is included when recording audio");
    // Grey while no input is chosen, orange once one is.
    let ink = if hov { ui.t.tx(0xffffff) } else if io.0 >= 0 { ui.t.h(0xf09a28) } else { ui.t.s(0x5a6266) };
    let mut g = Glyph::new();
    g.polygon(&[(r.x + 7.0, r.y + 32.0), (r.x + 13.0, r.y + 32.0), (r.x + 18.0, r.y + 37.5), (r.x + 13.0, r.y + 43.0), (r.x + 7.0, r.y + 43.0)]);
    ui.c.glyph(ink, &g);
    let label = mx::REC_SOURCES.get(usize::from(io.1)).map_or("", |s| s.1);
    let lf = Font::new(9).bold();
    let lw = ui.c.text_width(lf, label);
    ui.c.text(lf, r.x + 12.5 - lw / 2.0, r.y + 43.0, ink, label);
    let (text, any) = mx::input_label(s, n);
    let combo = Rect::new(r.x + 27.0, r.y + 28.0, 159.0, 23.0);
    if any {
        let hov = ui.hit(combo, Action::Mix(MHit::Input), "Audio input source");
        os_ui::kit::combo(ui.c, ui.t, combo, &text, Font::new(17), os_ui::kit::State::hover(hov));
    } else {
        off_combo(ui, combo, "Audio input source", &text, input_bg);
    }
    let mut g = Glyph::new();
    g.ring(r.x + 202.0, r.y + 39.5, 8.5, 1.8).rect(r.x + 201.2, r.y + 34.0, 1.6, 6.5).rect(r.x + 201.2, r.y + 38.9, 5.0, 1.6);
    ui.c.glyph(ui.t.tx(0xd4dade), &g);
    let clock = Rect::new(r.x + 192.0, r.y + 29.0, 21.0, 21.0);
    ui.off(clock, "Input latency");
    ui.veil(clock, input_bg);

    // Ten effect slots.
    for i in 0..10 {
        let row = Rect::new(r.x + 1.0, r.y + 61.0 + 33.0 * kp * i as f32, r.w - 2.0, 28.0 * kp);
        let k = i as u8;
        let plugin = mx::slot_name(s, n, k);
        let fs = (18.0 * kp.max(0.62)).round() as u16;
        let hint = format!("FX slot {} options", i + 1);
        let ha = ui.hit(Rect::new(row.x, row.y, 19.0, row.h), Action::Mix(MHit::SlotArrow(k)), hint.clone());
        let hn = ui.hit(Rect::new(row.x + 19.0, row.y, row.w - 64.0, row.h), Action::Mix(MHit::SlotName(k)), hint);
        let hov = ha || hn;
        if super::lemon(ui) {
            let face = if hov { ui.t.s(0x2f3335) } else { ui.t.s(0x2a2d2f) };
            lk::outlined(ui.c, Rect::new(row.x + 2.0, row.y, row.w - 4.0, row.h), lk::r_btn(t).min(row.h / 2.0), ui.t.s(0x1f2224), face);
        } else {
            ui.c.fill_rect(row, ui.t.s(0x1f2224));
            ui.c.fill_rect(row.inset(1.0), if hov { ui.t.s(0x2f3335) } else { ui.t.s(0x2a2d2f) });
        }
        chevron(ui, row.x + 10.0, row.cy(), true, if ha { ui.t.tx(0xffffff) } else { ui.t.tx(0xc4ccd0) });
        let (f, name, ink) = match &plugin {
            Some(p) => (Font::new(fs), p.to_string(), ui.t.tx(0xc9d0d4)),
            None => (Font::new(fs).italic(), format!("Slot {}", i + 1), ui.t.s(0x50575b)),
        };
        // Lemon centres the name's capitals on the row, level with the chevron, knob and LED: the baseline sits half a
        // cap height (the face's is about 0.72 of its size) under the middle, measured on the face drawn (the italic
        // one has its own ascent). Classic keeps FL's place.
        let ty = if super::lemon(ui) { (row.cy() + 0.36 * f32::from(fs) - ui.c.ascent(f)).round() } else { row.y + 4.0 * kp };
        ui.c.text_clip(f, row.x + 19.0, ty, 140.0, ink, &name);
        let mix = KnobId::SlotMix(n, k);
        ui.hit(Rect::new(row.right() - 46.0, row.y, 26.0, row.h), Action::Knob(mix), s.knob_text(mix));
        mixer_knob(ui, row.right() - 33.0, row.cy(), 10.5 * q, 12.0 * q, s.knob(mix), Some(false));
        ui.c.stroke_circle(row.right() - 33.0, row.cy(), 11.8 * q, 1.5, ui.t.s(0x55595c));
        ui.hit(Rect::new(row.right() - 18.0, row.y, 18.0, row.h), Action::Mix(MHit::SlotLed(k)), "Mute / solo  (ctrl)0..9");
        led_small(ui, row.right() - 10.0, row.cy(), track.is_none_or(|m| m.slot_on[i]));
    }
    let sep = r.y + 61.0 + 330.0 * kp;
    ui.c.fill_rect(Rect::new(r.x, sep, r.w, 1.0), ui.t.s(0x1f2224));

    // Equalizer: response graph, three band gain faders (±18 dB), band frequency and width knobs (spec: ui/mixer.md §8).
    let eq = Rect::new(r.x + 4.0, sep + 11.0, 146.0, 173.0 * kp);
    let settings = track.map_or(os_model::EQ_DEFAULT, |m| m.eq);
    // Its hint follows the cursor (App::mouse_move).
    let hint = if eq.contains(ui.mouse.0, ui.mouse.1) { mx::eq_graph_hint(s, eq, ui.mouse.0) } else { String::new() };
    ui.hit(eq, Action::Mix(MHit::EqGraph), hint);
    if super::lemon(ui) {
        lk::outlined(ui.c, eq, lk::r_btn(t), ui.t.s(0x1f2224), ui.t.s(0x272a2c));
    } else {
        ui.c.fill_rect(eq, ui.t.s(0x1f2224));
        ui.c.fill_rect(eq.inset(1.0), ui.t.s(0x272a2c));
    }
    for k in 1..5 {
        ui.c.fill_rect(Rect::new(eq.x + 14.0 + 31.0 * k as f32, eq.y + 1.0, 1.0, eq.h - 2.0), ui.t.s(0x2b2e30));
    }
    // The response curve: the three bands' summed gain over the graph's width, which spans the frequency knob's
    // 10 Hz .. 16 kHz law; ±18 dB fills the graph's half-height.
    let half = eq.h / 2.0 - 4.0;
    let steps = (eq.w - 2.0) as usize;
    let pts: Vec<(f32, f32)> = (0..=steps)
        .map(|i| {
            let p = i as f32 / steps as f32;
            let hz = os_dsp::mixer::eq_frequency((p * 65536.0).round() as i32);
            let db = os_dsp::mixer::eq_response_db(settings, 44100, f64::from(hz)) as f32;
            (eq.x + 1.0 + p * (eq.w - 2.0), eq.cy() - (db / 18.0).clamp(-1.0, 1.0) * half) // as mx::eq_graph_db
        })
        .collect();
    ui.c.push_clip(eq.inset(1.0));
    ui.c.polyline(&pts, 2.0, ui.t.tx(0xc9cfd3));
    ui.c.pop_clip();
    for k in 0..3 {
        let fx = r.x + 162.0 + 22.0 * k as f32;
        ui.c.fill_rect(Rect::new(fx - 2.0, eq.y + 1.0, 4.0, eq.h - 2.0), ui.t.s(0x17191b));
        ui.c.fill_rect(Rect::new(fx - 9.0, eq.y + 36.0 * kp, 18.0, 1.0), ui.t.s(0x3a3e41));
        ui.c.fill_rect(Rect::new(fx - 9.0, eq.y + 136.0 * kp, 18.0, 1.0), ui.t.s(0x3a3e41));
        ui.c.fill_rect(Rect::new(fx - 9.0, eq.cy(), 18.0, 1.0), ui.t.s(0x111315));
        let id = KnobId::Eq(n, k as u8, 0);
        let hov = ui.hit(Rect::new(fx - 9.0, eq.y, 18.0, eq.h), Action::Knob(id), s.knob_text(id));
        // The handle travels ±(half-height − 8) for ±18 dB.
        let hy = eq.cy() - (s.knob(id) * 2.0 - 1.0) * (eq.h / 2.0 - 8.0);
        let h = Rect::new(fx - 6.0, hy - 6.0, 12.0, 13.0);
        let face = if hov { 0x585c5f } else { 0x4a4e51 };
        if super::lemon(ui) {
            ui.c.fill_rrect(h.outset(1.0), lk::r_small(t), ui.t.s(0x121416));
            ui.c.fill_rrect(h, lk::r_small(t) - 1.0, ui.t.s(face));
        } else {
            ui.c.fill_rrect(h.outset(1.0), 2.0, ui.t.s(0x121416));
            ui.c.fill_rect_gradient(h, ui.t.s(if hov { 0x6c7073 } else { 0x5e6265 }), ui.t.s(0x2a2d30), 2.0);
        }
        ui.c.fill_rect(Rect::new(fx - 5.0, hy, 10.0, 1.0), ui.t.s(0x111315));
    }
    let ly = eq.bottom() + 8.0;
    if knobs {
        ui.c.text(Font::new(17), r.x + 4.0, ly, ui.t.tx(0xa9b2b7), "Equalizer");
    }
    for row in 0..2 {
        if !knobs {
            break;
        }
        for k in 0..3 {
            let (kx, ky) = (r.x + 162.0 + 22.0 * k as f32, ly + 20.0 + 20.0 * row as f32);
            let id = KnobId::Eq(n, k as u8, 1 + row as u8);
            ui.hit(Rect::new(kx - 9.0, ky - 9.0, 18.0, 18.0), Action::Knob(id), s.knob_text(id));
            mixer_knob(ui, kx, ky, 7.0, 8.0, s.knob(id), Some(false));
        }
    }
    let mut g = Glyph::new();
    for k in 0..3 {
        g.rect(r.x + 141.0 + 3.0 * k as f32, ly + 16.0, 1.4, 6.0);
    }
    g.rect(r.x + 141.0, ly + 21.0, 8.0, 1.4);
    let ay = ly + 40.0;
    g.line(r.x + 137.0, ay, r.x + 149.0, ay, 1.2);
    g.polygon(&[(r.x + 135.0, ay), (r.x + 139.0, ay - 3.0), (r.x + 139.0, ay + 3.0)]).polygon(&[(r.x + 151.0, ay), (r.x + 147.0, ay - 3.0), (r.x + 147.0, ay + 3.0)]);
    if knobs {
        ui.c.glyph(ui.t.tx(0x8c969c), &g);
    }
    let b = r.bottom();

    // Output rows at the bottom: the track latency combo (not available yet: greyed out), then the output combo.
    ui.c.fill_rect(Rect::new(r.x, b - 66.0, r.w, 1.0), ui.t.s(0x1f2224));
    ui.c.fill_rect(Rect::new(r.x, b - 33.0, r.w, 1.0), ui.t.s(0x1f2224));
    let mut g = Glyph::new();
    g.ring(r.x + 14.0, b - 48.5, 8.0, 1.5).rect(r.x + 13.3, b - 53.0, 1.4, 5.0).rect(r.x + 13.3, b - 49.2, 4.0, 1.4);
    ui.c.glyph(ui.t.s(0x40464a), &g);
    ui.off(Rect::new(r.x + 25.0, b - 60.0, r.w - 28.0, 22.0), "Track latency");
    ui.c.text(Font::new(17), r.x + 30.0, b - 59.0, ui.t.tx(0xd4dade), "(none)");
    chevron(ui, r.right() - 11.0, b - 48.5, true, ui.t.tx(0x8c969c));
    ui.veil(Rect::span(r.x, r.right(), b - 65.0, b - 34.0), ui.t.s(0x2e3133));
    let mut g = Glyph::new();
    g.ring(r.x + 13.0, b - 16.0, 8.0, 1.6);
    g.erase(true).rect(r.x + 13.0, b - 26.0, 10.0, 20.0);
    g.erase(false).polygon(&[(r.x + 11.0, b - 20.5), (r.x + 16.0, b - 20.5), (r.x + 21.0, b - 16.0), (r.x + 16.0, b - 11.5), (r.x + 11.0, b - 11.5)]);
    ui.c.glyph(ui.t.tx(0xc4ccd0), &g);
    let out = Rect::new(r.x + 26.0, b - 28.0, 189.0, 24.0);
    let hov = ui.hit(out, Action::Mix(MHit::Output), "Audio output target");
    os_ui::kit::combo(ui.c, ui.t, out, &mx::output_label(s, n), Font::new(17), os_ui::kit::State::hover(hov));
}

/// An input combo with nothing to choose (no device inputs): drawn greyed out with `text`, its hit only says so.
fn off_combo(ui: &mut Ui, r: Rect, name: &'static str, text: &str, bg: Color) {
    ui.off(r, name);
    let st = os_ui::kit::State::NORMAL.with_disabled(true);
    os_ui::kit::combo(ui.c, ui.t, r, text, Font::new(17), st);
    ui.veil(r, bg);
}

/// Small solid chevron centred on (x, y), pointing right or left.
pub fn chevron(ui: &mut Ui, x: f32, y: f32, right: bool, c: Color) {
    let d = if right { 1.0 } else { -1.0 };
    let mut g = Glyph::new();
    g.polygon(&[(x - 2.5 * d, y - 4.5), (x + 2.5 * d, y), (x - 2.5 * d, y + 4.5)]);
    ui.c.glyph(c, &g);
}
