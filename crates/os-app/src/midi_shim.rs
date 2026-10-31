// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The `midir` stand-in for Android: the same API surface `midi_in` and `midi_out` use, with
//! no devices ever listed and every connect refused (`midi_in` and `midi_out` alias it as
//! `midir`). The real `midir` stays the dependency on Windows, Linux and macOS; it links ALSA,
//! which Android has no system-wide one of. AMIDI can replace this when Android MIDI arrives.
#![allow(dead_code)]

use std::io;

/// An input client (`midir::MidiInput`): it lists no ports and connects to nothing.
pub struct MidiInput;

/// An output client (`midir::MidiOutput`): it lists no ports and connects to nothing.
pub struct MidiOutput;

/// An input port. None ever exists.
pub struct MidiInputPort;

/// An output port. None ever exists.
pub struct MidiOutputPort;

/// An open input connection, carrying its user data as the real one does.
pub struct MidiInputConnection<T>(std::marker::PhantomData<T>);

/// An open output connection.
pub struct MidiOutputConnection;

/// Which messages the driver filters (the real ones matter only once ports exist).
pub struct Ignore;

impl Ignore {
    pub const None: Ignore = Ignore;
    pub const Sysex: Ignore = Ignore;
    pub const SysEx: Ignore = Ignore;
    pub const TimeCode: Ignore = Ignore;
    pub const SongPos: Ignore = Ignore;
    pub const Sense: Ignore = Ignore;
    pub const ActiveSense: Ignore = Ignore;
    pub const SysexAndActiveSense: Ignore = Ignore;
}

impl MidiInput {
    pub fn new(_client: &str) -> io::Result<MidiInput> {
        Ok(MidiInput)
    }

    pub fn ports(&self) -> Vec<MidiInputPort> {
        Vec::new()
    }

    pub fn port_name(&self, _port: &MidiInputPort) -> io::Result<String> {
        Err(no_midi())
    }

    pub fn ignore(&self, _ignore: Ignore) {}

    pub fn connect<F, T>(&self, _port: &MidiInputPort, _port_name: &str, _callback: F, _data: T) -> io::Result<MidiInputConnection<T>>
    where
        F: FnMut(u64, &[u8], &mut T) + Send + 'static,
        T: Send + 'static,
    {
        Err(no_midi())
    }
}

impl MidiOutput {
    pub fn new(_client: &str) -> io::Result<MidiOutput> {
        Ok(MidiOutput)
    }

    pub fn ports(&self) -> Vec<MidiOutputPort> {
        Vec::new()
    }

    pub fn port_name(&self, _port: &MidiOutputPort) -> io::Result<String> {
        Err(no_midi())
    }

    pub fn connect(&self, _port: &MidiOutputPort, _port_name: &str) -> io::Result<MidiOutputConnection> {
        Err(no_midi())
    }
}

impl MidiOutputConnection {
    pub fn send(&mut self, _message: &[u8]) -> io::Result<()> {
        Err(no_midi())
    }
}

fn no_midi() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "no MIDI devices on Android yet")
}
