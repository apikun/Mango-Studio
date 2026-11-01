// Copyright (C) 2026 ravenmarkdev-alt
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The browser's Plugin database tab as a plugin browser: filter buttons (All, Effects, Instruments, Favorites,
//! Recent), a category column, and each plugin family once (`plugin_db::families`) with its vendor and star. Its plugins
//! are the browser's rows (`rows`): a click selects, a drag drops the plugin as the tree's rows do, a double click adds
//! it, and the row menu adds any variant.

use os_ui::{Color, Font, Rect};

use super::browser::{Item, Row, ROW_H};
use crate::app::{Action, State, Ui};
use crate::icons::Icon;
use crate::playlist::{Cmd, PItem};
use crate::plugin_db::{self as db, Family};
use crate::plugin_menu::PlugCmd;

/// The filter buttons: their labels, shorter ones for a narrow browser, and hints.
pub const FILTERS: [(&str, &str, &str); 5] = [
    ("All", "All", "Every plugin"),
    ("Effects", "FX", "Effects"),
    ("Instruments", "Inst", "Instruments"),
    ("Favorites", "Fav", "Starred plugins"),
    ("Recent", "Recent", "The plugins added last"),
];
pub const ALL: usize = 0;
pub const EFFECTS: usize = 1;
pub const INSTRUMENTS: usize = 2;
pub const FAVORITES: usize = 3;
pub const RECENT: usize = 4;

/// Height of the filter buttons' strip above the list.
pub const CHIPS_H: f32 = 30.0;

/// The families the filter and the search let through, every category: alphabetical, Recent newest first.
fn filtered(s: &State) -> Vec<Family> {
    let (db, filter) = (&s.plugdb, s.browser_plug_filter);
    let mut v = Vec::new();
    if filter != EFFECTS {
        v.extend(db::families(&db.lists.gens, false));
    }
    if filter != INSTRUMENTS {
        v.extend(db::families(&db.lists.fx, true));
    }
    v.sort_by_key(|f| f.name.to_lowercase());
    match filter {
        FAVORITES => v.retain(Family::favorite),
        RECENT => {
            let at = |f: &Family| f.variants.iter().filter_map(|e| db.prefs.recent.iter().position(|u| *u == e.uid)).min();
            v.retain(|f| at(f).is_some());
            v.sort_by_key(at);
        }
        _ => {}
    }
    let q = s.browser_search.to_lowercase();
    if !q.is_empty() {
        let has = |t: &str| t.to_lowercase().contains(&q);
        v.retain(|f| has(&f.name) || has(&f.vendor) || f.variants.iter().any(|e| has(&e.label)));
    }
    v
}

/// The categories with plugins in `fams`, in menu order (instruments', then effects', "Other" last), with their counts.
fn categories_of(fams: &[Family]) -> Vec<(&'static str, usize)> {
    let mut v: Vec<(&'static str, usize)> = Vec::new();
    let order = db::categories(false).chain(db::categories(true)).filter(|&c| c != db::OTHER).chain([db::OTHER]);
    for c in order {
        let n = fams.iter().filter(|f| f.category == c).count();
        if n > 0 && !v.iter().any(|(k, _)| *k == c) {
            v.push((c, n));
        }
    }
    v
}

/// The category shown: the chosen one while it has plugins, else all of them.
fn shown_cat(s: &State, cats: &[(&'static str, usize)]) -> Option<&'static str> {
    s.browser_plug_cat.filter(|c| cats.iter().any(|(k, _)| k == c))
}

/// The families listed, top to bottom.
pub fn list(s: &State) -> Vec<Family> {
    let mut v = filtered(s);
    if let Some(c) = shown_cat(s, &categories_of(&v)) {
        v.retain(|f| f.category == c);
    }
    v
}

/// A family's row key (`State::browser_sel`).
pub fn key(f: &Family) -> String {
    format!("{}:{}/{}/{}", super::browser::PLUGIN_TAB, if f.effect { "fx" } else { "gen" }, f.vendor, f.name)
}

/// The browser rows of the families listed.
pub fn rows(s: &State) -> Vec<Row> {
    list(s)
        .iter()
        .map(|f| {
            let (icon, item) = if f.effect { (Icon::Speaker, Item::Effect(f.entry().index)) } else { (Icon::Plug, Item::Generator(f.entry().index)) };
            Row { key: key(f), depth: 0, label: f.name.clone(), icon, cat: 1, open: None, bright: f.favorite(), item, path: None }
        })
        .collect()
}

/// Row menu entries after its Add: each variant of a family that has several, and the star.
pub fn row_menu(s: &State, row: &Row) -> Vec<PItem> {
    let Some(f) = list(s).into_iter().find(|f| key(f) == row.key) else { return Vec::new() };
    let mut v = Vec::new();
    if f.variants.len() > 1 {
        v.extend([PItem::sep(), PItem::head("Variants")]);
        for e in &f.variants {
            let cmd = if f.effect { PlugCmd::Effect { index: e.index, slot: None } } else { PlugCmd::Generator { index: e.index, after: None } };
            v.push(PItem::new(format!("Add {}", f.variant_label(e)), Cmd::Plugin(cmd)));
        }
    }
    v.push(PItem::sep());
    let label = if f.favorite() { "Remove from favorites" } else { "Add to favorites" };
    v.push(PItem::new(label, Cmd::Plugin(PlugCmd::Star { effect: f.effect, index: f.entry().index })));
    // FL's options for a plugin: where its file is, and what it is.
    let (effect, index) = (f.effect, f.entry().index);
    v.extend([
        PItem::sep(),
        PItem::new("Open file location", Cmd::Plugin(PlugCmd::Location { effect, index })),
        PItem::new("Show plugin details", Cmd::Plugin(PlugCmd::Details { effect, index })),
        PItem::new("Manage plugins...", Cmd::Plugin(PlugCmd::Manage)),
    ]);
    v
}

/// The star and the chosen filter's colour (the plugin manager's star).
fn accent(ui: &Ui) -> Color {
    if super::lemon(ui) { ui.t.brand_lit(os_theme::design::lemon::MAIN) } else { ui.t.h(0xfdc13d) }
}

/// Text `f` top for a row at `y` of height `h`.
fn text_y(ui: &mut Ui, f: Font, y: f32, h: f32) -> f32 {
    (y + h / 2.0 - ui.c.line_height(f) / 2.0).round()
}

/// Paints the panel into `r`: the filter buttons, the category column and the plugins (the browser's rows, scrolled
/// by `State::browser_scroll`).
pub fn paint(ui: &mut Ui, s: &State, r: Rect) {
    let t = ui.t;
    let gold = accent(ui);
    // Filter buttons, with their short labels when the long ones do not fit.
    let bar = Rect::new(r.x, r.y, r.w, CHIPS_H);
    ui.c.fill_rect(bar, t.browser_footer);
    let f = Font::new(13);
    let long: f32 = FILTERS.iter().map(|(l, _, _)| ui.c.text_width(f, l) + 18.0).sum();
    let short = long > r.w - 12.0;
    let mut x = r.x + 6.0;
    for (i, &(l, sh, hint)) in FILTERS.iter().enumerate() {
        let label = if short { sh } else { l };
        let b = Rect::new(x, bar.y + 5.0, ui.c.text_width(f, label) + 14.0, 20.0);
        let hov = ui.hit(b, Action::BrowserPlugFilter(i), hint);
        let on = s.browser_plug_filter == i;
        if on {
            ui.c.fill_rrect(b, 10.0, gold.with_alpha(40));
        }
        ui.c.stroke_rrect(b, 10.0, 1.0, if on { gold } else if hov { t.glyph } else { t.text_faint });
        ui.c.text_center(f, b, if on { gold } else if hov { t.glyph } else { t.text_dim }, label);
        x = b.right() + 4.0;
    }
    let body = Rect::span(r.x, r.right(), bar.bottom(), r.bottom());
    ui.hit(body, Action::BrowserList, "");
    // Categories.
    let fams = filtered(s);
    let cats = categories_of(&fams);
    let cat = shown_cat(s, &cats);
    let cw = (body.w * 0.36).clamp(70.0, 130.0).round();
    let col = Rect::new(body.x, body.y, cw, body.h);
    ui.c.fill_rect(col, t.browser_tabs.shade(0.02));
    ui.c.push_clip(col);
    let f = Font::new(14);
    let all = std::iter::once((None, fams.len())).chain(cats.iter().map(|&(c, n)| (Some(c), n)));
    for (i, (c, n)) in all.enumerate() {
        let b = Rect::new(col.x + 3.0, col.y + 4.0 + ROW_H * i as f32, cw - 6.0, ROW_H - 2.0);
        let name = c.unwrap_or("All");
        let hov = ui.hit(b, Action::BrowserPlugCat(c), format!("{name}: {n} plugins"));
        let on = c == cat;
        if on || hov {
            ui.c.fill_rrect(b, 3.0, if on { gold.with_alpha(40) } else { t.browser_row_hi });
        }
        let ty = text_y(ui, f, b.y, b.h);
        let count = n.to_string();
        let room = cw >= 100.0;
        let cnt_w = if room { ui.c.text_width(Font::new(12), &count) + 6.0 } else { 0.0 };
        ui.c.text_clip(f, b.x + 6.0, ty, b.w - 8.0 - cnt_w, if on { gold } else { t.text }, name);
        if room {
            let cy = text_y(ui, Font::new(12), b.y, b.h);
            ui.c.text_right(Font::new(12), b.right() - 5.0, cy, t.text_faint, &count);
        }
    }
    ui.c.pop_clip();
    // Plugins.
    let lr = Rect::span(col.right(), body.right(), body.y, body.bottom());
    ui.c.fill_rect(lr, t.browser_list);
    ui.c.push_clip(lr);
    let fams: Vec<Family> = match cat {
        Some(c) => fams.into_iter().filter(|f| f.category == c).collect(),
        None => fams,
    };
    let scroll = s.browser_scroll.min((fams.len() as f32 * ROW_H - lr.h).max(0.0)).max(0.0);
    let sel = s.browser_sel.as_deref();
    let (fname, fsmall) = (Font::new(15), Font::new(12));
    for (i, fam) in fams.iter().enumerate() {
        let y = lr.y + ROW_H * i as f32 - scroll;
        if y + ROW_H < lr.y || y > lr.bottom() {
            continue;
        }
        let row = Rect::new(lr.x, y, lr.w, ROW_H);
        let k = key(fam);
        let what = if fam.effect { "a mixer track" } else { "the channel rack or playlist" };
        let hov = ui.hit(row, Action::BrowserRow(i), format!("{}: double-click to add, drag onto {what}; right-click for more", fam.name));
        // Registered after the row's, so it answers first.
        let star = Rect::new(row.right() - 22.0, y, 22.0, ROW_H);
        let fav = fam.favorite();
        let shov = ui.hit(star, Action::BrowserPlugStar(i), if fav { "Remove from favorites" } else { "Add to favorites" });
        if sel == Some(k.as_str()) || hov || shov {
            ui.c.fill_rect(row, t.browser_row_hi);
        }
        let ir = Rect::new(star.cx() - 6.5, star.cy() - 6.5, 13.0, 13.0);
        if fav {
            ui.c.icon(&super::settings::STAR_ON, ir, gold);
        } else if hov || shov {
            ui.c.icon(&os_ui::tabler::STAR, ir, if shov { t.glyph } else { t.text_faint });
        }
        let mut right = star.x - 2.0;
        // The vendor, then the variant a click adds and how many more there are, while there is room.
        let vy = text_y(ui, fsmall, y, ROW_H);
        if lr.w >= 210.0 {
            let w = ui.c.text_width(fsmall, &fam.vendor).min((lr.w * 0.32).round());
            ui.c.text_clip(fsmall, right - w, vy, w, t.text_dim, &fam.vendor);
            right -= w + 8.0;
        }
        if fam.variants.len() > 1 && lr.w >= 160.0 {
            let v = format!("{} +{}", fam.variant_label(fam.entry()), fam.variants.len() - 1);
            let w = (ui.c.text_width(fsmall, &v) + 10.0).min(90.0);
            let p = Rect::new(right - w, y + 4.0, w, ROW_H - 8.0);
            ui.c.fill_rrect(p, p.h / 2.0, t.sunken);
            ui.c.text_clip(fsmall, p.x + 5.0, vy, p.w - 8.0, t.text_dim, &v);
            right = p.x - 6.0;
        }
        let c = if fav || hov { t.cat_plugin_db.shade(0.12) } else { t.cat_plugin_db };
        let ty = text_y(ui, fname, y, ROW_H);
        ui.c.text_clip(fname, lr.x + 8.0, ty, right - lr.x - 8.0, c, &fam.name);
    }
    if fams.is_empty() {
        let empty = match s.browser_plug_filter {
            _ if !s.browser_search.is_empty() => "No matches",
            FAVORITES => "No favorites yet: star a plugin",
            RECENT => "Nothing added yet",
            _ => "No plugins found",
        };
        ui.c.text_clip(Font::new(14).italic(), lr.x + 8.0, lr.y + 6.0, lr.w - 12.0, t.text_faint, empty);
    }
    ui.c.pop_clip();
    *s.browser_marquee.borrow_mut() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plugin family is listed once: its stereo variant first, its other variants in the row menu.
    #[test]
    fn variants_are_listed_once() {
        let mut app = crate::app::App::new(None);
        let names = ["CLA-76 Mono", "CLA-76 Stereo", "CLA-76 Live Mono", "H-Reverb Stereo/5.1", "H-Reverb Mono/Stereo", "Lemon Stereo Enhancer"];
        app.plugins.available.clear();
        app.plugins.effects = names.iter().map(|n| db::fake(n, os_host::Kind::Effect)).collect();
        app.state.plugdb.rebuild(&app.plugins);
        app.state.browser_tab = super::super::browser::PLUGIN_TAB;
        let fams = list(&app.state);
        let shown: Vec<(&str, &str)> = fams.iter().map(|f| (f.name.as_str(), f.entry().label.as_str())).collect();
        assert_eq!(shown, [("CLA-76", "CLA-76 Stereo"), ("H-Reverb", "H-Reverb Mono/Stereo"), ("Lemon Stereo Enhancer", "Lemon Stereo Enhancer")]);
        let rows = rows(&app.state);
        assert_eq!(rows[0].item, Item::Effect(1), "a drag adds CLA-76 Stereo");
        let menu = row_menu(&app.state, &rows[0]);
        let labels: Vec<String> = menu.iter().map(|i| i.label.to_string()).collect();
        assert!(labels.iter().any(|l| l == "Add Live Mono") && labels.iter().any(|l| l == "Add to favorites"), "{labels:?}");
        // The search and the filters.
        app.state.browser_search = "verb".into();
        assert_eq!(list(&app.state).len(), 1);
        app.state.browser_search.clear();
        app.state.browser_plug_filter = INSTRUMENTS;
        assert!(list(&app.state).is_empty());
        app.state.browser_plug_filter = FAVORITES;
        assert!(list(&app.state).is_empty());
    }

    /// The row menu's options (FL's): details in the status bar, and the file's folder opened (nothing in a test).
    #[test]
    fn plugin_options_menu() {
        let mut app = crate::app::App::new(None);
        app.plugins.available.clear();
        app.plugins.effects = ["Alpha Mono"].iter().map(|n| db::fake(n, os_host::Kind::Effect)).collect();
        app.state.plugdb.rebuild(&app.plugins);
        app.state.browser_tab = super::super::browser::PLUGIN_TAB;
        let row = rows(&app.state).remove(0);
        let labels: Vec<_> = row_menu(&app.state, &row).into_iter().map(|i| i.label.to_string()).collect();
        assert!(labels.iter().any(|l| l == "Open file location") && labels.iter().any(|l| l == "Show plugin details") && labels.iter().any(|l| l == "Manage plugins..."), "{labels:?}");
        let msg = crate::plugin_menu::run(&mut app, PlugCmd::Details { effect: true, index: 0 });
        assert!(msg.is_some_and(|m| m.starts_with("Alpha Mono: ")), "the details name the plugin");
    }

    /// The star marks the family's variant a click adds; Recent lists the plugins added, newest first.
    #[test]
    fn stars_and_recent() {
        let mut app = crate::app::App::new(None);
        app.plugins.available.clear();
        app.plugins.effects = ["Alpha Mono", "Alpha Stereo", "Beta"].iter().map(|n| db::fake(n, os_host::Kind::Effect)).collect();
        app.state.plugdb.rebuild(&app.plugins);
        app.state.browser_tab = super::super::browser::PLUGIN_TAB;
        crate::plugin_menu::run(&mut app, PlugCmd::Star { effect: true, index: 0 });
        app.state.browser_plug_filter = FAVORITES;
        let favs = list(&app.state);
        assert_eq!(favs.len(), 1);
        assert!(favs[0].variants[1].favorite && !favs[0].variants[0].favorite, "the stereo variant is starred");
        crate::plugin_menu::run(&mut app, PlugCmd::Star { effect: true, index: 1 });
        assert!(list(&app.state).is_empty(), "a second star unstars the family");
        app.state.plugdb.prefs.note_recent(&app.plugins.effects[2].desc.uid());
        app.state.plugdb.prefs.note_recent(&app.plugins.effects[0].desc.uid());
        app.state.browser_plug_filter = RECENT;
        let names: Vec<String> = list(&app.state).into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["Alpha", "Beta"]);
    }

    #[test]
    fn variant_words_come_off_the_end() {
        assert_eq!(db::variant_base("Abbey Road Plates Live Mono/Stereo"), "Abbey Road Plates");
        assert_eq!(db::variant_base("IR-L efficient Stereo/5.1"), "IR-L efficient");
        assert_eq!(db::variant_base("Lemon Stereo Shaper"), "Lemon Stereo Shaper");
        assert_eq!(db::variant_base("Stereo"), "Stereo");
    }
}
