// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The sample editor's operations: each takes the loop and a range of frames and gives the loop back changed (or, for
//! the ones that change its length, with how the slice starts move). They work on copies, so an edit can be undone.
//!
// spec: plugins/lemon-slicex.md §6

use os_engine::sample::Sample;

/// Frames `[a, b)` of the loop.
pub type Range = (usize, usize);

/// A piece of audio on the editor's clipboard.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clip {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

/// How an edit that changes the loop's length moves the slice starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shift {
    None,
    /// Frames `[a, b)` were removed.
    Removed(usize, usize),
    /// `n` frames were put in at `at`.
    Inserted(usize, usize),
    /// Only `[a, b)` was kept.
    Kept(usize, usize),
}

impl Shift {
    /// Where a slice that started at `start` starts now.
    pub fn map(self, start: u32) -> u32 {
        let s = start as usize;
        (match self {
            Shift::None => s,
            Shift::Removed(a, b) => {
                if s >= b {
                    s - (b - a)
                } else {
                    s.min(a)
                }
            }
            Shift::Inserted(at, n) => {
                if s >= at {
                    s + n
                } else {
                    s
                }
            }
            Shift::Kept(a, b) => s.clamp(a, b) - a,
        }) as u32
    }
}

/// `r` cut to the loop and ordered; the whole loop when it is empty.
pub fn range_or_all(s: &Sample, r: Option<Range>) -> Range {
    match r {
        Some((a, b)) if a != b => (a.min(b).min(s.frames()), a.max(b).min(s.frames())),
        _ => (0, s.frames()),
    }
}

fn map(s: &Sample, (a, b): Range, mut f: impl FnMut(usize, f32, f32) -> (f32, f32)) -> Sample {
    let mut out = s.clone();
    for i in a..b.min(out.left.len()) {
        let (l, r) = f(i - a, out.left[i], out.right[i]);
        out.left[i] = l;
        out.right[i] = r;
    }
    out
}

fn peak(s: &Sample, (a, b): Range) -> f32 {
    (a..b.min(s.frames())).fold(0.0f32, |m, i| m.max(s.left[i].abs()).max(s.right[i].abs()))
}

/// The range's loudest sample brought to full scale; silence stays.
pub fn normalize(s: &Sample, r: Range) -> Sample {
    let p = peak(s, r);
    if p <= 1e-9 {
        return s.clone();
    }
    map(s, r, |_, l, rr| (l / p, rr / p))
}

/// Each of `regions` (frame ranges) normalized on its own.
pub fn normalize_each(s: &Sample, regions: &[Range]) -> Sample {
    regions.iter().fold(s.clone(), |acc, &r| normalize(&acc, r))
}

pub fn fade_in(s: &Sample, r: Range) -> Sample {
    let n = (r.1 - r.0).max(1) as f32;
    map(s, r, |i, l, rr| {
        let g = i as f32 / n;
        (l * g, rr * g)
    })
}

pub fn fade_out(s: &Sample, r: Range) -> Sample {
    let n = (r.1 - r.0).max(1) as f32;
    map(s, r, |i, l, rr| {
        let g = 1.0 - (i as f32 + 1.0) / n;
        (l * g, rr * g)
    })
}

/// How long the declick fades are (s).
const DECLICK_S: f32 = 0.003;

/// A short fade at the start of `r`, so the region begins without a click.
pub fn declick_in(s: &Sample, r: Range) -> Sample {
    let n = ((DECLICK_S * s.rate as f32) as usize).clamp(1, (r.1 - r.0).max(1));
    fade_in(s, (r.0, r.0 + n))
}

pub fn declick_out(s: &Sample, r: Range) -> Sample {
    let n = ((DECLICK_S * s.rate as f32) as usize).clamp(1, (r.1 - r.0).max(1));
    fade_out(s, (r.1 - n, r.1))
}

pub fn reverse(s: &Sample, (a, b): Range) -> Sample {
    let mut out = s.clone();
    out.left[a..b].reverse();
    out.right[a..b].reverse();
    out
}

pub fn reverse_polarity(s: &Sample, r: Range) -> Sample {
    map(s, r, |_, l, rr| (-l, -rr))
}

/// The mean taken out of each channel (the DC offset).
pub fn center(s: &Sample, (a, b): Range) -> Sample {
    let n = (b - a).max(1) as f32;
    let (ml, mr) = (s.left[a..b].iter().sum::<f32>() / n, s.right[a..b].iter().sum::<f32>() / n);
    map(s, (a, b), |_, l, rr| (l - ml, rr - mr))
}

pub fn swap_channels(s: &Sample, r: Range) -> Sample {
    map(s, r, |_, l, rr| (rr, l))
}

/// Both channels the left (`Some(true)`), the right (`Some(false)`) or the mix of the two (`None`).
pub fn to_mono(s: &Sample, r: Range, from: Option<bool>) -> Sample {
    map(s, r, |_, l, rr| {
        let m = match from {
            Some(true) => l,
            Some(false) => rr,
            None => (l + rr) / 2.0,
        };
        (m, m)
    })
}

/// The range silent, the length kept.
pub fn clear(s: &Sample, r: Range) -> Sample {
    map(s, r, |_, _, _| (0.0, 0.0))
}

/// White noise over the range, replacing it.
pub fn noise(s: &Sample, r: Range) -> Sample {
    let mut x = 0x2545_f491u32;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    map(s, r, |_, _, _| (next() * 0.5, next() * 0.5))
}

pub fn copy(s: &Sample, (a, b): Range) -> Clip {
    Clip { left: s.left[a..b].to_vec(), right: s.right[a..b].to_vec() }
}

pub fn delete(s: &Sample, (a, b): Range) -> (Sample, Shift) {
    let mut out = s.clone();
    out.left.drain(a..b);
    out.right.drain(a..b);
    (out, Shift::Removed(a, b))
}

/// Only the range kept.
pub fn trim(s: &Sample, (a, b): Range) -> (Sample, Shift) {
    (Sample { rate: s.rate, left: s.left[a..b].to_vec(), right: s.right[a..b].to_vec() }, Shift::Kept(a, b))
}

/// `n` frames of silence put in at `at`.
pub fn insert_silence(s: &Sample, at: usize, n: usize) -> (Sample, Shift) {
    let at = at.min(s.frames());
    let mut out = s.clone();
    out.left.splice(at..at, std::iter::repeat_n(0.0, n));
    out.right.splice(at..at, std::iter::repeat_n(0.0, n));
    (out, Shift::Inserted(at, n))
}

/// The clip put in at `at`, the loop longer.
pub fn paste_insert(s: &Sample, at: usize, c: &Clip) -> (Sample, Shift) {
    let at = at.min(s.frames());
    let mut out = s.clone();
    out.left.splice(at..at, c.left.iter().copied());
    out.right.splice(at..at, c.right.iter().copied());
    (out, Shift::Inserted(at, c.left.len()))
}

/// The clip over the loop from `at`, the length kept (it stops at the end).
pub fn paste_replace(s: &Sample, at: usize, c: &Clip) -> Sample {
    let mut out = s.clone();
    for (i, (&l, &r)) in c.left.iter().zip(&c.right).enumerate() {
        if at + i < out.left.len() {
            out.left[at + i] = l;
            out.right[at + i] = r;
        }
    }
    out
}

/// The clip added to the loop from `at`.
pub fn paste_mix(s: &Sample, at: usize, c: &Clip) -> Sample {
    let mut out = s.clone();
    for (i, (&l, &r)) in c.left.iter().zip(&c.right).enumerate() {
        if at + i < out.left.len() {
            out.left[at + i] += l;
            out.right[at + i] += r;
        }
    }
    out
}

/// Short crossfades where an edit joins audio: `at` is the join (a frame of the new loop).
pub fn smooth_join(s: &Sample, at: usize) -> Sample {
    let n = ((0.002 * s.rate as f32) as usize).max(2);
    let (a, b) = (at.saturating_sub(n), (at + n).min(s.frames()));
    if b <= a + 2 {
        return s.clone();
    }
    // A fade down and up around the join.
    map(s, (a, b), |i, l, r| {
        let k = (i as f32 + 0.5) / (b - a) as f32;
        let g = (2.0 * k - 1.0).abs().max(0.0);
        (l * g, r * g)
    })
}

/// A drum loop stretched to `factor` times its length without changing pitch: the slice starts move and each slice's audio
/// stays as it is, cut short when the next one starts sooner and followed by silence when later. Gives the new loop and
/// the new starts.
pub fn drum_stretch(s: &Sample, starts: &[u32], factor: f64) -> (Sample, Vec<u32>) {
    let frames = s.frames();
    let new_len = ((frames as f64) * factor).round().max(1.0) as usize;
    let mut left = vec![0.0; new_len];
    let mut right = vec![0.0; new_len];
    let mut new_starts = Vec::with_capacity(starts.len());
    let bounds: Vec<usize> = starts.iter().map(|&x| (x as usize).min(frames)).chain(std::iter::once(frames)).collect();
    for i in 0..starts.len() {
        let (from, to) = (bounds[i], bounds[i + 1].max(bounds[i]));
        let at = ((from as f64) * factor).round() as usize;
        let room = if i + 1 < starts.len() { (((bounds[i + 1] as f64) * factor).round() as usize).saturating_sub(at) } else { new_len.saturating_sub(at) };
        let len = (to - from).min(room);
        for k in 0..len {
            if at + k < new_len {
                left[at + k] = s.left[from + k];
                right[at + k] = s.right[from + k];
            }
        }
        // The cut ends with a short fade, so a slice stopped early does not click.
        let fade = ((0.003 * s.rate as f32) as usize).min(len);
        for k in 0..fade {
            let g = k as f32 / fade.max(1) as f32;
            let j = at + len - 1 - k;
            if j < new_len && len < to - from {
                left[j] *= g;
                right[j] *= g;
            }
        }
        new_starts.push(at as u32);
    }
    (Sample { rate: s.rate, left, right }, new_starts)
}

/// `s` as a 32-bit float stereo WAV file's bytes.
pub fn wav_bytes(s: &Sample) -> Vec<u8> {
    let mut inter = Vec::with_capacity(s.frames() * 2);
    for (l, r) in s.left.iter().zip(&s.right) {
        inter.push(*l);
        inter.push(*r);
    }
    let mut out = Vec::new();
    let _ = os_engine::wav::write_f32_stereo(&mut out, s.rate, &inter);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize) -> Sample {
        let left: Vec<f32> = (0..n).map(|i| i as f32 / n as f32 * 0.5).collect();
        Sample { rate: 1000, right: left.iter().map(|v| -v).collect(), left }
    }

    #[test]
    fn levels_fades_and_polarity() {
        let s = ramp(100);
        let n = normalize(&s, (0, 100));
        assert!((peak(&n, (0, 100)) - 1.0).abs() < 1e-6);
        let part = normalize(&s, (0, 50));
        assert!((part.left[49] - 1.0).abs() < 1e-6 && (part.left[60] - s.left[60]).abs() < 1e-9, "only the range");
        let f = fade_in(&n, (0, 100));
        assert_eq!(f.left[0], 0.0);
        assert!(f.left[99] > 0.9);
        let o = fade_out(&n, (0, 100));
        assert!(o.left[99].abs() < 1e-6);
        let p = reverse_polarity(&s, (0, 100));
        assert_eq!(p.left[10], -s.left[10]);
        assert_eq!(normalize(&Sample { rate: 1000, left: vec![0.0; 4], right: vec![0.0; 4] }, (0, 4)).left, vec![0.0; 4]);
    }

    #[test]
    fn reversing_centering_and_channels() {
        let s = ramp(10);
        let r = reverse(&s, (0, 10));
        assert_eq!(r.left[0], s.left[9]);
        let mut d = ramp(10);
        d.left.iter_mut().for_each(|v| *v += 0.3);
        let c = center(&d, (0, 10));
        assert!((c.left.iter().sum::<f32>()).abs() < 1e-5);
        let w = swap_channels(&s, (0, 10));
        assert_eq!(w.left[3], s.right[3]);
        let m = to_mono(&s, (0, 10), Some(true));
        assert_eq!(m.right[4], s.left[4]);
        let mix = to_mono(&s, (0, 10), None);
        assert!(mix.left[4].abs() < 1e-6);
    }

    #[test]
    fn cut_paste_and_the_slices_that_follow() {
        let s = ramp(100);
        let (d, shift) = delete(&s, (20, 40));
        assert_eq!(d.frames(), 80);
        assert_eq!((shift.map(10), shift.map(30), shift.map(50)), (10, 20, 30));
        let (t, shift) = trim(&s, (10, 60));
        assert_eq!((t.frames(), shift.map(5), shift.map(30), shift.map(90)), (50, 0, 20, 50));
        let (i, shift) = insert_silence(&s, 50, 10);
        assert_eq!((i.frames(), i.left[55], shift.map(40), shift.map(60)), (110, 0.0, 40, 70));
        let clip = copy(&s, (0, 10));
        let (p, _) = paste_insert(&s, 50, &clip);
        assert_eq!((p.frames(), p.left[50]), (110, s.left[0]));
        let r = paste_replace(&s, 95, &clip);
        assert_eq!(r.frames(), 100, "stops at the end");
        let m = paste_mix(&s, 0, &clip);
        assert!((m.left[5] - 2.0 * s.left[5]).abs() < 1e-6);
        assert_eq!(clear(&s, (10, 20)).left[15], 0.0);
        assert_eq!(declick_in(&normalize(&s, (0, 100)), (0, 100)).left[0], 0.0);
    }

    /// A drum loop stretched to twice its length: the slices start twice as late, each keeps its audio (its pitch), and the
    /// gap is silence.
    #[test]
    fn a_drum_loop_stretches_without_changing_pitch() {
        let mut s = Sample { rate: 1000, left: vec![0.0; 400], right: vec![0.0; 400] };
        for i in 0..20 {
            s.left[i] = 1.0;
            s.right[i] = 1.0;
            s.left[200 + i] = 0.5;
            s.right[200 + i] = 0.5;
        }
        let (o, starts) = drum_stretch(&s, &[0, 200], 2.0);
        assert_eq!((o.frames(), starts), (800, vec![0, 400]));
        assert_eq!((o.left[10], o.left[410], o.left[100], o.left[300]), (1.0, 0.5, 0.0, 0.0));
        // Squeezed: a slice that does not fit is cut short.
        let (q, st) = drum_stretch(&s, &[0, 200], 0.1);
        assert_eq!((q.frames(), st), (40, vec![0, 20]));
        assert!(q.left[19].abs() < 1.0);
    }

    #[test]
    fn the_loop_is_written_as_a_wave_file() {
        let b = wav_bytes(&ramp(10));
        assert_eq!(&b[..4], b"RIFF");
        assert!(b.len() > 44);
    }
}
