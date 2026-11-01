// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Lemon Bass: Lemon Studio's bass guitar (a recreation of FL's BooBass) as a VST3 / CLAP plugin module. The plugin,
//! its parameters and its editor are `lemon_small_gen::boobass` (shared with the other small generators); this crate
//! gives it its own module file.
//!
// spec: plugins/lemon-bass.md, plugins/boobass.md
#![allow(semicolon_in_expressions_from_non_local_macros)]

#[cfg(not(feature = "linked"))]
use nih_plug::prelude::*;

pub use lemon_small_gen::boobass::LemonBass;

// Built as a plugin module: the C entry points hosts look for. Linked into Lemon Studio (`linked` feature): only the
// symbol-free table, so several Lemon plugins fit in one program (spec: plugins/plugin-formats.md).
#[cfg(not(feature = "linked"))]
nih_export_clap!(LemonBass);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(LemonBass);
os_plugkit::linked_clap_entry!(LemonBass);
