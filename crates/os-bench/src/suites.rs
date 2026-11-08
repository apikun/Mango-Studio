// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The benchmark suites: engine open + render over synthetic songs, the mixer-slot
//! effects one by one, the project model's file I/O and note editing, and the WAV
//! writer. Every benchmark owns its setup, so the timed closure does exactly one
//! iteration's work.

use std::path::Path;
use std::time::{Duration, Instant};

use os_engine::effects::from_slot;
use os_engine::{Engine, PlayMode, SampleLibrary};

use crate::harness::Work;
use crate::scenarios::{self, Scene, BARS_TICKS, PPQ, TEMPO};

pub struct Options {
    pub rate: u32,
    pub seconds: f64,
}

/// One runnable benchmark.
pub struct Def {
    pub name: String,
    pub group: &'static str,
    pub desc: String,
    pub work: Work,
    pub note: Option<String>,
    pub f: Box<dyn FnMut() -> Duration>,
}

/// The mixer-slot effects the factory knows, in factory order.
pub const FX_NAMES: &[&str] = &[
    "Fruity Parametric EQ 2",
    "Fruity Limiter",
    "Fruity Reeverb 2",
    "Fruity Delay 3",
    "Fruity Soft Clipper",
    "Fruity Fast Dist",
    "Fruity Balance",
    "Fruity Stereo Enhancer",
    "Fruity Chorus",
    "Fruity Flanger",
    "Fruity Phaser",
    "Vocodex",
    "Fruity Stereo Shaper",
    "Fruity Love Philter",
    "Maximus",
    "Transient Processor",
    "Fruity Filter",
];

/// Builds the first state candidate an effect accepts. Vocodex, Love Philter and
/// Maximus parse structured State bytes instead of int fields, so they build from their
/// own default-state constructors.
fn resolve(name: &str, rate: u32) -> Option<Box<dyn os_engine::effects::Effect>> {
    for state in scenarios::fx_state_candidates() {
        if let Some(fx) = from_slot(name, &state, rate) {
            return Some(fx);
        }
    }
    match name {
        "Vocodex" => Some(Box::new(os_engine::effects::vocoder::VocoderFx::new(
            rate,
            &os_engine::effects::vocoder::Settings::default(),
        ))),
        "Fruity Love Philter" => Some(Box::new(os_engine::effects::love_philter::LovePhilter::with_state(
            os_engine::effects::love_philter::state::State::new_default(),
            rate,
        ))),
        "Maximus" => Some(Box::new(os_engine::effects::maximus::Maximus::with_state(
            os_engine::effects::maximus::state::State::new_default(),
            rate,
        ))),
        _ => None,
    }
}

/// Extends a scene's arrangement with copies of its patterns until the song covers
/// `seconds` plus the engine's 5 s instrument tail, so `render_ticks` always renders
/// exactly the asked number of ticks.
fn fill_song(scene: &mut Scene, rate: u32, seconds: f64) {
    let target = scenarios::ticks_for(seconds + 8.0, rate) as u32;
    let patterns = scene.project.patterns.len() as u16;
    let mut pos = scene.project.arrangements[0].clips.iter().map(|c| c.position + c.length).max().unwrap_or(0);
    let mut p = 0u16;
    while pos < target {
        let clip = os_model::Clip::new(pos, BARS_TICKS, ((pos / BARS_TICKS) % 4) as u16, os_model::ClipSource::Pattern(p % patterns + 1));
        scene.project.arrangements[0].clips.push(clip);
        pos += BARS_TICKS;
        p += 1;
    }
}

fn engine_benches(opts: &Options, scene: &Scene, defs: &mut Vec<Def>) {
    let ticks = scenarios::ticks_for(opts.seconds, opts.rate);
    let frames = ticks as f64 * scenarios::frames_per_tick(opts.rate, TEMPO, PPQ);
    let cap = (frames as usize + 4096) * 2;

    // engine/open-<scene>: building the engine over a project whose samples are cached.
    {
        let project = scene.project.clone();
        let rate = opts.rate;
        let mut lib = SampleLibrary::new(None);
        // Warm the sample cache so the bench measures engine build, not file decode.
        drop(Engine::new(&project, &mut lib, rate, PlayMode::Song));
        defs.push(Def {
            name: format!("engine/open-{}", scene.name),
            group: "engine",
            desc: format!("engine build + markers ({})", scene.desc),
            work: Work::Opens { qty: 1.0 },
            note: None,
            f: Box::new(move || {
                let t = Instant::now();
                let mut e = Engine::new(&project, &mut lib, rate, PlayMode::Song);
                e.follow_markers();
                t.elapsed()
            }),
        });
    }

    // engine/render-<scene>: a fixed tick count, engine rebuilt untimed each iteration.
    {
        let project = scene.project.clone();
        let rate = opts.rate;
        let mut lib = SampleLibrary::new(None);
        drop(Engine::new(&project, &mut lib, rate, PlayMode::Song));
        defs.push(Def {
            name: format!("engine/render-{}", scene.name),
            group: "engine",
            desc: format!("{} ticks ({})", ticks, scene.desc),
            work: Work::Frames { qty: frames, rate: opts.rate },
            note: None,
            f: Box::new(move || {
                let mut e = Engine::new(&project, &mut lib, rate, PlayMode::Song);
                e.follow_markers();
                let t = Instant::now();
                let mut out = Vec::with_capacity(cap);
                e.render_ticks(ticks, &mut out);
                t.elapsed()
            }),
        });
    }
}

fn fx_benches(opts: &Options, defs: &mut Vec<Def>) {
    let rate = opts.rate;
    // One second of interleaved stereo, mid-level noise so limiters and clippers work.
    let mut seed = 0xDEADBEEFu64;
    let block: Vec<f32> = (0..opts.rate as usize * 2)
        .map(|_| {
            let x = crate::scenarios::rng(&mut seed);
            ((x >> 33) % 2000) as f32 / 1000.0 - 1.0
        })
        .collect();
    let mut resolved: Vec<&str> = Vec::new();
    for name in FX_NAMES {
        let mut fx = match resolve(name, rate) {
            Some(fx) => fx,
            None => continue,
        };
        resolved.push(name);
        let mut buf = block.clone();
        let frames = opts.rate as f64;
        let slug = name.to_lowercase().replace([' ', '.'], "-");
        defs.push(Def {
            name: format!("fx/{}", slug),
            group: "fx",
            desc: format!("{} on 1 s of stereo, in place", name),
            work: Work::Frames { qty: frames, rate },
            note: None,
            f: Box::new(move || {
                let t = Instant::now();
                fx.process(&mut buf);
                t.elapsed()
            }),
        });
    }
    if resolved.len() < FX_NAMES.len() {
        let missing: Vec<&str> = FX_NAMES.iter().copied().filter(|n| !resolved.contains(n)).collect();
        eprintln!("note: {} slot effect(s) did not build from any state candidate: {}", missing.len(), missing.join(", "));
    }
}

fn model_benches(defs: &mut Vec<Def>) {
    let scene = scenarios::dense();
    let project = scene.project;

    // model/flp-save
    let bytes = os_flp::project::create(&project).unwrap().to_bytes().unwrap();
    {
        let project = project.clone();
        let len = bytes.len();
        defs.push(Def {
            name: "model/flp-save".into(),
            group: "model",
            desc: "16 channels, 64 patterns x 256 notes, 64 clips -> FLP bytes".into(),
            work: Work::Bytes { qty: len as f64 },
            note: None,
            f: Box::new(move || {
                let t = Instant::now();
                let n = os_flp::project::create(&project).unwrap().to_bytes().unwrap().len();
                let d = t.elapsed();
                debug_assert!(n > 0);
                d
            }),
        });
    }

    // model/flp-load
    {
        let len = bytes.len();
        defs.push(Def {
            name: "model/flp-load".into(),
            group: "model",
            desc: "parse + load the saved FLP back into a Project".into(),
            work: Work::Bytes { qty: len as f64 },
            note: None,
            f: Box::new(move || {
                let t = Instant::now();
                let file = os_flp::FlpFile::parse(&bytes).unwrap();
                let project = os_flp::project::load(&file).unwrap();
                let d = t.elapsed();
                debug_assert_eq!(project.channels.len(), 16);
                d
            }),
        });
    }

    // model/note-ops
    {
        let base = project.patterns[0].clone();
        const INSERTS: usize = 2000;
        defs.push(Def {
            name: "model/note-ops".into(),
            group: "model",
            desc: format!("clone a {}-note pattern, insert {} sorted notes, drop every other", base.notes.len(), INSERTS),
            work: Work::Notes { qty: (INSERTS + base.notes.len() / 2) as f64 },
            note: None,
            f: Box::new(move || {
                let t = Instant::now();
                let mut pat = base.clone();
                let mut seed = crate::scenarios::SEED;
                for _ in 0..INSERTS {
                    let x = crate::scenarios::rng(&mut seed);
                    let pos = (x % BARS_TICKS as u64) as u32;
                    let at = pat.notes.binary_search_by_key(&pos, |n| n.position).unwrap_or_else(|i| i);
                    pat.notes.insert(at, os_model::Note::step(0, pos));
                }
                let mut k = 0usize;
                pat.notes.retain(|_| {
                    k += 1;
                    k % 2 == 0
                });
                t.elapsed()
            }),
        });
    }
}

fn wav_bench(opts: &Options, defs: &mut Vec<Def>) {
    let rate = opts.rate;
    let secs = opts.seconds.min(30.0);
    let buf: Vec<f32> = (0..(secs * rate as f64) as usize * 2).map(|k| if k % 2 == 0 { 0.25 } else { -0.25 }).collect();
    let frames = secs * f64::from(rate);
    defs.push(Def {
        name: format!("wav/write-{:.0}s", secs),
        group: "wav",
        desc: format!("{:.0} s stereo f32 through the WAV encoder into memory", secs),
        work: Work::Frames { qty: frames, rate },
        note: None,
        f: Box::new(move || {
            let t = Instant::now();
            let mut out = Vec::with_capacity(buf.len() * 4 + 44);
            os_engine::wav::write_f32_stereo(&mut out, rate, &buf).unwrap();
            t.elapsed()
        }),
    });
}

/// Every benchmark, in report order. `sample` is the WAV the sampler scenes play.
pub fn build(opts: &Options, sample: &Path) -> Vec<Def> {
    let mut defs = Vec::new();

    // Engine scenes; the fx-chain only places effects the factory actually built.
    let eq_ok = resolve("Fruity Parametric EQ 2", opts.rate).is_some();
    let mut scenes: Vec<Scene> = vec![scenarios::empty()];
    scenes.push(scenarios::tones(8));
    scenes.push(scenarios::tones(32));
    if eq_ok {
        scenes.push(scenarios::fx_chain(
            8,
            &["Fruity Delay 3", "Fruity Chorus"],
            &["Fruity Parametric EQ 2", "Fruity Limiter", "Fruity Reeverb 2", "Fruity Soft Clipper"],
        ));
    }
    scenes.push(scenarios::automation(8));
    if sample.exists() {
        scenes.push(scenarios::samplers(8, sample));
    }
    for mut scene in scenes {
        fill_song(&mut scene, opts.rate, opts.seconds);
        engine_benches(opts, &scene, &mut defs);
    }

    fx_benches(opts, &mut defs);
    model_benches(&mut defs);
    wav_bench(opts, &mut defs);
    defs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{measure, RunCfg, Summary};

    fn quick_opts(rate: u32) -> Options {
        Options { rate, seconds: 2.0 }
    }

    fn temp_sample(rate: u32) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("os-bench-suite-{}.wav", std::process::id()));
        scenarios::write_sine_wav(&p, rate, 0.25).unwrap();
        p
    }

    #[test]
    fn every_effect_resolves_from_a_candidate_state() {
        for name in FX_NAMES {
            assert!(resolve(name, 48000).is_some(), "{} did not build from any candidate state", name);
        }
    }

    #[test]
    fn the_suite_builds_in_a_sane_order() {
        let sample = temp_sample(48000);
        let defs = build(&quick_opts(48000), &sample);
        std::fs::remove_file(&sample).ok();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"engine/open-empty"));
        assert!(names.contains(&"engine/render-tones-32"));
        assert!(names.contains(&"engine/render-fx-chain"));
        assert!(names.contains(&"engine/render-sampler-8"));
        assert!(names.contains(&"fx/fruity-parametric-eq-2"));
        assert!(names.contains(&"model/flp-save"));
        assert!(names.contains(&"wav/write-2s"));
        // Groups come out in report order.
        let groups: Vec<&str> = defs.iter().map(|d| d.group).collect();
        let first_model = groups.iter().position(|g| *g == "model").unwrap();
        let last_engine = groups.iter().rposition(|g| *g == "engine").unwrap();
        assert!(last_engine < first_model);
    }

    #[test]
    fn render_benches_do_the_work_they_claim() {
        // The timed section must render the full tick count, not stop at the song's end.
        let sample = temp_sample(48000);
        let defs = build(&quick_opts(48000), &sample);
        std::fs::remove_file(&sample).ok();
        let mut render = defs.into_iter().find(|d| d.name == "engine/render-tones-8").unwrap();
        // Warmup plus one measured iteration, then check the returned audio was full length
        // by timing it: an empty render of 384 ticks at 250 frames/tick cannot take ~0 time.
        let times = measure(RunCfg { warmup: 1, min_iters: 1, max_iters: 1, target_ms: 0 }, &mut render.f);
        let s = Summary::of(&times.0);
        assert!(s.min > 100_000.0, "render of 2 s of song took only {} ns", s.min);
    }

    #[test]
    fn flp_save_and_load_round_trip_in_the_bench() {
        let scene = scenarios::dense();
        let bytes = os_flp::project::create(&scene.project).unwrap().to_bytes().unwrap();
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&bytes).unwrap()).unwrap();
        assert_eq!(back.patterns.len(), 64);
        assert_eq!(back.mixer.len(), 9);
    }
}
