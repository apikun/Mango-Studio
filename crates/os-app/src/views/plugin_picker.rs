// Copyright (C) 2026 ravenmarkdev-alt
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The plugin picker (`crate::plugin_picker`): a box in the middle of the window with its title, the search field and
//! the main list; the open category's list beside it, and the open plugin's variants beside that.

use os_ui::{Color, Font, Rect};

use crate::app::{Action, State, Ui};
use crate::icons::{self, Icon};
use crate::plugin_menu::Target;
use crate::plugin_picker::{self as pk, PickAct, Row};

/// Row pitch of the lists.
pub const ROW_H: f32 = 30.0;
/// Widths of the main box, the category's list and the variants' list.
const MAIN_W: f32 = 380.0;
const SIDE_W: f32 = 290.0;
const VAR_W: f32 = 190.0;
/// The title and search field above the main list, and the footer below it.
const TOP_H: f32 = 104.0;
const FOOT_H: f32 = 34.0;

/// The main box, in the middle of the window.
fn main_rect(s: &State) -> Rect {
    let (w, h) = s.screen;
    let bh = (h - 120.0).clamp(280.0, 660.0);
    Rect::new(((w - MAIN_W) / 2.0).round(), ((h - bh) / 2.0).round(), MAIN_W, bh)
}

/// The main list's viewport.
fn main_list(s: &State) -> Rect {
    let r = main_rect(s);
    Rect::new(r.x + 8.0, r.y + TOP_H, r.w - 16.0, r.h - TOP_H - FOOT_H)
}

/// The category's list box (beside the main box, or left of it where the window has no room) and its viewport.
fn side_rect(s: &State) -> (Rect, Rect) {
    let (m, l) = (main_rect(s), main_list(s));
    let x = if m.right() + 4.0 + SIDE_W <= s.screen.0 { m.right() + 4.0 } else { (m.x - 4.0 - SIDE_W).max(0.0) };
    let r = Rect::new(x, l.y - 8.0, SIDE_W, l.h + 16.0);
    (r, Rect::new(r.x + 6.0, r.y + 8.0, r.w - 12.0, r.h - 16.0))
}

/// Largest scroll of a list of `n` rows (both lists are as tall).
pub fn max_scroll(s: &State, n: usize) -> f32 {
    (n as f32 * ROW_H - main_list(s).h).max(0.0)
}

/// The chosen category's and highlighted rows' colour (the plugin browser's).
fn accent(ui: &Ui) -> Color {
    if super::lemon(ui) { ui.t.brand_lit(os_theme::design::lemon::MAIN) } else { ui.t.h(0xfdc13d) }
}

/// A box of the picker: the popup look.
fn panel(ui: &mut Ui, r: Rect) {
    let t = ui.t;
    if super::lemon(ui) {
        os_ui::kit::popup(ui.c, t, r);
    } else {
        ui.c.fill_rect(r, t.menu_pop);
        ui.c.stroke_rrect(r, 0.0, 1.0, t.menu_pop_border);
    }
    ui.hit(r, Action::Picker(PickAct::Body), "");
}

/// The tag of a Windows plugin's row.
const WINDOWS_TAG: &str = "Windows";

/// Text `f` top for a row at `y` of height `h`.
fn text_y(ui: &mut Ui, f: Font, y: f32, h: f32) -> f32 {
    (y + h / 2.0 - ui.c.line_height(f) / 2.0).round()
}

/// A right-pointing arrow centred at (x, y).
fn arrow(ui: &mut Ui, x: f32, y: f32, c: Color) {
    ui.c.line(x - 2.0, y - 4.5, x + 2.5, y, 1.4, c);
    ui.c.line(x + 2.5, y, x - 2.0, y + 4.5, 1.4, c);
}

/// What a list's rows answer: the row's press, its arrow's.
struct Acts {
    row: fn(usize) -> PickAct,
    more: fn(usize) -> PickAct,
}

/// The rows of a list in viewport `v` scrolled by `scroll`; `open` names the category or family shown beside it.
/// Returns each plugin row's rect by index (for the variants' box).
#[allow(clippy::too_many_arguments)]
fn rows(ui: &mut Ui, rows: &[Row], v: Rect, scroll: f32, acts: Acts, open_cat: Option<pk::Cat>, open_fam: Option<&str>) -> Vec<Option<Rect>> {
    let t = ui.t;
    let gold = accent(ui);
    let (fname, fhead, fcount) = (Font::new(17), Font::new(12), Font::new(14));
    let scroll = scroll.min((rows.len() as f32 * ROW_H - v.h).max(0.0)).max(0.0);
    let mut at = vec![None; rows.len()];
    ui.c.push_clip(v);
    for (i, row) in rows.iter().enumerate() {
        let y = v.y + ROW_H * i as f32 - scroll;
        if y + ROW_H < v.y || y > v.bottom() {
            continue;
        }
        let r = Rect::new(v.x, y, v.w, ROW_H);
        match row {
            Row::Head(h) => {
                let ty = text_y(ui, fhead, y + 4.0, ROW_H - 4.0);
                ui.c.text_clip(fhead.bold(), r.x + 10.0, ty, r.w - 16.0, t.menu_pop_head_text, &h.to_uppercase());
            }
            Row::Note(n) => {
                let ty = text_y(ui, fcount, y, ROW_H);
                ui.c.text_clip(fcount.italic(), r.x + 10.0, ty, r.w - 16.0, t.menu_pop_dim, n);
            }
            Row::Plugin(f) => {
                at[i] = Some(r);
                let what = if f.effect { "effect" } else { "instrument" };
                let from = if f.windows() { " (a Windows plugin, run through yabridge)" } else { "" };
                let hov = ui.hit(r, Action::Picker((acts.row)(i)), format!("{}{from}: click to add this {what}", f.name));
                let more = f.variants.len() > 1;
                let mr = Rect::new(r.right() - 34.0, y, 34.0, ROW_H);
                let mhov = more && ui.hit(mr, Action::Picker((acts.more)(i)), format!("{}: its {} variants", f.name, f.variants.len()));
                let on = open_fam == Some(pk::key(f).as_str());
                if hov || mhov || on {
                    ui.c.fill_rrect(r, 4.0, if on || mhov { gold.with_alpha(70) } else { t.menu_pop_hi });
                }
                let ty = text_y(ui, fname, y, ROW_H);
                let mut right = if more { mr.x } else { r.right() - 8.0 };
                // A Windows plugin (bridged by yabridge) says so in a small tag at the row's right.
                if f.windows() {
                    let ft = Font::new(12);
                    let tw = ui.c.text_width(ft, WINDOWS_TAG) + 10.0;
                    let tag = Rect::new(right - tw, r.cy() - 8.0, tw, 16.0);
                    ui.c.stroke_rrect(tag.inset(0.5), 4.0, 1.0, gold.with_alpha(170));
                    ui.c.text_center(ft, tag, gold, WINDOWS_TAG);
                    right = tag.x - 4.0;
                }
                // The tag says it: the name without its "(Windows)".
                let name = f.name.replace(" (Windows)", "").replace(", Windows)", ")");
                ui.c.text_clip(fname, r.x + 10.0, ty, right - r.x - 14.0, t.menu_pop_text, &name);
                if more {
                    arrow(ui, mr.cx(), r.cy(), if mhov || on { t.menu_pop_text } else { t.menu_pop_dim });
                }
            }
            Row::Cat(c, label, n) => {
                let hov = ui.hit(r, Action::Picker((acts.row)(i)), format!("{label}: {n} plugins"));
                let on = open_cat == Some(*c);
                if on {
                    ui.c.fill_rrect(r, 4.0, gold.with_alpha(110));
                } else if hov {
                    ui.c.fill_rrect(r, 4.0, t.menu_pop_hi);
                }
                let ty = text_y(ui, fname, y, ROW_H);
                ui.c.text_clip(fname, r.x + 10.0, ty, r.w - 70.0, t.menu_pop_text, label);
                let cy = text_y(ui, fcount, y, ROW_H);
                ui.c.text_right(fcount, r.right() - 24.0, cy, if on { t.menu_pop_text } else { t.menu_pop_dim }, &n.to_string());
                arrow(ui, r.right() - 12.0, r.cy(), if on || hov { t.menu_pop_text } else { t.menu_pop_dim });
            }
        }
    }
    ui.c.pop_clip();
    at
}

/// The picker's title for its target.
fn title(target: Target) -> &'static str {
    match target {
        Target::All => "Plugin Selection",
        Target::Gen(_) => "Add an instrument",
        Target::Replace(_) => "Replace the instrument",
        Target::Fx(_) => "Add an effect",
    }
}

pub fn paint(ui: &mut Ui, s: &State) {
    let Some(p) = &s.picker else { return };
    let t = ui.t;
    let gold = accent(ui);
    // Everything else answers nothing while it is open: a press there closes it.
    let (w, h) = s.screen;
    ui.c.fill_rect(Rect::new(0.0, 0.0, w, h), t.s(0x000000).with_alpha(80));
    ui.hit(Rect::new(0.0, 0.0, w, h), Action::Picker(PickAct::Close), "Close the plugin picker (Esc)");
    let m = main_rect(s);
    panel(ui, m);
    ui.c.text(Font::new(21), m.x + 18.0, m.y + 14.0, t.menu_pop_text, title(p.target));
    // The search field: it has the keyboard while the picker is open.
    let field = Rect::new(m.x + 14.0, m.y + 52.0, m.w - 28.0, 38.0);
    ui.c.fill_rrect(field, 5.0, t.sunken);
    ui.c.stroke_rrect(field, 5.0, 1.2, gold);
    icons::draw(ui.c, Icon::Search, Rect::new(field.x + 10.0, field.cy() - 9.0, 18.0, 18.0), t.menu_pop_dim);
    let fs = Font::new(18);
    let tx = field.x + 36.0;
    let ty = text_y(ui, fs, field.y, field.h);
    let room = field.right() - tx - 10.0;
    if p.query.is_empty() {
        ui.c.text_clip(fs.italic(), tx, ty, room, t.menu_pop_dim, "Type to search plugins...");
        ui.c.fill_rect(Rect::new(tx - 2.0, field.y + 9.0, 1.5, field.h - 18.0), t.menu_pop_text);
    } else {
        // The end of a long search shows.
        let mut shown: &str = &p.query;
        while ui.c.text_width(fs, shown) > room - 4.0 && !shown.is_empty() {
            let cut = shown.char_indices().nth(1).map_or(shown.len(), |(i, _)| i);
            shown = &shown[cut..];
        }
        ui.c.text(fs, tx, ty, t.menu_pop_text, shown);
        let cx = tx + ui.c.text_width(fs, shown) + 1.0;
        ui.c.fill_rect(Rect::new(cx, field.y + 9.0, 1.5, field.h - 18.0), t.menu_pop_text);
    }
    // The main list.
    let l = main_list(s);
    ui.hit(l, Action::Picker(PickAct::MainList), "");
    let main = pk::main_rows(s, p);
    let fam_main = p.fam.as_ref().filter(|f| !f.1).map(|f| f.0.as_str());
    let at_main = rows(ui, &main, l, p.scroll[0], Acts { row: PickAct::Main, more: PickAct::MainMore }, p.cat, fam_main);
    // Footer.
    let fy = m.bottom() - FOOT_H;
    ui.c.fill_rect(Rect::new(m.x + 1.0, fy, m.w - 2.0, 1.0), t.menu_pop_head);
    let (shown, all) = pk::counts(s, p);
    let ff = Font::new(14);
    let cy = text_y(ui, ff, fy, FOOT_H);
    let n = if p.query.trim().is_empty() { format!("{all} plugins") } else { format!("{shown} of {all} plugins") };
    ui.c.text(ff, m.x + 18.0, cy, t.menu_pop_dim, &n);
    ui.c.text_right(ff, m.right() - 16.0, cy, t.menu_pop_dim, "Enter adds the first  ·  Esc closes");
    // The open category's list.
    let (mut side, mut at_side) = (Vec::new(), Vec::new());
    if p.cat.is_some() {
        let (r, v) = side_rect(s);
        panel(ui, r);
        ui.hit(v, Action::Picker(PickAct::SideList), "");
        side = pk::side_rows(s, p);
        let fam_side = p.fam.as_ref().filter(|f| f.1).map(|f| f.0.as_str());
        at_side = rows(ui, &side, v, p.scroll[1], Acts { row: PickAct::Side, more: PickAct::SideMore }, None, fam_side);
    }
    // The open family's variants, beside its row; the variant a click on the family adds is marked.
    let Some(fam) = pk::open_family(s, p) else { return };
    let row_of = |list: &[Row], at: &[Option<Rect>]| list.iter().position(|r| matches!(r, Row::Plugin(f) if pk::key(f) == pk::key(&fam))).and_then(|i| at.get(i).copied().flatten());
    let from = if p.fam.as_ref().is_some_and(|f| f.1) { (row_of(&side, &at_side), side_rect(s).0) } else { (row_of(&main, &at_main), m) };
    let (Some(row), boxr) = from else { return };
    let vh = fam.variants.len() as f32 * ROW_H + 12.0;
    let x = if boxr.right() + 4.0 + VAR_W <= w { boxr.right() + 4.0 } else { (boxr.x - 4.0 - VAR_W).max(0.0) };
    let vr = Rect::new(x, (row.y - 6.0).min(h - vh - 4.0).max(0.0), VAR_W, vh);
    panel(ui, vr);
    let f = Font::new(17);
    for (i, e) in fam.variants.iter().enumerate() {
        let r = Rect::new(vr.x + 6.0, vr.y + 6.0 + ROW_H * i as f32, vr.w - 12.0, ROW_H);
        let hov = ui.hit(r, Action::Picker(PickAct::Variant(i)), format!("Add {}", e.label));
        if hov {
            ui.c.fill_rrect(r, 4.0, t.menu_pop_hi);
        }
        if i == fam.pick {
            ui.c.fill_circle(r.x + 12.0, r.cy(), 3.0, gold);
        }
        let ty = text_y(ui, f, r.y, ROW_H);
        ui.c.text_clip(f, r.x + 24.0, ty, r.w - 30.0, t.menu_pop_text, fam.variant_label(e));
    }
}
