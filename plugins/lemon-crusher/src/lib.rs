// Copyright (C) 2026 battlemountain2
// Copyright (C) 2026 Artig3nce
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Crusher: Mango Studio's crusher as a VST3 / CLAP plugin module. The plugin, its parameters and its editor are
//! `lemon_small_fx::crusher` (shared with the other small effects); this crate gives it its own module file.
#![allow(semicolon_in_expressions_from_non_local_macros)]

#[cfg(not(feature = "linked"))]
use nih_plug::prelude::*;

pub use lemon_small_fx::crusher::MangoCrusher;

// Built as a plugin module: the C entry points hosts look for. Linked into Mango Studio (`linked` feature): only the
// symbol-free table, so several of its plugins fit in one program (spec: plugins/plugin-formats.md).
#[cfg(not(feature = "linked"))]
nih_export_clap!(MangoCrusher);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(MangoCrusher);
os_plugkit::linked_clap_entry!(MangoCrusher);
