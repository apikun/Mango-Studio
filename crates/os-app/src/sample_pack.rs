// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The Mango Pack as files: thirty one-shots from Versilian Studios' Virtuosity Drums, recorded at Virtuosity Musical
//! Instruments and given to everyone under CC0 (`assets/samples/mango-pack/PROVENANCE.md` says which file is which).
//! Written once into `Sounds/Mango Pack` of the user folder and shown in the Browser as the folder "Mango Pack" — a
//! real drum kit and a shelf of percussion beside the Mango Kit's maths-made sounds, so a new session has something to
//! drag in before any sample of one's own exists. Made once: a pack the user deleted, or took out of the Browser, stays
//! gone.

use std::path::{Path, PathBuf};

use crate::prefs;

/// The compiled-in samples: (file name, bytes), from `build.rs`.
static PACK: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/sample_pack.rs"));

/// Where the pack is, under the user folder.
const FOLDER: [&str; 2] = ["Sounds", "Mango Pack"];
/// The name the Browser shows the folder under.
pub const BROWSER_NAME: &str = "Mango Pack";
/// Set once the pack was made.
const KEY: &str = "mango_pack_made";

/// Writes the pack in `root` once, and returns its folder when it was written now. Not again once written (nor over a
/// folder that is there).
pub fn ensure(root: &Path) -> Result<Option<PathBuf>, String> {
    let dir = FOLDER.iter().fold(root.to_path_buf(), |p, part| p.join(part));
    if prefs::text(KEY).is_some() || dir.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (name, bytes) in PACK {
        std::fs::write(dir.join(name), bytes).map_err(|e| e.to_string())?;
    }
    prefs::set_text(KEY, "1");
    Ok(Some(dir))
}

/// Starts with the pack in the Browser: written now (`ensure`), its folder is added to the Browser's folders under its
/// name and the list is kept. The list's order is the user's: the pack goes after the Mango Kit.
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
        let d = std::env::temp_dir().join(format!("os_sample_pack_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The pack is written once: thirty WAVs that read back as audio, in `Sounds/Mango Pack`, listed in the Browser as
    /// "Mango Pack" (once); a second call does nothing, and a folder the user deleted is not written again.
    #[test]
    fn the_pack_is_written_once_and_the_browser_lists_it() {
        let root = temp("once");
        let kept = prefs::text(KEY);
        prefs::set_text(KEY, "");
        let mut app = crate::app::App::new(None);
        offer(&mut app.state, &root).unwrap();
        let dir = root.join("Sounds").join("Mango Pack");
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names.len(), PACK.len());
        assert!(names.contains(&"Mango Kick.wav".to_string()) && names.contains(&"Mango Bell Tree.wav".to_string()));
        assert_eq!(names.len(), 30);
        for n in &names {
            let s = os_engine::sample::load(&dir.join(n)).unwrap_or_else(|e| panic!("{n}: {e}"));
            assert!(s.left.len() > 1500 && s.left.iter().any(|v| v.abs() > 0.5), "{n} holds audio");
        }
        assert!(app.state.browser_folders.contains(&dir));
        assert_eq!(app.state.browser_names.get(&dir).map(String::as_str), Some(BROWSER_NAME));
        let listed = app.state.browser_folders.len();
        offer(&mut app.state, &root).unwrap();
        assert_eq!(app.state.browser_folders.len(), listed, "listed once");
        assert_eq!(ensure(&root).unwrap(), None, "not written twice");
        // Written: deleting it does not bring it back.
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(ensure(&root).unwrap(), None);
        assert!(!dir.exists());
        prefs::set_text(KEY, kept.as_deref().unwrap_or(""));
        let _ = std::fs::remove_dir_all(&root);
    }
}
