// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! FILE ▸ Import ▸ Audio file...: the file becomes what the form's kind says (`App::audio_file_chosen`, painted by
//! `views::audio_form` as the MIDI import's form is): an audio clip, as a drop on the playlist makes, or the melody a
//! tracker reads from it, played by 8-bit squares (a 3x Osc channel whose three oscillators are squares) or by Mango
//! Monk. The tracker is YIN (de Cheveigné and Kawahara 2002): the file's frames give a difference function whose first
//! deep dip is the period, and the periods become notes — one line of them, for the tracker reads one pitch at a time,
//! so chords and drums have no melody to give.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

use os_model::Note;
use winit::keyboard::{Key, NamedKey};

use crate::app::App;
use crate::dialog::{spans, Answer, Body, Kind as Icon, Reply};
use crate::history::{self, Snap};
use crate::midi_file::{self as mf, Instrument};
use crate::project_forms::{FHit, Form};

/// The form's caption, as the MIDI import's is.
pub const TITLE: &str = "Import audio data";
/// The kind row's label.
pub const KIND_LABEL: &str = "What the file becomes";
/// The undo step of a melody import (the clip import's step is the playlist's own, "Playlist edit").
pub const AUDIO_STEP: &str = "Import audio file";

/// What the chosen file becomes at Accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The file as it is: an audio clip at the playhead (a drop on the playlist's answer, and the form's default).
    Clip,
    /// The melody as 8-bit squares: a 3x Osc channel whose saved state is squares all round.
    EightBit,
    /// The melody chanted: a Mango Monk channel.
    Monk,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Clip, Kind::EightBit, Kind::Monk];

    /// The combo's entry, as the MIDI import's type combo names the same two.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Clip => "Audio clip",
            Kind::EightBit => Instrument::EightBit.list_label(),
            Kind::Monk => Instrument::Monk.label(),
        }
    }
}

/// The form: the file, and what it becomes.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioForm {
    pub path: PathBuf,
    pub kind: Kind,
    /// The open combo's list (the kind's), if one is.
    pub list: Option<u8>,
}

impl AudioForm {
    pub fn new(path: PathBuf) -> AudioForm {
        AudioForm { path, kind: Kind::Clip, list: None }
    }

    /// The kind combo's text.
    pub fn combo_text(&self) -> String {
        self.kind.label().to_string()
    }

    /// The combo's entries and the chosen one's place.
    pub fn list_items(&self) -> (Vec<String>, usize) {
        let now = Kind::ALL.iter().position(|k| *k == self.kind).unwrap_or(0);
        (Kind::ALL.iter().map(|k| k.label().to_string()).collect(), now)
    }
}

/// The tracker's own sizes, at the tracked rate: the frame of samples, its half window (the difference function's
/// reach), the hop between frames, and the pitch's bounds in Hz. The file is decimated to about `TRACK_RATE` first
/// (a melody's pitches are under `PITCH_MAX`, and the difference function's work falls with the square of the
/// rate), so a song's minutes cost seconds of it. The window keeps its old reach in time (256 at 11025 is the
/// 1024 at 44100 it was), and the frame holds the window plus the longest period: the period's reach is capped at
/// `HALF`, so `2 * HALF` is at most `FRAME`.
const FRAME: usize = 512;
const HALF: usize = 256;
const HOP: usize = 128;
const PITCH_MIN: f64 = 55.0;
const PITCH_MAX: f64 = 1500.0;
/// The tracked rate the decimation aims at.
const TRACK_RATE: f64 = 11025.0;
/// A dip of the normalized difference at or under this is a tone's period.
const STRONG: f64 = 0.10;
/// A frame quieter than this rms is silence.
const FLOOR: f64 = 0.004;
/// The semitones of drift a note forgives before it is another.
const SMALL: f64 = 0.7;
/// The frames of dropout a note survives, and the frames a held pitch needs to be a note.
const GAP: usize = 4;
const MIN_RUN: usize = 4;

fn top(app: &mut App) -> Option<&mut AudioForm> {
    match &mut app.state.dialogs.last_mut()?.body {
        Body::Project(Form::Audio(f)) => Some(f),
        _ => None,
    }
}

/// The file's samples as one channel: the stereo pair's average (a mono file stands alone).
pub fn mono(s: &os_engine::sample::Sample) -> Vec<f32> {
    if s.right.is_empty() {
        return s.left.clone();
    }
    s.left.iter().zip(&s.right).map(|(l, r)| (l + r) * 0.5).collect()
}

/// The samples as the tracker reads them: `dec` of them box-averaged (a crude low-pass, so nothing above the new
/// Nyquist folds in) and every `dec`-th kept, the rate brought to about `TRACK_RATE`. A file already at or under
/// twice that stands as it is.
fn decimate(x: &[f32], rate: u32) -> (Vec<f32>, u32) {
    let dec = ((f64::from(rate) / TRACK_RATE) as usize).max(1);
    if dec == 1 {
        return (x.to_vec(), rate);
    }
    let out = x.chunks(dec).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect();
    let rate = (f64::from(rate) / f64::from(dec as u32)).round() as u32;
    (out, rate.max(1))
}

/// One frame's normalized difference (YIN's d'): the window's mean square difference from itself shifted by tau,
/// against its running mean. A deep dip at tau is a period of tau samples.
fn normalized_difference(x: &[f32], tau_min: usize, tau_max: usize) -> Vec<f64> {
    // The sums in f32: a difference is at most 2 and the window holds at most 512 of them, and the threshold's
    // tenth sits far above what rounding a few thousand f32 adds can lose.
    let mut d = vec![0.0f32; tau_max + 1];
    for tau in tau_min..=tau_max {
        let mut sum = 0.0f32;
        for i in 0..HALF {
            let t = x[i] - x[i + tau];
            sum += t * t;
        }
        d[tau] = sum;
    }
    let mut nd = vec![1.0f64; tau_max + 1];
    let mut run = 0.0f64;
    for tau in tau_min..=tau_max {
        run += f64::from(d[tau]);
        nd[tau] = if run > 0.0 { f64::from(d[tau]) * (tau - tau_min + 1) as f64 / run } else { 1.0 };
    }
    nd
}

/// The frames' pitches: per frame the MIDI key the frame sings (0.0 is unvoiced) and the frame's rms. YIN: the first
/// dip under the threshold, down to its bottom, then a parabolic tweak around it; a frame with no dip deep enough,
/// or too quiet, is unvoiced. Each frame's count goes into `progress` when it is some (the import's panel's %).
fn track(samples: &[f32], rate: u32, progress: Option<&AtomicU64>) -> Vec<(f64, f64)> {
    let tau_min = ((f64::from(rate) / PITCH_MAX) as usize).max(2);
    let tau_max = ((f64::from(rate) / PITCH_MIN) as usize).min(HALF);
    if samples.len() < FRAME || tau_max < tau_min + 2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for f in 0..=(samples.len() - FRAME) / HOP {
        let x = &samples[f * HOP..f * HOP + FRAME];
        if let Some(p) = progress {
            p.fetch_add(1, Ordering::Relaxed);
        }
        let rms = (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / FRAME as f64).sqrt();
        if rms < FLOOR {
            out.push((0.0, rms));
            continue;
        }
        let mean = f64::from(x.iter().sum::<f32>()) / FRAME as f64;
        let x: Vec<f32> = x.iter().map(|v| *v - mean as f32).collect();
        let nd = normalized_difference(&x, tau_min, tau_max);
        let mut best = None;
        for tau in tau_min..=tau_max {
            if nd[tau] < STRONG {
                let mut tau = tau;
                while tau + 1 <= tau_max && nd[tau + 1] < nd[tau] {
                    tau += 1;
                }
                best = Some(tau);
                break;
            }
        }
        if best.is_none() {
            // No dip under the threshold: the deepest one still stands for a noisy tone's period, unless it is deep
            // as noise is.
            let (tau, v) = (tau_min..=tau_max)
                .map(|t| (t, nd[t]))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap_or((tau_min, 1.0));
            if v < STRONG * 2.0 {
                best = Some(tau);
            }
        }
        // The parabolic tweak wants a dip with both neighbours; the frame's key otherwise.
        let Some(best) = best.filter(|&t| t > tau_min && t < tau_max) else {
            out.push((0.0, rms));
            continue;
        };
        let (y0, y1, y2) = (nd[best - 1], nd[best], nd[best + 1]);
        let den = y0 - 2.0 * y1 + y2;
        let shift = if den.abs() > 1e-12 { (0.5 * (y0 - y2) / den).clamp(-1.0, 1.0) } else { 0.0 };
        let f0 = f64::from(rate) / (best as f64 + shift);
        out.push((69.0 + 12.0 * (f0 / 440.0).log2(), rms));
    }
    out
}

/// The frames become notes: the keys rounded (a lone frame between two of one key takes theirs), the runs held and the
/// dropouts closed, and a run of MIN_RUN frames or more is a note at the project's ticks.
fn notes(tracked: &[(f64, f64)], rate: u32, ppq: u16, bpm: f64) -> Vec<Note> {
    let keys: Vec<Option<i32>> = tracked
        .iter()
        .enumerate()
        .map(|(i, (midi, _))| {
            if *midi <= 0.0 {
                return None;
            }
            let k = midi.round() as i32;
            let l = i.checked_sub(1).map_or(0.0, |j| tracked[j].0);
            let r = tracked.get(i + 1).map_or(0.0, |t| t.0);
            let (lk, rk) = (if l > 0.0 { l.round() as i32 } else { k }, if r > 0.0 { r.round() as i32 } else { k });
            // A lone frame between two of one key takes theirs: the period's ghosts jump like that.
            Some(if k != lk && k != rk && lk == rk { lk } else { k })
        })
        .collect();
    // The runs: a note holds while the frames sing its key or near it, and survives a few frames of dropout.
    let mut runs: Vec<(i32, usize, usize, f64)> = Vec::new();
    let mut cur: Option<(i32, usize, usize, f64)> = None;
    let mut gap = 0usize;
    for (i, k) in keys.iter().enumerate() {
        let Some(k) = k else {
            if let Some(c) = cur.as_mut() {
                gap += 1;
                if gap > GAP {
                    runs.push(*c);
                    cur = None;
                }
            }
            continue;
        };
        let (midi, rms) = tracked[i];
        match cur.as_mut() {
            Some(c) if gap <= GAP && (midi - f64::from(c.0)).abs() <= SMALL => {
                c.2 = i;
                c.3 = c.3.max(rms);
                gap = 0;
            }
            _ => {
                if let Some(c) = cur.take() {
                    runs.push(c);
                }
                cur = Some((*k, i, i, rms));
                gap = 0;
            }
        }
    }
    if let Some(c) = cur.take() {
        runs.push(c);
    }
    let tps = bpm / 60.0 * f64::from(ppq);
    let frame_sec = HOP as f64 / f64::from(rate);
    let mut out: Vec<Note> = Vec::new();
    for (key, f0, f1, rms) in runs {
        if f1 - f0 + 1 < MIN_RUN {
            continue;
        }
        let start = (f0 as f64 * frame_sec * tps).round() as u32;
        let end = ((f1 + 1) as f64 * frame_sec * tps).round() as u32;
        let velocity = (30.0 + 140.0 * rms).round().clamp(1.0, 127.0) as u8;
        out.push(Note { position: start, length: end.max(start + 1) - start, key: key.max(0) as u16, velocity, ..Note::step(0, 0) });
    }
    // The ticks' rounding may fold a note into the last one's place: the same key extends it, another key's is left
    // out (its moment is taken).
    let mut folded: Vec<Note> = Vec::new();
    for n in out {
        if let Some(l) = folded.last_mut() {
            if n.position < l.position + l.length - 1 {
                if n.key == l.key {
                    let end = (l.position + l.length).max(n.position + n.length);
                    l.length = end - l.position;
                    l.velocity = l.velocity.max(n.velocity);
                }
                continue;
            }
        }
        folded.push(n);
    }
    folded
}

/// The file's melody as notes at the project's ticks: the file decimated to the tracked rate, YIN's frames, then
/// the notes. One line — the tracker reads the strongest pitch a frame holds, so a chord's notes come one after
/// another, and drums come not at all. The frames' count goes into `done` of `total` when they are some (the
/// import's panel's %, on its thread).
pub fn melody(samples: &[f32], rate: u32, ppq: u16, bpm: f64, progress: Option<(&AtomicU64, &AtomicU64)>) -> Vec<Note> {
    let (x, rate) = decimate(samples, rate);
    if let Some((_, total)) = progress {
        let frames = if x.len() < FRAME { 0 } else { (x.len() - FRAME) / HOP + 1 };
        total.store(frames as u64, Ordering::Relaxed);
    }
    notes(&track(&x, rate, progress.map(|(d, _)| d)), rate, ppq, bpm)
}

impl App {
    /// FILE ▸ Import ▸ Audio file...: the file chosen in the open dialog: the form asks what it becomes (a drop on the
    /// playlist imports its clip without asking, as the playlist's drops do). The file stands read before the form: one
    /// that does not decode is only a message, as a MIDI file that does not parse is.
    pub(crate) fn audio_file_chosen(&mut self, path: PathBuf) {
        if let Err(e) = os_engine::sample::load_shared(&path) {
            return self.tell(Icon::Warning, TITLE, spans(&["Could not read ", &mf::file_name(&path), &format!(": {e}.")]));
        }
        self.show_form(Body::Project(Form::Audio(Box::new(AudioForm::new(path)))));
    }

    /// A click on a part of the audio form; `Some` closes it. With the list open, a click picks from it or only closes
    /// it.
    pub(crate) fn audio_form_click(&mut self, h: FHit) -> Option<Reply> {
        let f = top(self)?;
        if let Some(k) = f.list.take() {
            if let FHit::Item(i) = h {
                if k == 0 && let Some(kind) = Kind::ALL.get(usize::from(i)) {
                    f.kind = *kind;
                }
            }
            self.invalidate();
            return None;
        }
        match h {
            FHit::Combo(0) => f.list = Some(0),
            FHit::Main => return Some(Reply::Button(Answer::Ok)),
            _ => {}
        }
        self.invalidate();
        None
    }

    /// Keys on the audio form: Esc closes an open list, then the form; Enter accepts.
    pub(crate) fn audio_form_key(&mut self, key: &Key) -> Option<Reply> {
        let f = top(self)?;
        if f.list.is_some() {
            if matches!(key, Key::Named(NamedKey::Escape)) {
                f.list = None;
                self.invalidate();
            }
            return None;
        }
        match key {
            Key::Named(NamedKey::Escape) => Some(Reply::Button(Answer::Cancel)),
            Key::Named(NamedKey::Enter) => Some(Reply::Button(Answer::Ok)),
            _ => None,
        }
    }

    /// The form closed: Accept imports; Esc and the caption's × import nothing (as the MIDI form's answer does).
    pub(crate) fn audio_form_closed(&mut self, f: AudioForm, reply: Reply) {
        if reply == Reply::Button(Answer::Ok) {
            self.audio_accepted(f.path, f.kind);
        }
    }

    /// What the form chose: the file as an audio clip at the playhead (one "Playlist edit" step, as a drop makes), or
    /// its melody's notes on a new channel in the current pattern (one history step, when the tracker's thread is
    /// done — `melody_tick`). A file with no melody is only a message.
    fn audio_accepted(&mut self, path: PathBuf, kind: Kind) {
        if kind == Kind::Clip {
            let text = path.to_string_lossy().into_owned();
            let s = &mut self.state;
            let tick = s.playlist.pos;
            let len = crate::playlist::sample_len(s, &text).max(1);
            let track = crate::playlist::free_track(s, s.playlist.track, tick, tick + len);
            s.message = Some(match crate::playlist::add_audio_clip(s, &text, track, tick) {
                Some(_) => format!("Imported audio {}", path.display()),
                None => format!("Could not read {} as audio", path.display()),
            });
            self.invalidate();
            return;
        }
        let sample = match os_engine::sample::load_shared(&path) {
            Ok(s) => s,
            Err(e) => return self.tell(Icon::Warning, TITLE, spans(&["Could not read ", &mf::file_name(&path), &format!(": {e}.")])),
        };
        let ppq = self.state.project.ppq.max(1);
        let bpm = self.state.project.tick_tempo(self.state.project.tempo.unwrap_or(140.0).max(1.0));
        // The tracker works behind the progress panel, on a thread of its own: a song's minutes are seconds of its
        // frames, and the window must not freeze behind them.
        let (done, total) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        let (tx, rx) = channel();
        let file = mf::file_name(&path).to_string();
        let stem = mf::stem_of_audio(&path);
        let job = MelodyJob { rx, name: file.clone(), done: done.clone(), total: total.clone() };
        let spawned = std::thread::Builder::new().name("audio-melody".into()).spawn(move || {
            let notes = melody(&mono(&sample), sample.rate, ppq, bpm, Some((&done, &total)));
            let _ = tx.send(Tracked { path, kind, name: stem, notes });
        });
        match spawned {
            Ok(_) => {
                self.melody_job = Some(job);
                self.invalidate();
            }
            Err(e) => self.tell(Icon::Warning, TITLE, spans(&["Could not start reading ", &file, &format!(": {e}.")])),
        }
    }
}

/// A melody the tracker is reading (the audio import's Accept with 8-bit or monk): the frames' work runs on a thread
/// of its own behind the progress panel — a song's minutes are seconds of it, and the window must not freeze
/// behind them — and the notes come back to where the form left off (`melody_tick`).
pub struct MelodyJob {
    rx: Receiver<Tracked>,
    /// The file's name, for the panel and the messages.
    name: String,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
}

/// What the tracker's thread hands back: the file and the kind the form chose, and the melody's notes.
struct Tracked {
    path: PathBuf,
    kind: Kind,
    /// The channel's name: the file's stem.
    name: String,
    notes: Vec<Note>,
}

/// Follows the tracker's thread: while it works the panel's % is repainted; when it is over, the notes land
/// (`place_melody`) or the file says why it has none.
pub fn melody_tick(app: &mut App) {
    let Some(j) = &app.melody_job else { return };
    let result: Result<Tracked, String> = match j.rx.try_recv() {
        Ok(t) => Ok(t),
        Err(TryRecvError::Empty) => {
            app.damage_rect(crate::loading::panel_rect(app.state.screen).outset(4.0));
            return;
        }
        Err(TryRecvError::Disconnected) => Err("the tracker stopped".into()),
    };
    let job = app.melody_job.take().expect("checked");
    match result {
        Ok(t) if t.notes.is_empty() => app.tell(Icon::Info, TITLE, spans(&["", &job.name, " has no melody to give: the tracker reads one line of notes, and these frames hold none."])),
        Ok(t) => place_melody(app, t),
        Err(e) => app.tell(Icon::Warning, TITLE, spans(&["Could not read ", &job.name, &format!(": {e}.")])),
    }
    app.invalidate();
}

/// The tracked notes land: a new channel of the kind the form chose in the current pattern, one history step, the
/// status message — the tail of the import's Accept that waited for the tracker's frames.
fn place_melody(app: &mut App, t: Tracked) {
    let count = t.notes.len();
    let instrument = match t.kind {
        Kind::EightBit => Instrument::EightBit,
        _ => Instrument::Monk,
    };
    let s = &mut app.state;
    // As every playlist step does (`playlist::snapshot_as`), the arrangement exists before the step.
    crate::playlist::arr_mut(s);
    let snap = Snap::Several(vec![
        Snap::Patterns { patterns: s.project.patterns.clone(), current: s.pattern },
        Snap::Playlist(Box::new(crate::playlist::capture(s))),
        Snap::Tail(Box::new(history::Tail::now(s, s.project.channels.len()))),
    ]);
    history::record(s, AUDIO_STEP, snap);
    let n = s.pattern;
    mf::place_audio(s, mf::Part { name: t.name.clone(), notes: t.notes, controls: Vec::new() }, &t.name, instrument, n);
    s.message = Some(format!("Imported {} as {} ({} note{})", mf::file_name(&t.path), t.kind.label(), count, if count == 1 { "" } else { "s" }));
}

/// The progress panel: the file and how much of the tracker's frames are done.
pub fn paint_melody(c: &mut os_ui::Canvas, t: &os_theme::Palette, j: &MelodyJob) {
    let total = j.total.load(Ordering::Relaxed);
    let p = if total == 0 { 0.0 } else { (j.done.load(Ordering::Relaxed) as f64 / total as f64).min(1.0) as f32 };
    crate::loading::panel(c, t, "Importing audio", &format!("{}   {:.0} %", j.name, p * 100.0), p);
}

/// The wav a test writes: `tones`'s sines one after another (hz, seconds), at 44100.
#[cfg(test)]
fn sine_wav(path: &std::path::Path, tones: &[(f64, f64)], amp: f32) -> PathBuf {
    const RATE: f64 = 44100.0;
    let mut all = Vec::new();
    for (hz, secs) in tones {
        let mut phase = 0.0f64;
        for _ in 0..(RATE * secs) as usize {
            phase += std::f64::consts::TAU * hz / RATE;
            let v = (f64::from(amp) * phase.sin()) as f32;
            // The wav's samples interleave: the frame is the sample twice (the pair's average is the sample).
            all.push(v);
            all.push(v);
        }
    }
    let mut f = std::fs::File::create(path).expect("the test's wav opens");
    os_engine::wav::write_f32_stereo(&mut f, RATE as u32, &all).expect("the test's wav writes");
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        let mut app = App::new(None);
        app.state.project = crate::app::test_project();
        app.state.song_mode = false;
        app.plugins.available = crate::plugins::builtin();
        app
    }

    /// The tracker's thread is given ticks until its notes land (`melody_tick`), then the channel the import made
    /// is hosted (`sync_generators`).
    fn finish_melody(app: &mut App) {
        for _ in 0..5000 {
            melody_tick(app);
            if app.melody_job.is_none() {
                app.sync_generators();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("the tracker did not finish");
    }

    fn tone(hz: f64, secs: f64, amp: f64, rate: u32) -> Vec<f32> {
        let mut phase = 0.0;
        (0..(f64::from(rate) * secs) as usize)
            .map(|_| {
                phase += std::f64::consts::TAU * hz / f64::from(rate);
                (amp * phase.sin()) as f32
            })
            .collect()
    }

    fn form(app: &App) -> Option<&AudioForm> {
        match &app.state.dialogs.last()?.body {
            Body::Project(Form::Audio(f)) => Some(f),
            _ => None,
        }
    }

    /// The tracker reads a sine as its one note: A 440 is key 69, the note holds the second, and the velocity follows
    /// the loudness.
    #[test]
    fn the_tracker_reads_a_sine() {
        let n = melody(&tone(440.0, 1.0, 0.7, 44100), 44100, 96, 120.0, None);
        assert_eq!(n.len(), 1, "one note: {n:?}");
        assert_eq!(n[0].key, 69);
        assert_eq!(n[0].position, 0);
        assert!((185..=200).contains(&n[0].length), "the second's 192 ticks, the edges' frames short of it: {}", n[0].length);
        assert!(n[0].velocity > 90, "a loud sine sings loud: {}", n[0].velocity);
    }

    /// A melody of steps becomes its notes at their places; a quiet file and white noise have no melody to give.
    #[test]
    fn steps_become_notes_and_noise_stays_silent() {
        let mut a = tone(220.0, 0.6, 0.7, 44100);
        a.extend(tone(440.0, 0.6, 0.7, 44100));
        let n = melody(&a, 44100, 96, 120.0, None);
        assert_eq!(n.iter().map(|x| x.key).collect::<Vec<_>>(), [57, 69], "A2 then A3: {n:?}");
        assert!((110..=122).contains(&n[0].length), "0.6 s is 115 ticks: {}", n[0].length);
        assert!(melody(&[0.0; 44100], 44100, 96, 120.0, None).is_empty(), "silence sings nothing");
        // The noise is a xorshift's stream: a hash of the count is a sawtooth, and the tracker reads its pitch.
        let mut st = 0x9E37_79B9u32;
        let noise: Vec<f32> = (0..44100)
            .map(|_| {
                st ^= st << 13;
                st ^= st >> 17;
                st ^= st << 5;
                (st >> 8) as f32 / 8_388_608.0 - 1.0
            })
            .collect();
        assert!(melody(&noise, 44100, 96, 120.0, None).is_empty(), "noise sings nothing");
    }

    /// The form asks what the file becomes; Accept with the clip's kind places it at the playhead as the import always
    /// did.
    #[test]
    fn the_form_places_the_clip() {
        let path = std::env::temp_dir().join(format!("os_audio_form_clip_{}.wav", std::process::id()));
        sine_wav(&path, &[(440.0, 0.5)], 0.7);
        let mut app = test_app();
        app.audio_file_chosen(path.clone());
        let f = form(&app).expect("the form shows");
        assert_eq!(f.kind, Kind::Clip, "the clip is the default");
        assert_eq!(f.combo_text(), "Audio clip");
        let n0 = crate::playlist::clips(&app.state).len();
        app.dialog_click(crate::dialog::DHit::Form(FHit::Main));
        assert!(!app.modal_open(), "the form closed");
        assert_eq!(crate::playlist::clips(&app.state).len(), n0 + 1, "the clip landed");
        assert!(app.state.message.as_deref().is_some_and(|m| m.contains("Imported audio")));
        let _ = std::fs::remove_file(path);
    }

    /// Accept with the 8-bit's kind gives a 3x Osc channel whose state is squares, with the melody's notes; the monk's
    /// kind gives a Mango Monk channel. One history step takes the channel and its notes back.
    #[test]
    fn the_melody_imports_as_eight_bit_or_monk() {
        for (kind, name) in [(Kind::EightBit, "squares"), (Kind::Monk, "monk")] {
            let path = std::env::temp_dir().join(format!("os_audio_form_{name}_{}.wav", std::process::id()));
            sine_wav(&path, &[(440.0, 1.0)], 0.7);
            let mut app = test_app();
            app.audio_file_chosen(path.clone());
            // The kind through the form's own clicks: the combo's list, the kind's entry, then Accept.
            app.audio_form_click(FHit::Combo(0));
            app.audio_form_click(FHit::Item(match kind {
                Kind::EightBit => 1,
                _ => 2,
            }));
            app.dialog_click(crate::dialog::DHit::Form(FHit::Main));
            assert!(!app.modal_open(), "the form closed");
            finish_melody(&mut app);
            let msg = app.state.message.clone().unwrap_or_default();
            assert!(msg.contains("as ") && msg.contains("1 note"), "{msg}");
            match kind {
                Kind::EightBit => {
                    let ch = app.state.project.channels.last().unwrap();
                    let vals = lemon_3x_osc::params::values_of(ch.plugin_state.as_deref().expect("the patch is the state"));
                    for n in 0..3 {
                        assert_eq!(vals[lemon_3x_osc::params::osc(n) + lemon_3x_osc::params::SHAPE], 2, "osc {} is a square", n + 1);
                    }
                }
                _ => {
                    let monks: Vec<&str> = app.state.project.channels.iter().filter_map(|c| c.plugin.as_deref()).collect();
                    assert!(monks.contains(&crate::plugins::MANGO_MONK), "the monk chants: {monks:?}");
                    // The channel the import made is hosted (the engine plays only hosted instruments — the bug the
                    // silent monk import was): the sync picks the new row up.
                    let row = app.state.project.channels.len() - 1;
                    assert!(app.state.hosted_rows.contains(&row), "the monk is hosted: {:?}", app.state.message);
                    assert_eq!(app.plugins.descriptor(row).map(|d| d.name).as_deref(), Some(crate::plugins::MANGO_MONK));
                }
            }
            let notes = app.state.project.patterns[0].notes.clone();
            assert_eq!(notes.len(), 1, "the melody's one note: {notes:?}");
            assert_eq!(notes[0].key, 69);
            assert_eq!(crate::history::undo(&mut app.state), Some(AUDIO_STEP));
            assert!(app.state.project.patterns[0].notes.is_empty(), "the notes went");
            assert_eq!(app.state.project.channels.len(), crate::app::test_project().channels.len(), "the channel went");
            let _ = std::fs::remove_file(&path);
        }
    }

    /// A file with no melody (silence) is only a message; a file that does not decode never shows the form.
    #[test]
    fn silent_files_and_bad_files_say_so() {
        let dir = std::env::temp_dir().join(format!("os_audio_form_none_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let silent = sine_wav(&dir.join("Silence.wav"), &[(440.0, 0.5)], 0.0);
        let bad = dir.join("Nope.wav");
        std::fs::write(&bad, b"x").unwrap();
        let mut app = test_app();
        let before = app.state.project.clone();
        app.audio_file_chosen(bad);
        assert!(form(&app).is_none(), "no form for a file that does not decode");
        let Some(Body::Msg(m)) = app.state.dialogs.last().map(|d| &d.body) else { panic!("a message") };
        assert!(m.plain().contains("Could not read"), "{}", m.plain());
        app.audio_file_chosen(silent);
        let mut f = form(&app).expect("the form shows for the silent file").clone();
        f.kind = Kind::Monk;
        app.audio_form_closed(f, Reply::Button(Answer::Ok));
        finish_melody(&mut app);
        let Some(Body::Msg(m)) = app.state.dialogs.last().map(|d| &d.body) else { panic!("a message") };
        assert!(m.plain().contains("has no melody to give"), "{}", m.plain());
        assert_eq!(app.state.project, before, "nothing changed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The kind combo's list: a click on an entry picks it, a click elsewhere only closes it.
    #[test]
    fn the_kind_combo_picks() {
        let path = std::env::temp_dir().join(format!("os_audio_form_combo_{}.wav", std::process::id()));
        sine_wav(&path, &[(440.0, 0.3)], 0.7);
        let mut app = test_app();
        app.audio_file_chosen(path.clone());
        assert!(app.audio_form_click(FHit::Combo(0)).is_none());
        assert_eq!(form(&app).unwrap().list, Some(0));
        assert!(app.audio_form_click(FHit::Item(2)).is_none());
        assert_eq!(form(&app).unwrap().kind, Kind::Monk, "the third entry chants");
        assert_eq!(form(&app).unwrap().list, None, "the list closed");
        let (items, now) = form(&app).unwrap().list_items();
        assert_eq!(items, ["Audio clip".to_string(), "8-bit (squares)".to_string(), crate::plugins::MANGO_MONK.to_string()]);
        assert_eq!(now, 2);
        let _ = std::fs::remove_file(path);
    }
}
