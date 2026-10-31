// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Softbuffer Canvas vector renderer for Lemon Wavetable.

use os_plugkit::vals::Vals;
use os_theme::design::lemon as brand;
use os_ui::{Canvas, Font, Rect};

pub const W: u32 = 760;
pub const H: u32 = 540;

pub const PREVIOUS: Rect = Rect::new(500.0, 20.0, 32.0, 30.0);
pub const NEXT: Rect = Rect::new(704.0, 20.0, 32.0, 30.0);
pub const PRESET: Rect = Rect::new(536.0, 20.0, 164.0, 30.0);

pub const WAVETABLE_BOX: Rect = Rect::new(24.0, 68.0, 440.0, 160.0);
pub const FILTER_BOX: Rect = Rect::new(480.0, 68.0, 256.0, 160.0);
pub const ENV_BOX: Rect = Rect::new(24.0, 356.0, 344.0, 132.0);
pub const LFO_BOX: Rect = Rect::new(384.0, 356.0, 352.0, 132.0);

pub const TRACK_IDS: [&str; 7] = [
    "wt_pos", "warp_amt", "unison", "detune", "cutoff", "resonance", "drive",
];

pub const TRACK_NAMES: [&str; 7] = [
    "WT Pos", "Warp", "Unison", "Detune", "Cutoff", "Resonance", "Drive",
];

pub fn track(index: usize) -> Rect {
    let col = index % 4;
    let row = index / 4;
    let x = 24.0 + col as f32 * 182.0;
    let y = 252.0 + row as f32 * 48.0;
    Rect::new(x, y, 160.0, 22.0)
}

pub fn paint(
    canvas: &mut Canvas,
    vals: &Vals,
    voices_active: usize,
    focus: Option<usize>,
) {
    let palette = os_plugkit::palette();
    let text = palette.text;
    let secondary = text.mix(palette.toolbar, 0.38);
    let accent = brand::MAIN;
    let small = Font::new(13);
    let normal = Font::new(15);
    let bold = Font::new(16);
    let title = Font::new(26);

    // Background
    canvas.fill_rect(Rect::new(0.0, 0.0, W as f32, H as f32), palette.toolbar);

    // Top Header
    canvas.text(title, 24.0, 14.0, text, "MANGO WAVETABLE");
    canvas.text(small, 290.0, 25.0, secondary, "Serum V2 Architecture  /  2,048-Sample Mipmapped");

    // Preset selector buttons
    for (rect, label) in [
        (PREVIOUS, "<"),
        (NEXT, ">"),
        (PRESET, match vals.plain("table") as usize {
            1 => "Analog Saw",
            2 => "Digital Bell",
            _ => "Basic Shapes",
        }),
    ] {
        canvas.fill_rrect(rect, 5.0, palette.button);
        canvas.text_center(normal, rect, text, label);
    }

    // 1. WAVETABLE 3D ISOMETRIC VISUALIZER
    canvas.fill_rrect(WAVETABLE_BOX, 8.0, palette.sunken_dark);
    canvas.text(bold, 36.0, 78.0, text, "Wavetable Oscillator");
    let wt_pos = vals.norm("wt_pos");
    canvas.text_right(small, 452.0, 80.0, secondary, &format!("Frame {:.0}/256", wt_pos * 255.0 + 1.0));

    // Render 7 isometric wireframe slices
    let total_slices = 7;
    let active_slice = (wt_pos * (total_slices - 1) as f32).round() as usize;

    for s in (0..total_slices).rev() {
        let is_active = s == active_slice;
        let slice_pos = s as f32 / (total_slices - 1) as f32;
        let y_base = 120.0 + s as f32 * 12.0;
        let x_base = 48.0 + (total_slices - 1 - s) as f32 * 14.0;
        let slice_width = 310.0;

        let stroke_col = if is_active {
            accent
        } else {
            text.mix(palette.sunken_dark, 0.25 + (s as f32 / total_slices as f32) * 0.4)
        };

        let mut pts = Vec::with_capacity(64);
        for i in 0..=60 {
            let phase = i as f64 / 60.0;
            // Simulated waveform morph from sine to saw to square
            let phi = phase * std::f64::consts::TAU;
            let sine = phi.sin();
            let saw = 1.0 - 2.0 * phase;
            let val = if slice_pos < 0.5 {
                sine * (1.0 - slice_pos * 2.0) as f64 + saw * (slice_pos * 2.0) as f64
            } else {
                saw * (1.0 - (slice_pos - 0.5) * 2.0) as f64 + (if phase < 0.5 { 1.0 } else { -1.0 }) * ((slice_pos - 0.5) * 2.0) as f64
            };
            let x = x_base + (i as f32 / 60.0) * slice_width;
            let y = y_base - (val as f32) * 16.0;
            pts.push((x, y));
        }
        canvas.polyline(&pts, if is_active { 2.5 } else { 1.0 }, stroke_col);
    }

    // 2. ZERO-DELAY FILTER CURVE
    canvas.fill_rrect(FILTER_BOX, 8.0, palette.sunken_dark);
    canvas.text(bold, 492.0, 78.0, text, "ZDF Filter (24dB)");
    let cutoff = vals.plain("cutoff");
    let res = vals.norm("resonance");
    let cutoff_label = if cutoff >= 1000.0 { format!("{:.1} kHz", cutoff / 1000.0) } else { format!("{cutoff:.0} Hz") };
    canvas.text_right(small, 724.0, 80.0, secondary, &cutoff_label);

    // Calculate cutoff X on log scale
    let min_log = 20.0f32.log10();
    let max_log = 20000.0f32.log10();
    let cur_log = cutoff.log10();
    let cutoff_x = FILTER_BOX.x + 16.0 + ((cur_log - min_log) / (max_log - min_log)) * (FILTER_BOX.w - 32.0);

    let mut filter_pts = Vec::with_capacity(40);
    for i in 0..=36 {
        let x = FILTER_BOX.x + 16.0 + (i as f32 / 36.0) * (FILTER_BOX.w - 32.0);
        let y = if x < cutoff_x {
            FILTER_BOX.y + 60.0 - res * 28.0 * (-(x - cutoff_x).powi(2) / 600.0).exp()
        } else {
            let rolloff = (x - cutoff_x) * 1.6;
            (FILTER_BOX.y + 60.0 - res * 28.0) + rolloff
        };
        filter_pts.push((x, y.min(FILTER_BOX.bottom() - 10.0)));
    }
    canvas.polyline(&filter_pts, 2.0, accent);
    canvas.fill_circle(cutoff_x, FILTER_BOX.y + 60.0 - res * 28.0, 4.0, accent);

    // 3. PARAMETER TRACKS (Sliders)
    for (i, &id) in TRACK_IDS.iter().enumerate() {
        let r = track(i);
        canvas.text(normal, r.x, r.y - 18.0, text, TRACK_NAMES[i]);
        canvas.text_right(small, r.right(), r.y - 16.0, secondary, &vals.text(id));

        // Track line
        canvas.fill_rrect(Rect::new(r.x, r.cy() - 2.0, r.w, 4.0), 2.0, palette.sunken_dark);
        let filled_w = (vals.norm(id) * r.w).max(2.0);
        canvas.fill_rrect(Rect::new(r.x, r.cy() - 2.0, filled_w, 4.0), 2.0, accent);
        canvas.fill_circle(r.x + filled_w, r.cy(), 5.0, accent);

        if focus == Some(i) {
            canvas.stroke_rrect(r.outset(3.0), 5.0, 1.0, text);
        }
    }

    // 4. ENVELOPE 1 (AMP) CURVE
    canvas.fill_rrect(ENV_BOX, 8.0, palette.sunken_dark);
    canvas.text(bold, 36.0, 366.0, text, "Env 1 (Amp Curve)");
    canvas.text_right(small, 356.0, 368.0, secondary, &format!("A:{}ms  D:{}ms", vals.plain("attack"), vals.plain("decay")));

    // Draw ADSR curve
    let ea = vals.norm("attack");
    let ed = vals.norm("decay");
    let es = vals.norm("sustain");
    let er = vals.norm("release");

    let ax = ENV_BOX.x + 20.0 + ea * 60.0;
    let dx = ax + ed * 80.0;
    let sx = dx + 90.0;
    let rx = (sx + er * 70.0).min(ENV_BOX.right() - 20.0);

    let y_zero = ENV_BOX.bottom() - 16.0;
    let y_peak = ENV_BOX.y + 45.0;
    let y_sus = y_zero - es * (y_zero - y_peak);

    let env_pts = vec![
        (ENV_BOX.x + 20.0, y_zero),
        (ax, y_peak),
        (dx, y_sus),
        (sx, y_sus),
        (rx, y_zero),
    ];
    canvas.polyline(&env_pts, 2.0, text.mix(accent, 0.7));

    // 5. LFO 1 CURVE
    canvas.fill_rrect(LFO_BOX, 8.0, palette.sunken_dark);
    canvas.text(bold, 396.0, 366.0, text, "LFO 1 (Vector Wave)");
    canvas.text_right(small, 724.0, 368.0, secondary, &format!("Rate: {}  -> WT Pos", vals.text("lfo1_rate")));

    let lfo_w = LFO_BOX.w - 40.0;
    let mut lfo_pts = Vec::with_capacity(50);
    for i in 0..=48 {
        let phase = i as f32 / 48.0;
        let wave = (phase * std::f32::consts::TAU).sin();
        let x = LFO_BOX.x + 20.0 + phase * lfo_w;
        let y = LFO_BOX.y + 70.0 - wave * 30.0;
        lfo_pts.push((x, y));
    }
    canvas.polyline(&lfo_pts, 2.0, accent);

    // Footer Info Bar
    canvas.text(small, 24.0, 514.0, secondary, "Click & drag sliders to adjust  /  Scroll to fine-tune  /  Double-click resets");
    canvas.text_right(small, 736.0, 514.0, text, &format!("{voices_active} / 32 voices"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_bounds_are_valid() {
        for i in 0..7 {
            let r = track(i);
            assert!(r.x >= 24.0 && r.right() <= W as f32);
            assert!(r.y >= 250.0 && r.bottom() <= H as f32);
        }
    }
}
