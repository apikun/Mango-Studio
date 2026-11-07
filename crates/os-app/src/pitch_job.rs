// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Audio to MIDI (issue #64): an audio clip's sample's pitches become a pattern's notes. The detection is
//! `os_dsp::pitch` (the YIN tracker Tesla's pitch regions use) over the whole decoded sound, run on a thread of
//! its own as the Freesound searches are, so a long sample never stops the sound; the app collects the result in
//! its tick and the notes land in a new pattern on the clip's own channel.
//!
// spec: ui/playlist.md (the audio clip's menu)

use std::sync::mpsc::{channel, Receiver};

use crate::app::App;
use os_model::ClipSource;

/// One note the detector found: its start and end in seconds of the sample, and the MIDI key.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteOut {
    pub start: f64,
    pub end: f64,
    pub key: i32,
}

/// A finished job: the notes of the sample it read, under the sample's name, for the clip's channel.
pub struct Done {
    pub name: String,
    pub ch: u16,
    pub notes: Vec<NoteOut>,
}

/// A running job.
pub struct Job {
    rx: Receiver<Option<Done>>,
}

impl Job {
    /// What the job finished with, when it finished.
    pub fn tick(&mut self) -> Option<Option<Done>> {
        self.rx.try_recv().ok()
    }
}

/// Starts the detection of the audio clip `i`'s sample; the result lands in `App::tick`. Returns the message.
pub fn clip_to_midi(app: &mut App, i: usize) -> String {
    let s = &app.state;
    let Some(c) = crate::playlist::clips(s).get(i).copied() else {
        return "The clip is gone".into();
    };
    let ClipSource::Channel(ch) = c.source else {
        return "Only an audio clip converts to MIDI".into();
    };
    let Some(chan) = s.project.channel(ch) else {
        return "The clip's channel is gone".into();
    };
    let Some(path) = chan.sample_path.clone() else {
        return "The clip's channel has no sample".into();
    };
    if app.state.pitch_job.is_some() {
        return "A conversion is already running".into();
    }
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sample".into());
    app.state.pitch_job = Some(start(path, name.clone(), chan.index));
    format!("Converting {name} to MIDI...")
}

/// Starts the detection of `path` (resolved like the browser's) on a thread.
pub fn start(path: String, name: String, ch: u16) -> Job {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(detect(&path, name, ch));
    });
    Job { rx }
}

/// The notes of a sample file: `os_dsp::pitch::regions` over the whole decoded sound.
fn detect(path: &str, name: String, ch: u16) -> Option<Done> {
    let file = crate::plugins::resolve_sample(path);
    let s = os_engine::sample::load_shared(std::path::Path::new(&file)).ok()?;
    let regions = os_dsp::pitch::regions(&s.left, &s.right, s.rate);
    let rate = f64::from(s.rate);
    Some(Done {
        name,
        ch,
        notes: regions
            .into_iter()
            .map(|(a, len, key)| NoteOut { start: a as f64 / rate, end: (a + len) as f64 / rate, key })
            .collect(),
    })
}

/// The new pattern's name: the sample's, "MIDI" after it.
fn midi_name(file: &str) -> String {
    let stem = std::path::Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| file.into());
    format!("{stem} MIDI")
}

impl App {
    /// A finished conversion: the notes become a new pattern on the clip's channel, at the project's tempo
    /// (issue #64). A job that found nothing says so.
    pub(crate) fn pitch_done(&mut self, done: Option<Done>) {
        let Some(done) = done else {
            self.state.message = Some("Could not read the sample as audio".into());
            self.invalidate();
            return;
        };
        let s = &mut self.state;
        if done.notes.is_empty() {
            s.message = Some(format!("No pitches found in {}", done.name));
            self.invalidate();
            return;
        }
        let tempo = s.project.tick_tempo(s.project.tempo.unwrap_or(140.0).max(1.0));
        let ppq = f64::from(s.project.ppq.max(1));
        let tick = |secs: f64| (secs * tempo / 60.0 * ppq).round().max(0.0) as u32;
        let ch = done.ch;
        let mut notes: Vec<os_model::Note> = done
            .notes
            .iter()
            .map(|n| {
                let start = tick(n.start);
                let end = tick(n.end).max(start + 1);
                os_model::Note { length: end - start, key: n.key.clamp(0, 131) as u16, ..os_model::Note::step(ch, start) }
            })
            .collect();
        notes.sort_unstable_by_key(|note| note.position);
        let n = crate::patterns::first_empty(s);
        crate::piano::snapshot_as(s, "Convert to MIDI");
        let count = {
            let p = crate::patterns::pat_mut(s, n);
            p.name = Some(midi_name(&done.name));
            p.notes = notes;
            p.notes.len()
        };
        crate::patterns::select(s, n);
        crate::piano::finish_edit(s);
        s.piano.dirty = true;
        s.message = Some(format!("Found {count} notes in {} (pattern {n})", done.name));
        self.invalidate();
    }
}
