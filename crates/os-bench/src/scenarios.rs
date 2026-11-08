// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Synthetic projects the benchmarks render: no fixture files, everything built in
//! code with a fixed seed, so every run does exactly the same work.

use std::path::Path;

use os_model::target;
use os_model::{Automation, AutoPoint, Channel, ChannelKind, Clip, ClipSource, MixerTrack, Note, Pattern, Project, SlotPlugin};

/// Deterministic xorshift64*: the same seed builds the same project on every platform.
pub fn rng(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    seed.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

pub const SEED: u64 = 0x9E37_79B9_7F4A_7C15;
/// PPQ every synthetic project uses (the app default).
pub const PPQ: u16 = 96;
/// Tempo every synthetic project uses.
pub const TEMPO: f64 = 120.0;
/// A 4/4 pattern in ticks.
pub const BARS_TICKS: u32 = 4 * 96;

/// Frames one tick takes at a rate (the engine's tick math: rate·60 / (tempo·ppq)).
pub fn frames_per_tick(rate: u32, tempo: f64, ppq: u16) -> f64 {
    f64::from(rate) * 60.0 / (tempo * f64::from(ppq))
}

/// Ticks equal to `seconds` of audio at a rate.
pub fn ticks_for(seconds: f64, rate: u32) -> usize {
    (seconds * f64::from(rate) / frames_per_tick(rate, TEMPO, PPQ)) as usize
}

/// A channel the way the engine tests build them (spec: plugins/lemon-sampler.md §7 for the
/// empty-sampler fallback tone).
pub fn channel(index: u16, kind: ChannelKind, sample_path: Option<String>, mixer_track: u16) -> Channel {
    Channel {
        index,
        kind,
        name: None,
        plugin: None,
        color: None,
        enabled: true,
        volume: 0.78125,
        pan: 0.0,
        pitch_cents: 0,
        mixer_track,
        sample_path,
        layer_children: vec![],
        envelopes: Default::default(),
        rack: Default::default(),
        plugin_state: None,
        misc: Default::default(),
        sound: Default::default(),
    }
}

/// A pattern holding `notes`, 1-based numbering like the model wants.
pub fn pattern(number: u16, notes: Vec<Note>) -> Pattern {
    Pattern {
        number,
        name: None,
        color: None,
        notes,
        controls: vec![],
        length: None,
        loops: vec![],
        loop_off: None,
        markers: vec![],
    }
}

fn note(channel: u16, position: u32, key: u16, length: u32) -> Note {
    let mut n = Note::step(channel, position);
    n.key = key;
    n.length = length;
    n.velocity = 100;
    n
}

/// One 4-bar pattern: every channel plays one 1-beat note per bar.
fn pattern_for_channels(number: u16, channels: u16) -> Pattern {
    let mut notes = Vec::new();
    for bar in 0..4u32 {
        for c in 0..channels {
            notes.push(note(c, bar * 96, 48 + ((bar as u16) * 7 + c * 5) % 36, 96));
        }
    }
    pattern(number, notes)
}

fn song_of_patterns(pattern_count: u16) -> Vec<Clip> {
    (0..pattern_count)
        .map(|p| Clip::new(p as u32 * BARS_TICKS, BARS_TICKS, (p % 4) as u16, ClipSource::Pattern(p + 1)))
        .collect()
}

/// A synthesized 2-channel sine WAV, as a stand-in sample for the sampler benches.
pub fn write_sine_wav(path: &Path, rate: u32, secs: f64) -> std::io::Result<u64> {
    let frames = (secs * f64::from(rate)) as usize;
    let mut buf = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let t = i as f64 / f64::from(rate);
        let l = (t * 440.0 * std::f64::consts::TAU).sin() as f32 * 0.5;
        let r = (t * 554.37 * std::f64::consts::TAU).sin() as f32 * 0.5;
        buf.push(l);
        buf.push(r);
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    os_engine::wav::write_f32_stereo(&mut out, rate, &buf)?;
    Ok((frames * 8 + 44) as u64)
}

/// A benchmark scenario: a project plus what the runner must know about it.
pub struct Scene {
    pub name: String,
    pub desc: String,
    pub project: Project,
}

/// The state every saved mixer slot reads: a version field then parameter fields. Slots
/// only need to *build* for the bench, so one wide layout covers every parser.
pub fn fx_state() -> Vec<u8> {
    let mut fields = vec![7i32];
    fields.resize(64, 0);
    fields.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Candidate states tried in order until an effect builds (the first that does is used):
/// every version header the parsers have been seen to read, then parameter fields of zeros.
pub fn fx_state_candidates() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for version in 0..=7i32 {
        let mut fields = vec![version];
        fields.resize(64, 0);
        out.push(fields.iter().flat_map(|x| x.to_le_bytes()).collect());
    }
    out
}

fn base_project(channels: Vec<Channel>, patterns: Vec<Pattern>, clips: Vec<Clip>, mixer: Vec<MixerTrack>) -> Project {
    Project {
        ppq: PPQ,
        tempo: Some(TEMPO),
        channels,
        patterns,
        mixer,
        arrangements: vec![os_model::Arrangement { clips, ..Default::default() }],
        ..Project::default()
    }
}

fn empty_mixer(tracks: u16) -> Vec<MixerTrack> {
    (0..tracks).map(MixerTrack::new).collect()
}

fn slot(name: &str) -> SlotPlugin {
    SlotPlugin { name: name.to_string(), state: fx_state() }
}

/// An empty song: the engine's floor (mixer pass over silent bars, no channels).
pub fn empty() -> Scene {
    Scene {
        name: "empty".into(),
        desc: "no channels, no notes, silent bars".into(),
        project: base_project(vec![], vec![pattern(1, vec![])], song_of_patterns(1), empty_mixer(1)),
    }
}

/// `n` sampler channels with no sample: each voices the engine's built-in tone.
pub fn tones(n: u16) -> Scene {
    let channels: Vec<Channel> = (0..n).map(|i| channel(i, ChannelKind::Sampler, None, i + 1)).collect();
    let mut mixer = empty_mixer(n + 1);
    mixer[0].slots[0] = Some(slot("Fruity Parametric EQ 2"));
    Scene {
        name: format!("tones-{}", n),
        desc: format!("{} tone channels, one note per bar each, master EQ", n),
        project: base_project(channels, vec![pattern_for_channels(1, n)], song_of_patterns(1), mixer),
    }
}

/// Tones through effect slots: `per_track` slots on every channel's track, `on_master` on
/// track 0. Only effect names the factory builds are placed.
pub fn fx_chain(n_tones: u16, per_track: &[&str], on_master: &[&str]) -> Scene {
    let mut scene = tones(n_tones);
    for t in 1..=n_tones.min(scene.project.mixer.len() as u16 - 1) {
        for (k, name) in per_track.iter().enumerate() {
            scene.project.mixer[t as usize].slots[k] = Some(slot(name));
        }
    }
    for (k, name) in on_master.iter().enumerate() {
        scene.project.mixer[0].slots[k] = Some(slot(name));
    }
    scene.desc = format!(
        "{} tone channels, {} slots per track ({}), {} on master ({})",
        n_tones, per_track.len(), per_track.join(", "), on_master.len(), on_master.join(", "),
    );
    scene.name = "fx-chain".into();
    scene
}

/// Tones plus an automation clip sweeping a channel parameter over the song.
pub fn automation(n_tones: u16) -> Scene {
    let mut scene = tones(n_tones);
    let auto_channel = n_tones;
    scene.project.channels.push(channel(
        auto_channel,
        ChannelKind::Automation,
        None,
        0,
    ));
    scene.project.channels[auto_channel as usize].volume = 1.0;
    scene.project.channels[auto_channel as usize].pan = -1.0;
    scene.project.automations.push(Automation {
        channel: auto_channel,
        targets: vec![target::channel_param(0, 7)],
        points: vec![AutoPoint::new(0.0, 0.0), AutoPoint::new(32.0, 1.0)],
        header: vec![0; 21],
        tail: vec![],
        lfo: Default::default(),
    });
    let bars = scene.project.arrangements[0].clips.iter().map(|c| c.length).max().unwrap_or(BARS_TICKS);
    scene.project.arrangements[0].clips.push(Clip::new(0, bars, 0, ClipSource::Channel(auto_channel)));
    scene.desc = format!("{} tone channels + 1 automation clip sweeping a channel parameter", n_tones);
    scene.name = format!("automation-{}", n_tones);
    scene
}

/// `n` sampler channels playing a real WAV with two overlapping voices per bar each.
pub fn samplers(n: u16, sample: &Path) -> Scene {
    let channels: Vec<Channel> = (0..n)
        .map(|i| channel(i, ChannelKind::Sampler, Some(sample.display().to_string()), i + 1))
        .collect();
    let mut notes = Vec::new();
    for bar in 0..4u32 {
        for c in 0..n {
            notes.push(note(c, bar * 96, 48, 96));
            notes.push(note(c, bar * 96 + 48, 60, 96));
        }
    }
    Scene {
        name: format!("sampler-{}", n),
        desc: format!(
            "{} sampler channels on a decoded WAV, 2 voices per bar each ({} kB file)",
            n,
            std::fs::metadata(sample).map(|m| m.len() / 1024).unwrap_or(0)
        ),
        project: base_project(channels, vec![pattern(1, notes)], song_of_patterns(1), empty_mixer(n + 1)),
    }
}

/// A big arrangement for the model benches: 16 channels, 64 patterns of 256 notes,
/// 64 clips, a slotted mixer and two automation clips.
pub fn dense() -> Scene {
    let mut seed = SEED;
    let channels: Vec<Channel> = (0..16).map(|i| channel(i, ChannelKind::Sampler, None, i % 8 + 1)).collect();
    let patterns: Vec<Pattern> = (0..64)
        .map(|p| {
            let mut notes = Vec::new();
            for _ in 0..256 {
                let x = rng(&mut seed);
                notes.push(note((x % 16) as u16, ((x >> 8) % BARS_TICKS as u64) as u32, 24 + ((x >> 24) % 72) as u16, 24 + ((x >> 40) % 168) as u32));
            }
            notes.sort_by_key(|n| n.position);
            pattern(p as u16 + 1, notes)
        })
        .collect();
    let mut mixer = empty_mixer(9);
    for t in 1..9 {
        mixer[t as usize].slots[0] = Some(slot("Fruity Parametric EQ 2"));
        mixer[t as usize].slots[1] = Some(slot("Fruity Delay 3"));
    }
    let automations = (0..2)
        .map(|a| Automation {
            channel: a,
            targets: vec![target::channel_param(a, 7)],
            points: (0..8).map(|k| AutoPoint::new(k as f64 * 4.0, (k % 2) as f64)).collect(),
            header: vec![0; 21],
            tail: vec![],
            lfo: Default::default(),
        })
        .collect();
    Scene {
        name: "dense".into(),
        desc: "16 channels, 64 patterns x 256 notes, 64 clips, slotted mixer, 2 automation clips".into(),
        project: Project {
            automations,
            ..base_project(channels, patterns, song_of_patterns(64), mixer)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_math_matches_the_engine() {
        // 48 kHz, 120 BPM, PPQ 96: exactly 250 frames per tick.
        assert_eq!(frames_per_tick(48000, 120.0, 96), 250.0);
        assert_eq!(ticks_for(1.0, 48000), 192);
        assert_eq!(ticks_for(30.0, 48000), 5760);
    }

    #[test]
    fn tones_build_the_expected_project() {
        let s = tones(8);
        assert_eq!(s.project.channels.len(), 8);
        assert_eq!(s.project.patterns.len(), 1);
        // 4 bars, 8 channels, 1 note per bar per channel.
        assert_eq!(s.project.patterns[0].notes.len(), 32);
        assert_eq!(s.project.arrangements[0].clips.len(), 1);
        assert_eq!(s.project.mixer.len(), 9);
        assert!(s.project.mixer[0].slots[0].is_some());
    }

    #[test]
    fn fx_chain_fills_track_and_master_slots() {
        let s = fx_chain(4, &["Fruity Delay 3", "Fruity Chorus"], &["Fruity Limiter"]);
        assert_eq!(s.project.mixer[1].slots[0].as_ref().unwrap().name, "Fruity Delay 3");
        assert_eq!(s.project.mixer[1].slots[1].as_ref().unwrap().name, "Fruity Chorus");
        assert_eq!(s.project.mixer[0].slots[0].as_ref().unwrap().name, "Fruity Limiter");
        assert!(s.project.mixer[2].slots[2].is_none());
    }

    #[test]
    fn automation_adds_the_clip_channel() {
        let s = automation(8);
        assert_eq!(s.project.channels.len(), 9);
        assert_eq!(s.project.channels[8].kind, ChannelKind::Automation);
        assert_eq!(s.project.automations.len(), 1);
        assert_eq!(s.project.automations[0].targets.len(), 1);
        assert_eq!(s.project.automations[0].channel, 8);
        assert_eq!(s.project.arrangements[0].clips.len(), 2);
    }

    #[test]
    fn dense_is_dense() {
        let s = dense();
        assert_eq!(s.project.channels.len(), 16);
        assert_eq!(s.project.patterns.len(), 64);
        assert_eq!(s.project.patterns.iter().map(|p| p.notes.len()).sum::<usize>(), 64 * 256);
        assert_eq!(s.project.arrangements[0].clips.len(), 64);
        assert_eq!(s.project.automations.len(), 2);
        // Patterns stay sorted by position, as the model wants them.
        for p in &s.project.patterns {
            assert!(p.notes.windows(2).all(|w| w[0].position <= w[1].position));
        }
    }

    #[test]
    fn fx_states_are_wide_enough_for_every_parser() {
        for c in fx_state_candidates() {
            assert!(c.len() % 4 == 0);
        }
        assert!(fx_state().len() >= 256);
    }

    #[test]
    fn a_sine_wav_synthesizes() {
        let path = std::env::temp_dir().join(format!("os-bench-test-{}.wav", std::process::id()));
        let bytes = write_sine_wav(&path, 48000, 0.1).unwrap();
        assert!(bytes > 44);
        assert!(path.exists());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_engine_plays_the_tone_scene() {
        // The empty-sampler fallback must actually sound, or every engine bench is a mixer pass.
        let s = tones(2);
        let mut lib = os_engine::SampleLibrary::new(None);
        let mut e = os_engine::Engine::new(&s.project, &mut lib, 48000, os_engine::PlayMode::Song);
        e.follow_markers();
        let mut out = Vec::new();
        e.render_ticks(384, &mut out); // 4 bars: 1536/4 ticks rendered in a slice
        let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.01, "tone scene rendered silence (peak {})", peak);
    }

    #[test]
    fn a_denser_scene_round_trips_through_flp() {
        let s = dense();
        let file = os_flp::project::create(&s.project).unwrap();
        let bytes = file.to_bytes().unwrap();
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&bytes).unwrap()).unwrap();
        assert_eq!(back.channels.len(), 16);
        assert_eq!(back.patterns.len(), 64);
        assert_eq!(back.patterns[0].notes.len(), 256);
        assert_eq!(back.ppq, PPQ);
    }
}

