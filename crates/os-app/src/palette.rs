// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 battlemountain2
// Copyright (C) 2026 Artig3nce
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Omni Command Palette (Phase 4): Global searchable action runner, plugin inserter, and navigation hub.
//!
//! Toggled globally via `Ctrl+Shift+Space` and View ▸ Command palette (Ctrl+K stays FL's next channel and graph editor).
//! Provides instant fuzzy filtering over DAW actions, native & scanned plugins, open windows,
//! channels, and mixer tracks. Supports full keyboard navigation (Up/Down, Enter, Esc).

use os_ui::Color;
use crate::app::{Action, App, CloseAll, State, Win};

/// Categories for command palette entries, rendered with distinct badge colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Action,
    Window,
    Plugin,
    Effect,
    Channel,
    Mixer,
}

impl Category {
    pub fn label(&self) -> &'static str {
        match self {
            Category::Action => "ACTION",
            Category::Window => "WINDOW",
            Category::Plugin => "PLUGIN",
            Category::Effect => "EFFECT",
            Category::Channel => "CHANNEL",
            Category::Mixer => "MIXER",
        }
    }

    /// Returns (background, text) color for the category badge.
    pub fn colors(&self) -> (Color, Color) {
        match self {
            Category::Action => (Color::rgb(0x183050), Color::rgb(0x60a5fa)),
            Category::Window => (Color::rgb(0x14402a), Color::rgb(0x34d399)),
            Category::Plugin => (Color::rgb(0x483010), Color::rgb(0xfbbf24)),
            Category::Effect => (Color::rgb(0x3b1d54), Color::rgb(0xc084fc)),
            Category::Channel => (Color::rgb(0x4a1834), Color::rgb(0xf472b6)),
            Category::Mixer => (Color::rgb(0x133848), Color::rgb(0x38bdf8)),
        }
    }
}

/// An executable action triggered by choosing a palette entry.
#[derive(Clone, Debug, PartialEq)]
pub enum PaletteAction {
    DawAction(Action),
    AddGenerator(usize),
    AddEffect(usize),
    FocusWindow(Win),
    FocusChannel(usize),
    FocusMixer(u16),
    OpenSettings(usize),
    CloseAll,
    CloseUnfocused,
    ClosePlugins,
}

/// A single item searchable within the command palette.
#[derive(Clone, Debug)]
pub struct PaletteItem {
    pub title: String,
    pub subtitle: String,
    pub category: Category,
    pub shortcut: Option<&'static str>,
    pub action: PaletteAction,
}

/// Runtime state for the command palette.
#[derive(Clone, Debug, Default)]
pub struct Palette {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub scroll: usize,
}

/// A matched item with calculated fuzzy score.
#[derive(Clone, Debug)]
pub struct ScoredItem<'a> {
    pub item: &'a PaletteItem,
    pub original_index: usize,
    pub score: i32,
}

pub const PAGE_SIZE: usize = 9;

/// Gathers all searchable items given the current DAW state.
pub fn items(s: &State) -> Vec<PaletteItem> {
    let mut list = Vec::with_capacity(128);

    // 1. Windows & Primary Workspaces
    list.push(PaletteItem {
        title: "Focus Playlist".into(),
        subtitle: "Main arrangement & timeline workspace".into(),
        category: Category::Window,
        shortcut: Some("F5"),
        action: PaletteAction::FocusWindow(Win::Playlist),
    });
    list.push(PaletteItem {
        title: "Focus Piano Roll".into(),
        subtitle: "Note sequencer & melody editor".into(),
        category: Category::Window,
        shortcut: Some("F7"),
        action: PaletteAction::FocusWindow(Win::PianoRoll),
    });
    list.push(PaletteItem {
        title: "Focus Channel Rack".into(),
        subtitle: "Step sequencer & instrument rack".into(),
        category: Category::Window,
        shortcut: Some("F6"),
        action: PaletteAction::FocusWindow(Win::ChannelRack),
    });
    list.push(PaletteItem {
        title: "Focus Mixer".into(),
        subtitle: "Audio routing, inserts & effects rack".into(),
        category: Category::Window,
        shortcut: Some("F9"),
        action: PaletteAction::FocusWindow(Win::Mixer),
    });
    list.push(PaletteItem {
        title: "Toggle Browser".into(),
        subtitle: "Samples, presets, and project tree sidebar".into(),
        category: Category::Window,
        shortcut: Some("Alt+F8"),
        action: PaletteAction::DawAction(Action::ToggleWin(Win::Browser)),
    });
    list.push(PaletteItem {
        title: "Focus Plugin Performance Monitor".into(),
        subtitle: "CPU usage & processing latency per plugin".into(),
        category: Category::Window,
        shortcut: None,
        action: PaletteAction::FocusWindow(Win::Perf),
    });
    list.push(PaletteItem {
        title: "Focus Tempo Tapper".into(),
        subtitle: "Tap tempo BPM calculation tool".into(),
        category: Category::Window,
        shortcut: Some("Ctrl+F5"),
        action: PaletteAction::FocusWindow(Win::Tapper),
    });
    list.push(PaletteItem {
        title: "Focus Touch Controller".into(),
        subtitle: "Touch pads and on-screen keyboard".into(),
        category: Category::Window,
        shortcut: Some("Alt+F7"),
        action: PaletteAction::FocusWindow(Win::Touch),
    });
    list.push(PaletteItem {
        title: "Open Project Picker".into(),
        subtitle: "Visual grid of patterns and audio clips".into(),
        category: Category::Window,
        shortcut: Some("Ctrl+F8"),
        action: PaletteAction::DawAction(Action::ProjectPicker),
    });
    list.push(PaletteItem {
        title: "Open Plugin Picker".into(),
        subtitle: "Visual grid of installed instruments and effects".into(),
        category: Category::Window,
        shortcut: Some("F8"),
        action: PaletteAction::DawAction(Action::PickerOpen),
    });

    // 2. Core DAW Actions & Transport
    list.push(PaletteItem {
        title: "Play / Pause".into(),
        subtitle: "Start or pause playback from current position".into(),
        category: Category::Action,
        shortcut: Some("Space"),
        action: PaletteAction::DawAction(Action::Play),
    });
    list.push(PaletteItem {
        title: "Stop Playback".into(),
        subtitle: "Stop playback and rewind to song/pattern start".into(),
        category: Category::Action,
        shortcut: Some("Space"),
        action: PaletteAction::DawAction(Action::Stop),
    });
    list.push(PaletteItem {
        title: "Toggle Song / Pattern Mode".into(),
        subtitle: "Switch between Pattern and Song arrangement playback".into(),
        category: Category::Action,
        shortcut: Some("L"),
        action: PaletteAction::DawAction(Action::PatSong(!s.song_mode)),
    });
    list.push(PaletteItem {
        title: "Toggle Recording".into(),
        subtitle: "Arm audio and MIDI recording".into(),
        category: Category::Action,
        shortcut: Some("R"),
        action: PaletteAction::DawAction(Action::Record),
    });
    list.push(PaletteItem {
        title: "Toggle Metronome".into(),
        subtitle: "Audible click track during playback and recording".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+M"),
        action: PaletteAction::DawAction(Action::Metronome),
    });
    list.push(PaletteItem {
        title: "Toggle Typing Keyboard to Piano".into(),
        subtitle: "Use QWERTY keyboard as MIDI note controller".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+T"),
        action: PaletteAction::DawAction(Action::Typing),
    });
    list.push(PaletteItem {
        title: "Toggle Step Edit Mode".into(),
        subtitle: "Advance note cursor step-by-step on input".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+E"),
        action: PaletteAction::DawAction(Action::StepEdit),
    });
    list.push(PaletteItem {
        title: "Toggle Auto-Scrolling".into(),
        subtitle: "Keep playback cursor in view continuously".into(),
        category: Category::Action,
        shortcut: None,
        action: PaletteAction::DawAction(Action::AutoScroll),
    });
    list.push(PaletteItem {
        title: "Toggle Enable Groups".into(),
        subtitle: "Control grouped channel parameters simultaneously".into(),
        category: Category::Action,
        shortcut: Some("Shift+Alt+G"),
        action: PaletteAction::DawAction(Action::EnableGroups),
    });
    list.push(PaletteItem {
        title: "Quantize Notes (Tools Menu)".into(),
        subtitle: "Align selected notes to current snap grid".into(),
        category: Category::Action,
        shortcut: Some("Alt+Q"),
        action: PaletteAction::DawAction(Action::PianoToolsMenu),
    });
    list.push(PaletteItem {
        title: "Reverse Audio Clips".into(),
        subtitle: "Flip selected playlist audio clips backward in time".into(),
        category: Category::Action,
        shortcut: Some("Shift+R"),
        action: PaletteAction::DawAction(Action::PlTool(crate::playlist::PlTool::Select)),
    });
    list.push(PaletteItem {
        title: "Undo".into(),
        subtitle: "Revert the last edit or parameter change".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+Z"),
        action: PaletteAction::DawAction(Action::PianoUndo),
    });
    list.push(PaletteItem {
        title: "New Project".into(),
        subtitle: "Start a fresh project from the default template".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+N"),
        action: PaletteAction::DawAction(Action::MenuItem(0, crate::views::menus::FILE_NEW)),
    });
    list.push(PaletteItem {
        title: "Open Project...".into(),
        subtitle: "Open an existing Lemon Studio or FLP project".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+O"),
        action: PaletteAction::DawAction(Action::MenuItem(0, crate::views::menus::FILE_OPEN)),
    });
    list.push(PaletteItem {
        title: "Save Project".into(),
        subtitle: "Save changes to current project file".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+S"),
        action: PaletteAction::DawAction(Action::MenuItem(0, crate::views::menus::FILE_SAVE)),
    });
    list.push(PaletteItem {
        title: "Save Project As...".into(),
        subtitle: "Save project to a new filename or folder".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+Shift+S"),
        action: PaletteAction::DawAction(Action::MenuItem(0, crate::views::menus::FILE_SAVE_AS)),
    });
    list.push(PaletteItem {
        title: "Export as WAV Audio...".into(),
        subtitle: "Render project or loop to uncompressed 16/24/32-bit WAV".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+R"),
        action: PaletteAction::DawAction(Action::MenuItem(0, crate::views::menus::FILE_EXPORT)),
    });
    list.push(PaletteItem {
        title: "Export as MP3 Audio...".into(),
        subtitle: "Render project to compressed MP3".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+Shift+R"),
        action: PaletteAction::DawAction(Action::MenuItem(0, crate::views::menus::FILE_EXPORT_MP3)),
    });
    list.push(PaletteItem {
        title: "Export as MIDI File...".into(),
        subtitle: "Export all pattern notes to standard MIDI file".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+Shift+M"),
        action: PaletteAction::DawAction(Action::MenuItem(0, crate::views::menus::FILE_EXPORT_MIDI)),
    });
    list.push(PaletteItem {
        title: "Close All Windows".into(),
        subtitle: "Hide all floating sub-windows".into(),
        category: Category::Action,
        shortcut: Some("F12"),
        action: PaletteAction::CloseAll,
    });
    list.push(PaletteItem {
        title: "Close All Unfocused Windows".into(),
        subtitle: "Hide all background windows, keeping active focused".into(),
        category: Category::Action,
        shortcut: Some("Ctrl+F12"),
        action: PaletteAction::CloseUnfocused,
    });
    list.push(PaletteItem {
        title: "Close All Plugin Windows".into(),
        subtitle: "Dismiss all hosted and native plugin GUI windows".into(),
        category: Category::Action,
        shortcut: Some("Alt+F12"),
        action: PaletteAction::ClosePlugins,
    });
    list.push(PaletteItem {
        title: "Add Pattern".into(),
        subtitle: "Create a new musical pattern".into(),
        category: Category::Action,
        shortcut: Some("F4"),
        action: PaletteAction::DawAction(Action::PatternAdd),
    });
    list.push(PaletteItem {
        title: "Next Pattern".into(),
        subtitle: "Select the next pattern index".into(),
        category: Category::Action,
        shortcut: Some("+"),
        action: PaletteAction::DawAction(Action::PatternNext),
    });
    list.push(PaletteItem {
        title: "Previous Pattern".into(),
        subtitle: "Select the previous pattern index".into(),
        category: Category::Action,
        shortcut: Some("-"),
        action: PaletteAction::DawAction(Action::PatternPrev),
    });
    list.push(PaletteItem {
        title: "Add Instrument / Channel (+)".into(),
        subtitle: "Append a new generator channel to the Channel Rack".into(),
        category: Category::Action,
        shortcut: None,
        action: PaletteAction::DawAction(Action::RackAdd),
    });
    list.push(PaletteItem {
        title: "MIDI Settings".into(),
        subtitle: "Configure MIDI input controllers and outputs".into(),
        category: Category::Action,
        shortcut: Some("F10"),
        action: PaletteAction::OpenSettings(crate::views::settings::MIDI),
    });
    list.push(PaletteItem {
        title: "Audio Settings".into(),
        subtitle: "Buffer size, sample rate, and audio driver device".into(),
        category: Category::Action,
        shortcut: None,
        action: PaletteAction::OpenSettings(crate::views::settings::AUDIO),
    });
    list.push(PaletteItem {
        title: "General Settings".into(),
        subtitle: "GUI scaling, animations, and system preferences".into(),
        category: Category::Action,
        shortcut: None,
        action: PaletteAction::OpenSettings(crate::views::settings::GENERAL),
    });
    list.push(PaletteItem {
        title: "File Settings".into(),
        subtitle: "Extra search folders, VST search paths, and backups".into(),
        category: Category::Action,
        shortcut: None,
        action: PaletteAction::OpenSettings(crate::views::settings::FILE),
    });
    list.push(PaletteItem {
        title: "Theme Settings".into(),
        subtitle: "Customize DAW colors, accents, and visual theme".into(),
        category: Category::Action,
        shortcut: None,
        action: PaletteAction::OpenSettings(crate::views::settings::THEME),
    });
    list.push(PaletteItem {
        title: "Manage Plugins".into(),
        subtitle: "Scan, verify, and organize VST3 and CLAP plugins".into(),
        category: Category::Action,
        shortcut: None,
        action: PaletteAction::OpenSettings(crate::views::settings::PLUGINS),
    });

    // 3. Channels in Current Project
    for (i, ch) in s.project.channels.iter().enumerate() {
        let name = ch.name.as_deref().unwrap_or("Channel");
        let kind = match ch.kind {
            os_model::ChannelKind::Sampler => "Sampler",
            os_model::ChannelKind::Plugin => "Plugin",
            os_model::ChannelKind::AudioClip => "Audio Clip",
            os_model::ChannelKind::Layer => "Layer",
            os_model::ChannelKind::Automation => "Automation",
            _ => "Channel",
        };
        list.push(PaletteItem {
            title: format!("Channel: {name}"),
            subtitle: format!("{kind} · Row {}, Mixer Track {}", i + 1, ch.mixer_track),
            category: Category::Channel,
            shortcut: None,
            action: PaletteAction::FocusChannel(i),
        });
    }

    // 4. Mixer Tracks
    for m in &s.project.mixer {
        let name = if m.index == 0 {
            "Master".to_string()
        } else if let Some(n) = &m.name {
            n.clone()
        } else {
            format!("Insert {}", m.index)
        };
        let vol = m.volume.unwrap_or(0.8) * 100.0;
        list.push(PaletteItem {
            title: format!("Mixer Track {}: {name}", m.index),
            subtitle: format!("Fader {:.0}%", vol),
            category: Category::Mixer,
            shortcut: None,
            action: PaletteAction::FocusMixer(m.index),
        });
    }

    // 5. Scanned & Built-in Generator Plugins
    for entry in &s.plugdb.lists.gens {
        list.push(PaletteItem {
            title: format!("Insert Instrument: {}", entry.name),
            subtitle: format!("{} · {}", entry.vendor, entry.format.as_str().to_uppercase()),
            category: Category::Plugin,
            shortcut: None,
            action: PaletteAction::AddGenerator(entry.index),
        });
    }

    // 6. Scanned & Built-in Effect Plugins
    for entry in &s.plugdb.lists.fx {
        list.push(PaletteItem {
            title: format!("Insert Effect: {}", entry.name),
            subtitle: format!("{} · {} (to selected track {})", entry.vendor, entry.format.as_str().to_uppercase(), s.selected_mixer),
            category: Category::Effect,
            shortcut: None,
            action: PaletteAction::AddEffect(entry.index),
        });
    }

    list
}

/// Computes a fuzzy match score for ranking search candidates.
pub fn match_score(query: &str, title: &str, subtitle: &str) -> Option<i32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }

    let t_low = title.to_lowercase();
    let s_low = subtitle.to_lowercase();

    // Exact prefix match on title has highest priority
    if t_low.starts_with(&q) {
        return Some(10000 + 100 * q.len() as i32 - title.len() as i32);
    }

    // Exact substring match in title
    if let Some(pos) = t_low.find(&q) {
        let is_word_boundary = pos == 0 || title.as_bytes().get(pos - 1).is_some_and(|&b| {
            b == b' ' || b == b':' || b == b'-' || b == b'/' || b == b'('
        });
        let bonus = if is_word_boundary { 5000 } else { 2000 };
        return Some(bonus + 50 * q.len() as i32 - (pos as i32 * 10) - title.len() as i32);
    }

    // Fuzzy subsequence match in title
    let q_chars: Vec<char> = q.chars().collect();
    let t_chars: Vec<char> = t_low.chars().collect();
    let orig_chars: Vec<char> = title.chars().collect();

    let mut t_idx = 0;
    let mut consecutive = 0;
    let mut score = 500;
    let mut matched = 0;

    for &qc in &q_chars {
        let mut found = false;
        while t_idx < t_chars.len() {
            let tc = t_chars[t_idx];
            if tc == qc {
                found = true;
                let is_word_start = t_idx == 0 || {
                    let prev = orig_chars[t_idx - 1];
                    prev == ' ' || prev == ':' || prev == '-' || prev == '/' || prev == '('
                        || (prev.is_ascii_lowercase() && orig_chars[t_idx].is_ascii_uppercase())
                };
                if is_word_start {
                    score += 300;
                    consecutive += 1;
                } else if consecutive > 0 {
                    score += 100 * consecutive;
                    consecutive += 1;
                } else {
                    score += 30;
                    consecutive = 1;
                }
                t_idx += 1;
                matched += 1;
                break;
            } else {
                consecutive = 0;
                t_idx += 1;
            }
        }
        if !found {
            break;
        }
    }

    if matched == q_chars.len() {
        score -= title.len() as i32 * 2;
        return Some(score);
    }

    // Fallback: check match in subtitle
    if let Some(pos) = s_low.find(&q) {
        return Some(100 - pos as i32 - subtitle.len() as i32);
    }

    None
}

/// Filters and ranks items given the query string.
pub fn filter_items<'a>(items: &'a [PaletteItem], query: &str) -> Vec<ScoredItem<'a>> {
    let q = query.trim();
    if q.is_empty() {
        return items.iter().enumerate().map(|(i, item)| ScoredItem {
            item,
            original_index: i,
            score: 0,
        }).collect();
    }

    let mut scored = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if let Some(s) = match_score(q, &item.title, &item.subtitle) {
            scored.push(ScoredItem {
                item,
                original_index: i,
                score: s,
            });
        }
    }

    scored.sort_by(|a, b| b.score.cmp(&a.score));
    scored
}

/// Executes the given item and closes the command palette.
pub fn execute(app: &mut App, item: &PaletteItem) {
    app.state.palette.open = false;
    match &item.action {
        PaletteAction::DawAction(act) => {
            app.act(act.clone());
        }
        PaletteAction::AddGenerator(idx) => {
            crate::plugin_menu::add_generator(app, *idx, None);
        }
        PaletteAction::AddEffect(idx) => {
            crate::plugin_menu::add_effect(app, *idx, None);
        }
        PaletteAction::FocusWindow(win) => {
            app.state.raise(*win);
            if !app.state.shown(*win) {
                app.act(Action::ToggleWin(*win));
            }
        }
        PaletteAction::FocusChannel(ch) => {
            let ch = *ch;
            if ch < app.state.project.channels.len() {
                app.state.selected_channel = ch;
                app.state.rack.select_only(ch);
                app.state.raise(Win::ChannelRack);
                app.open_channel_window(ch);
            }
        }
        PaletteAction::FocusMixer(track) => {
            app.state.selected_mixer = *track;
            app.state.raise(Win::Mixer);
        }
        PaletteAction::OpenSettings(tab) => {
            app.open_settings(*tab);
        }
        PaletteAction::CloseAll => {
            app.close_windows(CloseAll::All);
        }
        PaletteAction::CloseUnfocused => {
            app.close_windows(CloseAll::Unfocused);
        }
        PaletteAction::ClosePlugins => {
            app.close_windows(CloseAll::Plugins);
        }
    }
    app.invalidate();
}

/// Handles keyboard input while the command palette modal is open.
pub fn key(app: &mut App, key: &winit::keyboard::Key, ctrl: bool, alt: bool) -> bool {
    use winit::keyboard::{Key, NamedKey};

    if !app.state.palette.open {
        return false;
    }

    // Toggle closed on Ctrl+Shift+Space, the key that opened it.
    if matches!(key, Key::Named(NamedKey::Space)) && ctrl && !alt && app.shift {
        app.state.palette.open = false;
        app.invalidate();
        return true;
    }

    let all_items = items(&app.state);
    let filtered = filter_items(&all_items, &app.state.palette.query);
    let count = filtered.len();

    match key {
        Key::Named(NamedKey::Escape) => {
            app.state.palette.open = false;
        }
        Key::Named(NamedKey::ArrowDown) => {
            if count > 0 {
                app.state.palette.selected = (app.state.palette.selected + 1).min(count - 1);
                update_scroll(&mut app.state.palette, count);
            }
        }
        Key::Named(NamedKey::ArrowUp) => {
            if count > 0 {
                app.state.palette.selected = app.state.palette.selected.saturating_sub(1);
                update_scroll(&mut app.state.palette, count);
            }
        }
        Key::Named(NamedKey::PageDown) => {
            if count > 0 {
                app.state.palette.selected = (app.state.palette.selected + PAGE_SIZE).min(count - 1);
                update_scroll(&mut app.state.palette, count);
            }
        }
        Key::Named(NamedKey::PageUp) => {
            if count > 0 {
                app.state.palette.selected = app.state.palette.selected.saturating_sub(PAGE_SIZE);
                update_scroll(&mut app.state.palette, count);
            }
        }
        Key::Named(NamedKey::Home) => {
            app.state.palette.selected = 0;
            app.state.palette.scroll = 0;
        }
        Key::Named(NamedKey::End) => {
            if count > 0 {
                app.state.palette.selected = count - 1;
                update_scroll(&mut app.state.palette, count);
            }
        }
        Key::Named(NamedKey::Enter) => {
            if let Some(target) = filtered.get(app.state.palette.selected) {
                let item = target.item.clone();
                execute(app, &item);
                return true;
            }
        }
        Key::Named(NamedKey::Backspace | NamedKey::Delete) => {
            if ctrl {
                // Delete previous word on Ctrl+Backspace
                let trimmed = app.state.palette.query.trim_end();
                if let Some(idx) = trimmed.rfind(|c: char| c.is_whitespace() || c == ':') {
                    app.state.palette.query.truncate(idx);
                } else {
                    app.state.palette.query.clear();
                }
            } else {
                app.state.palette.query.pop();
            }
            app.state.palette.selected = 0;
            app.state.palette.scroll = 0;
        }
        Key::Named(NamedKey::Space) if !alt => {
            if app.state.palette.query.len() < 128 {
                app.state.palette.query.push(' ');
                app.state.palette.selected = 0;
                app.state.palette.scroll = 0;
            }
        }
        Key::Character(text) if !alt => {
            // Allow typing even if Ctrl was momentarily held, unless it's a reserved shortcut
            if ctrl && (text.eq_ignore_ascii_case("k") || text.eq_ignore_ascii_case("u") || text.eq_ignore_ascii_case("a")) {
                if text.eq_ignore_ascii_case("u") || text.eq_ignore_ascii_case("a") {
                    app.state.palette.query.clear();
                    app.state.palette.selected = 0;
                    app.state.palette.scroll = 0;
                }
            } else {
                let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                if app.state.palette.query.len() + clean.len() <= 128 {
                    app.state.palette.query.push_str(&clean);
                    app.state.palette.selected = 0;
                    app.state.palette.scroll = 0;
                }
            }
        }
        _ => {}
    }

    app.invalidate();
    true
}

fn update_scroll(pal: &mut Palette, count: usize) {
    if count == 0 {
        pal.selected = 0;
        pal.scroll = 0;
        return;
    }
    if pal.selected < pal.scroll {
        pal.scroll = pal.selected;
    } else if pal.selected >= pal.scroll + PAGE_SIZE {
        pal.scroll = pal.selected + 1 - PAGE_SIZE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matching_prefers_prefix_and_word_boundaries() {
        // Prefix match scores higher than substring match
        let s_prefix = match_score("cit", "Citrus", "Synthesizer").unwrap();
        let s_mid = match_score("cit", "Electric City", "Effects").unwrap();
        assert!(s_prefix > s_mid, "Prefix match scores higher than middle match");

        // Subsequence matches
        assert!(match_score("rev", "Reverse Audio Clips", "").is_some());
        assert!(match_score("fpl", "Focus Playlist", "").is_some());
        assert!(match_score("xyz123", "Focus Playlist", "").is_none());

        // Empty query matches everything
        assert_eq!(match_score("", "Any Title", ""), Some(0));
    }

    #[test]
    fn filter_items_sorts_descending_by_score() {
        let dummy = vec![
            PaletteItem {
                title: "Electric Guitar".into(),
                subtitle: "".into(),
                category: Category::Plugin,
                shortcut: None,
                action: PaletteAction::CloseAll,
            },
            PaletteItem {
                title: "Guitar Amp".into(),
                subtitle: "".into(),
                category: Category::Effect,
                shortcut: None,
                action: PaletteAction::CloseAll,
            },
        ];

        let filtered = filter_items(&dummy, "guitar");
        assert_eq!(filtered.len(), 2);
        // "Guitar Amp" starts with "guitar" so it ranks first
        assert_eq!(filtered[0].item.title, "Guitar Amp");
        assert_eq!(filtered[1].item.title, "Electric Guitar");
    }

    #[test]
    fn scroll_pagination_keeps_selected_visible() {
        let mut p = Palette::default();
        p.selected = 5;
        update_scroll(&mut p, 20);
        assert_eq!(p.scroll, 0);

        p.selected = 12;
        update_scroll(&mut p, 20);
        assert_eq!(p.scroll, 12 + 1 - PAGE_SIZE);

        p.selected = 2;
        update_scroll(&mut p, 20);
        assert_eq!(p.scroll, 2);
    }

    #[test]
    fn category_metadata_and_colors() {
        assert_eq!(Category::Action.label(), "ACTION");
        assert_eq!(Category::Window.label(), "WINDOW");
        assert_eq!(Category::Plugin.label(), "PLUGIN");
        assert_eq!(Category::Effect.label(), "EFFECT");
        assert_eq!(Category::Channel.label(), "CHANNEL");
        assert_eq!(Category::Mixer.label(), "MIXER");

        let (bg, fg) = Category::Action.colors();
        assert_ne!(bg, fg);
    }
}
