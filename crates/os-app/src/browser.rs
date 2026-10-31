// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 ravenmarkdev-alt
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Browser behaviour: tabs, tree open/close, selection, scrolling and drag and drop into the project, and the menu
//! bar's ways in.
// spec: ui/browser.md, ui/browser-commands.md

use os_model::{ChannelKind, ClipSource};
use os_ui::Rect;

use crate::app::{Action, App, Drag, Win};
use crate::playlist::{Cmd, Mark, PItem, Popup};
use crate::playlist::{self as pl, PickKind};
use crate::views::browser::{self as view, Item};

/// Mouse travel before a press on an item becomes a drag (spec §3).
const DRAG_START: f32 = 4.0;
/// How long a clicked sample plays (spec §7).
const PREVIEW_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

impl App {
    /// Browser actions; returns whether `a` was one.
    pub(crate) fn browser_act(&mut self, a: &Action) -> bool {
        if *a == Action::BrowserSplitter {
            self.browser_split_press();
            return true;
        }
        let s = &mut self.state;
        match *a {
            Action::BrowserTab(i) => {
                self.browser_click = None;
                s.browser_tab = i;
                s.browser_scroll = 0.0;
            }
            Action::BrowserCollapse => {
                let tab = format!("{}:", s.browser_tab);
                s.browser_open.retain(|k| !k.starts_with(&tab));
                s.browser_scroll = 0.0;
            }
            Action::BrowserReread => reread(s),
            Action::BrowserList => {}
            Action::BrowserMenu => {
                let (x, y) = view::menu_at(s);
                let open = s.playlist.popup.as_ref().is_some_and(|p| p.x == x && p.y == y);
                s.playlist.popup = if open { None } else { Some(Popup { x, y, items: options_menu(s), sub: None, scroll: [0; 2] }) };
            }
            Action::BrowserSearch => s.browser_search_focus = true,
            Action::BrowserPlugFilter(i) => {
                s.browser_plug_filter = i;
                s.browser_plug_cat = None;
                s.browser_scroll = 0.0;
            }
            Action::BrowserPlugCat(c) => {
                s.browser_plug_cat = c;
                s.browser_scroll = 0.0;
            }
            Action::BrowserPlugStar(i) => {
                let Some(f) = crate::views::plugin_panel::list(s).into_iter().nth(i) else { return true };
                crate::plugin_menu::run(self, crate::plugin_menu::PlugCmd::Star { effect: f.effect, index: f.entry().index });
            }
            Action::BrowserRow(i) => {
                let Some(row) = view::rows(s).into_iter().nth(i) else { return true };
                let project = project_path(&row.item);
                let now = std::time::Instant::now();
                let double = project.as_ref().is_some_and(|path| {
                    self.browser_click.as_ref().is_some_and(|(last, time)| last == path && now.duration_since(*time).as_millis() < 400)
                });
                self.browser_click = if double { None } else { project.map(|path| (path, now)) };
                select(s, &row.key);
                // The plugin browser: a second press on the same plugin adds it, as its row menu's Add does.
                if s.browser_tab == view::PLUGIN_TAB {
                    let now = std::time::Instant::now();
                    let double = s.browser_plug_click.as_ref().is_some_and(|(k, t)| *k == row.key && now.duration_since(*t).as_millis() < 400);
                    s.browser_plug_click = if double { None } else { Some((row.key.clone(), now)) };
                    if double {
                        let m = match row.item {
                            Item::Generator(i) => crate::plugin_menu::add_generator(self, i, None),
                            Item::Effect(i) => crate::plugin_menu::add_effect(self, i, None),
                            _ => None,
                        };
                        if m.is_some() {
                            self.state.message = m;
                        }
                        self.invalidate();
                        return true;
                    }
                }
                if row.open.is_some() && !s.browser_open.remove(&row.key) {
                    s.browser_open.insert(row.key.clone());
                }
                match row.item {
                    Item::Pattern(n) => {
                        s.pattern = n;
                    }
                    Item::Channel(c) => {
                        s.selected_channel = c;
                        s.rack.select_only(c);
                    }
                    Item::History(e) => s.message = crate::history::go_to(s, e),
                    _ => {}
                }
                if double {
                    self.drop_file(project_path(&row.item).unwrap());
                    return true;
                }
                if let Item::Sample(path) = &row.item {
                    self.preview_sample(path);
                }
                let s = &mut self.state;
                s.browser_scroll = s.browser_scroll.min(view::max_scroll(s));
                if !matches!(row.item, Item::None | Item::File(_) | Item::History(_)) {
                    self.drag = Some(Drag::Browser { item: row.item, label: row.label, m0: self.mouse, started: false });
                }
            }
            _ => return false,
        }
        self.invalidate();
        true
    }

    /// Clicking a sample plays it once (spec §7): cut off after `PREVIEW_LIMIT` unless "Full sample preview" is ticked.
    pub(crate) fn preview_sample(&self, path: &str) {
        let mut p = crate::app::preview_project(path);
        p.tempo = Some(self.state.tempo);
        crate::preview_track::route(&mut p, &self.state.project, crate::prefs::number(&crate::prefs::BROWSER_PREVIEW_TRACK));
        let limit = (!view::opt(&self.state, view::OPT_PREVIEW)).then_some(PREVIEW_LIMIT);
        self.transport.preview_for(p, limit);
    }

    /// A browser options-menu command (spec §4); returns a message.
    pub(crate) fn browser_cmd(&mut self, c: BCmd) -> Option<String> {
        self.invalidate();
        let s = &mut self.state;
        match c {
            BCmd::Find(dir) => return find(s, dir),
            BCmd::Toggle(k) => {
                if !s.browser_opts.remove(k) {
                    s.browser_opts.insert(k);
                }
                s.browser_scroll = s.browser_scroll.min(view::max_scroll(s));
            }
            BCmd::History(dir) => {
                let pos = s.browser_hist_pos as isize + dir;
                let Some(key) = usize::try_from(pos).ok().and_then(|p| s.browser_hist.get(p)).cloned() else {
                    return Some("No more browser history".into());
                };
                s.browser_hist_pos = pos as usize;
                s.browser_sel = Some(key);
                reveal(s);
            }
            BCmd::Send(dir) => return send(s, dir),
            BCmd::Collapse => {
                let tab = format!("{}:", s.browser_tab);
                s.browser_open.retain(|k| !k.starts_with(&tab));
                s.browser_scroll = 0.0;
            }
            BCmd::Reread => reread(s),
            // A background scan (spec: ui/plugin-manager.md §4); plugins in use stay where they are.
            BCmd::Rescan => self.start_scan(crate::plugin_db::Mode::Changed),
            BCmd::Row(c) => return self.browser_row_cmd(c),
        }
        None
    }

    /// A way into the browser from the menu bar or its keys (spec: ui/browser-commands.md): the browser is shown first
    /// when hidden.
    pub(crate) fn browser_jump(&mut self, j: Jump) {
        self.state.show_browser = true;
        let s = &mut self.state;
        match j {
            // §1: the Current project tab with that section alone open, and selected. The search is cleared so that it
            // shows (as Browse parameters does).
            Jump::Section(name) => {
                let Some(key) = view::section_key(name) else { return };
                if s.browser_tab != view::PROJECT_TAB {
                    s.browser_tab = view::PROJECT_TAB;
                    s.browser_scroll = 0.0;
                }
                s.browser_search.clear();
                for other in view::sections().filter(|&l| l != name).filter_map(view::section_key) {
                    s.browser_open.remove(&other);
                }
                s.browser_open.insert(view::project_root());
                s.browser_open.insert(key.clone());
                select(s, &key);
                reveal(s);
            }
            // §1-2: the tab as it was left, its open nodes and selection kept.
            Jump::PluginDb => {
                if s.browser_tab != view::PLUGIN_TAB {
                    s.browser_tab = view::PLUGIN_TAB;
                    s.browser_scroll = 0.0;
                }
            }
            // §3: exactly as a click in the search field.
            Jump::Find => {
                self.browser_act(&Action::BrowserSearch);
            }
            // §4-5: Patterns opened, the current pattern's row selected when listed (else the section's).
            Jump::Patterns => {
                let Some(key) = view::section_key("Patterns") else { return };
                if s.browser_tab != view::PROJECT_TAB {
                    s.browser_tab = view::PROJECT_TAB;
                    s.browser_scroll = 0.0;
                }
                s.browser_search.clear();
                s.browser_open.insert(view::project_root());
                s.browser_open.insert(key.clone());
                let cur = view::Item::Pattern(s.pattern);
                let row = view::rows(s).into_iter().find(|r| r.item == cur).map(|r| r.key);
                select(s, row.as_ref().unwrap_or(&key));
                reveal(s);
            }
        }
        self.invalidate();
    }

    /// Whether the mouse is over the docked browser.
    pub(crate) fn over_browser(&self) -> bool {
        let (x, y) = self.mouse;
        self.state.show_browser && x < crate::views::browser_w(&self.state) && y >= crate::views::work_y(&self.state)
    }

    /// Whether typed letters belong to the browser search (the typing keyboard leaves them alone).
    pub(crate) fn browser_takes_typing(&self) -> bool {
        self.state.browser_search_focus || (view::opt(&self.state, view::OPT_TYPE) && self.over_browser())
    }

    /// Browser keys (spec §4, §5); returns whether the key was used.
    pub(crate) fn browser_key(&mut self, key: &winit::keyboard::Key, ctrl: bool, alt: bool) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let shift = self.shift;
        let focus = self.state.browser_search_focus;
        let over = self.over_browser() || focus;
        if over && !focus && !ctrl && !alt && matches!(key, Key::Named(NamedKey::Enter)) {
            let project = view::rows(&self.state).into_iter()
                .find(|row| self.state.browser_sel.as_ref() == Some(&row.key))
                .and_then(|row| project_path(&row.item));
            if let Some(path) = project {
                self.browser_click = None;
                self.drop_file(path);
                return true;
            }
        }
        let cmd = match key {
            Key::Named(NamedKey::F2) if !self.state.browser_search.is_empty() => Some(BCmd::Find(-1)),
            Key::Named(NamedKey::F3) if !self.state.browser_search.is_empty() => Some(BCmd::Find(1)),
            Key::Named(NamedKey::ArrowUp) if over && ctrl => Some(BCmd::Collapse),
            Key::Named(NamedKey::ArrowUp) if over && shift => Some(BCmd::Send(-1)),
            Key::Named(NamedKey::ArrowDown) if over && shift => Some(BCmd::Send(1)),
            Key::Character(c) if over && ctrl && c.eq_ignore_ascii_case("r") => Some(BCmd::Reread),
            _ => None,
        };
        if let Some(c) = cmd {
            if let Some(m) = self.browser_cmd(c) {
                self.state.message = Some(m);
            }
            return true;
        }
        // The arrow keys, as FL's browser has them (spec §7): Down / Up walk the visible rows, wrapping, and the
        // sample a move lands on is heard at once; Right replays the focused sample from its start, opens a closed
        // folder or steps into its first row; Left closes an opened folder or selects the parent. From the search
        // field Up / Down still walk the rows (the results); Left / Right stay with the field. The older Shift and
        // Ctrl arrows above keep their meanings, and an open menu or popup takes its keys first.
        if over && !ctrl && !alt && !shift && self.state.playlist.popup.is_none() && self.state.menu_open.is_none() {
            let arrow = matches!(key, Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown | NamedKey::ArrowLeft | NamedKey::ArrowRight));
            if arrow && (!focus || matches!(key, Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown))) {
                let s = &mut self.state;
                let sample = match key {
                    Key::Named(NamedKey::ArrowDown) => move_sel(s, 1),
                    Key::Named(NamedKey::ArrowUp) => move_sel(s, -1),
                    Key::Named(NamedKey::ArrowRight) => into_sel(s),
                    _ => {
                        out_sel(s);
                        None
                    }
                };
                if let Some(p) = sample {
                    self.preview_sample(&p);
                }
                self.invalidate();
                return true;
            }
        }
        let s = &mut self.state;
        if focus {
            match key {
                Key::Named(NamedKey::Escape) => {
                    s.browser_search.clear();
                    s.browser_search_focus = false;
                }
                Key::Named(NamedKey::Enter) => {
                    if let Some(m) = find(s, 1) {
                        s.message = Some(m);
                    }
                }
                Key::Named(NamedKey::Backspace) => {
                    s.browser_search.pop();
                }
                Key::Named(NamedKey::Space) => s.browser_search.push(' '),
                Key::Character(c) if !ctrl && !alt => s.browser_search.push_str(c),
                _ => return false,
            }
            s.browser_scroll = 0.0;
            self.invalidate();
            return true;
        }
        // "Type to filter": letters typed over the browser start a search.
        if let Key::Character(c) = key {
            if !ctrl && !alt && view::opt(s, view::OPT_TYPE) && self.over_browser() {
                let s = &mut self.state;
                s.browser_search_focus = true;
                s.browser_search.push_str(c);
                s.browser_scroll = 0.0;
                self.invalidate();
                return true;
            }
        }
        false
    }

    /// A press on the browser's splitter: drags it; a double click gives the browser its default width (spec §9).
    fn browser_split_press(&mut self) {
        let now = std::time::Instant::now();
        let s = &mut self.state;
        let double = s.browser_split_click.is_some_and(|t| now.duration_since(t).as_millis() < 400);
        s.browser_split_click = if double { None } else { Some(now) };
        if double {
            s.browser_w = crate::views::BROWSER_W;
            save_width(s.browser_w);
            crate::win_clamp::keep_all(s);
            return self.invalidate();
        }
        self.drag = Some(Drag::BrowserSplit { dx: self.mouse.0 - crate::views::browser_w(&self.state) });
    }

    /// The splitter dragged so that the browser (its splitter included) ends at `right`: whole px, from its default width
    /// to half the window. Everything right of it moves, so the whole frame is repainted.
    pub(crate) fn browser_split_drag(&mut self, right: f32) {
        let s = &mut self.state;
        let w = right.min(s.screen.0 / 2.0).max(crate::views::BROWSER_W).round();
        if w != s.browser_w {
            s.browser_w = w;
            crate::win_clamp::keep_all(s);
            self.invalidate();
        }
    }

    /// A hovered row's scrolling label (spec §9): repainted whenever its offset moves; the scroll ends with the hover.
    pub(crate) fn marquee_tick(&mut self, now: std::time::Instant) {
        let hovered = self.state.show_browser && matches!(self.hover, Some(Action::BrowserRow(_)));
        let m = self.state.browser_marquee.get_mut();
        let Some(mq) = m.as_ref() else { return };
        if !hovered {
            *m = None;
            return;
        }
        let r = mq.rect;
        if view::marquee_offset(now.duration_since(mq.since).as_secs_f32(), mq.over) != mq.off {
            self.damage_rect(r);
        }
    }

    /// Wheel over the browser list.
    pub(crate) fn browser_wheel(&mut self, dy: f32) {
        let s = &mut self.state;
        s.browser_scroll = (s.browser_scroll - dy * 3.0 * view::ROW_H).clamp(0.0, view::max_scroll(s));
        self.invalidate();
    }

    /// Mouse moved while an item is pressed: start the drag, move the label.
    pub(crate) fn browser_drag_move(&mut self, label: String, m0: (f32, f32), started: bool, item: Item) {
        let (x, y) = self.mouse;
        if !started && (x - m0.0).hypot(y - m0.1) < DRAG_START {
            return;
        }
        // The playlist's drop-zone ghost (issue #4): damaged where it was (the drag's last place) and where it goes,
        // so the moving preview leaves no trail.
        let grid = self.pl_grid();
        let old_zone = self.state.browser_drag.as_ref().and_then(|d| zone_rect(&self.state, grid, d.x, d.y));
        // The label too: redrawn where it was, or each old copy stays on screen as a trail.
        let old_ghost = self.state.browser_drag.as_ref().map(|d| view::ghost_rect(d.x, d.y));
        if !started {
            self.drag = Some(Drag::Browser { item: item.clone(), label: label.clone(), m0, started: true });
        }
        self.state.browser_drag = Some(view::BrowserDrag { item: item.clone(), label, x, y, alt: self.alt });
        for r in old_zone.into_iter().chain(zone_rect(&self.state, grid, x, y)) {
            self.damage_rect(r.outset(1.0));
        }
        for r in old_ghost.into_iter().chain([view::ghost_rect(x, y)]) {
            self.damage_rect(r.outset(1.0));
        }
    }
}

/// The drop-zone ghost's rect over the playlist's grid, when the drag at (x, y) would land one (issue #4): None off
/// the grid, while the playlist is not shown, or where no clip would be placed.
fn zone_rect(s: &crate::app::State, grid: Option<Rect>, x: f32, y: f32) -> Option<Rect> {
    let grid = grid.filter(|g| g.contains(x, y) && s.shown(Win::Playlist))?;
    crate::views::playlist::drop_ghost(s, grid, x, y).map(|dg| dg.rect)
}

impl App {
    /// Release: drop the dragged item on whatever is under the mouse (spec §3).
    pub(crate) fn browser_drop(&mut self, item: Item, started: bool) {
        self.state.browser_drag = None;
        self.invalidate();
        if !started {
            return;
        }
        // A preset goes into the plugin under the mouse, or a new instance of its plugin (spec: plugins/presets.md).
        if let Item::Preset(path) = &item {
            self.state.message = crate::presets::dropped(self, std::path::Path::new(path));
            return;
        }
        // Plugin windows float above the main window: a sample released over one goes into that plugin.
        #[cfg(any(windows, target_os = "macos"))]
        if let Item::Sample(path) = &item {
            if self.sample_to_plugin_window(path) {
                return;
            }
        }
        let (x, y) = self.mouse;
        let win = self.top_hit(x, y).and_then(|h| h.win);
        // A drop on a track header (issue #2): the track it names, and where on it the instrument lands — the song
        // position, snapped like a drop on the rows (a header has no place of its own).
        let head = self.top_hit(x, y).and_then(|h| match h.action { Action::PlHeader(t) => Some(t), _ => None });
        let grid = self.pl_grid().filter(|g| g.contains(x, y) && win == Some(Win::Playlist));
        let alt = self.alt;
        let on_header = head.filter(|_| grid.is_none()).map(|track| {
            (track, self.pl_grid().map_or(0, |grid| {
                let g = pl::PGeom::new(&self.state, grid);
                pl::snap_place(&self.state, &g, self.state.playlist.pos as f32, alt)
            }))
        });
        let s = &mut self.state;
        // Snapped playlist position under the mouse.
        let spot = grid.map(|grid| {
            let g = pl::PGeom::new(s, grid);
            (g.track_at(y), pl::snap_place(s, &g, g.tick_at(x), alt))
        });
        match item {
            Item::Pattern(n) => {
                if let Some((track, tick)) = spot {
                    pl::snapshot(s);
                    s.playlist.source = Some(ClipSource::Pattern(n));
                    s.playlist.picker = PickKind::Pattern;
                    let i = pl::place(s, track, tick);
                    s.playlist.sel = vec![i];
                } else if matches!(win, Some(Win::PianoRoll | Win::ChannelRack)) {
                    s.pattern = n;
                }
            }
            Item::Channel(c) => {
                let Some(ch) = s.project.channels.get(c) else { return };
                let (kind, index) = (ch.kind, ch.index);
                if let Some((track, tick)) = spot {
                    if matches!(kind, ChannelKind::AudioClip | ChannelKind::Automation) {
                        pl::snapshot(s);
                        s.playlist.source = Some(ClipSource::Channel(index));
                        s.playlist.picker = if kind == ChannelKind::AudioClip { PickKind::Audio } else { PickKind::Automation };
                        let i = pl::place(s, track, tick);
                        s.playlist.sel = vec![i];
                    } else {
                        s.message = Some("Only audio and automation channels go on the playlist; drop a pattern instead".into());
                    }
                } else if matches!(win, Some(Win::PianoRoll | Win::ChannelRack)) {
                    s.selected_channel = c;
                    s.rack.select_only(c);
                }
            }
            Item::Sample(path) => {
                if let Some((track, tick)) = spot {
                    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
                    s.message = Some(match pl::add_audio_clip(s, &path, track, tick) {
                        Some(_) => format!("Added audio clip {name}"),
                        None => format!("Could not read {name} as audio"),
                    });
                } else if win == Some(Win::ChannelRack) {
                    self.sample_to_rack(path, y);
                }
            }
            // On the playlist — a track row or its header (issue #2) — the instrument lands there: the channel as
            // the F8 menu adds it, plus a clip of a new pattern named after it on that track, at the drop (on a
            // header, at the song position). On the rack: a new channel at the bottom (spec: ui/plugin-menu.md §5).
            Item::Generator(i) => {
                if let Some((track, tick)) = spot.or(on_header) {
                    if let Some(m) = crate::plugin_menu::add_generator_on_track(self, i, track, tick) {
                        self.state.message = Some(m);
                    }
                } else if win == Some(Win::ChannelRack) {
                    if let Some(m) = crate::plugin_menu::add_generator(self, i, None) {
                        self.state.message = Some(m);
                    }
                }
            }
            // An effect dropped on the mixer goes into the slot under the mouse, else the current track's first free
            // slot (spec §3).
            Item::Effect(i) => {
                if win == Some(Win::Mixer) {
                    let slot = match self.top_hit(x, y).map(|h| &h.action) {
                        Some(Action::Mix(crate::mixer::MHit::SlotName(k) | crate::mixer::MHit::SlotArrow(k))) => Some(*k),
                        _ => None,
                    };
                    if let Some(m) = crate::plugin_menu::add_effect(self, i, slot) {
                        self.state.message = Some(m);
                    }
                }
            }
            Item::Param(key, index) => {
                if let Some((track, tick)) = spot {
                    let msg = self.params_drop(key, index, track, tick);
                    self.state.message = msg;
                }
            }
            Item::None | Item::File(_) | Item::History(_) | Item::Preset(_) => {}
        }
    }

    /// A folder dropped on the window becomes an All-tab root, opened and selected (spec §6).
    pub(crate) fn add_browser_folder(&mut self, path: std::path::PathBuf) {
        let s = &mut self.state;
        if !s.browser_folders.contains(&path) {
            s.browser_folders.push(path.clone());
            save_folders(s);
        }
        s.browser_tab = 0;
        s.browser_search.clear();
        let key = view::disk_key(&path);
        s.browser_open.insert(key.clone());
        select(s, &key);
        reveal(s);
        s.message = Some(format!("Browser: added {}", path.display()));
        self.invalidate();
    }

    /// Right click on row `i` (spec §8): selects it and opens its menu at the mouse; returns whether there was a row.
    pub(crate) fn browser_row_menu(&mut self, i: usize, x: f32, y: f32) -> bool {
        let s = &mut self.state;
        let Some(row) = view::rows(s).into_iter().nth(i) else { return false };
        select(s, &row.key);
        s.playlist.popup = Some(Popup { x, y, items: row_menu(s, &row), sub: None, scroll: [0; 2] });
        self.invalidate();
        true
    }

    /// Middle click on row `i` (spec §7): selects it, and a sample goes into the selected channel as the row menu's
    /// "Send to selected channel" sends it (§8). Returns whether there was a row.
    pub(crate) fn browser_row_middle(&mut self, i: usize) -> bool {
        let s = &mut self.state;
        let Some(row) = view::rows(s).into_iter().nth(i) else { return false };
        select(s, &row.key);
        if let Item::Sample(path) = &row.item {
            s.message = send_sample(s, path, &row.label);
        }
        self.invalidate();
        true
    }

    /// A row menu command on the selected row (spec §8); returns a message.
    fn browser_row_cmd(&mut self, c: RowCmd) -> Option<String> {
        let s = &mut self.state;
        let row = view::rows(s).into_iter().find(|r| s.browser_sel.as_ref() == Some(&r.key))?;
        let sample = match &row.item {
            Item::Sample(p) => Some(p.clone()),
            _ => None,
        };
        match c {
            RowCmd::Toggle => {
                if !s.browser_open.remove(&row.key) {
                    s.browser_open.insert(row.key);
                }
                s.browser_scroll = s.browser_scroll.min(view::max_scroll(s));
            }
            // What a click on the row does, without the click's open / close.
            RowCmd::Activate => match row.item {
                Item::Pattern(n) => s.pattern = n,
                Item::Channel(c) => {
                    s.selected_channel = c;
                    s.rack.select_only(c);
                }
                Item::History(e) => return crate::history::go_to(s, e),
                _ => {}
            },
            RowCmd::Pattern(pc) => {
                let Item::Pattern(n) = row.item else { return None };
                s.pattern = n;
                return self.pattern_cmd(pc);
            }
            RowCmd::ChannelWindow => {
                let Item::Channel(i) = row.item else { return None };
                if s.hosted_rows.contains(&i) {
                    s.pending_open = Some(i);
                } else {
                    self.open_channel_window(i);
                }
            }
            RowCmd::Preview => self.preview_sample(&sample?),
            RowCmd::Send => return send_sample(s, &sample?, &row.label),
            RowCmd::NewChannel => self.sample_to_channel(sample?, None),
            RowCmd::OpenFolder => open_folder(&row.path?),
            RowCmd::ShowInFolder => crate::theme_library::show_in_folder(&row.path?),
            // File ▸ Browser extra search folders, the root's Browser name selected for typing.
            RowCmd::Rename => {
                let i = s.browser_folders.iter().position(|f| Some(f) == row.path.as_ref())?;
                self.open_settings(crate::views::settings::FILE);
                self.search_folder_cell(i, crate::search_folders::Col::Name);
            }
            RowCmd::Remove => return Some(remove_root(s, &row.path?)),
        }
        None
    }
}

/// A dropped folder root out of the browser (spec §6, §8), with its Browser name and its open nodes.
fn remove_root(s: &mut crate::app::State, path: &std::path::Path) -> String {
    let key = view::disk_key(path);
    s.browser_folders.retain(|f| f != path);
    s.browser_names.remove(path);
    save_folders(s);
    let below = format!("{key}/");
    s.browser_open.retain(|k| *k != key && !k.starts_with(&below));
    s.browser_scroll = s.browser_scroll.min(view::max_scroll(s));
    format!("Removed {} from the browser", path.display())
}

/// Opens folder `dir` in the system's file manager (tests: nothing).
pub(crate) fn open_folder(dir: &std::path::Path) {
    if cfg!(test) {
        return;
    }
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(dir).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
}

/// `browser.txt` in the user folder: `width=` the browser's width in px (anything else, or missing, = the default).
fn width_file() -> Option<std::path::PathBuf> {
    if cfg!(test) {
        return None;
    }
    os_paths::user_file("browser.txt")
}

fn parse_width(text: &str) -> f32 {
    let w = text.lines().filter_map(|l| l.split_once('=')).find(|(k, _)| k.trim() == "width").and_then(|(_, v)| v.trim().parse::<f32>().ok());
    w.filter(|w| w.is_finite()).map_or(crate::views::BROWSER_W, |w| w.max(crate::views::BROWSER_W))
}

/// The browser's width kept from the last run (spec §9; tests: the default).
pub(crate) fn load_width() -> f32 {
    width_file().and_then(|p| std::fs::read_to_string(p).ok()).map_or(crate::views::BROWSER_W, |t| parse_width(&t))
}

/// Keeps the browser's width for the next run.
pub(crate) fn save_width(w: f32) {
    let Some(p) = width_file() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, format!("width={w}\n"));
}

/// `browser_folders.txt` in the user folder (`os_paths::user_dir`): one dropped folder per line (spec §6), its Browser
/// name after a tab when it has one (`crate::search_folders::file_text`).
fn folders_file() -> Option<std::path::PathBuf> {
    if cfg!(test) {
        return None;
    }
    os_paths::user_file("browser_folders.txt")
}

fn read_folders() -> Vec<(std::path::PathBuf, String)> {
    let Some(file) = folders_file() else { return Vec::new() };
    match std::fs::read_to_string(file) {
        Ok(text) => crate::search_folders::parse_file(&text),
        // No list kept yet: FL Studio's Projects folder when there is one. A kept list, even an empty one, wins.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            initial_folders(None, documents_dir().as_deref()).into_iter().map(|f| (f, String::new())).collect()
        }
        Err(_) => Vec::new(),
    }
}

fn project_path(item: &Item) -> Option<std::path::PathBuf> {
    let Item::File(path) = item else { return None };
    let path = std::path::PathBuf::from(path);
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("flp")).then_some(path)
}

fn initial_folders(saved: Option<&str>, documents: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
    if let Some(text) = saved {
        return text.lines().map(str::trim).filter(|l| !l.is_empty()).map(std::path::PathBuf::from).collect();
    }
    documents.map(|dir| dir.join("Image-Line").join("FL Studio").join("Projects"))
        .filter(|path| path.is_dir()).into_iter().collect()
}

/// Ask Windows for Documents so OneDrive and other redirected folders work.
#[cfg(windows)]
fn documents_dir() -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows_sys::Win32::UI::Shell::{SHGetKnownFolderPath, FOLDERID_Documents};
    unsafe {
        let initialized = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let mut path = std::ptr::null_mut();
        let result = SHGetKnownFolderPath(&FOLDERID_Documents, 0, std::ptr::null_mut(), &mut path);
        let folder = if result >= 0 && !path.is_null() {
            let mut length = 0;
            while *path.add(length) != 0 {
                length += 1;
            }
            Some(std::path::PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(path, length))))
        } else {
            None
        };
        CoTaskMemFree(path.cast());
        if initialized >= 0 {
            CoUninitialize();
        }
        folder
    }
}

#[cfg(not(windows))]
fn documents_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join("Documents"))
}

pub(crate) fn load_folders() -> Vec<std::path::PathBuf> {
    read_folders().into_iter().map(|f| f.0).collect()
}

/// The dropped folders' Browser names.
pub(crate) fn load_names() -> std::collections::BTreeMap<std::path::PathBuf, String> {
    read_folders().into_iter().filter(|f| !f.1.is_empty()).collect()
}

/// Keeps the dropped folders and their names.
pub(crate) fn save_folders(s: &crate::app::State) {
    let Some(p) = folders_file() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, crate::search_folders::file_text(&s.browser_folders, &s.browser_names));
}

/// The menu bar's ways into the browser (spec: ui/browser-commands.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jump {
    /// VIEW's Browser section: this section of the Current project tree (§1).
    Section(&'static str),
    /// VIEW > Plugin database (Shift+F8), ADD > Browse plugin database: the Plugin database tab (§1, §2).
    PluginDb,
    /// TOOLS > Browser find... (Ctrl+F): the keyboard goes to the search field (§3).
    Find,
    /// PATTERNS > Open in project browser (Shift+Ctrl+P): the Patterns section, the other sections left as they are (§4).
    Patterns,
}

impl Jump {
    /// The jump behind entry `label` of menu `menu`. `None` for other entries, and for a section the tree does not list
    /// (History, Remote control: their entries stay greyed out).
    pub fn from_label(menu: usize, label: &str) -> Option<Jump> {
        use crate::views::menus::{ADD_MENU, TOOLS_MENU, VIEW};
        let j = match (menu, label) {
            (VIEW, "Undo history") => Jump::Section("History"),
            (VIEW, "Patterns") => Jump::Section("Patterns"),
            (VIEW, "Generators in use") => Jump::Section("Generators"),
            (VIEW, "Effects in use") => Jump::Section("Effects"),
            (VIEW, "Remote control") => Jump::Section("Remote control"),
            (VIEW, "Plugin database") | (ADD_MENU, "Browse plugin database") => Jump::PluginDb,
            (TOOLS_MENU, "Browser find...") => Jump::Find,
            _ => return None,
        };
        match j {
            Jump::Section(name) => view::section_key(name).map(|_| j),
            _ => Some(j),
        }
    }
}

#[cfg(test)]
mod project_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn browser_initial_projects_follow_documents_and_saved_choices() {
        let documents = std::env::temp_dir().join(format!("os_browser_documents_{}", std::process::id()));
        let projects = documents.join("Image-Line/FL Studio/Projects");
        std::fs::create_dir_all(&projects).unwrap();
        assert_eq!(initial_folders(None, Some(&documents)), vec![projects.clone()]);
        assert!(initial_folders(Some(""), Some(&documents)).is_empty(), "removing all folders remains saved");
        let custom = documents.join("My projects");
        assert_eq!(initial_folders(Some(&format!("{}\r\n", custom.display())), Some(&documents)), vec![custom]);
        assert!(initial_folders(None, Some(&documents.join("Missing"))).is_empty());
        std::fs::remove_dir(&projects).unwrap();
        std::fs::remove_dir(projects.parent().unwrap()).unwrap();
        std::fs::remove_dir(documents.join("Image-Line")).unwrap();
        std::fs::remove_dir(documents).unwrap();
    }

    #[test]
    fn browser_projects_open_on_double_click_and_enter() {
        use winit::keyboard::{Key, NamedKey};
        let dir = std::env::temp_dir().join(format!("os_browser_projects_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("First.FLP");
        let second = dir.join("Second.flp");
        let mut source = App::new(None);
        source.state.project.info.title = Some("Browser project".into());
        source.save_to(first.clone());
        source.save_to(second.clone());
        let mut app = App::new(None);
        app.add_browser_folder(dir.clone());
        let index = |app: &App, path: &PathBuf| view::rows(&app.state).iter()
            .position(|row| row.path.as_ref() == Some(path)).unwrap();
        app.browser_act(&Action::BrowserRow(index(&app, &first)));
        assert!(app.state.path.is_none(), "one click only selects");
        app.browser_act(&Action::BrowserRow(index(&app, &first)));
        assert_eq!(app.state.path.as_ref(), Some(&first));
        assert_eq!(app.state.project.info.title.as_deref(), Some("Browser project"));
        app.browser_act(&Action::BrowserRow(index(&app, &second)));
        assert_eq!(app.state.path.as_ref(), Some(&first), "a different row is not a double click");
        app.mouse = (10.0, view::LIST_Y + 10.0);
        assert!(app.browser_key(&Key::Named(NamedKey::Enter), false, false));
        assert_eq!(app.state.path.as_ref(), Some(&second));
        std::fs::remove_file(first).unwrap();
        std::fs::remove_file(second).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn browser_text_files_cannot_open_as_projects() {
        assert!(project_path(&Item::File("readme.txt".into())).is_none());
        assert!(project_path(&Item::Sample("audio.wav".into())).is_none());
        assert_eq!(project_path(&Item::File("Track.FLP".into())), Some(PathBuf::from("Track.FLP")));
    }

    #[test]
    #[ignore = "reads the native user's Documents location; does not modify it"]
    fn browser_native_documents_discovery() {
        let documents = documents_dir().expect("Documents location");
        println!("Documents: {}", documents.display());
        let folders = initial_folders(None, Some(&documents));
        assert!(!folders.is_empty(), "FL Studio Projects folder must exist for this native check");
        println!("Projects: {}", folders[0].display());
    }
}

/// Options-menu commands (spec §4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BCmd {
    /// Find previous (-1) / next (1).
    Find(isize),
    Toggle(&'static str),
    /// Navigation history back (-1) / forward (1).
    History(isize),
    /// Send previous (-1) / next (1) sample to the selected channel.
    Send(isize),
    Collapse,
    Reread,
    Rescan,
    /// A row menu command (spec §8), on the selected row (the right click selected it).
    Row(RowCmd),
}

/// What a row menu entry does (spec §8).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RowCmd {
    /// Open / close the node.
    Toggle,
    /// What a click on the row does: the pattern or channel selected, the project taken to the history step.
    Activate,
    /// The row's pattern made current, then this PATTERNS command.
    Pattern(crate::patterns::Cmd),
    /// The channel's window (its plugin's, Lemon Sampler's, or the channel settings).
    ChannelWindow,
    /// A sample played (§7), loaded into the selected channel, or into a new Lemon Sampler channel.
    Preview,
    Send,
    NewChannel,
    /// A disk folder opened in the system's file manager; a disk file shown there, selected in its folder.
    OpenFolder,
    ShowInFolder,
    /// A dropped folder root: its Browser name typed in File ▸ Browser extra search folders; out of the browser.
    Rename,
    Remove,
}

/// The row menu's way to the system's file manager: a folder, a file.
#[cfg(windows)]
const FILE_MANAGER: [&str; 2] = ["Open in Explorer", "Show in Explorer"];
#[cfg(target_os = "macos")]
const FILE_MANAGER: [&str; 2] = ["Open in Finder", "Show in Finder"];
#[cfg(all(unix, not(target_os = "macos")))]
const FILE_MANAGER: [&str; 2] = ["Open in file manager", "Show in file manager"];

/// A row's menu (spec §8): Open / Close for a node, what the row's kind offers, the file manager for a disk entry, then
/// the structure commands every row has.
pub fn row_menu(s: &crate::app::State, row: &view::Row) -> Vec<PItem> {
    use crate::patterns::Cmd as P;
    use crate::rack::RCmd;
    let r = |label: &'static str, c: RowCmd| PItem::new(label, Cmd::Browser(BCmd::Row(c)));
    let sep = |items: &mut Vec<PItem>| {
        if items.last().is_some_and(|i| !i.sep) {
            items.push(PItem::sep());
        }
    };
    let mut items = Vec::new();
    if let Some(open) = row.open {
        // While searching every node shows open.
        let it = r(if open { "Close" } else { "Open" }, RowCmd::Toggle);
        items.push(if s.browser_search.is_empty() { it } else { it.off() });
    }
    match row.item {
        Item::Pattern(_) => {
            let p = |label: &'static str, c: P| r(label, RowCmd::Pattern(c));
            items.extend([
                r("Select pattern", RowCmd::Activate),
                PItem::sep(),
                p("Rename and color...", P::Rename),
                p("Change color...", P::ChangeColor),
                p("Clone", P::Clone),
                p("Delete", P::Delete),
                PItem::sep(),
                p("Select in playlist", P::SelectInPlaylist),
                p("Split by channel", P::SplitByChannel),
            ]);
        }
        Item::Channel(c) => {
            let k = |label: &'static str, rc: RCmd| PItem::new(label, Cmd::Rack(rc));
            items.extend([
                r("Select channel", RowCmd::Activate),
                r("Show window", RowCmd::ChannelWindow),
                k("Piano roll", RCmd::PianoRoll(c)),
                PItem::sep(),
                k("Rename, color and icon...", RCmd::Rename(c)),
                k("Clone", RCmd::Clone(c)),
                k("Delete...", RCmd::Delete(c)),
            ]);
        }
        Item::Sample(_) => {
            let send = r("Send to selected channel", RowCmd::Send);
            let sampler = s.project.channels.get(s.selected_channel).is_some_and(|c| c.kind.is_sampler());
            items.extend([r("Preview", RowCmd::Preview), if sampler { send } else { send.off() }, r("Add to channel rack", RowCmd::NewChannel)]);
        }
        Item::Generator(index) => items.push(PItem::new("Add to channel rack", Cmd::Plugin(crate::plugin_menu::PlugCmd::Generator { index, after: None }))),
        Item::Effect(index) => items.push(PItem::new("Add to selected mixer track", Cmd::Plugin(crate::plugin_menu::PlugCmd::Effect { index, slot: None }))),
        Item::History(_) => items.push(r("Go to this step", RowCmd::Activate)),
        // Into the selected channel's plugin (spec: plugins/presets.md "Browser").
        Item::Preset(ref p) => {
            let key = s.selected_channel;
            let it = PItem::new("Load into selected channel", Cmd::Preset(crate::presets::PCmd::Load(key, p.into())));
            items.push(if s.hosted_rows.contains(&key) { it } else { it.off() });
        }
        Item::None | Item::File(_) | Item::Param(..) => {}
    }
    // The plugin browser's variants and star.
    if s.browser_tab == view::PLUGIN_TAB {
        items.extend(crate::views::plugin_panel::row_menu(s, row));
    }
    if let Some(path) = &row.path {
        sep(&mut items);
        items.push(if row.open.is_some() { r(FILE_MANAGER[0], RowCmd::OpenFolder) } else { r(FILE_MANAGER[1], RowCmd::ShowInFolder) });
        if s.browser_folders.contains(path) && row.key == view::disk_key(path) {
            items.extend([r("Rename...", RowCmd::Rename), r("Remove from browser", RowCmd::Remove)]);
        }
    }
    sep(&mut items);
    items.extend([
        PItem::new("Collapse structure", Cmd::Browser(BCmd::Collapse)).key("Ctrl+Up"),
        PItem::new("Reread structure", Cmd::Browser(BCmd::Reread)).key("Ctrl+R"),
    ]);
    items
}

/// The browser options menu, from the FL screenshot (spec §4).
pub fn options_menu(s: &crate::app::State) -> Vec<PItem> {
    let b = |label: &'static str, c: BCmd| PItem::new(label, Cmd::Browser(c));
    let t = |label: &'static str, k: &'static str| b(label, BCmd::Toggle(k)).mark(Mark::Check(view::opt(s, k)));
    // Options with no effect yet: greyed out, showing their default tick.
    let dead = |label: &'static str, k: &'static str| PItem::not_yet(label).mark(Mark::Check(view::opt(s, k)));
    let searching = !s.browser_search.is_empty();
    let find = |label, key, dir| {
        let it = b(label, BCmd::Find(dir)).key(key);
        if searching { it } else { it.off() }
    };
    vec![
        PItem::head("Search"),
        find("Find previous", "F2", -1),
        find("Find next", "F3", 1),
        PItem::sep(),
        t("Show parent folders in search results", view::OPT_PARENTS),
        dead("Expand folders", view::OPT_EXPAND),
        t("Search for folders", view::OPT_FOLDERS),
        t("Search box on top", view::OPT_TOP),
        t("Type to filter", view::OPT_TYPE),
        dead("Load plugin tags", view::OPT_TAGS),
        dead("Enable indexing", view::OPT_INDEX),
        dead("Re-index changed folders on startup", view::OPT_REINDEX),
        PItem::head("Navigation history"),
        b("Previous", BCmd::History(-1)),
        b("Next", BCmd::History(1)),
        PItem::head("Actions"),
        b("Send previous to selected channel", BCmd::Send(-1)).key("Shift+Up"),
        b("Send next to selected channel", BCmd::Send(1)).key("Shift+Down"),
        PItem::head("View"),
        dead("Auto-hide", view::OPT_AUTOHIDE),
        dead("Hide while dragging (when detached)", view::OPT_HIDEDRAG),
        dead("Auto contrast text", view::OPT_CONTRAST),
        PItem::sub("Show icons and text on tabs", vec![PItem::not_yet("Contents not captured yet")]),
        t("Full sample preview", view::OPT_PREVIEW),
        PItem::head("Structure"),
        b("Collapse structure", BCmd::Collapse).key("Ctrl+Up"),
        b("Reread structure", BCmd::Reread).key("Ctrl+R"),
        PItem::head("Content"),
        PItem::not_yet("Configure extra folders"),
        b("Refresh plugin list (fast scan)", BCmd::Rescan),
    ]
}

/// Reread structure (spec §4): folders are read from disk again; open nodes and the selection stay.
fn reread(s: &mut crate::app::State) {
    s.browser_dirs.borrow_mut().clear();
    s.browser_scroll = s.browser_scroll.min(view::max_scroll(s));
}

/// Select a row and record it in the navigation history (spec §4).
pub(crate) fn select(s: &mut crate::app::State, key: &str) {
    s.browser_sel = Some(key.to_string());
    if s.browser_hist.get(s.browser_hist_pos).map(String::as_str) != Some(key) {
        let keep = if s.browser_hist.is_empty() { 0 } else { s.browser_hist_pos + 1 };
        s.browser_hist.truncate(keep);
        s.browser_hist.push(key.to_string());
        s.browser_hist_pos = s.browser_hist.len() - 1;
    }
}

/// Ctrl+Z with "Show undo history in the browser when undoing changes": the browser shows Current project ▸ History,
/// opened, where the project now is selected (spec: ui/settings-choices.md "Undo history: the four checks
/// (2026-10-16)"). The All tab shows it too; the others switch to the project's.
pub(crate) fn show_history(s: &mut crate::app::State) {
    s.show_browser = true;
    if s.browser_tab > 1 {
        s.browser_tab = 1;
    }
    s.browser_search.clear();
    let root = format!("{}:Current project", s.browser_tab);
    let folder = format!("{root}/History");
    s.browser_sel = Some(format!("{folder}/{}", s.history.at()));
    s.browser_open.extend([root, folder]);
    reveal(s);
}

/// Scroll the selected row into view.
pub(crate) fn reveal(s: &mut crate::app::State) {
    let rows = view::rows(s);
    let Some(i) = rows.iter().position(|r| Some(&r.key) == s.browser_sel.as_ref()) else { return };
    let y = i as f32 * view::ROW_H;
    let h = view::list_h(s);
    if y < s.browser_scroll {
        s.browser_scroll = y;
    } else if y + view::ROW_H > s.browser_scroll + h {
        s.browser_scroll = y + view::ROW_H - h;
    }
    s.browser_scroll = s.browser_scroll.clamp(0.0, view::max_scroll(s));
}

/// Index of the next row after the selection (wrapping) for which `ok` holds.
fn step(s: &crate::app::State, rows: &[view::Row], dir: isize, ok: impl Fn(&view::Row) -> bool) -> Option<usize> {
    let n = rows.len() as isize;
    let cur = rows.iter().position(|r| Some(&r.key) == s.browser_sel.as_ref()).map_or(if dir > 0 { -1 } else { n }, |i| i as isize);
    (1..=n).map(|k| (cur + dir * k).rem_euclid(n.max(1)) as usize).find(|&i| ok(&rows[i]))
}

/// Find previous / next match of the search text.
fn find(s: &mut crate::app::State, dir: isize) -> Option<String> {
    let q = s.browser_search.to_lowercase();
    if q.is_empty() {
        return None;
    }
    let rows = view::rows(s);
    let Some(i) = step(s, &rows, dir, |r| view::matches(s, r, &q)) else {
        return Some(format!("\"{}\" not found", s.browser_search));
    };
    let key = rows[i].key.clone();
    select(s, &key);
    reveal(s);
    None
}

/// Send the previous / next sample row to the selected channel (`send_sample`).
fn send(s: &mut crate::app::State, dir: isize) -> Option<String> {
    let rows = view::rows(s);
    let Some(i) = step(s, &rows, dir, |r| matches!(r.item, Item::Sample(_))) else {
        return Some("No samples in the browser to send".into());
    };
    let Item::Sample(path) = rows[i].item.clone() else { return None };
    let key = rows[i].key.clone();
    select(s, &key);
    reveal(s);
    send_sample(s, &path, &rows[i].label)
}

/// Sample `path` (shown as `label`) into the selected channel, which takes the file's name while "Auto name channels"
/// is on (`rack::auto_name`). One history step (`history::load_sample`).
fn send_sample(s: &mut crate::app::State, path: &str, label: &str) -> Option<String> {
    let row = s.selected_channel;
    if !s.project.channels.get(row)?.kind.is_sampler() {
        return Some("The selected channel is not a sampler".into());
    }
    // With Copy used samples to the project folder: the sample's copy in the data folder (`data_folder::adopt`).
    let path = crate::data_folder::adopt(s, path);
    crate::history::load_sample(s, row);
    let ch = &mut s.project.channels[row];
    crate::rack::auto_name(ch, std::path::Path::new(&path));
    crate::rack::read_tags(ch, std::path::Path::new(&path));
    ch.sample_path = Some(path.clone());
    s.sample_loads.push((row, path));
    s.piano.dirty = true;
    Some(format!("Sent {label} to the selected channel"))
}

/// Arrow Down / Up over the browser: the selection moves a visible row, wrapping from the last to the first and back
/// (FL's browser). The row landed on is returned when it is a sample, so the move itself plays it (§7, the sample
/// heard as soon as it is focused); every other kind of row is only selected.
fn move_sel(s: &mut crate::app::State, dir: isize) -> Option<String> {
    let rows = view::rows(s);
    let i = step(s, &rows, dir, |_| true)?;
    let key = rows[i].key.clone();
    select(s, &key);
    reveal(s);
    match &rows[i].item {
        Item::Sample(p) => Some(p.clone()),
        _ => None,
    }
}

/// Arrow Right over the browser: the focused sample is returned to be replayed from its start; a closed folder opens
/// where it stands; an opened folder selects its first row (the one below it), which is heard when it is a sample.
fn into_sel(s: &mut crate::app::State) -> Option<String> {
    let rows = view::rows(s);
    let i = rows.iter().position(|r| Some(&r.key) == s.browser_sel.as_ref())?;
    if let Item::Sample(p) = &rows[i].item {
        return Some(p.clone());
    }
    match rows[i].open? {
        true => {
            let child = rows.get(i + 1)?;
            select(s, &child.key);
            reveal(s);
            match &child.item {
                Item::Sample(p) => Some(p.clone()),
                _ => None,
            }
        }
        false => {
            s.browser_open.insert(rows[i].key.clone());
            None
        }
    }
}

/// Arrow Left over the browser: an opened folder closes; a leaf or a closed folder selects the nearest folder above
/// it (its parent, when the tree shows it), so walking out of a folder of samples takes the arrows alone.
fn out_sel(s: &mut crate::app::State) {
    let rows = view::rows(s);
    let Some(i) = rows.iter().position(|r| Some(&r.key) == s.browser_sel.as_ref()) else { return };
    if rows[i].open == Some(true) {
        s.browser_open.remove(rows[i].key.as_str());
        s.browser_scroll = s.browser_scroll.min(view::max_scroll(s));
        return;
    }
    if let Some(p) = rows[..i].iter().rposition(|r| r.open.is_some() && r.depth < rows[i].depth) {
        select(s, &rows[p].key);
        reveal(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `browser.txt` (spec: ui/browser.md §9): the dragged width, never below the default; anything else is the default.
    #[test]
    fn kept_width() {
        assert_eq!(parse_width("width=359\n"), 359.0);
        assert_eq!(parse_width("width=120"), crate::views::BROWSER_W);
        assert_eq!(parse_width("width=wide\n"), crate::views::BROWSER_W);
        assert_eq!(parse_width(""), crate::views::BROWSER_W);
    }

    /// The arrows' walk over the browser's rows (FL's browser): Down / Up step a row, wrapping, every kind of row
    /// selected, a sample heard the moment it is focused, files and folders silent; Right replays a sample, opens a
    /// closed folder or steps into an opened one's first row; Left closes it again or walks out to the parent.
    #[test]
    fn arrows_walk_the_rows_and_hear_the_samples() {
        let dir = std::env::temp_dir().join(format!("os_browser_arrows_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wav = |n: &str| {
            let p = dir.join(format!("{n}.wav"));
            os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&p).unwrap(), 44100, &[0.0; 8]).unwrap();
            p.to_string_lossy().into_owned()
        };
        let (kick, snare, hat) = (wav("A Kick"), wav("B Snare"), wav("C Hat"));
        std::fs::write(dir.join("readme.txt"), b"hello").unwrap();
        let mut app = App::new(None);
        app.state.browser_folders.push(dir.clone());
        app.state.browser_open.insert(view::disk_key(&dir));
        let keys: Vec<String> = view::rows(&app.state).into_iter().map(|r| r.key).collect();
        assert_eq!(keys.len(), 8, "the three closed roots, the dropped folder and its four rows");
        // From no selection Down takes the first row: the closed Current project root, nothing heard.
        assert_eq!(move_sel(&mut app.state, 1), None);
        assert_eq!(app.state.browser_sel.as_deref(), Some(keys[0].as_str()));
        for k in &keys[1..4] {
            assert_eq!(move_sel(&mut app.state, 1), None, "roots and the folder row are silent");
            assert_eq!(app.state.browser_sel.as_deref(), Some(k.as_str()));
        }
        // The folder's samples, each heard the moment it is focused:
        assert_eq!(move_sel(&mut app.state, 1).as_deref(), Some(kick.as_str()));
        assert_eq!(move_sel(&mut app.state, 1).as_deref(), Some(snare.as_str()));
        assert_eq!(move_sel(&mut app.state, 1).as_deref(), Some(hat.as_str()));
        // The readme is a file, not a sample: silent, and the walk wraps to the top.
        assert_eq!(move_sel(&mut app.state, 1), None);
        assert_eq!(move_sel(&mut app.state, 1), None);
        assert_eq!(app.state.browser_sel.as_deref(), Some(keys[0].as_str()), "wrapped to the first row");
        // Up from the first row wraps to the last (the readme, silent), then back onto the samples.
        assert_eq!(move_sel(&mut app.state, -1), None);
        assert_eq!(move_sel(&mut app.state, -1).as_deref(), Some(hat.as_str()));
        assert_eq!(move_sel(&mut app.state, -1).as_deref(), Some(snare.as_str()));
        assert_eq!(move_sel(&mut app.state, -1).as_deref(), Some(kick.as_str()));
        assert_eq!(move_sel(&mut app.state, -1), None, "the folder row again");
        assert_eq!(app.state.browser_sel.as_deref(), Some(keys[3].as_str()));
        // Right on the opened folder steps into its first row, the kick, heard.
        assert_eq!(into_sel(&mut app.state).as_deref(), Some(kick.as_str()));
        assert_eq!(app.state.browser_sel.as_deref(), Some(keys[4].as_str()));
        // Right on a sample replays it, the selection staying where it is.
        assert_eq!(into_sel(&mut app.state).as_deref(), Some(kick.as_str()));
        assert_eq!(app.state.browser_sel.as_deref(), Some(keys[4].as_str()));
        // Left from the sample back to its parent folder...
        out_sel(&mut app.state);
        assert_eq!(app.state.browser_sel.as_deref(), Some(keys[3].as_str()));
        // ...and Left on the opened folder closes it.
        out_sel(&mut app.state);
        assert!(!app.state.browser_open.contains(&keys[3]));
        // Right on the closed folder opens it where the selection stands, nothing heard yet.
        assert_eq!(into_sel(&mut app.state), None);
        assert!(app.state.browser_open.contains(&keys[3]));
        assert_eq!(app.state.browser_sel.as_deref(), Some(keys[3].as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The arrows in the app's keys: over the browser (or from its search field) they are the browser's, elsewhere
    /// they are left alone, and Shift+Down keeps its older meaning of sending the next sample to the channel.
    #[test]
    fn the_browser_takes_its_arrows_only_over_the_browser() {
        use winit::keyboard::{Key, NamedKey};
        let dir = std::env::temp_dir().join(format!("os_browser_arrows_key_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("A Kick.wav");
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&p).unwrap(), 44100, &[0.0; 8]).unwrap();
        let mut app = App::new(None);
        app.state.browser_folders.push(dir.clone());
        app.state.browser_open.insert(view::disk_key(&dir));
        app.mouse_move(100.0, 400.0);
        assert!(app.over_browser());
        let keys: Vec<String> = view::rows(&app.state).into_iter().map(|r| r.key).collect();
        // Down takes the first row; Right opens the closed root without moving; Down walks into it; Left back out.
        assert!(app.browser_key(&Key::Named(NamedKey::ArrowDown), false, false));
        assert_eq!(app.state.browser_sel.as_deref(), Some(keys[0].as_str()));
        assert!(app.browser_key(&Key::Named(NamedKey::ArrowRight), false, false));
        assert!(app.state.browser_open.contains(&keys[0]));
        assert_eq!(app.state.browser_sel.as_deref(), Some(keys[0].as_str()), "opening keeps the row");
        assert!(app.browser_key(&Key::Named(NamedKey::ArrowDown), false, false));
        assert_ne!(app.state.browser_sel.as_deref(), Some(keys[0].as_str()));
        assert!(app.browser_key(&Key::Named(NamedKey::ArrowLeft), false, false));
        assert_eq!(app.state.browser_sel.as_deref(), Some(keys[0].as_str()));
        // From the search field the Up / Down walk goes on (the results).
        app.state.browser_search_focus = true;
        app.state.browser_sel = None;
        assert!(app.browser_key(&Key::Named(NamedKey::ArrowDown), false, false));
        assert_eq!(app.state.browser_sel.as_deref(), Some(keys[0].as_str()));
        app.state.browser_search_focus = false;
        // With the pointer off the browser the arrow is somebody else's.
        app.mouse_move(1500.0, 400.0);
        app.state.browser_sel = None;
        assert!(!app.browser_key(&Key::Named(NamedKey::ArrowDown), false, false));
        assert!(app.state.browser_sel.is_none());
        // Over the browser again, Shift+Down still sends the next sample to the selected channel (and selects it).
        app.mouse_move(100.0, 400.0);
        app.shift = true;
        assert!(app.browser_key(&Key::Named(NamedKey::ArrowDown), false, false));
        app.shift = false;
        let row = view::rows(&app.state).into_iter().find(|r| Some(&r.key) == app.state.browser_sel.as_ref());
        assert_eq!(row.map(|r| r.label), Some("A Kick".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
