// Copyright (C) 2026 Artig3nce, apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Master: a mastering chain (EQ, compressor, width, saturation, maximizer) with loudness meters and Auto Master,
//! which listens to a song and sets the chain for a loudness target. A VST3 / CLAP plugin (nih-plug).
#![allow(semicolon_in_expressions_from_non_local_macros)]

pub mod auto;
pub mod dsp;
pub mod editor;
pub mod params;
pub mod view;

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nih_plug::prelude::*;

use auto::{Capture, Loudness, Solution, FLOOR_LUFS, TARGETS};
use dsp::{gain_to_db, Chain, TruePeak};
use params::MasterParams;

/// How much of a song Auto Master listens to, at most, and how much it needs at least, seconds.
pub const LISTEN_S: f64 = 30.0;
pub const LISTEN_LEAST_S: f64 = 3.0;

/// An f32 the audio thread writes and the editor reads.
#[derive(Default)]
pub struct Reading(AtomicU32);

impl Reading {
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// What the audio thread and the editor share.
#[derive(Default)]
pub struct Shared {
    /// The running sample rate (f64 bits).
    pub rate: AtomicU64,
    /// Output's momentary loudness, integrated loudness since the last reset, true peak, and the maximizer's reduction.
    pub momentary: Reading,
    pub integrated: Reading,
    pub true_peak: Reading,
    pub reduction: Reading,
    /// -1 (out of phase) .. 1 (mono).
    pub correlation: Reading,
    /// The song being captured for Auto Master, while one is. The audio thread only ever tries the lock.
    pub capture: Mutex<Option<Capture>>,
    /// Seconds captured so far (the editor's progress).
    pub captured_s: Reading,
    pub listening: AtomicBool,
    /// A worker is working out the settings.
    pub solving: AtomicBool,
    /// The song Auto Master last heard: a new target is worked out from it again, without listening.
    pub heard: Mutex<Option<Arc<Capture>>>,
    /// What has played through since the plugin started (the first `LISTEN_S`), kept all the while: a target works from
    /// it at once, without pressing Auto Master first.
    pub background: Mutex<Option<Capture>>,
    pub solution: Mutex<Option<Option<Solution>>>,
    /// Ask the audio thread to start the integrated loudness over.
    pub reset_meters: AtomicBool,
}

impl Shared {
    pub fn rate(&self) -> f64 {
        let r = f64::from_bits(self.rate.load(Ordering::Relaxed));
        if r > 0.0 { r } else { 44100.0 }
    }

    /// Starts listening: makes room for the capture here, off the audio thread.
    pub fn start_listening(&self) {
        if let Ok(mut c) = self.capture.lock() {
            *c = Some(Capture::new(LISTEN_S, self.rate()));
        }
        self.captured_s.set(0.0);
        self.listening.store(true, Ordering::Release);
    }

    pub fn cancel_listening(&self) {
        self.listening.store(false, Ordering::Release);
        if let Ok(mut c) = self.capture.lock() {
            *c = None;
        }
    }

    /// Enough has been heard for an answer.
    pub fn can_finish(&self) -> bool {
        f64::from(self.captured_s.get()) >= LISTEN_LEAST_S
    }

    /// Ends listening and works the settings out on a worker thread.
    pub fn finish_listening(self: &Arc<Self>, target: usize) {
        self.listening.store(false, Ordering::Release);
        let Some(capture) = self.capture.lock().ok().and_then(|mut c| c.take()) else { return };
        let capture = Arc::new(capture);
        if let Ok(mut h) = self.heard.lock() {
            *h = Some(capture.clone());
        }
        self.work(capture, target);
    }

    /// Makes room for the background capture, off the audio thread.
    pub fn start_background(&self) {
        if let Ok(mut b) = self.background.lock() {
            *b = Some(Capture::new(LISTEN_S, self.rate()));
        }
    }

    /// Whether there is a song to work a target out from: the one Auto Master heard, else what has played through
    /// (it takes the latter when it holds enough).
    pub fn has_heard(&self) -> bool {
        if self.heard.lock().is_ok_and(|h| h.is_some()) {
            return true;
        }
        let Ok(b) = self.background.lock() else { return false };
        let Some(c) = b.as_ref().filter(|c| c.len() as f64 / self.rate() >= LISTEN_LEAST_S) else { return false };
        let copy = Arc::new(c.clone());
        drop(b);
        if let Ok(mut h) = self.heard.lock() {
            *h = Some(copy);
        }
        true
    }

    /// Works the settings out again for another target from the song heard last (the answer comes as Auto Master's does).
    pub fn resolve(self: &Arc<Self>, target: usize) -> bool {
        if self.solving.load(Ordering::Acquire) {
            return true;
        }
        let Some(capture) = self.heard.lock().ok().and_then(|h| h.clone()) else { return false };
        self.work(capture, target);
        true
    }

    fn work(self: &Arc<Self>, capture: Arc<Capture>, target: usize) {
        let rate = self.rate();
        let target = TARGETS[target.min(TARGETS.len() - 1)];
        self.solving.store(true, Ordering::Release);
        let me = self.clone();
        std::thread::spawn(move || {
            let answer = auto::solve(&capture.left, &capture.right, rate, &target);
            if let Ok(mut s) = me.solution.lock() {
                *s = Some(answer);
            }
            me.solving.store(false, Ordering::Release);
        });
    }
}

/// The most samples the chain takes in one go.
const CHUNK: usize = 256;
/// How long the integrated meter remembers, seconds.
const METER_KEEP_S: f64 = 1200.0;

pub struct MangoMaster {
    params: Arc<MasterParams>,
    shared: Arc<Shared>,
    chain: Chain,
    /// The dry signal's delay in Compare, as long as the chain's.
    dry: Vec<[f32; 2]>,
    dry_at: usize,
    meter_in: Loudness,
    meter_out: Loudness,
    meter_all: Loudness,
    tp: [TruePeak; 2],
    peak: f32,
    /// Compare's level, and the correlation's running sums.
    corr: [f64; 3],
    latency: Option<usize>,
}

impl Default for MangoMaster {
    fn default() -> Self {
        Self::at_rate(44100.0)
    }
}

impl MangoMaster {
    fn at_rate(rate: f64) -> MangoMaster {
        let chain = Chain::new(rate);
        let dry = vec![[0.0; 2]; chain.latency() + 1];
        MangoMaster {
            params: Arc::new(MasterParams::default()),
            shared: Arc::new(Shared::default()),
            chain,
            dry,
            dry_at: 0,
            meter_in: Loudness::new(rate, 0.0),
            meter_out: Loudness::new(rate, 0.0),
            meter_all: Loudness::new(rate, METER_KEEP_S),
            tp: [TruePeak::new(), TruePeak::new()],
            peak: 0.0,
            corr: [0.0; 3],
            latency: None,
        }
    }

    fn set_rate(&mut self, rate: f64) {
        let (params, shared) = (self.params.clone(), self.shared.clone());
        *self = MangoMaster { params, shared, ..Self::at_rate(rate) };
        self.shared.rate.store(rate.to_bits(), Ordering::Relaxed);
    }

    /// One stereo block, in place.
    pub fn block(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.shared.listening.load(Ordering::Acquire)
            && let Ok(mut c) = self.shared.capture.try_lock()
            && let Some(c) = c.as_mut()
        {
            c.push(left, right);
            self.shared.captured_s.set((c.len() as f64 / self.shared.rate()) as f32);
            if c.full() {
                self.shared.listening.store(false, Ordering::Release);
            }
        }
        if let Ok(mut b) = self.shared.background.try_lock()
            && let Some(c) = b.as_mut().filter(|c| !c.full())
        {
            c.push(left, right);
        }
        if self.shared.reset_meters.swap(false, Ordering::AcqRel) {
            self.meter_all = Loudness::new(self.shared.rate(), METER_KEEP_S);
            self.peak = 0.0;
        }
        self.chain.set(self.params.settings());
        let comparing = self.params.comparing();
        // In pieces of a known size, so the dry signal fits on the stack: the audio thread never allocates.
        let mut reduction = 0f32;
        for (l, r) in left.chunks_mut(CHUNK).zip(right.chunks_mut(CHUNK)) {
            let mut input = [[0f32; 2]; CHUNK];
            for (i, f) in input.iter_mut().zip(l.iter().zip(r.iter())) {
                *i = [*f.0, *f.1];
            }
            self.chain.process(l, r);
            reduction = reduction.min(self.chain.take_reduction());
            for (i, (x, y)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                self.meter_in.push(input[i][0], input[i][1]);
                self.meter_out.push(*x, *y);
                self.meter_all.push(*x, *y);
                self.peak = self.peak.max(self.tp[0].run(*x)).max(self.tp[1].run(*y));
                let (a, b) = (f64::from(*x), f64::from(*y));
                const FORGET: f64 = 0.9995;
                self.corr[0] = self.corr[0] * FORGET + a * b;
                self.corr[1] = self.corr[1] * FORGET + a * a;
                self.corr[2] = self.corr[2] * FORGET + b * b;
                // The ring is one longer than the chain's lag: its next slot is the input from `lag` samples ago.
                self.dry[self.dry_at] = input[i];
                self.dry_at = (self.dry_at + 1) % self.dry.len();
                if comparing {
                    let [p, q] = self.dry[self.dry_at];
                    // The song as it came in, at its own level (hearing what the master did to the volume too).
                    *x = p;
                    *y = q;
                }
            }
        }
        self.shared.reduction.set(reduction);
        let s = &self.shared;
        s.momentary.set(self.meter_out.momentary());
        s.integrated.set(self.meter_all.integrated_quick().unwrap_or(FLOOR_LUFS));
        s.true_peak.set(gain_to_db(self.peak));
        let denominator = (self.corr[1] * self.corr[2]).sqrt();
        s.correlation.set(if denominator > 1e-9 { (self.corr[0] / denominator) as f32 } else { 1.0 });
    }
}

impl Plugin for MangoMaster {
    const NAME: &'static str = "Mango Master";
    const VENDOR: &'static str = "Mango Studio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] =
        &[AudioIOLayout { main_input_channels: NonZeroU32::new(2), main_output_channels: NonZeroU32::new(2), ..AudioIOLayout::const_default() }];
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(editor::MasterEditor::new(self.params.clone(), self.shared.clone())))
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, config: &BufferConfig, ctx: &mut impl InitContext<Self>) -> bool {
        self.set_rate(f64::from(config.sample_rate.round().max(1.0)));
        self.shared.start_background();
        self.chain.set(self.params.settings());
        self.chain.reset();
        self.latency = Some(self.chain.latency());
        ctx.set_latency_samples(self.chain.latency() as u32);
        true
    }

    fn reset(&mut self) {
        self.chain.reset();
        self.dry.iter_mut().for_each(|f| *f = [0.0; 2]);
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, _ctx: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let out = buffer.as_slice();
        if out.len() < 2 {
            return ProcessStatus::Normal;
        }
        let (left, right) = out.split_at_mut(1);
        self.block(left[0], right[0]);
        // The look-ahead and the compressor ring on after the input stops.
        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for MangoMaster {
    const CLAP_ID: &'static str = "org.openstudio.mango-master";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Mastering chain with loudness meters and automatic setup");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Mastering, ClapFeature::Limiter, ClapFeature::Stereo];
}

impl Vst3Plugin for MangoMaster {
    const VST3_CLASS_ID: [u8; 16] = *b"MangoStudioMaste";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Mastering, Vst3SubCategory::Dynamics];
}

// Built as a plugin module: the C entry points hosts look for. Linked into Mango Studio (`linked` feature): only the
// symbol-free table, so several plugins fit in one program.
#[cfg(not(feature = "linked"))]
nih_export_clap!(MangoMaster);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(MangoMaster);
os_plugkit::linked_clap_entry!(MangoMaster);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::tests::sine;

    fn plugin(rate: f64) -> MangoMaster {
        let mut p = MangoMaster::default();
        p.set_rate(rate);
        p
    }

    #[test]
    fn compare_is_the_dry_song_at_its_own_level() {
        let rate = 48000.0;
        let mut p = plugin(rate);
        p.params.live.set(p.params.gain.as_ptr(), p.params.gain.preview_normalized(6.0));
        let n = 512;
        let mut wave = sine(1000.0, 0.1, rate, 480000);
        // Warm the meters up, master on.
        for chunk in wave.chunks_mut(n).take(300) {
            let mut r = chunk.to_vec();
            p.block(chunk, &mut r);
        }
        assert_eq!(p.params.settings().gain_db, 6.0);
        p.params.live.set(p.params.compare.as_ptr(), 1.0);
        let source = sine(1000.0, 0.1, rate, 480000);
        let mut last = vec![0f32; n];
        for start in (300 * n..source.len() - n).step_by(n) {
            last.copy_from_slice(&source[start..start + n]);
            let mut r = last.clone();
            p.block(&mut last, &mut r);
        }
        let rms = (last.iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt();
        let want = 0.1 / 2f32.sqrt();
        assert!(gain_to_db(rms / want).abs() < 0.3, "{} dB off: the original, not the 6 dB louder master", gain_to_db(rms / want));
    }

    /// A new Mango Master changes nothing: the song comes out as it went in (delayed by the chain's latency), until Auto
    /// Master or a knob changes it.
    #[test]
    fn a_new_master_leaves_the_song_alone() {
        let rate = 48000.0;
        let mut p = plugin(rate);
        let lag = p.chain.latency();
        let n = 4800usize;
        let src: Vec<f32> = (0..n * 20).map(|i| 0.8 * (i as f32 * 0.07).sin() + 0.15 * (i as f32 * 0.9).sin()).collect();
        let mut out = src.clone();
        for chunk in out.chunks_mut(512) {
            let mut r = chunk.to_vec();
            p.block(chunk, &mut r);
        }
        let worst = (n * 4..src.len() - lag).map(|i| (out[i + lag] - src[i]).abs()).fold(0f32, f32::max);
        assert!(worst < 0.02, "the song changed by {worst}");
    }

    /// What plays through is kept, so a target has a song to work from without Auto Master.
    #[test]
    fn what_plays_through_is_kept_for_the_targets() {
        let rate = 48000.0;
        let mut p = plugin(rate);
        p.shared.start_background();
        assert!(!p.shared.has_heard());
        let mut wave = sine(1000.0, 0.1, rate, 4 * 48000);
        for chunk in wave.chunks_mut(512) {
            let mut r = chunk.to_vec();
            p.block(chunk, &mut r);
        }
        assert!(p.shared.has_heard());
    }

    /// Compare stays lined up with the master whatever the block size, so switching doesn't jump in time.
    #[test]
    fn the_dry_path_has_the_chains_delay() {
        for block in [64, 480, 4096] {
            let mut p = plugin(48000.0);
            p.params.live.set(p.params.compare.as_ptr(), 1.0);
            let mut l = vec![0f32; 16384];
            l[1000] = 0.5;
            let mut r = l.clone();
            let lag = p.chain.latency();
            for (a, b) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
                p.block(a, b);
            }
            let at = l.iter().position(|v| v.abs() > 1e-4).unwrap();
            assert_eq!(at, 1000 + lag, "block {block}");
        }
    }

    #[test]
    fn listening_captures_the_input_and_the_answer_arrives() {
        let rate = 44100.0;
        let mut p = plugin(rate);
        let shared = p.shared.clone();
        shared.start_listening();
        assert!(!shared.can_finish());
        let wave = sine(220.0, 0.1, rate, 44100 * 5);
        for chunk in wave.chunks(512) {
            let (mut l, mut r) = (chunk.to_vec(), chunk.to_vec());
            p.block(&mut l, &mut r);
        }
        assert!(shared.can_finish(), "{}", shared.captured_s.get());
        shared.finish_listening(0);
        for _ in 0..1000 {
            if !shared.solving.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let answer = shared.solution.lock().unwrap().take().expect("an answer").expect("settings");
        assert!((answer.lufs + 14.0).abs() < 0.5, "{}", answer.lufs);
        assert!(shared.capture.lock().unwrap().is_none());
    }

    #[test]
    fn listening_stops_by_itself_when_the_capture_is_full() {
        let mut p = plugin(1000.0);
        p.shared.start_listening();
        let block = vec![0.5f32; 8192];
        for _ in 0..4 {
            let (mut l, mut r) = (block.clone(), block.clone());
            p.block(&mut l, &mut r);
        }
        assert!(!p.shared.listening.load(Ordering::Acquire));
    }

    #[test]
    fn the_meters_read_the_output() {
        let rate = 48000.0;
        let mut p = plugin(rate);
        let wave = sine(997.0, 0.5, rate, 96000);
        for chunk in wave.chunks(512) {
            let (mut l, mut r) = (chunk.to_vec(), chunk.to_vec());
            p.block(&mut l, &mut r);
        }
        let s = &p.shared;
        assert!((s.momentary.get() + 6.02).abs() < 0.3, "{}", s.momentary.get());
        assert!((s.integrated.get() + 6.02).abs() < 0.5, "{}", s.integrated.get());
        assert!((s.true_peak.get() + 6.02).abs() < 0.2, "{}", s.true_peak.get());
        assert!(s.reduction.get().abs() < 0.01);
        assert!((s.correlation.get() - 1.0).abs() < 0.01);
    }
}
