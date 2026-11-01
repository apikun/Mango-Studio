// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The "Import audio data" form (`crate::audio_file::AudioForm`), painted in the dialogs' frame as the MIDI import's
//! form is: the file's name, then Options (what the file becomes), then Accept. A combo's list is drawn over
//! everything, as the MIDI form's are.

use os_ui::{kit, Font, Rect};

use super::dialog::{button, frame, hit, place, section, BTN_H, BTN_W};
use super::export_form::list;
use super::project_forms::label;
use crate::app::{Action, State, Ui};
use crate::audio_file::{self as af, AudioForm};
use crate::dialog::{DHit, Modal};
use crate::project_forms::FHit;

pub const W: f32 = 420.0;
const TOP: f32 = 34.0;
const BODY_H: f32 = 96.0;

/// The form's height.
pub fn height() -> f32 {
    TOP + BODY_H + 24.0 + BTN_H
}

/// Paints form `f` of dialog `m`.
pub fn paint(ui: &mut Ui, s: &State, m: &Modal, f: &AudioForm) {
    let r = place(s, m, W, height());
    frame(ui, r, af::TITLE);
    let t = ui.t;
    let font = Font::new(13);
    let lh = ui.c.line_height(font);
    let g = Rect::new(r.x + 8.0, r.y + TOP, r.w - 16.0, BODY_H);
    section(ui, g, "Options");
    // The file, named.
    let cx = g.x + 150.0;
    let w = g.right() - 12.0 - cx;
    label(ui, g.x + 12.0, g.y + 30.0, "The file", true);
    let name = f.path.file_name().map_or_else(|| f.path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
    ui.c.text_clip(font, cx, g.y + 30.0, w, t.text_dim, &name);
    // What the file becomes.
    let cb = Rect::new(cx, g.y + 56.0, w, 22.0);
    label(ui, g.x + 12.0, (cb.cy() - lh / 2.0).round(), af::KIND_LABEL, true);
    let hov = hit(ui, cb, DHit::Form(FHit::Combo(0)), "The file becomes a clip on the playlist, or its melody the new channel plays");
    kit::combo(ui.c, t, cb, &f.combo_text(), font, kit::State::new(hov, f.list == Some(0), false));
    // Accept (Esc and the caption's × import nothing).
    let bw = (ui.c.text_width(font, "Accept") + 28.0).max(BTN_W).round();
    let b = Rect::new(r.right() - 12.0 - bw, r.bottom() - 12.0 - BTN_H, bw, BTN_H);
    let act = Action::Dlg(DHit::Form(FHit::Main));
    let down = ui.down(b, &act);
    let hov = ui.hit(b, act, "Import the file as the kind says (Enter)");
    button(ui, b, "Accept", true, hov, down, false);
    if f.list == Some(0) {
        let (items, now) = f.list_items();
        list(ui, s, &items, now, cb);
    }
}
