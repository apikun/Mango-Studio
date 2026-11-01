// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Transient Slicing & Drum Chopper (Phase 7): High-precision onset detection and rhythmic chopping for breakbeats and audio clips.
//!
//! Features:
//! 1. Audio Onset & Energy Derivative Detection:
//!    - Analyzes audio frames using short-term energy flux and adaptive thresholding.
//!    - Refines detected onsets to exact zero-crossings to eliminate audio clicks and phase discontinuity.
//! 2. Rhythmic Grid Chopping:
//!    - Quantized slicing at Bar (1/1), Beat (1/4), Half-Beat (1/8), Step (1/16), 32nd (1/32), and Triplet (1/6).
//!    - Zero-crossing snapping for grid cuts when audio samples are loaded.
//! 3. Stutter & Roll Generation:
//!    - Divides clips into repeated micro-slices for drum rolls, build-ups, and glitch fills.
//! 4. Seamless Undo / History:
//!    - All chopping operations are recorded as a single snapshot in `crate::history`.

use std::path::Path;
use os_model::ClipSource;
use crate::app::State;
use crate::playlist::{self, clips, ChopGrid};

/// Detects transient onsets in an audio waveform.
/// Returns a sorted list of sample frame indices where transients occur,
/// with each onset refined to the nearest zero-crossing.
pub fn detect_transients(left: &[f32], right: &[f32], rate: u32, sensitivity: f32) -> Vec<usize> {
    let total_frames = left.len().min(right.len());
    if total_frames < 512 || rate == 0 {
        return Vec::new();
    }

    let window_size = 512usize;
    let hop_size = 128usize;
    let min_separation_frames = ((rate as f32) * 0.040).max(512.0) as usize; // 40ms minimum distance
    let sensitivity = sensitivity.clamp(0.0, 1.0);

    // 1. Compute RMS energy per hop
    let num_hops = total_frames.saturating_sub(window_size) / hop_size;
    if num_hops < 4 {
        return Vec::new();
    }

    let mut energies = Vec::with_capacity(num_hops);
    let mut max_energy = 0.0001f32;

    for h in 0..num_hops {
        let start = h * hop_size;
        let end = (start + window_size).min(total_frames);
        let count = end - start;

        let mut sum_sq = 0.0f32;
        for i in 0..count {
            let l = left[start + i];
            let r = right[start + i];
            sum_sq += (l * l + r * r) * 0.5;
        }
        let rms = (sum_sq / count as f32).sqrt();
        energies.push(rms);
        if rms > max_energy {
            max_energy = rms;
        }
    }

    // 2. Compute energy rise (first derivative) & moving average baseline
    let ma_window = 12usize; // ~35ms moving average
    let mut rise = vec![0.0f32; num_hops];
    for h in 1..num_hops {
        rise[h] = (energies[h] - energies[h - 1]).max(0.0);
    }

    // 3. Peak detection with adaptive threshold
    let mut candidate_frames = Vec::new();
    let mult = 1.1 + (1.0 - sensitivity) * 0.8;
    let floor_thresh = max_energy * (0.02 + (1.0 - sensitivity) * 0.08);

    for h in 1..(num_hops - 1) {
        let ma_start = h.saturating_sub(ma_window);
        let ma_sum: f32 = energies[ma_start..h].iter().sum();
        let ma_avg = ma_sum / (h - ma_start).max(1) as f32;

        let threshold = ma_avg * mult + floor_thresh;

        if energies[h] > threshold && rise[h] > 0.004 && rise[h] > rise[h - 1] && rise[h] >= rise[h + 1] {
            let frame = h * hop_size;
            candidate_frames.push(frame);
        }
    }

    // 4. Refine to nearest zero-crossing and enforce minimum separation
    let mut final_onsets = Vec::new();
    let mut last_frame = 0usize;

    for &cand in &candidate_frames {
        if !final_onsets.is_empty() && cand.saturating_sub(last_frame) < min_separation_frames {
            continue;
        }

        let refined = find_nearest_zero_crossing(left, right, cand, 192);
        if final_onsets.is_empty() || refined.saturating_sub(last_frame) >= min_separation_frames {
            final_onsets.push(refined);
            last_frame = refined;
        }
    }

    final_onsets
}

/// Finds the sample frame closest to `target_frame` where the waveform crosses zero.
pub fn find_nearest_zero_crossing(left: &[f32], right: &[f32], target_frame: usize, radius: usize) -> usize {
    let total = left.len().min(right.len());
    if total == 0 {
        return target_frame;
    }

    let start = target_frame.saturating_sub(radius);
    let end = (target_frame + radius + 1).min(total - 1);

    let mut best_frame = target_frame.min(total - 1);
    let mut best_dist = usize::MAX;
    let mut min_val = f32::MAX;
    let mut min_val_frame = best_frame;

    for i in start..end {
        let v0 = (left[i] + right[i]) * 0.5;
        let v1 = (left[i + 1] + right[i + 1]) * 0.5;

        let abs_v0 = v0.abs();
        if abs_v0 < min_val {
            min_val = abs_v0;
            min_val_frame = i;
        }

        // True zero crossing condition
        if v0 * v1 <= 0.0 {
            let dist = i.abs_diff(target_frame);
            if dist < best_dist {
                best_dist = dist;
                best_frame = i;
            }
        }
    }

    if best_dist != usize::MAX {
        best_frame
    } else {
        min_val_frame
    }
}

/// Slices a clip at the given song tick positions.
/// Returns the indices of all resulting clip pieces in the playlist.
pub fn slice_clip_at_ticks(s: &mut State, clip_idx: usize, cut_ticks: &[u32]) -> Vec<usize> {
    let Some(c) = clips(s).get(clip_idx).copied() else {
        return Vec::new();
    };

    // Filter cuts that strictly lie inside the clip's duration, sorted ascending
    let mut valid_cuts: Vec<u32> = cut_ticks
        .iter()
        .copied()
        .filter(|&t| t > c.position && t < c.end())
        .collect();
    valid_cuts.sort_unstable();
    valid_cuts.dedup();

    if valid_cuts.is_empty() {
        return vec![clip_idx];
    }

    let mut result_indices = vec![clip_idx];
    let mut current_idx = clip_idx;

    for tick in valid_cuts {
        if let Some(right_idx) = playlist::split(s, current_idx, tick) {
            result_indices.push(right_idx);
            current_idx = right_idx;
        }
    }

    result_indices
}

/// Slices an audio clip at its detected acoustic transient points.
/// Returns the list of all created clip pieces.
pub fn chop_clip_transients(s: &mut State, clip_idx: usize) -> Vec<usize> {
    let Some(c) = clips(s).get(clip_idx).copied() else {
        return Vec::new();
    };

    let ClipSource::Channel(ch) = c.source else {
        return vec![clip_idx];
    };

    let Some(path) = s.project.channel(ch).and_then(|chan| chan.sample_path.clone()) else {
        return vec![clip_idx];
    };

    let resolved = crate::plugins::resolve_sample(&path);
    let Ok(sample) = os_engine::sample::load_shared(Path::new(&resolved)) else {
        return vec![clip_idx];
    };

    let rate = sample.rate;
    if rate == 0 {
        return vec![clip_idx];
    }

    // Detect transients across the whole sample
    let onsets = detect_transients(&sample.left, &sample.right, rate, 0.50);
    if onsets.is_empty() {
        return vec![clip_idx];
    }

    // Convert sample onset frames to song ticks
    let (w0, w1) = playlist::window(s, &c);
    let ms_per_tick = playlist::to_src(s, c.source, 1.0);
    if ms_per_tick <= 0.0 {
        return vec![clip_idx];
    }

    let frames_per_ms = (rate as f64) / 1000.0;
    let window_start_frame = (w0 * frames_per_ms).round() as usize;
    let window_end_frame = (w1 * frames_per_ms).round() as usize;

    let mut cut_ticks = Vec::new();
    for onset in onsets {
        if onset > window_start_frame && onset < window_end_frame {
            let delta_frames = onset - window_start_frame;
            let delta_ms = (delta_frames as f64) / frames_per_ms;
            let tick = c.position + (delta_ms / ms_per_tick).round() as u32;
            if tick > c.position && tick < c.end() {
                cut_ticks.push(tick);
            }
        }
    }

    slice_clip_at_ticks(s, clip_idx, &cut_ticks)
}

/// Chops a clip into rhythmic segments aligned to the musical grid.
/// If an audio sample is loaded, cut points are snapped to nearby zero-crossings.
pub fn chop_clip_grid(s: &mut State, clip_idx: usize, grid: ChopGrid) -> Vec<usize> {
    let Some(c) = clips(s).get(clip_idx).copied() else {
        return Vec::new();
    };

    let step = grid.ticks(s.project.ppq);
    if step == 0 {
        return vec![clip_idx];
    }

    // Calculate grid alignment
    let mut cut_ticks = Vec::new();
    let mut t = ((c.position + step) / step) * step;
    while t < c.end() {
        if t > c.position {
            cut_ticks.push(t);
        }
        t += step;
    }

    if cut_ticks.is_empty() {
        return vec![clip_idx];
    }

    // Refine to zero crossings if it's an audio clip with loaded sample
    if let ClipSource::Channel(ch) = c.source {
        if let Some(path) = s.project.channel(ch).and_then(|chan| chan.sample_path.clone()) {
            let resolved = crate::plugins::resolve_sample(&path);
            if let Ok(sample) = os_engine::sample::load_shared(Path::new(&resolved)) {
                let (w0, _) = playlist::window(s, &c);
                let ms_per_tick = playlist::to_src(s, c.source, 1.0);
                let rate = sample.rate;
                if ms_per_tick > 0.0 && rate > 0 {
                    let frames_per_ms = (rate as f64) / 1000.0;
                    for tick_ref in &mut cut_ticks {
                        let dt_ticks = *tick_ref - c.position;
                        let dt_ms = f64::from(dt_ticks) * ms_per_tick;
                        let target_frame = ((w0 + dt_ms) * frames_per_ms).round() as usize;
                        let refined_frame = find_nearest_zero_crossing(&sample.left, &sample.right, target_frame, 128);
                        let refined_ms = (refined_frame as f64) / frames_per_ms - w0;
                        if refined_ms > 0.0 {
                            let refined_tick = c.position + (refined_ms / ms_per_tick).round() as u32;
                            if refined_tick > c.position && refined_tick < c.end() {
                                *tick_ref = refined_tick;
                            }
                        }
                    }
                }
            }
        }
    }

    slice_clip_at_ticks(s, clip_idx, &cut_ticks)
}

/// Chops a clip into `repeats` micro-slices and repeats the first slice's content
/// to produce a rapid stutter or drum roll effect.
pub fn chop_clip_stutter(s: &mut State, clip_idx: usize, repeats: usize) -> Vec<usize> {
    let Some(c) = clips(s).get(clip_idx).copied() else {
        return Vec::new();
    };

    if repeats < 2 || c.length < repeats as u32 {
        return vec![clip_idx];
    }

    let slice_len = c.length / repeats as u32;
    let mut cut_ticks = Vec::new();
    for i in 1..repeats {
        let t = c.position + slice_len * (i as u32);
        if t < c.end() {
            cut_ticks.push(t);
        }
    }

    let slices = slice_clip_at_ticks(s, clip_idx, &cut_ticks);

    // Stutter repeat: repeat the first piece's audio content across all pieces
    if let Some(&first_idx) = slices.first() {
        if let Some(first_clip) = clips(s).get(first_idx).copied() {
            let first_win = playlist::window(s, &first_clip);
            let arr = playlist::arr_mut(s);
            for &idx in &slices[1..] {
                if let Some(piece) = arr.clips.get_mut(idx) {
                    piece.window = Some(first_win);
                }
            }
        }
    }

    slices
}

/// Chops clip `target` with `chop_fn`, and the other selected clips with it when it is one of them (as FL chops the
/// selection); one undo step. The pieces become the selection. Returns how many pieces there are.
pub fn chop_selected_or_target<F>(s: &mut State, target: usize, chop_fn: F) -> usize
where
    F: Fn(&mut State, usize) -> Vec<usize>,
{
    let mut targets: Vec<usize> = if s.playlist.sel.contains(&target) { s.playlist.sel.clone() } else { vec![target] };
    targets.retain(|&k| k < clips(s).len());
    if targets.is_empty() {
        return 0;
    }
    playlist::snapshot_as(s, "Chop clips");
    let mut pieces = Vec::new();
    for k in targets {
        pieces.extend(chop_fn(s, k));
    }
    let count = pieces.len();
    s.playlist.sel = pieces;
    s.piano.dirty = true;
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_crossing_detection_finds_exact_root() {
        // Construct a synthetic waveform with a clear zero crossing at index 50
        let mut left = vec![0.5f32; 100];
        let mut right = vec![0.5f32; 100];
        for i in 50..100 {
            left[i] = -0.5;
            right[i] = -0.5;
        }

        let zc = find_nearest_zero_crossing(&left, &right, 48, 10);
        // Between index 49 and 50 is a zero crossing
        assert!(zc == 49 || zc == 50, "Zero crossing found near target: {zc}");
    }

    #[test]
    fn transient_detection_identifies_synthetic_drum_hit() {
        // Construct 1 second at 44.1kHz with silence, then an impulse burst at frame 10000
        let rate = 44100u32;
        let mut left = vec![0.001f32; 44100];
        let mut right = vec![0.001f32; 44100];

        // Transient burst at frame 10000
        for i in 0..1000 {
            let amp = (1.0 - (i as f32 / 1000.0)) * ((i as f32 * 0.1).sin());
            left[10000 + i] = amp;
            right[10000 + i] = amp;
        }

        let onsets = detect_transients(&left, &right, rate, 0.5);
        println!("ONSETS: {onsets:?}");
        assert!(!onsets.is_empty(), "Transient detected");
        // Onset should be near frame 10000
        assert!(onsets.iter().any(|&f| f.abs_diff(10000) < 1024), "Onset detected near burst: {onsets:?}");
    }

    /// An audio clip of four drum hits a half second apart, the first at its start, chops into four pieces at the hits.
    #[test]
    fn an_audio_clip_chops_at_its_drum_hits() {
        use crate::app::App;
        let rate = 44100u32;
        let mut wave = vec![0.0f32; rate as usize * 2];
        for hit in 0..4 {
            let at = hit * rate as usize / 2;
            for i in 0..3000 {
                wave[at + i] = (1.0 - i as f32 / 3000.0) * (i as f32 * 0.3).sin() * 0.8;
            }
        }
        let path = std::env::temp_dir().join(format!("mango-drum-hits-{}.wav", std::process::id()));
        let s = tesla::sample::Sample::from_channels(rate, vec![wave.clone(), wave], tesla::sample::Format::Int16);
        std::fs::write(&path, tesla::wav::write(&s, tesla::wav::Depth::Own)).unwrap();
        let mut app = App::new(None);
        app.state.project = crate::app::test_project();
        app.state.playlist.pos = 0;
        app.sample_from_plugin(path.to_string_lossy().into_owned(), false);
        let i = *app.state.playlist.sel.first().expect("the audio clip");
        let m = playlist::run(&mut app, playlist::Cmd::ChopTransients(i));
        let _ = std::fs::remove_file(&path);
        assert_eq!(m.as_deref(), Some("Chopped into 4 slices at the drum hits"));
    }

    #[test]
    fn chop_grid_ticks_calculation() {
        let ppq = 96u16;
        assert_eq!(ChopGrid::Bar.ticks(ppq), 96 * 4);
        assert_eq!(ChopGrid::Beat.ticks(ppq), 96);
        assert_eq!(ChopGrid::HalfBeat.ticks(ppq), 48);
        assert_eq!(ChopGrid::Step.ticks(ppq), 24);
        assert_eq!(ChopGrid::ThirtySecond.ticks(ppq), 12);
        assert_eq!(ChopGrid::TripletSixth.ticks(ppq), 64);
    }

    #[test]
    fn playlist_clip_grid_chopping_and_undo_workflow() {
        use crate::app::App;
        use crate::playlist::{self, Cmd, ChopGrid};
        let mut app = App::new(None);
        app.state.project.ppq = 96;
        let clip = os_model::Clip::new(0, 384, 0, os_model::ClipSource::Pattern(1));
        playlist::arr_mut(&mut app.state).clips = vec![clip];

        // 1. Initial State: 1 clip in playlist
        assert_eq!(playlist::clips(&app.state).len(), 1);

        // 2. The clip menu's Region ▸ Chop: the grids and the stutters (a pattern clip has no drum hits to chop at)
        let menu = playlist::clip_menu(&app.state, 0, 100.0, 100.0);
        let sub = menu.items.iter().find(|it| it.label == "Chop").expect("Chop ▸ in the clip menu");
        let labels: Vec<&str> = sub.children.iter().filter(|c| !c.sep).map(|c| c.label.as_ref()).collect();
        assert_eq!(labels, ["Bar", "Half bar", "Beat", "Half beat", "Step", "32nd", "Triplet", "Stutter 4x", "Stutter 8x"]);

        // 3. Chop to Beat (1/4)
        let res = playlist::run(&mut app, Cmd::ChopGrid(0, ChopGrid::Beat));
        assert!(res.is_some());
        assert_eq!(playlist::clips(&app.state).len(), 4, "Clip was sliced into 4 quarter-note pieces");
        let cls = playlist::clips(&app.state);
        assert_eq!(cls[0].position, 0);
        assert_eq!(cls[0].length, 96);
        assert_eq!(cls[1].position, 96);
        assert_eq!(cls[1].length, 96);
        assert_eq!(cls[2].position, 192);
        assert_eq!(cls[2].length, 96);
        assert_eq!(cls[3].position, 288);
        assert_eq!(cls[3].length, 96);

        // 4. Selection contains all 4 pieces
        assert_eq!(app.state.playlist.sel, vec![0, 1, 2, 3]);

        // 5. Undo restores original 1 clip
        app.act(crate::app::Action::PianoUndo);
        assert_eq!(playlist::clips(&app.state).len(), 1, "Undo restored original unsliced clip");
        assert_eq!(playlist::clips(&app.state)[0].length, 384);

        // 6. Stutter 4x
        let res = playlist::run(&mut app, Cmd::ChopStutter(0, 4));
        assert!(res.is_some());
        assert_eq!(playlist::clips(&app.state).len(), 4, "Stutter sliced into 4 pieces");

        // 7. Undo again
        app.act(crate::app::Action::PianoUndo);
        assert_eq!(playlist::clips(&app.state).len(), 1, "Undo restored original clip again");
    }
}
