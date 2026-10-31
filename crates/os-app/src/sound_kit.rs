// Copyright (C) 2026 Artig3nce
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The Mango Kit as files: the eighteen drum sounds of `os_engine::kit`, written once as WAVs into `Sounds/Mango Kit` of
//! the user folder (the folder the Android build keeps its starter kit in too) and shown in the Browser as the folder
//! "Mango Kit". Made from maths alone, so they are free to ship with the app (GPL 3.0 or later). Made once: a kit the
//! user deleted, or took out of the Browser, stays gone.

use std::path::{Path, PathBuf};

use crate::prefs;

/// Where the kit is, under the user folder.
const FOLDER: [&str; 2] = ["Sounds", "Mango Kit"];
/// The name the Browser shows the folder under.
pub const BROWSER_NAME: &str = "Mango Kit";
/// Set once the kit was made.
const KEY: &str = "mango_kit_made";
/// The rate the kit is made at.
const RATE: u32 = 44100;

/// Makes the kit in `root` once, and returns its folder when it was made now. Not again once made (nor over a folder
/// that is there).
pub fn ensure(root: &Path) -> Result<Option<PathBuf>, String> {
    let dir = FOLDER.iter().fold(root.to_path_buf(), |p, part| p.join(part));
    if prefs::text(KEY).is_some() || dir.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (name, make) in os_engine::kit::SOUNDS {
        let s = make(RATE);
        let frames: Vec<f32> = s.left.iter().zip(&s.right).flat_map(|(l, r)| [*l, *r]).collect();
        let mut bytes = Vec::new();
        os_engine::wav::write_f32_stereo(&mut bytes, s.rate, &frames).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{name}.wav")), bytes).map_err(|e| e.to_string())?;
    }
    prefs::set_text(KEY, "1");
    Ok(Some(dir))
}

/// Starts with the kit in the Browser: made now (`ensure`), its folder is added to the Browser's folders under its
/// name and the list is kept. The list's order is the user's: the kit goes last.
pub fn offer(s: &mut crate::app::State, root: &Path) -> Result<(), String> {
    if let Some(dir) = ensure(root)? {
        if !s.browser_folders.contains(&dir) {
            s.browser_folders.push(dir.clone());
        }
        s.browser_names.insert(dir, BROWSER_NAME.into());
        crate::browser::save_folders(s);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("os_sound_kit_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The kit is made once: eighteen WAVs that read back as audio, in `Sounds/Mango Kit`, listed in the Browser as
    /// "Mango Kit" (once); a second call does nothing, and a folder the user deleted is not made again.
    #[test]
    fn the_kit_is_made_once_and_the_browser_lists_it() {
        let root = temp("once");
        let kept = prefs::text(KEY);
        prefs::set_text(KEY, "");
        let mut app = crate::app::App::new(None);
        offer(&mut app.state, &root).unwrap();
        let dir = root.join("Sounds").join("Mango Kit");
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names.len(), 18);
        assert!(names.contains(&"Mango Kick Deep.wav".to_string()) && names.contains(&"Mango Crash.wav".to_string()));
        for n in &names {
            let s = os_engine::sample::load(&dir.join(n)).unwrap_or_else(|e| panic!("{n}: {e}"));
            assert!(s.left.len() > 4000 && s.left.iter().any(|v| v.abs() > 0.5), "{n} holds audio");
        }
        assert!(app.state.browser_folders.contains(&dir));
        assert_eq!(app.state.browser_names.get(&dir).map(String::as_str), Some(BROWSER_NAME));
        let listed = app.state.browser_folders.len();
        offer(&mut app.state, &root).unwrap();
        assert_eq!(app.state.browser_folders.len(), listed, "listed once");
        assert_eq!(ensure(&root).unwrap(), None, "not made twice");
        // Made: deleting it does not bring it back.
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(ensure(&root).unwrap(), None);
        assert!(!dir.exists());
        prefs::set_text(KEY, kept.as_deref().unwrap_or(""));
        let _ = std::fs::remove_dir_all(&root);
    }
}
