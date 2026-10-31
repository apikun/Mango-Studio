// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Shapes and an LFO baked into automation clips (issue #29). The point menu's Shape ▸ (as Ableton's shape menu
//! names them) fills the segment that ends on a point with a waveform's points; the clip menu's LFO ▸ replaces the
//! curve over a clip's window with a wave. Both bake [`AutoPoint`]s, so the engine's walk, the .flp round trip and
//! the drawn curve need nothing new (spec: audio/automation.md "Segment shapes"; the piano roll's LFO tool,
//! `crate::lfo`, works the same way on control lanes).
//!
//! A wave runs between the segment's (or window's) two levels: 0 stands for the start value and 1 for the end value,
//! so a shape between a higher and a lower level draws the same wave upside down.

use os_model::AutoPoint;

use crate::lfo::Shape as LfoShape;

/// The point menu's Shape ▸ shapes (issue #29, as Ableton names them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoShape {
    Sine,
    Triangle,
    SawUp,
    SawDown,
    Square,
}

impl AutoShape {
    pub const ALL: [AutoShape; 5] = [AutoShape::Sine, AutoShape::Triangle, AutoShape::SawUp, AutoShape::SawDown, AutoShape::Square];

    pub fn name(self) -> &'static str {
        match self {
            AutoShape::Sine => "Sine",
            AutoShape::Triangle => "Triangle",
            AutoShape::SawUp => "Saw up",
            AutoShape::SawDown => "Saw down",
            AutoShape::Square => "Square",
        }
    }
}

/// How many waves a segment of `beats` gets: one a beat, at least 1, at most 16 (a Lemon Studio choice: Ableton's
/// wave count is a handle on the shape, which baked points cannot be dragged to change).
pub fn waves(beats: f64) -> u32 {
    (beats.round() as i64).clamp(1, 16) as u32
}

/// The points that fill the straight segment from (`b0`, `y0`) to (`b1`, `y1`) with `shape`'s wave, one wave a beat
/// ([`waves`]): every wave between the two levels, the wave leaving `y0` and the last one landing on `y1`. The pairs
/// share a beat where the wave jumps (a saw's drop): the walk passes two points at the same place in one step, so the
/// jump lands whole (spec: audio/automation.md "Walking the curve"). Nothing to fill on a zero length.
pub fn shape_points(b0: f64, y0: f64, b1: f64, y1: f64, shape: AutoShape) -> Vec<(f64, f64)> {
    let len = b1 - b0;
    if len <= 0.0 {
        return Vec::new();
    }
    let k = waves(len);
    // `y0 * (1 - w) + y1 * w` maps w 0..1 into the two levels without rounding past them (y0 + (y1 - y0) * w can
    // land a hair outside).
    let at = |x: f64, w: f64| {
        let w = w.clamp(0.0, 1.0);
        (b0 + len * x, y0 * (1.0 - w) + y1 * w)
    };
    match shape {
        // (2k + 1) half-waves: k full waves and a final rise to y1; each half-wave drawn with four points, as points
        // only draw straight lines.
        AutoShape::Sine => {
            let h = (2 * k + 1) as f64;
            let n = (4 * (2 * k + 1)) as f64;
            (1..n as u32).map(|j| at(j as f64 / n, 0.5 - 0.5 * (std::f64::consts::PI * h * j as f64 / n).cos())).collect()
        }
        // (2k + 1) half-waves through their extrema: the straight lines between them are the triangle.
        AutoShape::Triangle => {
            let h = f64::from(2 * k + 1);
            (1..2 * k + 1).map(|j| at(j as f64 / h, if j % 2 == 1 { 1.0 } else { 0.0 })).collect()
        }
        // 2k half-waves, low then high: the last wave ends on its high half, on y1.
        AutoShape::Square => {
            let h = f64::from(2 * k);
            (1..2 * k).map(|j| at(j as f64 / h, if j % 2 == 1 { 1.0 } else { 0.0 })).collect()
        }
        // k rises: each interior boundary carries the last wave's top and the next one's bottom.
        AutoShape::SawUp => (1..k).flat_map(|j| { let (b, w) = at(j as f64 / f64::from(k), 1.0); [(b, w), (b, y0)] }).collect(),
        // k falls: the wave jumps to y1 on the start's beat, and the last fall's bottom lands on the end's beat.
        AutoShape::SawDown => {
            let mut v = vec![(b0, y1)];
            for j in 1..k {
                let (b, w) = at(j as f64 / f64::from(k), 1.0);
                v.push((b, y0));
                v.push((b, w));
            }
            v.push((b1, y0));
            v
        }
    }
}

/// The LFO wave's points over the beat window `a..b` (issue #29): `shape` around `centre` by ±`range`, one period
/// every `period` beats, leaving the centre where the wave starts (the square leaves the top, as the piano roll's LFO
/// does). Points every eighth of a beat, at most 512 (a longer window steps wider); the wave never reaches `b`, so
/// the point that ends the curve there stays the caller's. Nothing to fill on a zero length.
pub fn lfo_points(a: f64, b: f64, shape: LfoShape, centre: f64, range: f64, period: f64) -> Vec<(f64, f64)> {
    let len = b - a;
    if len <= 0.0 || period <= 0.0 {
        return Vec::new();
    }
    let step = 0.125_f64.max(len / 512.0);
    let mut v = Vec::new();
    let mut x = 0.0f64;
    while x < len - 1e-9 {
        let y = (centre + range * shape.at(x / period)).clamp(0.0, 1.0);
        v.push((a + x, y));
        x += step;
    }
    v
}

/// The wave's points as automation points: straight lines between them.
pub(crate) fn baked_points(wave: Vec<(f64, f64)>) -> Vec<AutoPoint> {
    wave.into_iter().map(|(beat, value)| AutoPoint { beat, value: value.clamp(0.0, 1.0), tension: 0.0, mode: os_model::AutoMode::Single, flag: 2 }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// Every shape's wave stays between the two levels, never leaves the segment and lands on its end.
    #[test]
    fn shapes_stay_between_the_levels() {
        let (b0, b1, y0, y1) = (2.0, 6.0, 0.3, 0.9);
        for shape in AutoShape::ALL {
            let pts = shape_points(b0, y0, b1, y1, shape);
            assert!(!pts.is_empty(), "{shape:?}");
            // The saw down's jump shares the start's beat; nothing leaves the segment.
            assert!(pts.iter().all(|(b, _)| *b >= b0 && *b <= b1), "{shape:?}: {pts:?}");
            assert!(pts.windows(2).all(|w| w[0].0 <= w[1].0), "{shape:?} unsorted: {pts:?}");
            let (lo, hi) = (y0.min(y1), y0.max(y1));
            assert!(pts.iter().all(|(_, v)| (lo..=hi).contains(v)), "{shape:?}: {pts:?}");
            // Every interior beat of the saws is shared by the pair that jumps there.
            if matches!(shape, AutoShape::SawUp | AutoShape::SawDown) {
                assert!(pts.windows(2).filter(|w| close(w[0].0, w[1].0)).count() >= 1, "{shape:?} needs a jump: {pts:?}");
            }
        }
    }

    /// One wave a beat: a two-beat segment gets two of them, and the endpoints hold the levels.
    #[test]
    fn one_wave_a_beat() {
        // Sine: k = 2, h = 5 half-waves of four points: peaks at 1/5, 3/5 and the final rise; valleys at 2/5, 4/5.
        let pts = shape_points(0.0, 0.0, 2.0, 1.0, AutoShape::Sine);
        assert_eq!(pts.len(), 4 * 5 - 1);
        let peak = |j: usize| pts[j - 1].1;
        assert!(close(peak(4), 1.0) && close(peak(12), 1.0), "{pts:?}");
        assert!(close(peak(8), 0.0) && close(peak(16), 0.0), "{pts:?}");
        assert!(pts.iter().all(|(b, _)| *b > 0.0 && *b < 2.0));
        // Triangle: the extrema only.
        let pts = shape_points(0.0, 0.2, 2.0, 0.8, AutoShape::Triangle);
        assert_eq!(pts, vec![(2.0 / 5.0, 0.8), (4.0 / 5.0, 0.2), (6.0 / 5.0, 0.8), (8.0 / 5.0, 0.2)]);
        // Square: 50 % duty, low first, the last wave's high half runs to the end.
        let pts = shape_points(0.0, 0.0, 2.0, 1.0, AutoShape::Square);
        assert_eq!(pts, vec![(0.5, 1.0), (1.0, 0.0), (1.5, 1.0)]);
        // Saw up: the tops and the next bottoms share the interior beats; one wave needs no point.
        let pts = shape_points(0.0, 0.0, 2.0, 1.0, AutoShape::SawUp);
        assert_eq!(pts, vec![(1.0, 1.0), (1.0, 0.0)]);
        assert!(shape_points(0.0, 0.0, 1.0, 1.0, AutoShape::SawUp).is_empty());
        // Saw down: the jump on the start's beat, the last bottom on the end's.
        let pts = shape_points(0.0, 0.0, 2.0, 1.0, AutoShape::SawDown);
        assert_eq!(pts, vec![(0.0, 1.0), (1.0, 0.0), (1.0, 1.0), (2.0, 0.0)]);
        // A zero or negative length fills nothing.
        assert!(shape_points(1.0, 0.0, 1.0, 1.0, AutoShape::Sine).is_empty());
        assert!(shape_points(2.0, 0.0, 1.0, 1.0, AutoShape::Sine).is_empty());
        // A short segment still gets its one wave.
        assert_eq!(shape_points(0.0, 0.0, 0.1, 1.0, AutoShape::Triangle).len(), 2);
    }

    /// The LFO wave: it leaves the centre, swings ±range, never reaches the window's end and keeps a point every
    /// eighth of a beat (a long window steps wider).
    #[test]
    fn the_lfo_wave() {
        let pts = lfo_points(0.0, 4.0, LfoShape::Sine, 0.5, 0.25, 4.0);
        assert!(close(pts[0].0, 0.0) && close(pts[0].1, 0.5), "leaves the centre: {:?}", pts[0]);
        let at = |t: f64| pts.iter().find(|(b, _)| close(*b, t)).map(|(_, v)| *v);
        assert!(close(at(1.0).unwrap(), 0.75), "a quarter in, the top: {:?}", at(1.0));
        assert!(close(at(3.0).unwrap(), 0.25), "three quarters in, the bottom: {:?}", at(3.0));
        assert!(pts.iter().all(|(b, _)| *b >= 0.0 && *b < 4.0));
        assert!(pts.iter().all(|(_, v)| (0.25..=0.75).contains(v)));
        assert_eq!(pts.len(), 32, "a point every eighth of a beat");
        // The square leaves the top, as the piano roll's LFO does.
        let sq = lfo_points(0.0, 4.0, LfoShape::Square, 0.5, 0.25, 4.0);
        assert!(close(sq[0].1, 0.75) && close(sq[16].1, 0.25), "{:?} {:?}", sq[0], sq[16]);
        // A long window steps wider, at most 512 points.
        let long = lfo_points(0.0, 1024.0, LfoShape::Sine, 0.5, 0.25, 4.0);
        assert_eq!(long.len(), 512);
        // A zero length or period fills nothing.
        assert!(lfo_points(1.0, 1.0, LfoShape::Sine, 0.5, 0.25, 4.0).is_empty());
        assert!(lfo_points(0.0, 4.0, LfoShape::Sine, 0.5, 0.25, 0.0).is_empty());
    }
}
