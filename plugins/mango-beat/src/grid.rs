// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Beat's envelope grids: the four time slots and the four volume slots, their points and FL's thirteen
//! segment shapes (ported from the automation curve, `os-model::curve`), and the shared state the editor writes and
//! the sound reads.
//!
// spec: plugins/mango-beat.md (§2 envelopes)

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// One envelope point: `x` across the beat (0..1), `y` the value (0..1), `t` the segment's tension (-1..1) and `m`
/// the segment's shape (`Mode`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
    pub t: f32,
    pub m: u8,
}

impl Pt {
    pub const fn new(x: f32, y: f32, t: f32, m: u8) -> Pt {
        Pt { x, y, t, m }
    }
}

/// The thirteen segment shapes, FL's own (the numbers are the saved `m`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Single = 0,
    Double = 1,
    Stairs = 2,
    SmoothStairs = 3,
    Pulse = 4,
    Wave = 5,
    Single2 = 6,
    Double2 = 7,
    HalfSine = 8,
    Smooth = 9,
    Single3 = 10,
    Double3 = 11,
    Hold = 12,
}

impl Mode {
    pub const ALL: [Mode; 13] = [
        Mode::Single,
        Mode::Double,
        Mode::Stairs,
        Mode::SmoothStairs,
        Mode::Pulse,
        Mode::Wave,
        Mode::Single2,
        Mode::Double2,
        Mode::HalfSine,
        Mode::Smooth,
        Mode::Single3,
        Mode::Double3,
        Mode::Hold,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Mode::Single => "Single curve",
            Mode::Double => "Double curve",
            Mode::Stairs => "Step",
            Mode::SmoothStairs => "Smooth step",
            Mode::Pulse => "Pulse",
            Mode::Wave => "Wave",
            Mode::Single2 => "Single curve 2",
            Mode::Double2 => "Double curve 2",
            Mode::HalfSine => "Half sine",
            Mode::Smooth => "Smooth",
            Mode::Single3 => "Single curve 3",
            Mode::Double3 => "Double curve 3",
            Mode::Hold => "Hold",
        }
    }

    /// Modes with a tension handle on the drawn segment.
    pub const fn has_tension(self) -> bool {
        !matches!(self, Mode::Hold)
    }
}

/// One envelope: the points, kept sorted by `x`; the first point sits at `x` 0.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Curve {
    pub pts: Vec<Pt>,
}

/// `2^-24`: the smallest segment length, slope term and rise the smooth mode works with.
const TINY: f64 = 1.0 / 16_777_216.0;

/// Rounding to the nearest whole number, halves to even (as the automation curve's).
fn round(x: f64) -> f64 {
    x.round_ties_even()
}

/// The count of steps, pulses or waves of the stepped modes: 1000 at tension 0, falling to 0 at full tension.
fn count(t: f32) -> f64 {
    let reach = (1.0 - f64::from(t.abs()) * 0.6) as f32;
    let e = ((f64::from(reach) * 1_000_001f64.ln()).exp() - 1.0) as f32;
    round(f64::from(e) * 0.001)
}

/// The shape of one segment: `progress(x)` maps the position 0..1 along it to 0..1 between its two levels. Its
/// coefficients are worked out once, from the point the segment ends on (and for the smooth mode its neighbours).
/// Ported from the automation curve so a Mango Beat envelope reads like one drawn in the playlist.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    mode: Mode,
    /// The tension is below 0.
    neg: bool,
    /// A straight line, whatever the mode.
    line: bool,
    c0: f32,
    c1: f32,
    c2: f32,
}

impl Segment {
    /// The segment that ends on point `i` (`i >= 1`; point 0 shapes nothing and gives a straight line).
    pub fn new(pts: &[Pt], i: usize) -> Segment {
        let line = Segment { mode: Mode::Single, neg: false, line: true, c0: 0.0, c1: 0.0, c2: 0.0 };
        let Some(p) = pts.get(i).filter(|_| i > 0) else { return line };
        let t = p.t;
        let side: f64 = if t < 0.0 { -1.0 } else { 1.0 };
        let mut s = Segment {
            mode: Mode::ALL.get(p.m as usize).copied().unwrap_or(Mode::Single),
            neg: t < 0.0,
            line: false,
            c0: 0.0,
            c1: 0.0,
            c2: 0.0,
        };
        // Modes 0, 1, 6, 7, 8, 10 and 11 are a straight line at a tension of exactly 0.
        if t == 0.0 && matches!(s.mode, Mode::Single | Mode::Double | Mode::Single2 | Mode::Double2 | Mode::HalfSine | Mode::Single3 | Mode::Double3) {
            return line;
        }
        match s.mode {
            Mode::Single | Mode::Double => {
                let v = ((f64::from(t.abs()) * 1_000_001f64.ln()).exp() - 1.0) as f32;
                s.c1 = (1.0 / f64::from(v)) as f32;
                s.c2 = (f64::from(v) + 1.0).ln() as f32;
            }
            Mode::Stairs => {
                let n = count(t) + 1.0 + if t >= 0.0 { 1.0 } else { 0.0 };
                s.c0 = n as f32;
                s.c1 = (1.0 / (n - side)) as f32;
                s.c2 = (side * 0.5) as f32;
            }
            Mode::SmoothStairs => {
                s.c0 = (count(t) + 1.0) as f32;
                s.c1 = (1.0 / f64::from(s.c0)) as f32;
            }
            Mode::Pulse => {
                s.c0 = ((count(t) + 1.0) * 2.0) as f32;
                s.c2 = (side * 0.5) as f32;
            }
            Mode::Wave => {
                let n = count(t);
                if n == 0.0 && t > 0.0 {
                    return line;
                }
                s.c0 = (n + 0.5) as f32;
                s.c1 = std::f64::consts::TAU as f32;
            }
            Mode::Single2 | Mode::Double2 => s.c0 = t,
            Mode::HalfSine => s.c0 = t.abs(),
            Mode::Single3 | Mode::Double3 => {
                let a = 1.0 - (f64::from(t.abs()) * 0.9).max(1.0 / 65536.0);
                let k = (1.0 - a * a * a) as f32;
                let mut c1 = (1.0 - k) / k;
                let mut c2 = c1;
                if t > 0.0 {
                    c1 = -c1;
                    c2 = c1 - 1.0;
                }
                if s.mode == Mode::Double3 {
                    c1 *= 0.5;
                    c2 *= 0.5;
                }
                (s.c0, s.c1, s.c2) = (k, c1, c2);
            }
            Mode::Smooth => {
                let (y0, y1) = (f64::from(pts[i - 1].y), f64::from(p.y));
                let len = f64::from((p.x - pts[i - 1].x).max(0.0)).max(TINY);
                let rise = y1 - y0;
                if rise.abs() < TINY {
                    return line;
                }
                // The neighbours count when they are smooth too.
                let (mut before, mut back) = (y0, 1.0);
                if pts[i - 1].m == Mode::Smooth as u8 {
                    back = f64::from((pts[i - 1].x - if i >= 2 { pts[i - 2].x } else { 0.0 }).max(0.0)) / len;
                    if i >= 2 {
                        before = f64::from(pts[i - 2].y);
                    }
                }
                let (mut after, mut ahead) = (y1, 1.0);
                if let Some(q) = pts.get(i + 1).filter(|q| q.m == Mode::Smooth as u8) {
                    ahead = f64::from((q.x - p.x).max(0.0)) / len;
                    after = f64::from(q.y);
                }
                let back = back.max(TINY) as f32;
                let ahead = ahead.max(TINY) as f32;
                let rise = rise as f32;
                let out = ((((after - y0) / f64::from(rise)) - 1.0) / f64::from(ahead)) as f32;
                let into = (-((before - y0) / f64::from(rise)) / f64::from(back)) as f32;
                s.c0 = if out > 0.0 {
                    let w = 2.0 * ahead + 1.0;
                    out * (w + ahead + 2.0) / (w * out + (ahead + 2.0))
                } else {
                    0.0
                };
                s.c1 = if into > 0.0 {
                    let w = 2.0 * back + 1.0;
                    into * (back + 2.0 + w) / ((back + 2.0) + w * into)
                } else {
                    0.0
                };
                s.c2 = 1.0 - t.abs();
            }
            Mode::Hold => {}
        }
        s
    }

    /// The shape at `x` (0..1 along the segment), computed in doubles and rounded to single.
    pub fn progress(&self, x: f64) -> f32 {
        if self.line {
            return x as f32;
        }
        let (c0, c1, c2) = (f64::from(self.c0), f64::from(self.c1), f64::from(self.c2));
        let neg = self.neg;
        let single = |x: f64| if neg { ((x * c2).exp() - 1.0) * c1 } else { 1.0 - (((1.0 - x) * c2).exp() - 1.0) * c1 };
        let curve2 = |a: f64| {
            let u = if neg { a } else { 1.0 - a };
            a - (u.powi(6) - u) * c0
        };
        let curve3 = |a: f64| {
            let u = if neg { a } else { 1.0 - a };
            c1 / (1.0 - c0 * u)
        };
        let v = match self.mode {
            Mode::Single => single(x),
            Mode::Double => {
                if x < 0.5 {
                    single(2.0 * x) / 2.0
                } else {
                    1.0 - single(2.0 * (1.0 - x)) / 2.0
                }
            }
            Mode::Hold => 0.0,
            Mode::Stairs => f64::from(round(x * c0 - c2) as f32) * c1,
            Mode::SmoothStairs => {
                let a = x * c0 - 0.5;
                let k = round(a - 0.5);
                let mut d = (a - k) * 2.0 - 1.0;
                if neg {
                    let e = 1.0 - d.abs();
                    d = (1.0 - e * e * e).copysign(d);
                } else {
                    d = d * d * d;
                }
                (k + d / 2.0 + 1.0) * c1
            }
            Mode::Pulse => (round(x * c0 - c2) as i64 & 1) as f64,
            Mode::Wave => {
                if neg {
                    (((x * c0 + 0.75) * c1).sin() + 1.0) / 2.0
                } else {
                    let a = x * c0 - 0.5;
                    ((a - round(a - 0.5)) * 2.0 - 1.0).abs()
                }
            }
            Mode::Single2 => curve2(x),
            Mode::Double2 => {
                if x < 0.5 {
                    curve2(2.0 * x) / 2.0
                } else {
                    1.0 - curve2(2.0 * (1.0 - x)) / 2.0
                }
            }
            Mode::HalfSine => {
                let a = x * f64::from(std::f64::consts::FRAC_PI_2 as f32);
                let h = if neg { 1.0 - a.cos() } else { a.sin() };
                x - (x - h) * c0
            }
            Mode::Smooth => {
                let (x2, x3) = (x * x, x * x * x);
                x + ((3.0 * x2 - 2.0 * x3) + x2 * (x - 1.0) * c0 + x * (x - 1.0) * (x - 1.0) * c1 - x) * c2
            }
            Mode::Single3 => curve3(x) - c2,
            Mode::Double3 => {
                if x < 0.5 {
                    curve3(2.0 * x) - c2
                } else {
                    (1.0 - curve3(2.0 * (1.0 - x))) + c2
                }
            }
        };
        v as f32
    }
}

impl Curve {
    /// A curve with one point (a constant).
    pub fn constant(y: f32) -> Curve {
        Curve { pts: vec![Pt::new(0.0, y, 0.0, Mode::Single as u8)] }
    }

    /// The envelope's value at `x` (0..1 across the beat); an empty curve is `None`.
    pub fn value_at(&self, x: f64) -> Option<f32> {
        let pts = &self.pts;
        let first = pts.first()?;
        let x = x.clamp(0.0, 1.0);
        if pts.len() == 1 || x <= f64::from(first.x) {
            return Some(first.y);
        }
        // The segment that ends on the first point past `x`.
        let i = pts.partition_point(|p| f64::from(p.x) <= x).max(1);
        if i >= pts.len() {
            return Some(pts[pts.len() - 1].y);
        }
        let (a, b) = (pts[i - 1], pts[i]);
        let span = f64::from(b.x - a.x).max(TINY);
        let t = ((x - f64::from(a.x)) / span).clamp(0.0, 1.0);
        let by = b.y - a.y;
        Some(if by == 0.0 { a.y } else { Segment::new(pts, i).progress(t) * by + a.y })
    }

    /// The drawn polyline over `steps` columns (x, y), for the editor.
    pub fn polyline(&self, steps: usize) -> Vec<(f32, f32)> {
        (0..=steps)
            .map(|k| {
                let x = k as f64 / f64::from(steps.max(1) as u32);
                (x as f32, self.value_at(x).unwrap_or(0.5))
            })
            .collect()
    }

    /// Puts the points in `x` order, the first pinned at 0, every value inside 0..1, the shapes inside the thirteen.
    pub fn normalize(&mut self) {
        self.pts.sort_by(|a, b| a.x.total_cmp(&b.x));
        if let Some(first) = self.pts.first_mut() {
            first.x = 0.0;
        }
        for p in &mut self.pts {
            p.y = p.y.clamp(0.0, 1.0);
            p.t = p.t.clamp(-1.0, 1.0);
            p.m = (p.m as u32 % 13) as u8;
        }
    }
}

/// The eight grids: four time slots, four volume slots (an empty curve = the section's default line).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridState {
    pub time: [Curve; 4],
    pub volume: [Curve; 4],
}

impl GridState {
    /// The JSON the plugin state stores the grids as (a string field).
    pub fn to_json(&self) -> String {
        let slot = |c: &Curve| {
            serde_json::Value::Array(c.pts.iter().map(|p| serde_json::json!({"x": p.x, "y": p.y, "t": p.t, "m": p.m})).collect())
        };
        let v = serde_json::json!({
            "version": 1,
            "time": self.time.iter().map(slot).collect::<Vec<_>>(),
            "volume": self.volume.iter().map(slot).collect::<Vec<_>>(),
        });
        v.to_string()
    }

    /// Reads what [`GridState::to_json`](Self::to_json) wrote; `None` when it is not one.
    pub fn from_json(s: &str) -> Option<GridState> {
        let v: serde_json::Value = serde_json::from_str(s).ok()?;
        let slot = |v: &serde_json::Value| -> Option<Curve> {
            let mut pts = Vec::new();
            for p in v.as_array()? {
                let o = p.as_object()?;
                pts.push(Pt {
                    x: o.get("x")?.as_f64()? as f32,
                    y: o.get("y")?.as_f64()? as f32,
                    t: o.get("t").and_then(|t| t.as_f64()).unwrap_or(0.0) as f32,
                    m: o.get("m").and_then(|m| m.as_u64()).unwrap_or(0) as u8,
                });
            }
            let mut c = Curve { pts };
            c.normalize();
            Some(c)
        };
        let arr = |v: &serde_json::Value| -> Option<[Curve; 4]> {
            let a = v.as_array()?;
            Some([slot(a.first()?)?, slot(a.get(1)?)?, slot(a.get(2)?)?, slot(a.get(3)?)?])
        };
        Some(GridState { time: arr(v.get("time")?)?, volume: arr(v.get("volume")?)? })
    }

    /// A rate's position on the time grid: the vertical is exponential around the centre — 1x at the middle,
    /// 16x at the top, a quarter of the sound at a quarter up from the bottom, and below that the same range
    /// backwards (the bottom edge is full reverse). Forward rates below a quarter clamp to the quarter line.
    pub fn y_of_rate(rate: f32) -> f32 {
        let r = rate.max(-16.0).min(16.0);
        if r >= 0.0 {
            (0.5 + r.max(1.0 / 16.0).log2() / 8.0).clamp(0.25, 1.0)
        } else {
            (0.25 - (-r).max(1.05).log2() / 16.0).clamp(0.0, 0.25)
        }
    }

    /// The rate a time grid position plays at (see [`GridState::y_of_rate`](Self::y_of_rate)): the centre is 1x,
    /// the top 16x, a quarter up from the bottom a quarter speed, under it backwards down to -16x at the bottom.
    pub fn rate_of_y(y: f32) -> f32 {
        let y = y.clamp(0.0, 1.0);
        if y >= 0.25 {
            2f32.powf(8.0 * (y - 0.5))
        } else {
            -2f32.powf(16.0 * (0.25 - y))
        }
    }
}

/// The grids as the editor writes and the sound reads them: a mutex state with a version, so the audio thread
/// never locks (it polls the version and copies when it moved).
#[derive(Default)]
pub struct SharedGrid {
    state: Mutex<GridState>,
    version: AtomicU64,
}

impl SharedGrid {
    /// The grids now (a copy).
    pub fn load(&self) -> GridState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Puts `g` in (the editor's edits, a preset, a loaded project).
    pub fn store(&self, g: GridState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = g;
        self.version.fetch_add(1, Ordering::Release);
    }

    /// The version of the grids; the sound keeps its copy until this moves.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    /// The grids, with the version they were read at.
    pub fn load_and_seen(&self) -> (GridState, u64) {
        let v = self.version();
        (self.load(), v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32, t: f32, m: u8) -> Pt {
        Pt::new(x, y, t, m)
    }

    /// Every shape starts on the level of its first point and arrives at the level of its second, staying between
    /// (the pulse, wave and hold shapes land where their cycles put them, as on the automation curve).
    #[test]
    fn shapes_run_from_the_start_to_the_end() {
        let (s, e) = (0.2f32, 0.8f32);
        let endpointed = [Mode::Single, Mode::Double, Mode::Stairs, Mode::SmoothStairs, Mode::Single2, Mode::Double2, Mode::HalfSine, Mode::Smooth, Mode::Single3, Mode::Double3];
        for (mi, mode) in endpointed.iter().enumerate() {
            let m = *mode as u8;
            for t in [-1.0f32, -0.5, -0.1, 0.0, 0.1, 0.5, 1.0] {
                let c = Curve { pts: vec![pt(0.0, s, 0.0, 0), pt(1.0, e, t, m)] };
                let (a, b) = (c.value_at(0.0).unwrap(), c.value_at(1.0 - 1e-9).unwrap());
                assert!((a - s).abs() < 2e-3, "{mode:?} {t}: starts at {a}");
                if !(*mode == Mode::Stairs && t < 0.0) {
                    assert!((b - e).abs() < 2e-3, "{mode:?} {t}: ends at {b}");
                }
                for k in 1..100 {
                    let v = c.value_at(f64::from(k) / 100.0).unwrap();
                    let (lo, hi) = (s.min(e) - 1e-3, s.max(e) + 1e-3);
                    assert!((lo..=hi).contains(&v), "{mode:?} {t} at {k}%: {v}");
                }
            }
        }
    }

    /// Spot values: a hold stays, the straight line at tension 0, stairs at full tension, the half sine.
    #[test]
    fn shapes_follow_the_page() {
        let seg = |m: u8, t: f32| Curve { pts: vec![pt(0.0, 0.0, 0.0, 0), pt(1.0, 1.0, t, m)] };
        assert_eq!(seg(Mode::Hold as u8, 0.3).value_at(0.99), Some(0.0));
        assert_eq!(seg(Mode::Single as u8, 0.0).value_at(0.25), Some(0.25), "a straight line at tension 0");
        let st = seg(Mode::Stairs as u8, 1.0);
        assert_eq!((st.value_at(0.2), st.value_at(0.8)), (Some(0.0), Some(1.0)));
        let wa = seg(Mode::Wave as u8, 0.0);
        assert_eq!(wa.value_at(0.5), Some(0.5), "the wave at tension 0 is the triangle");
        let hs = seg(Mode::HalfSine as u8, 1.0);
        assert!((hs.value_at(0.5).unwrap() - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
    }

    /// One point is a constant; past the last point the level stays; the first point pins at 0.
    #[test]
    fn constants_and_holds() {
        assert_eq!(Curve::constant(0.75).value_at(0.4), Some(0.75));
        let c = Curve { pts: vec![pt(0.0, 1.0, 0.0, 0), pt(0.5, 0.25, 0.0, 0)] };
        assert_eq!(c.value_at(0.75), Some(0.25), "past the last point");
        let mut moved = Curve { pts: vec![pt(0.1, 0.5, 0.0, 0), pt(0.9, 0.6, 0.0, 0)] };
        moved.normalize();
        assert_eq!(moved.pts[0].x, 0.0, "the first point pins at 0");
    }

    /// The rate mapping: centre 1x, an eighth up 2x, a quarter up 4x, the top 16x, half speed a quarter down
    /// from the centre, a quarter speed at the quarter line, the bottom full backwards.
    #[test]
    fn rate_mapping_is_exponential_around_the_centre() {
        assert!((GridState::rate_of_y(0.5) - 1.0).abs() < 1e-6);
        assert!((GridState::rate_of_y(0.625) - 2.0).abs() < 1e-4, "an eighth up doubles");
        assert!((GridState::rate_of_y(0.75) - 4.0).abs() < 1e-3);
        assert!((GridState::rate_of_y(1.0) - 16.0).abs() < 1e-2, "the top is 16x");
        assert!((GridState::rate_of_y(0.375) - 0.5).abs() < 1e-4, "half speed sits under the centre");
        assert!((GridState::rate_of_y(0.25) - 0.25).abs() < 1e-4, "the quarter line is a quarter speed");
        assert!(GridState::rate_of_y(0.0) < -15.0, "the bottom is full backwards");
        assert!((GridState::y_of_rate(1.0) - 0.5).abs() < 1e-6, "the sound's own speed sits at the centre");
        assert!((GridState::y_of_rate(2.0) - 0.625).abs() < 1e-4);
        assert!((GridState::y_of_rate(0.5) - 0.375).abs() < 1e-4, "half speed sits under the centre");
        assert!(GridState::y_of_rate(-1.0) < 0.25, "backwards rates sit under the quarter line");
        assert!(GridState::y_of_rate(-1.0) >= 0.0);
    }

    /// The JSON round trip keeps every point.
    #[test]
    fn json_round_trip() {
        let mut g = GridState::default();
        g.time[0] = Curve {
            pts: vec![pt(0.0, 0.5, 0.0, 0), pt(0.5, 0.375, 0.4, Mode::Single as u8), pt(1.0, 0.75, 0.0, Mode::Hold as u8)],
        };
        g.volume[2] = Curve { pts: vec![pt(0.0, 1.0, 0.0, Mode::Hold as u8), pt(0.25, 0.0, 0.0, Mode::Hold as u8)] };
        let back = GridState::from_json(&g.to_json()).unwrap();
        assert_eq!(back, g);
        assert!(GridState::from_json("not a grid").is_none());
        assert!(GridState::from_json("{}").is_none());
    }

    /// The shared grids: a store bumps the version and a load sees it; no edit, no version move.
    #[test]
    fn shared_grid_versions_move() {
        let s = SharedGrid::default();
        let (g, seen) = s.load_and_seen();
        assert_eq!(g, GridState::default());
        s.store(GridState {
            time: [Curve::constant(0.375), Default::default(), Default::default(), Default::default()],
            volume: Default::default(),
        });
        assert_ne!(s.version(), seen);
        let (g, seen2) = s.load_and_seen();
        assert_eq!(g.time[0].value_at(0.7), Some(0.375));
        let (_, again) = s.load_and_seen();
        assert_eq!(seen2, again, "no edit, no version move");
    }
}
