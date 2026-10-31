// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 thacer13
// Copyright (C) 2026 Artig3nce
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Lemon Studio: the application window.

// Plugin windows exist on Windows, Linux and macOS (`plugin_window`): elsewhere what only they reach (the Misc tab, the
// window and preset menus, their requests) is built but unused. Those builds still report real dead code.
#![cfg_attr(not(plugin_windows), allow(dead_code))]
// Windows: a GUI program, so started from Explorer, the Start menu or a shortcut it opens no console window beside its
// own. Started from a terminal it still writes there (`attach_console`).
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod app;
mod arrange;
mod help_links;
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
mod browser_index;
mod cable;
mod capture;
#[cfg(target_os = "linux")]
mod rt;
mod one_click;
mod track_rec;
mod audio_log;
mod score_log;
mod clip_colors;
mod clock;
mod control;
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
mod mix_threads;
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
mod slicer;
mod drums;
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
mod events;
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
mod mobile;

include!("main_body.rs");

<<<<<<< HEAD
/// Windows: a GUI program gets a console only through handles its parent passes it (`cargo run`, a pipe, the plugin
/// scanner's parent). Without them, the terminal it was started from (if any) takes its output, so `--render` and the
/// errors printed before the window opens still show there.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).is_null() && GetStdHandle(STD_ERROR_HANDLE).is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

fn main() {
    #[cfg(windows)]
    attach_console();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // A plugin scanner process (spec: ui/plugin-manager.md §4): loads one file, prints its plugins, ends.
    if args.first().map(String::as_str) == Some(plugin_scan::ARG) {
        plugin_scan::child_main(&args[1..]);
    }
    // A child the app starts to try a native plugin before its first load (spec: plugins/fruity-abi.md).
    if args.first().map(String::as_str) == Some(os_native::check::CHECK_ARG) {
        os_host::guard::exit(os_native::check::run(&args[1..]));
    }
    // macOS: the startup splash, in a process of its own (`splash`).
    #[cfg(target_os = "macos")]
    if args.first().map(String::as_str) == Some(splash::ARG) {
        splash::child_main();
    }
    // Plugin editors in real plugin windows, checked from the main thread (`plugins::window_check`).
    #[cfg(target_os = "macos")]
    if args.first().map(String::as_str) == Some("--plugin-window-check") {
        std::process::exit(plugins::window_check::run(&args[1..]));
    }
    app::move_old_settings();
    // The mixing threads every engine renders on, from here on (spec: audio/multicore.md).
    mix_threads::start();
    // A render without a window (FILE > Export audio from the command line, `render`).
    if args.first().map(String::as_str) == Some("--render") {
        std::process::exit(render::cli(&args[1..]));
    }
    if args.iter().any(|a| a == "--screenshot") {
        screenshot(&args);
        return;
    }
    // The control plane's socket (spec: roadmap/control-plane.md): beside the window, or without one.
    let control = control::server::Flags::take(&mut args).unwrap_or_else(|e| {
        eprintln!("Lemon Studio: {e}");
        std::process::exit(2);
    });
    let level = control.level.unwrap_or(control::Level::Edit);
    if let Some(at) = &control.serve {
        control::server::serve_headless(at, level, control.token.clone(), args.first().map(std::path::PathBuf::from));
    }
    // Run from source: Lemon Studio's own plugins are rebuilt and installed before the plugin scan looks for them.
    plugin_rebuild::run();
    let path = args.first().map(std::path::PathBuf::from);
    let mut builder = EventLoop::<()>::with_user_event();
    #[cfg(target_os = "linux")]
    if use_x11() {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    let event_loop = builder.build().expect("event loop");
    // The startup splash, from here until startup is done (spec: ui/settings-choices.md "Miscellaneous: naming,
    // detached plugins and startup"); each step below moves its gauge on. After the event loop: on Windows building it
    // makes the process per-monitor DPI aware, which the splash's size follows.
    let splash = splash::open();
    // Before the launch: the files the app is launched to open come ahead of `resumed`.
    #[cfg(target_os = "macos")]
    macos::hook_app();
    if let Ok(mut p) = PROXY.lock() {
        *p = Some(event_loop.create_proxy());
    }
    os_clap::set_waker(wake_ui);
    // CLAP plugins asking which thread they are on (`thread-check`): this one is the main thread.
    os_clap::set_main_thread();
    // Lemon plugins' editors run on threads of their own off Windows and wake the app when they send something.
    #[cfg(not(windows))]
    os_hostlink::set_host_waker(wake_ui);
    // Linux: the cards the sound server gave up on while a run before this one had them through ALSA (one that
    // crashed, or ended with a plugin) are given back (`device::heal_sound_server`).
    #[cfg(target_os = "linux")]
    std::thread::spawn(device::heal_sound_server);
    let start = path.is_none();
    splash::step(&splash, splash::SETTINGS, if start { "Reading settings" } else { "Opening project" });
    let mut app = App::new(path);
    // Without a project file: the Startup project (spec: ui/settings-choices.md "Miscellaneous: seven more checks,
    // Default template and Startup project").
    if start {
        splash::step(&splash, splash::PROJECT, "Opening project");
        app.startup_project();
        // Ask for data folder "On startup": the New project form over it (spec: ui/data-folder.md §7).
        app.startup_form();
    }
    if let Some(at) = &control.control {
        app.state.message = Some(match control::server::start(at, level, wake_ui) {
            Ok(mut server) => {
                if control.token.is_some() {
                    server.token = control.token.clone();
                }
                let m = format!("Control plane: {}", server.describe());
                eprintln!("Lemon Studio: {m}");
                app.listen(server);
                m
            }
            Err(e) => format!("Control plane: could not listen at {at}: {e}"),
        });
    }
    // The windows where the last run left them.
    layout::restore(&mut app.state);
    // MIDI devices (spec: ui/midi-settings.md §2): the enabled inputs play the selected channel; master sync goes out.
    splash::step(&splash, splash::MIDI, "Opening MIDI devices");
    app.midi = midi_in::MidiIn::open(wake_ui);
    app.midi_out = midi_out::MidiOut::start(app.transport.watch());
    let message = app.state.message.clone();
    app.midi_refresh(false);
    if message.is_some() {
        app.state.message = message;
    }
    // Plugin menus and dialogs run modal loops on this thread on Windows (elsewhere Lemon plugins' editors have threads
    // of their own).
    let watchdog = stall::Watchdog::new(stall::LATE);
    #[cfg(windows)]
    watchdog.start(wake_ui);
    let mut shell = Shell {
        app,
        window: None,
        surface: None,
        canvas: None,
        mods: ModifiersState::default(),
        last_click: Instant::now() - Duration::from_secs(10),
        lock: None,
        pointer_locked: false,
        cursor: app::Cursor::Default,
        inset: (0, 0, 0, 0),
        edge: None,
        grab: None,
        unmax: None,
        pending: false,
        painted: Instant::now(),
        frame_start: Instant::now(),
        frame: FRAME,
        frame_monitor: None,
        splash,
        shown: false,
        frames: FrameLog::open(),
        watchdog,
        ctrl_click: false,
        main_win: layout::main_win(),
    };
    event_loop.run_app(&mut shell).expect("run");
    // After a contained plugin crash the process ends here, before plugin files are unloaded (a stopped plugin's code
    // must not run again).
    if os_host::guard::tripped() {
        os_host::guard::exit(0);
    }
    // The app's plugin instances are destroyed here, on this (UI) thread, as far as the engine has let go of them.
    drop(shell);
    plugins::sweep_retired();
=======
#[cfg(not(target_os = "android"))]
fn main() {
    main_impl();
>>>>>>> main
}

// Android runs the cdylib (`crates/os-android`), not this bin; cargo still wants a main.
#[cfg(target_os = "android")]
fn main() {}
