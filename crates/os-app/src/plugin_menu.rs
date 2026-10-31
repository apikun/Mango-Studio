// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 thacer13
// Copyright (C) 2026 ravenmarkdev-alt
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Plugin menus: F8, the toolbar's plugin button, the channel rack's "+" and an empty mixer slot open the plugin
//! picker (`crate::plugin_picker`); an empty mixer slot opens the larger effect menu there. Add one ▸ / Insert ▸ /
//! Replace ▸ and ADD ▸ Effect list the installed plugins as submenus. A pick adds the plugin where that menu says.
//! Each plugin family is listed once (`plugin_db::families`).
// spec: ui/plugin-menu.md

use std::collections::BTreeMap;

use crate::app::{App, State};
use crate::playlist::{Cmd, Mark, PItem};
use crate::plugin_db::{self as db, Family, MenuView, Mode};

/// What a plugin menu entry does (`Cmd::Plugin`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlugCmd {
    /// Generator `index` (into `Plugins::available`): a new channel at the bottom of the rack, or right below row
    /// `after`.
    Generator { index: usize, after: Option<usize> },
    /// Generator `index` loaded straight onto arranger track `track` (a generator dropped on the playlist's grid or a
    /// track header, the header menu's Insert instrument ▸; issue #2): the channel, and a clip of a new pattern named
    /// after the instrument on the track at `tick`.
    GeneratorOnTrack { index: usize, track: u16, tick: u32 },
    /// Generator `index` in place of rack row `row`'s (channel menu / plugin window menu > Replace ▸).
    Replace { index: usize, row: usize },
    /// Effect `index` (into `Plugins::effects`) into the selected mixer track: slot `slot`, else its first empty slot.
    Effect { index: usize, slot: Option<u8> },
    /// Find new plugins (a background scan).
    Find,
    /// Settings > Plugins.
    Manage,
    View(MenuView),
    /// The plugin browser's star on the family of plugin `index` (an effect when `effect`, else a generator).
    Star { effect: bool, index: usize },
    /// The file manager opens the folder the plugin's file is in (the options menu's Open File Location).
    Location { effect: bool, index: usize },
    /// The status bar says what the plugin is and where its file is (the options menu's Show Plugin Details).
    Details { effect: bool, index: usize },
}

/// Which plugins a menu lists and where a pick goes (spec §1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// F8, the toolbar button, VIEW / ADD > Plugin picker: generators and effects.
    All,
    /// Generators: a new channel at the bottom (`None`) or right below row `r`.
    Gen(Option<usize>),
    /// Generators in place of rack row `r`'s generator.
    Replace(usize),
    /// Effects into the selected track: slot `k`, else its first empty slot.
    Fx(Option<u8>),
}

/// What picking plugin `index` (an effect when `effect`) in `target`'s menu does.
pub(crate) fn pick(target: Target, effect: bool, index: usize) -> Cmd {
    Cmd::Plugin(match (target, effect) {
        (Target::Fx(slot), _) => PlugCmd::Effect { index, slot },
        (_, true) => PlugCmd::Effect { index, slot: None },
        (Target::Gen(after), false) => PlugCmd::Generator { index, after },
        (Target::Replace(row), false) => PlugCmd::Replace { index, row },
        (Target::All, false) => PlugCmd::Generator { index, after: None },
    })
}

/// A family's entry: its name, adding the variant a click adds.
fn item(f: &Family, target: Target) -> PItem {
    PItem::new(f.name.clone(), pick(target, f.effect, f.entry().index))
}

/// A header with a run-time label.
fn head(label: String) -> PItem {
    PItem { head: true, ..PItem::new(label, Cmd::Nothing) }
}

/// How many plugins a menu's Recent lists.
pub(crate) const RECENT_SHOWN: usize = 5;

/// The families of `fams` among the plugins added last (`Prefs::recent`), newest first.
pub(crate) fn recent<'a>(s: &State, fams: &'a [Family]) -> Vec<&'a Family> {
    let at = |f: &Family| f.variants.iter().filter_map(|e| s.plugdb.prefs.recent.iter().position(|u| *u == e.uid)).min();
    let mut v: Vec<&Family> = fams.iter().filter(|f| at(f).is_some()).collect();
    v.sort_by_key(|f| at(f));
    v.truncate(RECENT_SHOWN);
    v
}

/// One section's entries (spec §2): favourites, the plugins added last, then every plugin by display mode; under
/// Categories, a submenu per category. `nested`: the list is a submenu itself, so categories and vendors use headers
/// instead of submenus (popups open one submenu level). `mk` names what a pick of family `f` does (`item` for a menu's
/// target, or the track menu's GeneratorOnTrack).
fn section(s: &State, effect: bool, mk: impl Fn(&Family) -> PItem + Copy, nested: bool) -> Vec<PItem> {
    let list = if effect { &s.plugdb.lists.fx } else { &s.plugdb.lists.gens };
    if list.is_empty() {
        return vec![PItem::new(if effect { "(no effect plugins found)" } else { "(no instruments found)" }, Cmd::Nothing).off()];
    }
    let fams = db::families(list, effect);
    let mut v = Vec::new();
    let favs: Vec<PItem> = fams.iter().filter(|f| f.favorite()).map(|f| mk(f)).collect();
    if !favs.is_empty() {
        v.push(PItem::head("Favorites"));
        v.extend(favs);
    }
    let recent = recent(s, &fams);
    if !recent.is_empty() {
        v.push(PItem::head("Recent"));
        v.extend(recent.into_iter().map(|f| mk(f)));
    }
    match s.plugdb.prefs.menu_view {
        MenuView::Simple => {
            if !v.is_empty() {
                v.push(PItem::head("All"));
            }
            v.extend(fams.iter().map(|f| mk(f)));
        }
        MenuView::Categories => {
            if !nested {
                v.push(PItem::head("Categories"));
            }
            for cat in db::categories(effect) {
                let items: Vec<PItem> = fams.iter().filter(|f| f.category == cat).map(|f| mk(f)).collect();
                if items.is_empty() {
                    continue;
                }
                if nested {
                    v.push(PItem::head(cat));
                    v.extend(items);
                } else {
                    v.push(PItem::sub(format!("{cat} ({})", items.len()), items));
                }
            }
        }
        MenuView::Vendor => {
            let mut by: BTreeMap<String, (String, Vec<&Family>)> = BTreeMap::new();
            for f in &fams {
                by.entry(f.vendor.to_lowercase()).or_insert_with(|| (f.vendor.clone(), Vec::new())).1.push(f);
            }
            for (_, (vendor, fs)) in by {
                let items: Vec<PItem> = fs.iter().map(|f| mk(f)).collect();
                if nested {
                    v.push(head(vendor));
                    v.extend(items);
                } else {
                    v.push(PItem::sub(vendor, items));
                }
            }
        }
    }
    v
}

/// The display modes as their radio items name them (spec §2-3); ADD's radios are the same.
pub const VIEWS: &[(&str, MenuView)] = &[("Categories", MenuView::Categories), ("Simple", MenuView::Simple), ("By vendor", MenuView::Vendor)];

/// After the lists (spec §2): Find new plugins, Manage plugins..., the display modes.
fn footer(s: &State) -> Vec<PItem> {
    let view = s.plugdb.prefs.menu_view;
    let scanning = s.plugdb.progress.is_some();
    let find = PItem::new(if scanning { "Find new plugins (scanning...)" } else { "Find new plugins" }, Cmd::Plugin(PlugCmd::Find));
    let mut v = vec![PItem::sep(), if scanning { find.off() } else { find }, PItem::new("Manage plugins...", Cmd::Plugin(PlugCmd::Manage)), PItem::sep()];
    v.extend(VIEWS.iter().map(|&(label, m)| PItem::new(label, Cmd::Plugin(PlugCmd::View(m))).mark(Mark::Radio(view == m))));
    v
}

/// The entries of a plugin menu for `target`; `nested` when they fill a submenu.
pub fn items(s: &State, target: Target, nested: bool) -> Vec<PItem> {
    let mut v = Vec::new();
    match target {
        Target::All => {
            v.push(PItem::head("Generators"));
            v.extend(section(s, false, |f| item(f, target), nested));
            v.push(PItem::col());
            v.push(PItem::head("Effects"));
            v.extend(section(s, true, |f| item(f, target), nested));
        }
        Target::Gen(_) | Target::Replace(_) => v.extend(section(s, false, |f| item(f, target), nested)),
        Target::Fx(_) => v.extend(section(s, true, |f| item(f, target), nested)),
    }
    v.extend(footer(s));
    v
}

/// The generators as "load onto track" entries (the arranger track header menu's Insert instrument ▸, issue #2): a
/// pick adds the instrument as `add_generator` does and places a clip of a new pattern named after it on the track
/// at `tick` — FL adds an instrument dropped straight on a track. `nested`: the list fills a submenu.
pub fn items_on_track(s: &State, track: u16, tick: u32, nested: bool) -> Vec<PItem> {
    let mut v = section(
        s,
        false,
        |f| PItem::new(f.name.clone(), Cmd::Plugin(PlugCmd::GeneratorOnTrack { index: f.entry().index, track, tick })),
        nested,
    );
    v.extend(footer(s));
    v
}

/// Whether the plugin picker is open (F8 closes it).
pub fn is_open(s: &State) -> bool {
    s.picker.is_some()
}

impl App {
    /// Opens the plugin picker for `target` in the middle of the window (`crate::plugin_picker`); `_at` is where the
    /// way in was (the picker does not follow it).
    pub fn open_plugin_menu(&mut self, target: Target, _at: (f32, f32)) {
        self.open_picker(target);
    }

    /// F8 (spec §1): the plugin picker for every plugin, or closes an open one.
    pub fn toggle_plugin_menu(&mut self) {
        if is_open(&self.state) {
            self.state.picker = None;
            self.invalidate();
        } else {
            self.open_picker(Target::All);
        }
    }
}

/// Runs a plugin menu command. Returns a status message.
pub fn run(app: &mut App, c: PlugCmd) -> Option<String> {
    let m = match c {
        PlugCmd::Generator { index, after } => add_generator(app, index, after),
        PlugCmd::GeneratorOnTrack { index, track, tick } => add_generator_on_track(app, index, track, tick),
        PlugCmd::Replace { index, row } => replace_generator(app, index, row),
        PlugCmd::Effect { index, slot } => add_effect(app, index, slot),
        PlugCmd::Find => {
            app.start_scan(Mode::Changed);
            None
        }
        PlugCmd::Manage => {
            app.open_settings(crate::views::settings::PLUGINS);
            None
        }
        PlugCmd::View(v) => {
            app.state.plugdb.prefs.menu_view = v;
            app.prefs_changed();
            None
        }
        PlugCmd::Star { effect, index } => {
            star(app, effect, index);
            None
        }
        PlugCmd::Location { effect, index } => match plugin_desc(app, effect, index) {
            Some(d) if crate::project_plugins::is_builtin(d) => Some(format!("{} is built in: it has no file", d.name)),
            Some(d) => {
                let folder = d.path.parent().unwrap_or(&d.path).to_path_buf();
                crate::browser::open_folder(&folder);
                Some(format!("Opened folder: {}", folder.display()))
            }
            None => None,
        },
        PlugCmd::Details { effect, index } => plugin_desc(app, effect, index).map(|d| {
            let place = if crate::project_plugins::is_builtin(d) { "built in".to_string() } else { d.path.display().to_string() };
            format!("{}: {} · {} · {}", d.name, d.vendor, d.format.as_str(), place)
        }),
    };
    app.invalidate();
    m
}

/// The descriptor of plugin `index` of the effects (`effect`) or the generators.
fn plugin_desc(app: &App, effect: bool, index: usize) -> Option<&os_host::Descriptor> {
    let list = if effect { &app.plugins.effects } else { &app.plugins.available };
    list.get(index).map(|a| &a.desc)
}

/// Stars the family of plugin `index` (its variant a click adds), or unstars every starred variant of it.
fn star(app: &mut App, effect: bool, index: usize) {
    let db = &mut app.state.plugdb;
    let list = if effect { &db.lists.fx } else { &db.lists.gens };
    let Some(fam) = db::families(list, effect).into_iter().find(|f| f.variants.iter().any(|e| e.index == index)) else { return };
    let starred: Vec<&str> = fam.variants.iter().filter(|e| e.favorite).map(|e| e.uid.as_str()).collect();
    if starred.is_empty() {
        db.prefs.toggle_favorite(&fam.entry().uid);
    } else {
        for uid in starred {
            db.prefs.toggle_favorite(uid);
        }
    }
    app.prefs_changed();
}

/// Plugin `uid` was added: first in the plugin browser's Recent.
fn note_recent(app: &mut App, uid: &str) {
    let prefs = &mut app.state.plugdb.prefs;
    prefs.note_recent(uid);
    prefs.save();
}

/// A new channel for generator `index` (spec §1): at the bottom of the rack, or right below row `after`; selected and
/// in view. Lemon Sampler makes an empty sampler channel (hosted on the next tick); any other generator a plugin
/// channel hosting it. Its plugin window opens once it is hosted.
pub fn add_generator(app: &mut App, index: usize, after: Option<usize>) -> Option<String> {
    let d = app.plugins.available.get(index)?.desc.clone();
    note_recent(app, &d.uid());
    let s = &mut app.state;
    let sampler = d.name == crate::plugins::LEMON_SAMPLER && crate::project_plugins::is_builtin(&d);
    // One step, at the bottom (`history::add_channel`) or inserted higher up (`history::insert_channel`).
    let plugin = (!sampler).then_some(index);
    match after.filter(|&r| r + 1 < s.project.channels.len()) {
        Some(r) => crate::history::insert_channel(s, r + 1, plugin),
        None => crate::history::add_channel(s, plugin),
    }
    let mut ch = if sampler { crate::app::add_channel(&mut s.project) } else { crate::app::add_plugin_channel(&mut s.project, &d.name) };
    if sampler {
        s.pending_open = Some(ch);
    } else {
        s.pending_plugin = Some((ch, index));
    }
    s.piano.dirty = true;
    if let Some(r) = after.filter(|&r| r + 1 < ch) {
        let order: Vec<usize> = (0..=r).chain([ch]).chain(r + 1..ch).collect();
        crate::rack::reorder(app, &order);
        ch = r + 1;
    }
    let s = &mut app.state;
    s.selected_channel = ch;
    s.rack.select_only(ch);
    crate::rack::autosize(s);
    if after.is_none() {
        // Keep the new row in view.
        let r = crate::views::rack::rect(s);
        let view_h = crate::views::rack::panel(r).h;
        s.rack_scroll = (crate::views::rack::content_h(s) - view_h).max(0.0);
    }
    sampler.then(|| "Added a sampler channel (Mango Sampler)".into())
}

/// Generator `index` loaded straight onto arranger track `track` (a generator dropped on the playlist's grid or a
/// track header, the header menu's Insert instrument ▸; issue #2): the channel as `add_generator` makes it (at the
/// bottom of the rack, hosted, selected), then a new pattern named after the instrument whose clip is placed on the
/// track at `tick` — on a header that is the song position, FL adds an instrument dropped on the playlist as a clip
/// on it. One undo step ("Add instrument to track"): undone, the clip, the pattern's name and the channel (with its
/// instance) go. Returns the status message.
pub fn add_generator_on_track(app: &mut App, index: usize, track: u16, tick: u32) -> Option<String> {
    let d = app.plugins.available.get(index)?.desc.clone();
    let s = &mut app.state;
    let sampler = d.name == crate::plugins::LEMON_SAMPLER && crate::project_plugins::is_builtin(&d);
    // One step holding what the whole load changes: the playlist's side (snapshot_with's), the rack's (the channel
    // goes and comes back with it, a redo hosts the generator again) and the patterns' (the name).
    let side = crate::history::RackSide::now(s, s.project.channels.len());
    let snap = crate::history::Snap::Several(vec![
        crate::history::Snap::Generator(Box::new(side), (!sampler).then_some(index)),
        crate::history::Snap::Patterns { patterns: s.project.patterns.clone(), current: s.pattern },
    ]);
    crate::playlist::snapshot_with(s, crate::history::ADD_TO_TRACK, snap);
    // The channel, as add_generator makes it at the bottom of the rack.
    let ch = if sampler { crate::app::add_channel(&mut s.project) } else { crate::app::add_plugin_channel(&mut s.project, &d.name) };
    if sampler {
        s.pending_open = Some(ch);
    } else {
        s.pending_plugin = Some((ch, index));
    }
    s.piano.dirty = true;
    let s = &mut app.state;
    s.selected_channel = ch;
    s.rack.select_only(ch);
    crate::rack::autosize(s);
    let r = crate::views::rack::rect(s);
    let view_h = crate::views::rack::panel(r).h;
    s.rack_scroll = (crate::views::rack::content_h(s) - view_h).max(0.0);
    // A pattern of its own, named after the instrument ("Lemon Trio", then "Lemon Trio #2" once taken), current and
    // the playlist's clip source, as the rack's "+" leaves a picked pattern.
    let n = crate::patterns::first_empty(s);
    let taken = |cand: &str| s.project.patterns.iter().any(|p| p.name.as_deref() == Some(cand));
    let name = (1..).map(|k| if k == 1 { d.name.clone() } else { format!("{} #{k}", d.name) }).find(|c| !taken(&c)).expect("unbounded");
    crate::patterns::pat_mut(s, n).name = Some(name.clone());
    crate::patterns::select(s, n);
    s.playlist.picker = crate::playlist::PickKind::Pattern;
    let i = crate::playlist::place(s, track, tick);
    s.playlist.sel = vec![i];
    Some(format!("Added {name} on track {}", track + 1))
}

/// Generator `index` in place of rack row `row`'s (channel menu and plugin window menu > Replace ▸; FL: "the plugin list
/// of Add one", spec ui/channel-rack.md §18). The channel keeps its row, notes and steps, mixer track, levels and
/// Misc settings; its old instance goes, the new one starts from its defaults, and the channel takes the new
/// plugin's name. Lemon Studio choice where FL was not measured: the name follows the plugin.
pub fn replace_generator(app: &mut App, index: usize, row: usize) -> Option<String> {
    let d = app.plugins.available.get(index)?.desc.clone();
    app.state.project.channels.get(row)?;
    let sampler = d.name == crate::plugins::LEMON_SAMPLER && crate::project_plugins::is_builtin(&d);
    // One step, with the old instrument's state (`history::replace_channel`).
    let old = app.plugin_copy(row);
    crate::history::replace_channel(&mut app.state, row, old, (!sampler).then_some(index));
    app.plugins.drop_key(row);
    app.state.hosted_rows.retain(|&r| r != row);
    app.transport.set_instruments(app.plugins.instruments());
    let s = &mut app.state;
    let ch = &mut s.project.channels[row];
    ch.plugin_state = None;
    if sampler {
        ch.kind = os_model::ChannelKind::Sampler;
        ch.plugin = None;
        ch.clear_sample();
        ch.name = Some(d.name.clone());
        s.pending_open = Some(row);
    } else {
        ch.kind = os_model::ChannelKind::Plugin;
        ch.plugin = Some(crate::osc3::file_name(&d.name).to_string());
        ch.name = Some(d.name.clone());
        // A Lemon Trio channel's sample is its custom shape's (spec: plugins/lemon-3x-osc.md §6): the new instance has
        // none, so neither does the channel.
        if crate::osc3::is_osc3(ch) {
            ch.sample_path = None;
        }
        s.pending_plugin = Some((row, index));
    }
    s.piano.dirty = true;
    s.selected_channel = row;
    s.rack.select_only(row);
    Some(format!("Replaced with {}", d.name))
}

/// Effect `index` into the selected mixer track (spec §1): slot `slot`, else the first empty one.
pub fn add_effect(app: &mut App, index: usize, slot: Option<u8>) -> Option<String> {
    let s = &app.state;
    let t = s.selected_mixer;
    let slot = slot.or_else(|| s.project.mixer.iter().find(|m| m.index == t).and_then(|m| m.slots.iter().position(Option::is_none)).map(|k| k as u8));
    match slot {
        Some(k) => {
            if let Some(a) = app.plugins.effects.get(index) {
                let uid = a.desc.uid();
                note_recent(app, &uid);
            }
            crate::mixer::run(app, crate::mixer::MCmd::AddEffect(k, index))
        }
        None => Some("Every slot of this track is in use".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::osc3::LEMON_TRIO;
    use crate::playlist::Cmd;

    /// Insert instrument ▸ on a track header, a generator dropped on the track (issue #2): the pick adds the
    /// instrument and places a clip of a new pattern named after it on the track, at the tick given; one undo step
    /// takes the clip, the name and the channel back, a redo brings all three (and the hosting) up again.
    #[test]
    fn an_instrument_loaded_onto_a_track_places_its_clip() {
        let mut a = App::new(None);
        a.state.project = crate::app::test_project();
        a.plugins.available = crate::plugins::builtin();
        a.state.plugdb.rebuild(&a.plugins);
        let index = a.plugins.index_of(LEMON_TRIO).unwrap();
        let n = a.state.project.channels.len();
        let (track, tick) = (2, 384);
        let cmd = Cmd::Plugin(PlugCmd::GeneratorOnTrack { index, track, tick });
        let m = crate::playlist::run(&mut a, cmd).expect("a status message");
        assert_eq!(m, format!("Added {LEMON_TRIO} on track 3"));
        assert_eq!(a.state.project.channels.len(), n + 1);
        assert_eq!(a.state.project.channels[n].name.as_deref(), Some(LEMON_TRIO));
        assert_eq!(a.state.pending_plugin, Some((n, index)), "the instrument is asked to host");
        assert_eq!(a.state.selected_channel, n);
        assert_eq!(a.state.pattern, 1, "the project's first empty pattern takes the name");
        assert_eq!(crate::patterns::name_of(&a.state, 1), LEMON_TRIO);
        assert_eq!(a.state.playlist.source, Some(os_model::ClipSource::Pattern(1)));
        let last = *crate::playlist::clips(&a.state).last().expect("the clip");
        assert_eq!((last.track, last.position, last.source), (track, tick, os_model::ClipSource::Pattern(1)));
        assert_eq!(a.state.playlist.sel, vec![crate::playlist::clips(&a.state).len() - 1]);
        // One step back: the clip, the name and the channel (with its hosting) go.
        assert_eq!(crate::history::undo(&mut a.state), Some(crate::history::ADD_TO_TRACK));
        assert_eq!(a.state.project.channels.len(), n);
        assert!(a.state.pending_plugin.is_none());
        assert!(crate::playlist::clips(&a.state).iter().all(|c| c.source != os_model::ClipSource::Pattern(1)));
        assert_eq!(crate::patterns::name_of(&a.state, 1), "Pattern 1");
        // And forward again: clip, name, channel and its hosting come back.
        assert_eq!(crate::history::redo(&mut a.state), Some(crate::history::ADD_TO_TRACK));
        assert_eq!(a.state.project.channels.len(), n + 1);
        assert_eq!(a.state.pending_plugin, Some((n, index)));
        let last = *crate::playlist::clips(&a.state).last().expect("the clip");
        assert_eq!((last.track, last.position, last.source), (track, tick, os_model::ClipSource::Pattern(1)));
        assert_eq!(crate::patterns::name_of(&a.state, 1), LEMON_TRIO);
    }

    /// The same twice on one track: the second pattern takes the instrument's name with a " #2", as Clone's does.
    #[test]
    fn a_second_instrument_on_a_track_names_its_pattern_uniquely() {
        let mut a = App::new(None);
        a.state.project = crate::app::test_project();
        a.plugins.available = crate::plugins::builtin();
        a.state.plugdb.rebuild(&a.plugins);
        let index = a.plugins.index_of(LEMON_TRIO).unwrap();
        crate::playlist::run(&mut a, Cmd::Plugin(PlugCmd::GeneratorOnTrack { index, track: 0, tick: 0 })).unwrap();
        a.state.pending_plugin = None; // the windows are noise here
        crate::playlist::run(&mut a, Cmd::Plugin(PlugCmd::GeneratorOnTrack { index, track: 3, tick: 96 })).unwrap();
        assert_eq!(crate::patterns::name_of(&a.state, 2), format!("{LEMON_TRIO} #2"));
        let last = *crate::playlist::clips(&a.state).last().unwrap();
        assert_eq!((last.track, last.position, last.source), (3, 96, os_model::ClipSource::Pattern(2)));
    }

    /// The track header menu's Insert instrument ▸ lists the generators as on-track loads for the track it was
    /// opened on, at the tick it was opened with (issue #2).
    #[test]
    fn the_track_menu_lists_the_instruments_for_its_track() {
        let mut a = App::new(None);
        a.state.project = crate::app::test_project();
        a.plugins.available = crate::plugins::builtin();
        a.state.plugdb.rebuild(&a.plugins);
        let index = a.plugins.index_of(LEMON_TRIO).unwrap();
        let p = crate::playlist::track_menu(&a.state, 3, 100.0, 200.0, 480);
        let ins = p.items.iter().find(|i| i.label == "Insert instrument").expect("the submenu");
        let pick = ins.children.iter().find(|i| i.label == LEMON_TRIO).expect("the generator");
        assert_eq!(pick.cmd, Cmd::Plugin(PlugCmd::GeneratorOnTrack { index, track: 3, tick: 480 }));
    }

    /// The submenu's items come from the same database view the rack's Add one ▸ shows: favourites first, then the
    /// rest by the display mode.
    #[test]
    fn on_track_items_match_the_racks_menu() {
        let mut a = App::new(None);
        a.state.project = crate::app::test_project();
        a.plugins.available = crate::plugins::builtin();
        a.state.plugdb.rebuild(&a.plugins);
        let rack = crate::rack::add_one_items(&a.state).into_iter().filter(|i| !i.head && !i.sep && i.children.is_empty()).map(|i| i.label).collect::<Vec<_>>();
        let on = items_on_track(&a.state, 0, 0, true).into_iter().filter(|i| !i.head && !i.sep && i.children.is_empty()).map(|i| i.label).collect::<Vec<_>>();
        assert_eq!(on, rack);
    }

    /// A submenu (Replace ▸, ADD ▸ Effect) lists each family once under its category's header, favourites and the
    /// plugins added last above them; it has no submenus of its own.
    #[test]
    fn submenus_list_families_by_category() {
        let mut app = App::new(None);
        app.plugins.available.clear();
        app.plugins.effects = ["CLA-76 Mono", "CLA-76 Stereo", "Echo Thing"].iter().map(|n| db::fake(n, os_host::Kind::Effect)).collect();
        app.state.plugdb.prefs.toggle_favorite(&app.plugins.effects[2].desc.uid());
        app.state.plugdb.prefs.note_recent(&app.plugins.effects[0].desc.uid());
        app.state.plugdb.rebuild(&app.plugins);
        let v = items(&app.state, Target::Fx(Some(2)), true);
        assert!(v.iter().all(|it| it.children.is_empty()));
        let heads: Vec<&str> = v.iter().filter(|it| it.head).map(|it| it.label.as_ref()).collect();
        assert_eq!(heads, ["Favorites", "Recent", "Other"]);
        let names: Vec<&str> = v.iter().filter(|it| matches!(it.cmd, Cmd::Plugin(PlugCmd::Effect { .. }))).map(|it| it.label.as_ref()).collect();
        assert_eq!(names, ["Echo Thing", "CLA-76", "CLA-76", "Echo Thing"]);
        assert!(v.iter().any(|it| it.cmd == Cmd::Plugin(PlugCmd::Effect { index: 1, slot: Some(2) })), "CLA-76 adds its stereo variant");
    }
}
