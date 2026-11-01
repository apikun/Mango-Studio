// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The application as a library: the same modules `main.rs` runs, so Android's entry
//! (`crates/os-android`, a cdylib) can drive them from a NativeActivity. On the desktop
//! this crate is empty — the bin in `main.rs` is the program there.
//! The Android side of the port (the entry glue, the private folder) is `mobile`.

#![cfg_attr(not(plugin_windows), allow(dead_code))]
#![cfg(target_os = "android")]

mod app;
mod audio_cfg;
mod audiolog;
mod auto_close;
mod rolling;
mod auto_tools;
mod autoscroll;
mod backup;
#[cfg(test)]
mod audio_bench;
#[cfg(test)]
mod open_bench;
mod browser;
mod cable;
mod capture;
mod one_click;
mod score_log;
mod clip_colors;
mod clock;
mod cpu;
mod data_folder;
mod device;
mod dialog;
mod filedlg;
mod enable_groups;
mod export;
mod export_form;
mod factory_folder;
mod high_visibility;
mod history;
mod hold;
mod keep_on_disk;
mod icons;
mod init;
mod knobs;
mod layout;
mod loading;
mod missing;
mod zipped;
mod logo;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod menu_bar;
mod messages;
mod meters;
mod metronome;
mod midi_cfg;
mod midi_file;
mod midi_in;
mod midi_out;
mod midi_play;
mod mixer;
mod osc3;
mod citrus;
mod monitor;
mod palette;
mod params;
mod piano;
mod piano_snap;
mod piano_split;
mod piano_tools;
mod rack;
mod remote;
mod render;
mod pattern_render;
mod patterns;
mod perf;
mod playlist;
mod plugin_db;
mod plugin_manager;
mod plugin_menu;
mod plugin_picker;
mod plugin_popup;
mod plugin_rebuild;
mod plugin_scan;
#[cfg(plugin_windows)]
mod plugin_window;
mod plugins;
mod ppi;
mod prefs;
mod preview_track;
mod project_files;
mod project_forms;
mod project_info;
mod project_picker;
mod project_plugins;
mod presets;
mod misc;
mod misc_page;
mod note_names;
mod save_form;
mod scroll_speed;
mod search_folders;
mod settings_file;
mod shortcuts;
mod smooth;
mod snap;
mod song_pos;
mod toolbar;
mod splash;
mod stall;
mod step_edit;
mod strum;
mod flam;
mod flip;
mod levels;
mod chords;
mod claw;
mod limit;
mod lfo;
mod quantize;
mod randomize;
mod chop;
mod drum_chop;
mod piano_dlg;
mod arpeggiate;
mod tapper;
mod touch;
mod record;
mod recent;
mod theme_adjust;
mod theme_library;
mod sample_pack;
mod sound_kit;
mod templates;
mod timesig;
mod tool_cursor;
mod tooltip;
mod transport;
mod typein;
mod views;
mod wallpaper;
mod waves;
mod yabridge;
mod win_clamp;
#[cfg(target_os = "android")]
mod midi_shim;
#[cfg(target_os = "android")]
mod touch_mouse;
#[cfg(windows)]
mod file_assoc;

#[cfg(target_os = "android")]
pub mod mobile;

include!("main_body.rs");
