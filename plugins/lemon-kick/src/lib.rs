// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Lemon Kick: Lemon Studio's kick drum (a recreation of FL's Fruit Kick) as a VST3 / CLAP plugin module. The plugin,
//! its parameters and its editor are `lemon_small_gen::fruit_kick` (shared with the other small generators); this
//! crate gives it its own module file.
//!
// spec: plugins/lemon-kick.md, plugins/fruit-kick.md
#![allow(semicolon_in_expressions_from_non_local_macros)]

#[cfg(not(feature = "linked"))]
use nih_plug::prelude::*;

pub use lemon_small_gen::fruit_kick::LemonKick;

// Built as a plugin module: the C entry points hosts look for. Linked into Lemon Studio (`linked` feature): only the
// symbol-free table, so several Lemon plugins fit in one program (spec: plugins/plugin-formats.md).
#[cfg(not(feature = "linked"))]
nih_export_clap!(LemonKick);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(LemonKick);
os_plugkit::linked_clap_entry!(LemonKick);
