// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The BeepMap sound: an image becomes a voice the way FL's BeepMap sings one — the picture's columns are time, its
//! rows are partials (the bottom edge the base pitch, climbing towards the top), and a pixel's brightness is its
//! partial's loudness. A sweep runs across the picture at the scan time, the played key carries the base pitch, and
//! the tone knob is a lowpass over the bank of sines.
//!
// spec: plugins/lemon-beepmap.md

use std::sync::Arc;

/// The partials: one per grid row, the picture decodes into.
pub const BANDS: usize = 32;
/// The octaves the bands span above the base.
pub const OCTAVES: f32 = 5.0;
/// The picture's grid (any image decodes into this size).
pub const GRID_W: usize = 128;
pub const GRID_H: usize = BANDS;

/// An image as the synthesizer reads it: luminance per grid cell, rows from the top, each `0.0..1.0`.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub lum: Vec<f32>,
}

impl Grid {
    /// Decodes a PNG into the grid, and the image itself (RGB, at most 256 on a side) for the editor's picture.
    pub fn decode(bytes: &[u8]) -> Option<(Grid, (usize, usize, Vec<u32>))> {
        let pm = tiny_skia::Pixmap::decode_png(bytes).ok()?;
        let (w, h) = (pm.width() as usize, pm.height() as usize);
        // Luminance straight to the grid: each cell averages the source pixels it covers, whatever their size.
        let mut lum = vec![0.0f32; GRID_W * GRID_H];
        for gy in 0..GRID_H {
            for gx in 0..GRID_W {
                let x0 = gx * w / GRID_W;
                let x1 = ((gx + 1) * w / GRID_W).max(x0 + 1).min(w);
                let y0 = gy * h / GRID_H;
                let y1 = ((gy + 1) * h / GRID_H).max(y0 + 1).min(h);
                let mut sum = 0.0f32;
                let mut n = 0u32;
                for y in y0..y1 {
                    for x in x0..x1 {
                        if let Some(p) = pm.pixel(x as u32, y as u32) {
                            let p = p.demultiply();
                            sum += (0.2126 * f32::from(p.red()) + 0.7152 * f32::from(p.green()) + 0.0722 * f32::from(p.blue())) / 255.0;
                            n += 1;
                        }
                    }
                }
                lum[gy * GRID_W + gx] = if n > 0 { sum / n as f32 } else { 0.0 };
            }
        }
        // The editor's picture: RGB pixels, shrunk to at most 256 on a side (the canvas stretches it).
        let k = (w.max(h) / 256).max(1);
        let (tw, th) = ((w + k - 1) / k, (h + k - 1) / k);
        let mut px = Vec::with_capacity(tw * th);
        for ty in 0..th {
            for tx in 0..tw {
                let p = pm.pixel(((tx * k).min(w - 1)) as u32, ((ty * k).min(h - 1)) as u32).unwrap_or(tiny_skia::PremultipliedColorU8::TRANSPARENT);
                let p = p.demultiply();
                px.push(u32::from(p.red()) << 16 | u32::from(p.green()) << 8 | u32::from(p.blue()));
            }
        }
        Some((Grid { lum }, (tw, th, px)))
    }

    /// A band's loudness in column `col`: band `b` takes its row from the bottom (the bottom edge sings the base
    /// pitch), the pixel's brightness against the gate and the bright gamma.
    pub fn band_amp(&self, band: usize, col: usize, gate: f32, bright: f32) -> f32 {
        let row = GRID_H - 1 - band.min(GRID_H - 1);
        let l = self.lum[row * GRID_W + col.min(GRID_W - 1)];
        if l <= gate {
            0.0
        } else {
            ((l - gate) / (1.0 - gate).max(1e-6)).powf(bright.max(1e-3))
        }
    }

    /// A band's frequency over `base_hz` (the bottom edge's pitch).
    pub fn band_hz(band: usize, base_hz: f32) -> f32 {
        base_hz * 2f32.powf(band as f32 / BANDS as f32 * OCTAVES)
    }
}

/// What one block of the scan plays: the settings read fresh every block (`BeepMapParams`).
#[derive(Clone, Copy, Debug)]
pub struct ScanParams {
    pub base_hz: f32,
    pub bright: f32,
    pub gate: f32,
    pub cutoff: f32,
    pub speed_secs: f32,
}

/// The voice: a sweep across the picture, a band of sines under it, a fast attack and a short release.
pub struct Scan {
    grid: Option<Arc<Grid>>,
    rate: f32,
    x: f32,
    key: u8,
    vel: f32,
    phase: [f32; BANDS],
    lp_l: f32,
    lp_r: f32,
    env: f32,
    held: bool,
    col: usize,
    amps: [f32; BANDS],
}

impl Scan {
    pub fn new(rate: f32) -> Self {
        Self {
            grid: None,
            rate: rate.max(1.0),
            x: 0.0,
            key: 60,
            vel: 1.0,
            phase: [0.0; BANDS],
            lp_l: 0.0,
            lp_r: 0.0,
            env: 0.0,
            held: false,
            col: usize::MAX,
            amps: [0.0; BANDS],
        }
    }

    pub fn set_rate(&mut self, rate: f32) {
        self.rate = rate.max(1.0);
    }

    pub fn set_grid(&mut self, grid: Option<Arc<Grid>>) {
        self.grid = grid;
        self.col = usize::MAX;
    }

    pub fn key(&self) -> u8 {
        self.key
    }

    /// A key starts the sweep from the picture's left edge again.
    pub fn note_on(&mut self, key: u8, vel: f32) {
        self.key = key;
        self.vel = vel.clamp(0.05, 1.0);
        self.x = 0.0;
        self.col = usize::MAX;
        self.held = true;
        self.env = 0.0;
    }

    pub fn note_off(&mut self) {
        self.held = false;
    }

    pub fn is_active(&self) -> bool {
        self.held || self.env > 0.0
    }

    /// Fills the next samples (the caller's buffers are zeros; the scan adds its voice into them).
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32], p: ScanParams) {
        let Some(grid) = self.grid.as_ref() else {
            self.env = 0.0;
            return;
        };
        let gate = p.gate.clamp(0.0, 0.95);
        let cutoff = p.cutoff.clamp(40.0, self.rate * 0.45);
        let k = 1.0 - (-std::f32::consts::TAU * cutoff / self.rate).exp();
        // The sweep: the picture's width takes `speed_secs` seconds, over and over.
        let step = (GRID_W as f32) / (p.speed_secs.max(0.05) * self.rate);
        let attack = 1.0 / (0.002 * self.rate);
        let release = 1.0 / (0.15 * self.rate);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.env <= 0.0 && !self.held {
                self.lp_l = 0.0;
                self.lp_r = 0.0;
                continue;
            }
            self.env = (self.env + if self.held { attack } else { -release }).clamp(0.0, 1.0);
            let col = (self.x as usize).min(GRID_W - 1);
            if col != self.col {
                for b in 0..BANDS {
                    self.amps[b] = grid.band_amp(b, col, gate, p.bright);
                }
                self.col = col;
            }
            let mut out = 0.0f32;
            for b in 0..BANDS {
                let amp = self.amps[b];
                if amp <= 0.0 {
                    continue;
                }
                let d = std::f32::consts::TAU * Grid::band_hz(b, p.base_hz) / self.rate;
                let ph = self.phase[b] + d;
                self.phase[b] = if ph >= std::f32::consts::TAU { ph - std::f32::consts::TAU } else { ph };
                out += amp * self.phase[b].sin();
            }
            self.x += step;
            if self.x >= GRID_W as f32 {
                self.x -= GRID_W as f32;
                self.col = usize::MAX;
            }
            self.lp_l += k * (out - self.lp_l);
            self.lp_r += k * (out - self.lp_r);
            let g = self.env * self.vel / 16.0;
            *l += self.lp_l * g;
            *r += self.lp_r * g;
        }
    }

    /// A key's frequency.
    fn key_hz(key: u8) -> f32 {
        (440.0f64 * 2f64.powf((f64::from(key) - 69.0) / 12.0)) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A black-white picture decodes: the white half's cells bright, the black half's dark, and the editor's
    /// picture beside it.
    #[test]
    fn an_image_decodes_into_the_grid() {
        let mut pm = tiny_skia::Pixmap::new(8, 8).unwrap();
        for y in 0..8u32 {
            for x in 0..8u32 {
                let v = if y < 4 { 255u8 } else { 0u8 };
                pm.pixels_mut()[(y * 8 + x) as usize] = tiny_skia::PremultipliedColorU8::from_rgba(v, v, v, 255).unwrap();
            }
        }
        let bytes = pm.encode_png().unwrap();
        let (grid, (tw, th, px)) = Grid::decode(&bytes).unwrap();
        assert_eq!((tw, th), (8, 8));
        assert_eq!(px.len(), 64);
        assert!(px[0] & 0xff == 0xff, "the top pixel is white");
        // The top rows of the grid are bright, the bottom rows dark.
        let top: f32 = grid.lum[..GRID_W].iter().sum::<f32>() / GRID_W as f32;
        let bottom: f32 = grid.lum[GRID_W * (GRID_H - 1)..].iter().sum::<f32>() / GRID_W as f32;
        assert!(top > 0.9, "the white half is bright ({top})");
        assert!(bottom < 0.1, "the black half is dark ({bottom})");
    }

    /// Not an image: the decode says so.
    #[test]
    fn garbage_does_not_decode() {
        assert!(Grid::decode(b"not a png").is_none());
    }

    /// The bands: a lit row sings its band, the gate silences dark cells, the frequency climbs to the top.
    #[test]
    fn bands_climb_and_the_gate_silences() {
        let mut grid = Grid { lum: vec![0.0; GRID_W * GRID_H] };
        let row = 5;
        for l in grid.lum[row * GRID_W..(row + 1) * GRID_W].iter_mut() {
            *l = 0.8;
        }
        let band = GRID_H - 1 - row;
        let amp = grid.band_amp(band, 0, 0.5, 1.0);
        assert!(amp > 0.0 && amp <= 1.0, "the lit row sings ({amp})");
        assert_eq!(grid.band_amp(band, 0, 0.9, 1.0), 0.0, "the gate silences it");
        assert_eq!(Grid::band_hz(0, 110.0), 110.0);
        assert!(Grid::band_hz(BANDS - 1, 110.0) > 110.0 * 16.0, "the top band is octaves up");
    }

    /// A lit picture sings, the release ends it, and the same render comes out the same way again.
    #[test]
    fn the_scan_sings_and_ends() {
        let grid = Arc::new(Grid { lum: vec![1.0; GRID_W * GRID_H] });
        let p = ScanParams { base_hz: 220.0, bright: 1.0, gate: 0.0, cutoff: 12_000.0, speed_secs: 2.0 };
        let mut scan = Scan::new(44_100.0);
        scan.set_grid(Some(grid.clone()));
        scan.note_on(60, 1.0);
        let (mut l, mut r) = (vec![0.0f32; 4096], vec![0.0f32; 4096]);
        scan.render(&mut l, &mut r, p);
        let peak = l.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!(peak > 0.001, "the picture sings (peak {peak})");
        let mut again = Scan::new(44_100.0);
        again.set_grid(Some(grid));
        again.note_on(60, 1.0);
        let (mut l2, mut r2) = (vec![0.0f32; 4096], vec![0.0f32; 4096]);
        again.render(&mut l2, &mut r2, p);
        assert_eq!(l, l2, "the render is the same every time");
        // The release: the voice ends, then silence.
        scan.note_off();
        let mut quiet = 0;
        for _ in 0..100 {
            for x in l.iter_mut() {
                *x = 0.0;
            }
            scan.render(&mut l, &mut r, p);
            if l.iter().all(|x| x.abs() < 1e-6) {
                quiet += 1;
            }
        }
        assert!(quiet > 0, "the release ends the voice");
        assert!(!scan.is_active());
    }

    /// The sweep wraps: a long render keeps sounding past the picture's right edge.
    #[test]
    fn the_sweep_wraps_around() {
        let grid = Arc::new(Grid { lum: vec![1.0; GRID_W * GRID_H] });
        let mut scan = Scan::new(44_100.0);
        scan.set_grid(Some(grid));
        scan.note_on(48, 1.0);
        let p = ScanParams { base_hz: 110.0, bright: 1.0, gate: 0.0, cutoff: 12_000.0, speed_secs: 0.1 };
        let (mut l, mut r) = (vec![0.0f32; 44_100], vec![0.0f32; 44_100]);
        scan.render(&mut l, &mut r, p);
        let first = l[..1000].iter().fold(0.0f32, |a, x| a.max(x.abs()));
        let last = l[44_100 - 1000..].iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!(first > 0.001 && last > 0.001, "still singing after the wrap ({first}, {last})");
    }
}
