// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! A small record of the audio path, appended to `audio-log.txt` in the log dir: what the device
//! opened with, what a playback started with, and how the queue holds on while it plays. On
//! Android it is always on (the log dir is the external files folder, which a file manager
//! reaches — what happened is readable on the device itself); elsewhere when `OS_DEBUG` is set.
//! A report that says only "no sound" then carries the numbers that say which part failed.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

/// The log file's path, opened on the first line (the log dir set before anything plays).
static PATH: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

fn on() -> bool {
    cfg!(target_os = "android") || std::env::var_os("OS_DEBUG").is_some()
}

/// Appends one line with a millisecond stamp. A failed write is skipped: the log never gets in
/// the way of playing (and a stream that opens before the log dir is set starts the file late).
pub fn log(line: &str) {
    if !on() {
        return;
    }
    let Ok(mut at) = PATH.lock() else { return };
    let path = at.get_or_insert_with(|| {
        std::env::var_os("OPENSTUDIO_LOG_DIR").map_or_else(|| "audio-log.txt".into(), std::path::PathBuf::from)
    });
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&*path) {
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let _ = writeln!(f, "[{ms}] {line}");
    }
}
