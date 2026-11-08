// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The wave's spectrum view (the info strip's Wave / spectrum switch): the loop as a picture of its frequencies over
//! time, one column per column of the well, low sounds at the bottom, louder ones brighter.
//!
// spec: plugins/lemon-slicex.md

/// Samples of one analysis window (a power of two).
const WINDOW: usize = 1024;
/// The lowest frequency shown (Hz); the highest is half the sample rate.
const LOW_HZ: f32 = 40.0;
/// The level (dB below full scale) shown as black.
const FLOOR_DB: f32 = -84.0;

/// In-place radix-2 FFT of `re` + `im` (their length a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

/// The spectrum of the summed channels as `cols` columns of `rows` levels (0 = silent .. 255 = full scale), column by
/// column and each from the top row (the highest frequency) down. Empty when there is nothing to show.
pub fn spectrogram(left: &[f32], right: &[f32], rate: u32, cols: usize, rows: usize) -> Vec<u8> {
    let n = left.len().min(right.len());
    if n == 0 || cols == 0 || rows == 0 || rate == 0 {
        return Vec::new();
    }
    let hann: Vec<f32> = (0..WINDOW).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / WINDOW as f32).cos()).collect();
    // A full-scale sine reads WINDOW / 4 in a bin with this window.
    let norm = WINDOW as f32 / 4.0;
    let nyquist = rate as f32 / 2.0;
    let top = nyquist.max(LOW_HZ * 2.0);
    // The bins each row covers (log spaced between LOW_HZ and the top).
    let edges: Vec<usize> = (0..=rows)
        .map(|r| {
            let hz = LOW_HZ * (top / LOW_HZ).powf(r as f32 / rows as f32);
            ((hz / nyquist * (WINDOW / 2) as f32).round() as usize).clamp(1, WINDOW / 2)
        })
        .collect();
    let mut out = vec![0u8; cols * rows];
    let (mut re, mut im) = (vec![0.0f32; WINDOW], vec![0.0f32; WINDOW]);
    for c in 0..cols {
        let centre = (c * n + n / 2) / cols;
        let start = centre as isize - (WINDOW / 2) as isize;
        for i in 0..WINDOW {
            let k = start + i as isize;
            let v = if k >= 0 && (k as usize) < n { (left[k as usize] + right[k as usize]) * 0.5 } else { 0.0 };
            re[i] = v * hann[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);
        for r in 0..rows {
            let (a, b) = (edges[r], edges[r + 1].max(edges[r] + 1).min(WINDOW / 2));
            let mag = (a..b).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt()).fold(0.0f32, f32::max) / norm;
            let db = 20.0 * (mag + 1e-9).log10();
            let level = ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
            // Row 0 is the top: the highest frequency.
            out[c * rows + (rows - 1 - r)] = (level * 255.0) as u8;
        }
    }
    out
}

/// The colour of a level (0..255): from the well's blue through violet and orange to white.
pub fn colour(level: u8) -> (u8, u8, u8) {
    let t = f32::from(level) / 255.0;
    let lerp = |a: f32, b: f32, k: f32| a + (b - a) * k.clamp(0.0, 1.0);
    let stops: [(f32, [f32; 3]); 4] = [(0.0, [0.18, 0.22, 0.31]), (0.35, [0.37, 0.28, 0.62]), (0.7, [0.95, 0.55, 0.15]), (1.0, [1.0, 0.95, 0.8])];
    for w in stops.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let k = (t - t0) / (t1 - t0);
            return ((lerp(c0[0], c1[0], k) * 255.0) as u8, (lerp(c0[1], c1[1], k) * 255.0) as u8, (lerp(c0[2], c1[2], k) * 255.0) as u8);
        }
    }
    (255, 242, 204)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone lights its own row and not the far ones; silence stays dark; the picture is columns of rows, top down.
    #[test]
    fn a_tone_shows_at_its_pitch() {
        let rate = 48_000u32;
        let tone: Vec<f32> = (0..rate as usize).map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate as f32).sin() * 0.8).collect();
        let (cols, rows) = (8, 48);
        let s = spectrogram(&tone, &tone, rate, cols, rows);
        assert_eq!(s.len(), cols * rows);
        let col = &s[3 * rows..4 * rows];
        let (loudest, level) = col.iter().enumerate().max_by_key(|(_, v)| **v).map(|(i, v)| (i, *v)).unwrap();
        assert!(level > 150, "the tone shows: {level}");
        // 1 kHz of 40 Hz .. 24 kHz on a log scale of 48 rows: about 0.53 of the way up from the bottom.
        let from_bottom = (rows - 1 - loudest) as f32 / rows as f32;
        assert!((from_bottom - 0.53).abs() < 0.08, "the row's height: {from_bottom}");
        assert!(col[0] < 60 && col[rows - 1] < 60, "the ends stay dark");
        let silent = spectrogram(&vec![0.0; 4800], &vec![0.0; 4800], rate, 4, rows);
        assert!(silent.iter().all(|&v| v == 0));
        assert!(spectrogram(&[], &[], rate, 4, rows).is_empty());
    }

    #[test]
    fn levels_run_from_dark_to_bright() {
        let (dark, bright) = (colour(0), colour(255));
        let luma = |c: (u8, u8, u8)| u32::from(c.0) + u32::from(c.1) + u32::from(c.2);
        assert!(luma(bright) > luma(dark) * 2);
    }
}
