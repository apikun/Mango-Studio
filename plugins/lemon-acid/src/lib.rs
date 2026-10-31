// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Lemon Acid: Lemon Studio's acid bass (a clean-room take on the classic 303-style bassline synth) as a VST3 / CLAP
//! plugin module. The plugin, its parameters and its editor are `lemon_small_gen::acid` (shared with the other small
//! generators); this crate gives it its own module file.
//!
// spec: plugins/lemon-acid.md, plugins/lemon-small-generators.md
#![allow(semicolon_in_expressions_from_non_local_macros)]

#[cfg(not(feature = "linked"))]
use nih_plug::prelude::*;

pub use lemon_small_gen::acid::LemonAcid;

// Built as a plugin module: the C entry points hosts look for. Linked into Lemon Studio (`linked` feature): only the
// symbol-free table, so several Lemon plugins fit in one program (spec: plugins/plugin-formats.md).
#[cfg(not(feature = "linked"))]
nih_export_clap!(LemonAcid);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(LemonAcid);
os_plugkit::linked_clap_entry!(LemonAcid);
