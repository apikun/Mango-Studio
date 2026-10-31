// Copyright (C) 2026 ravenmarkdev-alt
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The plugin picker: the window F8, the toolbar's plugin button, the channel rack's "+" and an empty mixer slot open
//! in the middle of the main window. A search field (it takes the keyboard), then the favourites, the plugins added
//! last and the categories; a category lists its plugins beside it by vendor, and a plugin family's variants open beside
//! that. A pick adds the plugin where the picker says (`plugin_menu::Target`), as the plugin menus do. Drawn by
//! `views::plugin_picker`.

use std::collections::BTreeMap;

use winit::keyboard::{Key, NamedKey};

use crate::app::{Action, App, State};
use crate::plugin_db::{self as db, Family};
use crate::plugin_menu::{self as menu, Target};

/// An open picker.
#[derive(Debug, Clone, PartialEq)]
pub struct Picker {
    pub target: Target,
    /// What is typed in the search field.
    pub query: String,
    /// The category listed beside the main list.
    pub cat: Option<Cat>,
    /// The family whose variants show beside its list: its key (`key`), and whether it is in the category's list.
    pub fam: Option<(String, bool)>,
    /// Scroll offsets (px) of the main list and of the category's list.
    pub scroll: [f32; 2],
}

impl Picker {
    pub fn new(target: Target) -> Picker {
        Picker { target, query: String::new(), cat: None, fam: None, scroll: [0.0; 2] }
    }
}

/// A category of the main list: the effects or generators of category `name`, or every generator (`None`, the
/// "Instruments" row of a picker for both kinds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cat {
    pub effect: bool,
    pub name: Option<&'static str>,
}

/// A row of the main list or of a category's list.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// A section's title (Favorites, a vendor...).
    Head(String),
    Plugin(Family),
    /// A category, its label and how many plugins it has.
    Cat(Cat, String, usize),
    /// Said when a list is empty.
    Note(&'static str),
}

/// What a press in the picker does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickAct {
    /// Outside the picker: it closes.
    Close,
    /// The picker's own background and its lists' (the wheel scrolls a list).
    Body,
    MainList,
    SideList,
    /// Row `i` of the main list / the category's list: a plugin is added, a category opens.
    Main(usize),
    Side(usize),
    /// The arrow of a plugin with variants in row `i` of the main list / the category's list: its variants open.
    MainMore(usize),
    SideMore(usize),
    /// Variant `i` of the open family.
    Variant(usize),
}

/// Whether `target`'s picker lists generators, and effects.
fn kinds(target: Target) -> (bool, bool) {
    match target {
        Target::All => (true, true),
        Target::Gen(_) | Target::Replace(_) => (true, false),
        Target::Fx(_) => (false, true),
    }
}

/// Every family `p` can add, alphabetical.
pub fn families(s: &State, target: Target) -> Vec<Family> {
    let (gens, fx) = kinds(target);
    let mut v = Vec::new();
    if gens {
        v.extend(db::families(&s.plugdb.lists.gens, false));
    }
    if fx {
        v.extend(db::families(&s.plugdb.lists.fx, true));
    }
    v.sort_by_key(|f| f.name.to_lowercase());
    v
}

/// A family's key (`Picker::fam`).
pub fn key(f: &Family) -> String {
    format!("{}/{}/{}", if f.effect { "fx" } else { "gen" }, f.vendor, f.name)
}

/// The families whose name, vendor or variants contain `q` (lower case): those whose name does first.
fn matching(all: Vec<Family>, q: &str) -> Vec<Family> {
    let has = |t: &str| t.to_lowercase().contains(q);
    let mut v: Vec<Family> = all.into_iter().filter(|f| has(&f.name) || has(&f.vendor) || f.variants.iter().any(|e| has(&e.label))).collect();
    v.sort_by_key(|f| !has(&f.name));
    v
}

/// The main list: the search results while something is typed, else Favorites, Recent and the categories.
pub fn main_rows(s: &State, p: &Picker) -> Vec<Row> {
    let all = families(s, p.target);
    let q = p.query.trim().to_lowercase();
    if !q.is_empty() {
        let found = matching(all, &q);
        if found.is_empty() {
            return vec![Row::Note("No plugins match")];
        }
        return found.into_iter().map(Row::Plugin).collect();
    }
    if all.is_empty() {
        return vec![Row::Note("No plugins found: Find new plugins in Manage plugins")];
    }
    let mut v = Vec::new();
    let favs: Vec<Row> = all.iter().filter(|f| f.favorite()).cloned().map(Row::Plugin).collect();
    if !favs.is_empty() {
        v.push(Row::Head("Favorites".into()));
        v.extend(favs);
    }
    let recent: Vec<Row> = menu::recent(s, &all).into_iter().cloned().map(Row::Plugin).collect();
    if !recent.is_empty() {
        v.push(Row::Head("Recent".into()));
        v.extend(recent);
    }
    v.push(Row::Head("Categories".into()));
    let (gens, fx) = kinds(p.target);
    let mut cat = |effect: bool, name: Option<&'static str>, label: &str| {
        let n = all.iter().filter(|f| f.effect == effect && name.is_none_or(|c| f.category == c)).count();
        if n > 0 {
            v.push(Row::Cat(Cat { effect, name }, label.to_string(), n));
        }
    };
    if fx {
        for c in db::categories(true) {
            cat(true, Some(c), c);
        }
    }
    if gens && fx {
        cat(false, None, "Instruments");
    } else if gens {
        for c in db::categories(false) {
            cat(false, Some(c), c);
        }
    }
    v
}

/// The open category's list: its plugins by vendor.
pub fn side_rows(s: &State, p: &Picker) -> Vec<Row> {
    let Some(c) = p.cat else { return Vec::new() };
    let mut by: BTreeMap<String, Vec<Family>> = BTreeMap::new();
    for f in families(s, p.target).into_iter().filter(|f| f.effect == c.effect && c.name.is_none_or(|n| f.category == n)) {
        by.entry(f.vendor.to_lowercase()).or_default().push(f);
    }
    let mut v = Vec::new();
    for fams in by.into_values() {
        v.push(Row::Head(fams[0].vendor.clone()));
        v.extend(fams.into_iter().map(Row::Plugin));
    }
    v
}

/// The family whose variants are open.
pub fn open_family(s: &State, p: &Picker) -> Option<Family> {
    let (k, _) = p.fam.as_ref()?;
    families(s, p.target).into_iter().find(|f| key(f) == *k)
}

/// The first plugin of the main list (Enter adds it).
fn first(s: &State, p: &Picker) -> Option<Family> {
    main_rows(s, p).into_iter().find_map(|r| if let Row::Plugin(f) = r { Some(f) } else { None })
}

/// How many families the picker can add, and how many the main list shows (the search's matches).
pub fn counts(s: &State, p: &Picker) -> (usize, usize) {
    let all = families(s, p.target);
    let q = p.query.trim().to_lowercase();
    let n = all.len();
    (if q.is_empty() { n } else { matching(all, &q).len() }, n)
}

impl App {
    /// Opens the plugin picker for `target`; menus close.
    pub fn open_picker(&mut self, target: Target) {
        self.state.menu_open = None;
        self.state.playlist.popup = None;
        self.state.picker = Some(Picker::new(target));
        self.invalidate();
    }

    /// Variant `v` of family `f` added where the picker says; the picker closes.
    fn picker_add(&mut self, f: &Family, v: usize) {
        let Some(p) = self.state.picker.take() else { return };
        let Some(e) = f.variants.get(v) else { return };
        if let Some(m) = crate::playlist::run(self, menu::pick(p.target, f.effect, e.index)) {
            self.state.message = Some(m);
        }
    }

    /// Opens the variants of `f` (in the category's list when `side`), or closes them.
    fn picker_more(&mut self, f: &Family, side: bool) {
        let Some(p) = self.state.picker.as_mut() else { return };
        let k = (key(f), side);
        p.fam = if p.fam.as_ref() == Some(&k) { None } else { Some(k) };
    }

    /// A press in the picker; returns whether `a` was one.
    pub(crate) fn picker_act(&mut self, a: &Action) -> bool {
        let Action::Picker(pa) = *a else { return false };
        self.invalidate();
        let Some(p) = self.state.picker.clone() else { return true };
        let row = |rows: Vec<Row>, i: usize| rows.into_iter().nth(i);
        match pa {
            PickAct::Close => self.state.picker = None,
            PickAct::Body | PickAct::MainList | PickAct::SideList => {}
            PickAct::Main(i) => match row(main_rows(&self.state, &p), i) {
                Some(Row::Plugin(f)) => self.picker_add(&f, f.pick),
                Some(Row::Cat(c, ..)) => {
                    if let Some(p) = self.state.picker.as_mut() {
                        p.cat = if p.cat == Some(c) { None } else { Some(c) };
                        p.fam = None;
                        p.scroll[1] = 0.0;
                    }
                }
                _ => {}
            },
            PickAct::Side(i) => {
                if let Some(Row::Plugin(f)) = row(side_rows(&self.state, &p), i) {
                    self.picker_add(&f, f.pick);
                }
            }
            PickAct::MainMore(i) => {
                if let Some(Row::Plugin(f)) = row(main_rows(&self.state, &p), i) {
                    self.picker_more(&f, false);
                }
            }
            PickAct::SideMore(i) => {
                if let Some(Row::Plugin(f)) = row(side_rows(&self.state, &p), i) {
                    self.picker_more(&f, true);
                }
            }
            PickAct::Variant(i) => {
                if let Some(f) = open_family(&self.state, &p) {
                    self.picker_add(&f, i);
                }
            }
        }
        true
    }

    /// The picker's keys: typing searches, Backspace takes a letter back, Esc clears the search (then closes), Enter
    /// adds the first plugin listed, F8 closes. It is modal: every other key is used up. Returns whether it is open.
    pub(crate) fn picker_key(&mut self, key: &Key, ctrl: bool, alt: bool) -> bool {
        if self.state.picker.is_none() {
            return false;
        }
        self.invalidate();
        let Some(p) = self.state.picker.as_mut() else { return false };
        let p0 = p.query.clone();
        match key {
            Key::Named(NamedKey::Escape) if p.query.is_empty() => self.state.picker = None,
            Key::Named(NamedKey::F8) => self.state.picker = None,
            Key::Named(NamedKey::Escape) => p.query.clear(),
            Key::Named(NamedKey::Backspace) => {
                p.query.pop();
            }
            Key::Named(NamedKey::Space) if !p.query.is_empty() => p.query.push(' '),
            Key::Named(NamedKey::Enter) => {
                let p = p.clone();
                if let Some(f) = first(&self.state, &p) {
                    self.picker_add(&f, f.pick);
                }
            }
            Key::Character(c) if !ctrl && !alt => p.query.push_str(c),
            _ => {}
        }
        if let Some(p) = self.state.picker.as_mut().filter(|p| p.query != p0) {
            p.scroll[0] = 0.0;
            p.fam = None;
        }
        true
    }

    /// The wheel over the picker scrolls the list under the mouse (`area`, its hit); returns whether the picker is
    /// open.
    pub(crate) fn picker_wheel(&mut self, area: Option<&Action>, dy: f32) -> bool {
        let Some(p) = self.state.picker.as_ref() else { return false };
        let side = matches!(area, Some(Action::Picker(PickAct::SideList | PickAct::Side(_) | PickAct::SideMore(_))));
        let main = matches!(area, Some(Action::Picker(PickAct::MainList | PickAct::Main(_) | PickAct::MainMore(_))));
        if side || main {
            let n = if side { side_rows(&self.state, p).len() } else { main_rows(&self.state, p).len() };
            let max = crate::views::plugin_picker::max_scroll(&self.state, n);
            let k = usize::from(side);
            let p = self.state.picker.as_mut().expect("open");
            p.scroll[k] = (p.scroll[k] - dy * 3.0 * crate::views::plugin_picker::ROW_H).clamp(0.0, max);
            self.invalidate();
        }
        true
    }
}

/// Every command `target`'s picker can run, each variant of each family (tests).
#[cfg(test)]
pub fn cmds(s: &State, target: Target) -> Vec<crate::playlist::Cmd> {
    families(s, target).iter().flat_map(|f| f.variants.iter().map(|e| menu::pick(target, f.effect, e.index))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playlist::Cmd;
    use crate::plugin_menu::PlugCmd;

    fn app_with(names: &[&str]) -> App {
        let mut app = App::new(None);
        app.plugins.available.clear();
        app.plugins.effects = names.iter().map(|n| db::fake(n, os_host::Kind::Effect)).collect();
        app.state.plugdb.rebuild(&app.plugins);
        app
    }

    fn labels(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Head(h) => format!("# {h}"),
                Row::Plugin(f) => f.name.clone(),
                Row::Cat(_, l, n) => format!("{l} ({n})"),
                Row::Note(t) => t.to_string(),
            })
            .collect()
    }

    /// An empty slot's picker: favourites, the plugins added last and the categories; a category opens its plugins by
    /// vendor beside the list; a plugin's variants open beside that, and a pick adds the variant to the slot.
    #[test]
    fn categories_open_beside_and_variants_beside_them() {
        let mut app = app_with(&["CLA-76 Mono", "CLA-76 Stereo", "Echo Thing", "Hall Verb"]);
        app.state.plugdb.prefs.toggle_favorite(&app.plugins.effects[3].desc.uid());
        app.state.plugdb.prefs.note_recent(&app.plugins.effects[2].desc.uid());
        app.state.plugdb.rebuild(&app.plugins);
        app.open_picker(Target::Fx(Some(3)));
        let p = app.state.picker.clone().unwrap();
        assert_eq!(labels(&main_rows(&app.state, &p)), ["# Favorites", "Hall Verb", "# Recent", "Echo Thing", "# Categories", "Other (3)"]);
        assert!(app.picker_act(&Action::Picker(PickAct::Main(5))));
        let p = app.state.picker.clone().unwrap();
        assert_eq!(labels(&side_rows(&app.state, &p)), ["# Test", "CLA-76", "Echo Thing", "Hall Verb"]);
        app.picker_act(&Action::Picker(PickAct::SideMore(1)));
        let p = app.state.picker.clone().unwrap();
        let fam = open_family(&app.state, &p).expect("CLA-76's variants");
        assert_eq!(fam.variant_label(fam.entry()), "Stereo");
        // Mono, into slot 3 of the selected track.
        app.picker_act(&Action::Picker(PickAct::Variant(0)));
        assert!(app.state.picker.is_none());
        assert_eq!(app.state.plugdb.prefs.recent.first(), Some(&app.plugins.effects[0].desc.uid()), "CLA-76 Mono was added");
        assert!(cmds(&app.state, Target::Fx(Some(3))).contains(&Cmd::Plugin(PlugCmd::Effect { index: 1, slot: Some(3) })));
    }

    /// Typed letters search (the typing keyboard leaves them alone); Backspace and Esc take them back; Enter adds the
    /// first match; Esc with nothing typed closes the picker.
    /// A Windows plugin bridged by yabridge says so: "(Windows)" in the menus' and browser's label, a tag in the picker.
    #[test]
    #[cfg(target_os = "linux")]
    fn windows_plugins_say_so() {
        let mut app = app_with(&["Echo Thing"]);
        let mut win = db::fake("Serum FX", os_host::Kind::Effect);
        win.desc.path = std::path::PathBuf::from("/home/me/.vst3/yabridge/Serum FX.vst3");
        app.plugins.effects.push(win);
        app.state.plugdb.rebuild(&app.plugins);
        let fx = &app.state.plugdb.lists.fx;
        assert_eq!(fx.iter().map(|e| (e.label.as_str(), e.windows)).collect::<Vec<_>>(), [("Echo Thing", false), ("Serum FX (Windows)", true)]);
        app.open_picker(Target::Fx(None));
        typed_search(&mut app, "serum");
        let rows = main_rows(&app.state, app.state.picker.as_ref().unwrap());
        assert!(matches!(&rows[..], [Row::Plugin(f)] if f.windows()), "{rows:?}");
    }

    fn typed_search(app: &mut App, q: &str) {
        for c in q.chars() {
            app.picker_key(&Key::Character(c.to_string().into()), false, false);
        }
    }

    #[test]
    fn typing_searches() {
        let mut app = app_with(&["CLA-76 Mono", "CLA-76 Stereo", "Echo Thing", "Reverb Room"]);
        app.open_picker(Target::Fx(None));
        let typed = |app: &mut App, keys: &[&str]| {
            for k in keys {
                assert!(app.picker_key(&Key::Character((*k).into()), false, false));
            }
        };
        let shown = |app: &App| labels(&main_rows(&app.state, app.state.picker.as_ref().unwrap()));
        typed(&mut app, &["e", "c", "h"]);
        assert_eq!(shown(&app), ["Echo Thing"]);
        app.picker_key(&Key::Named(NamedKey::Backspace), false, false);
        app.picker_key(&Key::Named(NamedKey::Backspace), false, false);
        assert_eq!(shown(&app), ["Echo Thing", "Reverb Room", "CLA-76"], "names with \"e\" first, then CLA-76 for its Stereo variant");
        app.picker_key(&Key::Named(NamedKey::Escape), false, false);
        assert!(shown(&app).contains(&"# Categories".to_string()), "Esc clears the search");
        app.picker_key(&Key::Named(NamedKey::Escape), false, false);
        assert!(app.state.picker.is_none(), "a second Esc closes");
        app.open_picker(Target::Fx(None));
        typed(&mut app, &["r", "o", "o", "m"]);
        app.picker_key(&Key::Named(NamedKey::Enter), false, false);
        assert!(app.state.picker.is_none());
        assert_eq!(app.state.plugdb.prefs.recent.first(), Some(&app.plugins.effects[3].desc.uid()), "Reverb Room was added");
    }
}
