// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Channel plugin window, opened from a channel's name button.
// spec: ui/channel-rack.md §5 (name button). FL shows the generator plugin's own window here; until
// plugins have editors Lemon Studio shows a plain settings window for the channel (Lemon Studio layout).
// An automation clip's window adds its range and its LFO (audio/automation.md "Range", "The LFO"; no page measures
// FL's automation clip window, so the layout is Lemon Studio's own).

use os_model::ChannelKind;
use os_ui::{Font, Rect};

use super::{ball_knob, led, rack, FrameInfo};
use crate::knobs::{KnobId, LfoKnob};
use crate::app::{Action, State, Ui, Win};
use crate::icons::{self, Icon};

pub const SIZE: (f32, f32) = (430.0, 200.0);
/// An automation clip's window: room for the LFO's row.
pub const CLIP_SIZE: (f32, f32) = (430.0, 236.0);

pub fn rect(s: &State) -> Rect {
    let (w, h) = if s.is_clip_row(s.plugin_channel) { CLIP_SIZE } else { SIZE };
    Rect::new(s.plugin_pos.0, s.plugin_pos.1, w, h)
}

pub fn bounds(s: &State) -> Rect {
    rect(s).outset(1.0)
}

pub fn paint(ui: &mut Ui, s: &State, _f: &FrameInfo) {
    let t = ui.t;
    let win = rect(s);
    ui.c.fill_rect(win, t.window_border);
    ui.c.fill_rect(win.inset(1.0), t.window_body);
    ui.hit(win, Action::Swallow, "");
    ui.c.fill_rect(Rect::new(win.x + 1.0, win.y + 1.0, win.w - 2.0, 1.0), t.window_title_hi);
    let bar = Rect::span(win.x + 1.0, win.right() - 1.0, win.y + 1.0, win.y + 29.0);
    ui.hit(bar, Action::WinDrag(Win::Plugin), "Drag to move");
    let fx = s.plugin_fx;
    let name = fx.map_or_else(
        || {
            s.project
                .channels
                .get(s.plugin_channel)
                .map(|ch| ch.name.clone().unwrap_or_else(|| format!("Channel {}", ch.index + 1)))
                .unwrap_or_else(|| "Channel".into())
        },
        |(track, slot)| fx_name(s, track, slot),
    );
    icons::draw(ui.c, Icon::Plug, Rect::new(win.x + 9.0, win.y + 7.0, 15.0, 15.0), t.glyph);
    ui.c.text_clip(Font::new(17), win.x + 30.0, win.y + 5.0, win.w - 60.0, t.text, &name);
    let close = Rect::new(win.right() - 22.0, win.y + 6.0, 16.0, 16.0);
    let hov = ui.hit(close, Action::CloseWin(Win::Plugin), "Close");
    icons::draw(ui.c, Icon::Close, close.inset(1.5), if hov { ui.t.tx(0xffffff) } else { t.glyph });

    let body = Rect::span(win.x + 5.0, win.right() - 6.0, win.y + 30.0, win.bottom() - 6.0);
    ui.c.fill_rect(body.outset(1.0), t.rack_inset_border);
    ui.c.fill_rect(body, t.rack_inset);
    // A mixer slot's window (Android, issue #117): the effect's settings instead of a channel's.
    if let Some((track, slot)) = fx {
        return paint_fx(ui, s, body, track, slot);
    }
    let Some(ch) = s.project.channels.get(s.plugin_channel) else { return };
    let kind = match ch.kind {
        ChannelKind::Sampler => "Sampler".to_string(),
        ChannelKind::Plugin => crate::project_plugins::plugin_title(ch).unwrap_or_else(|| "Plugin".into()),
        ChannelKind::Layer => "Layer".into(),
        ChannelKind::AudioClip => "Audio clip".into(),
        ChannelKind::Automation => "Automation clip".into(),
        ChannelKind::Other(n) => format!("Unknown type {n}"),
    };
    let sample = match (&ch.sample_path, ch.kind) {
        (Some(p), _) => p.rsplit(['/', '\\']).next().unwrap_or(p).to_string(),
        (None, ChannelKind::Sampler) => "(empty - plays the built-in pluck)".into(),
        (None, _) => "-".into(),
    };
    if ch.kind == ChannelKind::Automation {
        return paint_clip(ui, s, body, kind);
    }
    let lines = [
        ("Generator", kind),
        ("Sample", sample),
        ("Mixer track", rack::insert_name(ch.mixer_track)),
        ("Volume", format!("{:.0}%", ch.volume * 100.0)),
        ("Panning", format!("{:+.0}", ch.pan * 100.0)),
        ("Pitch", format!("{:+} cents", ch.pitch_cents)),
    ];
    let f = Font::new(15);
    for (i, (k, v)) in lines.iter().enumerate() {
        let ly = body.y + 10.0 + 22.0 * i as f32;
        ui.c.text(f, body.x + 12.0, ly, t.text_dim, k);
        ui.c.text_clip(f, body.x + 100.0, ly, body.w - 180.0, t.text, v);
    }
    let (vol, pan) = (KnobId::ChanVol(s.plugin_channel), KnobId::ChanPan(s.plugin_channel));
    ui.hit(Rect::new(body.right() - 68.0, body.y + 20.0, 28.0, 28.0), Action::Knob(vol), s.knob_text(vol));
    ball_knob(ui, body.right() - 54.0, body.y + 34.0, 12.0, s.knob(vol));
    ui.hit(Rect::new(body.right() - 68.0, body.y + 70.0, 28.0, 28.0), Action::Knob(pan), s.knob_text(pan));
    ball_knob(ui, body.right() - 54.0, body.y + 84.0, 12.0, s.knob(pan));
    ui.c.text_center(Font::new(13), Rect::new(body.right() - 84.0, body.y + 48.0, 60.0, 14.0), t.text_dim, "VOL");
    ui.c.text_center(Font::new(13), Rect::new(body.right() - 84.0, body.y + 98.0, 60.0, 14.0), t.text_dim, "PAN");
}

/// A knob with its hit area, hint and caption below it.
fn knob(ui: &mut Ui, s: &State, id: KnobId, cx: f32, cy: f32, caption: &str) {
    let t = ui.t;
    ui.hit(Rect::new(cx - 14.0, cy - 14.0, 28.0, 28.0), Action::Knob(id), s.knob_text(id));
    ball_knob(ui, cx, cy, 12.0, s.knob(id));
    ui.c.text_center(Font::new(13), Rect::new(cx - 30.0, cy + 14.0, 60.0, 14.0), t.text_dim, caption);
}

/// A switch drawn as an LED with its label; a click flips it (`Action::Knob` on a two-position LFO control).
fn switch(ui: &mut Ui, s: &State, id: KnobId, x: f32, cy: f32, label: &str) {
    let t = ui.t;
    let w = 20.0 + ui.c.text_width(Font::new(13), label);
    ui.hit(Rect::new(x, cy - 9.0, w, 18.0), Action::Knob(id), s.knob_text(id));
    led(ui, x + 6.0, cy, s.knob(id) >= 0.5);
    ui.c.text(Font::new(13), x + 16.0, cy - 8.0, t.text, label);
}

/// An automation clip's settings: its curve, its range (the channel's pan knob is the minimum, its volume knob the
/// maximum) and its LFO.
fn paint_clip(ui: &mut Ui, s: &State, body: Rect, kind: String) {
    let t = ui.t;
    let row = s.plugin_channel;
    let Some(ch) = s.project.channels.get(row) else { return };
    let points = s.project.automation(ch.index).map_or(0, |a| a.points.len());
    let (min, max) = (s.knob(KnobId::ChanPan(row)), s.knob(KnobId::ChanVol(row)));
    let lines = [("Generator", kind), ("Points", points.to_string()), ("Range", format!("{:.0}% to {:.0}%", min * 100.0, max * 100.0))];
    let f = Font::new(15);
    for (i, (k, v)) in lines.iter().enumerate() {
        let ly = body.y + 10.0 + 22.0 * i as f32;
        ui.c.text(f, body.x + 12.0, ly, t.text_dim, k);
        ui.c.text_clip(f, body.x + 100.0, ly, body.w - 200.0, t.text, v);
    }
    knob(ui, s, KnobId::ChanPan(row), body.right() - 104.0, body.y + 34.0, "MIN");
    knob(ui, s, KnobId::ChanVol(row), body.right() - 44.0, body.y + 34.0, "MAX");

    // The LFO's row.
    let ly = body.y + 96.0;
    ui.c.fill_rect(Rect::new(body.x + 8.0, ly, body.w - 16.0, 1.0), t.rack_inset_border);
    ui.c.text(Font::new(15), body.x + 12.0, ly + 10.0, t.text_dim, "LFO");
    switch(ui, s, KnobId::AutoLfo(row, LfoKnob::On), body.x + 12.0, ly + 50.0, "On");
    switch(ui, s, KnobId::AutoLfo(row, LfoKnob::Mode), body.x + 12.0, ly + 74.0, "Add");
    let knobs = [(LfoKnob::Amount, "AMOUNT"), (LfoKnob::Speed, "SPEED"), (LfoKnob::Tension, "TENSION"), (LfoKnob::Skew, "SKEW"), (LfoKnob::Pulse, "PULSE")];
    for (i, (k, caption)) in knobs.into_iter().enumerate() {
        knob(ui, s, KnobId::AutoLfo(row, k), body.x + 110.0 + 60.0 * i as f32, ly + 50.0, caption);
    }
}

/// The docked effect window's title: the plugin's name with its mixer target after it, "(Insert N)" / "(Master)"
/// like a floating frame's (spec: ui/mixer.md §7).
fn fx_name(s: &State, track: u16, slot: u8) -> String {
    let m = s.project.mixer.iter().find(|m| m.index == track);
    let name = s
        .mixer
        .slot_names
        .get(&(track, slot))
        .cloned()
        .or_else(|| m.and_then(|m| m.slots.get(usize::from(slot)).and_then(|p| p.as_ref()).map(crate::project_plugins::slot_title)))
        .unwrap_or_else(|| "Effect".into());
    format!("{name} ({})", if track == 0 { "Master".to_string() } else { format!("Insert {track}") })
}

/// The docked effect window's body (Android, issue #117): the effect's target, its enable switch and its menu
/// (presets, replace, remove). The controls are the mixer slot's own actions, so they work wherever the mixer's do.
fn paint_fx(ui: &mut Ui, s: &State, body: Rect, track: u16, slot: u8) {
    let t = ui.t;
    let on = s.project.mixer.iter().find(|m| m.index == track).is_some_and(|m| m.slot_on[usize::from(slot)]);
    let lines = [("Target", if track == 0 { "Master".to_string() } else { format!("Insert {track}") }), ("State", if on { "On".into() } else { "Bypassed".into() })];
    let f = Font::new(15);
    for (i, (k, v)) in lines.iter().enumerate() {
        let ly = body.y + 10.0 + 22.0 * i as f32;
        ui.c.text(f, body.x + 12.0, ly, t.text_dim, k);
        ui.c.text_clip(f, body.x + 100.0, ly, body.w - 120.0, t.text, v);
    }
    // The enable switch: the slot's LED and the mixer's own action.
    let sw = Rect::new(body.x + 12.0, body.y + 74.0, 110.0, 20.0);
    ui.hit(sw, crate::app::Action::Mix(crate::mixer::MHit::SlotLed(slot)), "Enable or bypass the effect");
    led(ui, sw.x + 6.0, sw.cy(), on);
    ui.c.text(Font::new(13), sw.x + 18.0, sw.y + 3.0, t.text, if on { "On" } else { "Bypassed" });
    // The slot's menu (presets, replace, remove): the mixer slot's arrow action.
    let menu = Rect::new(body.x + 12.0, body.y + 110.0, 110.0, 24.0);
    let hov = ui.hit(menu, crate::app::Action::Mix(crate::mixer::MHit::SlotArrow(slot)), "The effect's menu");
    ui.c.fill_rrect(menu, 4.0, if hov { t.button_hi } else { t.button });
    ui.c.text_center(Font::new(13), menu, if hov { t.tx(0xffffff) } else { t.text }, "Menu");
}
