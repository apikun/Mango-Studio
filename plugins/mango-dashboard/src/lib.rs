// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Dashboard: Mango Studio's hardware panel (a recreation of FL's Dashboard) as a VST3 / CLAP plugin module.
//! The plugin, its parameters and its editor are `lemon_small_gen::dashboard` (shared with the other small
//! generators); this crate gives it its own module file.
//!
// spec: plugins/lemon-small-generators.md
#![allow(semicolon_in_expressions_from_non_local_macros)]

#[cfg(not(feature = "linked"))]
use nih_plug::prelude::*;

pub use lemon_small_gen::dashboard::MangoDashboard;

// Built as a plugin module: the C entry points hosts look for. Linked into Mango Studio (`linked`
// feature): only the symbol-free table, so several Mango plugins fit in one program (spec:
// plugins/plugin-formats.md).
#[cfg(not(feature = "linked"))]
nih_export_clap!(MangoDashboard);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(MangoDashboard);
os_plugkit::linked_clap_entry!(MangoDashboard);
