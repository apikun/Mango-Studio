// Copyright (C) 2026 mikobuntu
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The rolling sampler (issue #82): a background ring of the master's newest output, a strip of it between the
//! toolbar and the playlist, a selection on the strip that drops onto the playlist as a rendered audio clip.
//!
//! The audio side ([`Rolling`]) lives in the transport's `Shared`, pushed from the renderers' post-master copy (the
//! same place the monitor's tap is fed, `transport.rs`): no mixer insert, previews excluded. It is a rolling tape of
//! the last `cap` frames (30 s to 10 min, Audio ▸ Rolling sampler), RAM only, session lifespan: it never enters a
//! project file, and a project change clears it.
//!
//! The strip's view state ([`Strip`]) lives in the app state: a zoom, a pan back from the live edge and a selection
//! in absolute frames, so a selection stays put on the tape while the newest audio keeps coming.

use std::io::Write as _;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Frames of one pyramid entry: min/max of this many frames, kept as they are written so the strip can draw the
/// whole buffer without walking every frame of it.
const PYR: usize = 256;

/// How many device blocks wait in the spool at most (about 1.3 s of a 128-frame device), then the newest are kept:
/// the spool only fills while the UI holds the data lock, and the UI holds it for a scan at worst.
const SPOOL: usize = 128;

/// The audio side: a ring of the newest output frames (L, R) with its per-`PYR` peaks, and when the device took the
/// last block of them. Zero-capacity until enabled: off, a push is one relaxed atomic load.
pub struct Rolling {
    on: AtomicBool,
    paused: AtomicBool,
    /// The frames pushed while the data lock was busy (the UI's scan or render had it): written into the tape by the
    /// next push that gets in, in order, so a busy UI never loses a device block (a lost block is a hole in the tape
    /// — every clip dragged out over it plays that bit short, and faster than it was heard).
    pending: Mutex<Vec<Pending>>,
    dropped: AtomicUsize,
    data: Mutex<Data>,
}

/// One spooled device block.
struct Pending {
    stereo: Vec<f32>,
    w: usize,
    block: u64,
    at: Instant,
    rate: u32,
}

/// `Data` behind the lock: the ring and the bookkeeping the push and the readers share. Capacity follows the
/// settings; a change reallocates (the tape restarts empty).
struct Data {
    l: Vec<f32>,
    r: Vec<f32>,
    /// Per-`PYR` min/max of `l` / `r` (entry `k` covers ring frames `k * PYR ..`).
    pl: Vec<(f32, f32)>,
    pr: Vec<(f32, f32)>,
    /// Seconds of tape the settings ask for (f32 bits; the frames follow the rate).
    secs: f32,
    /// Frames written since the start.
    total: u64,
    /// The last block: the device block it was for (`device::blocks`), its first frame, its length, when the device
    /// took it and the rate it plays at — as the monitor's `Tap` keeps them.
    block: u64,
    start: u64,
    frames: u64,
    at: Option<Instant>,
    rate: u32,
}

impl Default for Rolling {
    fn default() -> Self {
        Rolling {
            on: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            pending: Mutex::new(Vec::new()),
            dropped: AtomicUsize::new(0),
            data: Mutex::new(Data::empty(0.0)),
        }
    }
}

impl Data {
    fn empty(secs: f32) -> Data {
        Data { l: Vec::new(), r: Vec::new(), pl: Vec::new(), pr: Vec::new(), secs, total: 0, block: 0, start: 0, frames: 0, at: None, rate: 0 }
    }

    /// The capacity the settings ask for at `rate`.
    fn want(&self, rate: u32) -> usize {
        ((self.secs.max(0.0) as f64 * f64::from(rate.max(1))) as usize).next_multiple_of(PYR)
    }

    /// Resizes to `want` frames, keeping nothing (the tape restarts).
    fn resize(&mut self, want: usize) {
        *self = Data::empty(self.secs);
        self.l = vec![0.0; want];
        self.r = vec![0.0; want];
        self.pl = vec![(0.0, 0.0); want / PYR];
        self.pr = vec![(0.0, 0.0); want / PYR];
    }
}

impl Rolling {
    /// Turns the tap on or off, and points it at `secs` seconds of tape. Off, nothing is kept (and the memory is
    /// let go); on, the next push fills the ring at the device's rate.
    pub fn sync(&self, on: bool, secs: f32) {
        self.on.store(on && secs > 0.0, Ordering::Relaxed);
        if let Ok(mut d) = self.data.lock() {
            if d.secs != secs || !on {
                if on {
                    // The empty ring fills at the rate of the first push.
                    d.secs = secs;
                } else {
                    *d = Data::empty(secs);
                }
            }
        }
    }

    /// Whether the strip shows at all (the setting is on).
    pub fn enabled(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// Pauses or resumes the capture: paused, the tape stands still (a Ctrl+click on the strip's wave).
    pub fn set_paused(&self, on: bool) {
        self.paused.store(on, Ordering::Relaxed);
    }

    /// Whether the capture is paused.
    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// The frames the tape holds, and their rate (0 before the first push): the first and the last frame now on it.
    pub fn window(&self, now: Instant) -> (u64, u64, u32) {
        let Ok(d) = self.data.lock() else { return (0, 0, 0) };
        let (heard, rate) = heard_of(&d, now);
        (d.total.saturating_sub(d.l.len() as u64), heard, rate)
    }

    /// One stream's block (interleaved, `w` channels: its first pair is taken) for device block `block`, taken `at`.
    /// The streams of one device block (playback and live notes) are summed, as the device hears them; a new device
    /// block comes after the last. A rate or capacity change reallocates here, where the frames flow. Off, nothing
    /// is kept.
    pub fn push(&self, stereo: &[f32], w: usize, block: u64, at: Instant, rate: u32) {
        if !self.on.load(Ordering::Relaxed) || self.paused.load(Ordering::Relaxed) {
            return;
        }
        match self.data.try_lock() {
            Ok(mut d) => {
                // The blocks that waited for the lock go in first, in order, so the tape keeps every frame.
                let mut waited = Vec::new();
                if let Ok(mut p) = self.pending.try_lock() {
                    waited = std::mem::take(&mut *p);
                }
                for one in &waited {
                    write_block(&mut d, &one.stereo, one.w, one.block, one.at, one.rate);
                }
                write_block(&mut d, stereo, w, block, at, rate);
            }
            Err(_) => {
                // The UI holds the lock (a scan, a drag-out's copy): the block waits in the spool, and goes into
                // the tape with the next push that gets in.
                if let Ok(mut p) = self.pending.try_lock() {
                    if p.len() >= SPOOL {
                        p.remove(0);
                        self.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    p.push(Pending { stereo: stereo.to_owned(), w, block, at, rate });
                } else {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    /// The frames heard by `now`: into the last block as far as the time since the device took it (all of it once
    /// that is past) — the strip draws up to here, as the monitor does.
    pub fn heard(&self, now: Instant) -> (u64, u32) {
        let Ok(d) = self.data.lock() else { return (0, 0) };
        heard_of(&d, now)
    }

    /// Per-column min/max of the absolute frames `from .. from + len` (columns with no tape on them span 0.0), L
    /// and R mixed into `out` (older column first). Reads peaks when a column spans several pyramid entries, the
    /// ring itself when it is zoomed in.
    pub fn scan(&self, from: u64, len: u64, out: &mut [(f32, f32)]) {
        out.fill((0.0, 0.0));
        if len == 0 || out.is_empty() {
            return;
        }
        let Ok(d) = self.data.lock() else { return };
        let cap = d.l.len() as u64;
        if cap == 0 {
            return;
        }
        // The tape holds `oldest .. total`; anything else on the view is silence, as it reads.
        let oldest = d.total.saturating_sub(cap);
        let total = d.total;
        let cols = out.len() as u64;
        for (c, o) in out.iter_mut().enumerate() {
            let a = from + len * c as u64 / cols;
            let b = (from + len * (c as u64 + 1) / cols).max(a + 1);
            let a = a.max(oldest);
            let b = b.min(total);
            if a >= b {
                continue;
            }
            let (mut lo_l, mut hi_l, mut lo_r, mut hi_r) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            if b - a >= 2 * PYR as u64 {
                // Pyramid: the entries fully in the column, plus the ragged ends from the ring.
                let e0 = (a + PYR as u64 - 1) / PYR as u64;
                let e1 = b / PYR as u64;
                if e1 > e0 {
                    for e in e0..e1 {
                        // Entry `e` lives in the slot its ring position says (`e % the entries' count`): the
                        // write side stores it there, and reading any other slot scrambles the zoomed-out
                        // wave with peaks that are not the column's own.
                        let p = d.pl[e as usize % d.pl.len()];
                        let q = d.pr[e as usize % d.pl.len()];
                        (lo_l, hi_l) = (lo_l.min(p.0), hi_l.max(p.1));
                        (lo_r, hi_r) = (lo_r.min(q.0), hi_r.max(q.1));
                    }
                }
                for f in a..(e0 * PYR as u64).min(b) {
                    let i = (f % cap) as usize;
                    (lo_l, hi_l) = (lo_l.min(d.l[i]), hi_l.max(d.l[i]));
                    (lo_r, hi_r) = (lo_r.min(d.r[i]), hi_r.max(d.r[i]));
                }
                for f in (e1 * PYR as u64).max(a)..b {
                    let i = (f % cap) as usize;
                    (lo_l, hi_l) = (lo_l.min(d.l[i]), hi_l.max(d.l[i]));
                    (lo_r, hi_r) = (lo_r.min(d.r[i]), hi_r.max(d.r[i]));
                }
            } else {
                for f in a..b {
                    let i = (f % cap) as usize;
                    (lo_l, hi_l) = (lo_l.min(d.l[i]), hi_l.max(d.l[i]));
                    (lo_r, hi_r) = (lo_r.min(d.r[i]), hi_r.max(d.r[i]));
                }
            }
            let l = if lo_l <= hi_l { (lo_l, hi_l) } else { (0.0, 0.0) };
            let r = if lo_r <= hi_r { (lo_r, hi_r) } else { (0.0, 0.0) };
            // The strip draws the two channels mixed, as the monitor's wave does.
            *o = (l.0.min(r.0), l.1.max(r.1));
        }
    }

    /// Renders the absolute frames `from .. from + len` as a 32-bit float stereo WAV at the tape's rate; returns
    /// the frames written (0 when there is nothing to render).
    pub fn render(&self, from: u64, len: u64, wav: &std::path::Path) -> std::io::Result<u64> {
        self.render_clip(from, len, wav, false, 0.0)
    }

    /// `render`, as a clip: `trim` cuts the silence at either end, `fade_ms` fades in and out over that many
    /// milliseconds, aligned to a zero crossing (a clip that starts mid-waveform clicks). The tape's lock is held
    /// only for the copy; the file is written after, so a long render never holds the audio side up.
    pub fn render_clip(&self, from: u64, len: u64, wav: &std::path::Path, trim: bool, fade_ms: f32) -> std::io::Result<u64> {
        let (data, rate) = {
            let d = self.data.lock().map_err(|_| std::io::Error::other("the rolling buffer is busy"))?;
            let cap = d.l.len() as u64;
            if cap == 0 || len == 0 {
                return Ok(0);
            }
            let oldest = d.total.saturating_sub(cap);
            let from = from.max(oldest);
            let to = (from + len).min(d.total.max(from));
            if to <= from {
                return Ok(0);
            }
            let rate = if d.rate > 0 { d.rate } else { 44100 };
            let mut data = Vec::with_capacity(((to - from) * 2) as usize);
            let mut f = from;
            while f < to {
                let step = (to - f).min(8192) as usize;
                for i in 0..step {
                    let k = ((f + i as u64) % cap) as usize;
                    data.push(d.l[k]);
                    data.push(d.r[k]);
                }
                f += step as u64;
            }
            (data, rate)
        };
        let mut data = if trim { trim_silence(data) } else { data };
        fade_edges(&mut data, rate, fade_ms);
        let frames = (data.len() / 2) as u64;
        if frames == 0 {
            return Ok(0);
        }
        let file = std::fs::File::create(wav)?;
        let mut out = std::io::BufWriter::new(file);
        os_engine::wav::write_f32_stereo_header(&mut out, rate, frames)?;
        out.write_all(bytemuck_bytes(&data))?;
        out.flush()?;
        Ok(frames)
    }

    /// Empties the tape (a project changed, or the setting went off).
    pub fn clear(&self) {
        if let Ok(mut d) = self.data.lock() {
            let secs = d.secs;
            *d = Data::empty(secs);
        }
    }
}

/// Writes one device block into the ring: the streams of one device block sum (the same rule the monitor's tap
/// keeps), a new block overwrites its frames and moves the bookkeeping on, then the pyramid entries it touched are
/// read back from the ring.
fn write_block(d: &mut Data, stereo: &[f32], w: usize, block: u64, at: Instant, rate: u32) {
    let want = d.want(rate);
    if d.l.len() != want {
        d.resize(want);
        d.rate = rate;
    }
    let cap = d.l.len();
    if cap == 0 {
        return;
    }
    let n = (stereo.len() / w) as u64;
    let same = block != 0 && block == d.block && n == d.frames;
    if !same {
        (d.block, d.start, d.frames, d.at, d.rate) = (block, d.total, n, Some(at), rate);
        d.total += n;
    }
    for (i, f) in stereo.chunks_exact(w).enumerate() {
        let k = ((d.start as usize + i) % cap) as usize;
        let (l, r) = (f[0], f[1]);
        if same {
            d.l[k] += l;
            d.r[k] += r;
        } else {
            d.l[k] = l;
            d.r[k] = r;
        }
    }
    // The pyramid entries the block's frames fall in, from the ring (the summed frames included).
    let k0 = ((d.start as usize) % cap) / PYR;
    let k1 = ((d.start as usize + n as usize + cap - 1) % cap) / PYR;
    let steps = if k1 >= k0 { k1 - k0 + 1 } else { d.pl.len() - k0 + k1 + 1 };
    for s in 0..steps.min(d.pl.len()) {
        let k = (k0 + s) % d.pl.len();
        let (mut lo_l, mut hi_l, mut lo_r, mut hi_r) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for j in 0..PYR {
            let i = (k * PYR + j) % cap;
            (lo_l, hi_l) = (lo_l.min(d.l[i]), hi_l.max(d.l[i]));
            (lo_r, hi_r) = (lo_r.min(d.r[i]), hi_r.max(d.r[i]));
        }
        d.pl[k] = (lo_l, hi_l);
        d.pr[k] = (lo_r, hi_r);
    }
}

/// Amplitude under which a frame counts as silence (about -100 dBFS): the trim's edges.
const SILENCE: f32 = 1e-4;

/// The frames between the first and the last that are not silence, one frame of quiet kept on either side (the
/// fade starts there — often the zero crossing the attack rides on): a clip that starts with a second of silence
/// drops that second (it only ever made the clip late).
fn trim_silence(data: Vec<f32>) -> Vec<f32> {
    let loud = |f: &[f32]| f[0].abs() >= SILENCE || f[1].abs() >= SILENCE;
    let first = data.chunks_exact(2).position(loud).map_or(0, |i| i.saturating_sub(1));
    let last = data.chunks_exact(2).rposition(loud).map_or(0, |i| (i + 2).min(data.len() / 2));
    if first >= last {
        return Vec::new();
    }
    let mut out = data;
    out.drain(..first * 2);
    out.truncate((last - first) * 2);
    out
}

/// Fades the clip's edges in and out over `fade_ms`, from (to) the nearest zero crossing within that fade, so a
/// clip that starts mid-waveform does not click. Nothing at 0 ms.
fn fade_edges(data: &mut [f32], rate: u32, fade_ms: f32) {
    let frames = data.len() / 2;
    let fade = (fade_ms.max(0.0) * rate.max(1) as f32 / 1000.0).floor() as usize;
    if fade == 0 || frames < 4 {
        return;
    }
    let fade = fade.min(frames / 2);
    // In: from the first frame on, 0 up to full; out: from the last frame back, down to near 0 (the file ends
    // there, there is nothing to click against).
    let quietest = |mut i: usize, end: usize, back: bool| -> usize {
        let (mut best, mut best_v) = (i, f32::MAX);
        loop {
            let v = data[i * 2].abs() + data[i * 2 + 1].abs();
            if v < best_v {
                (best, best_v) = (i, v);
            }
            if v == 0.0 || i == end {
                break;
            }
            i = if back { i - 1 } else { i + 1 };
        }
        best
    };
    let in0 = quietest(0, fade - 1, false);
    let out0 = quietest(frames - 1, frames - fade, true);
    let in_len = (frames - in0).min(fade);
    for k in 0..in_len {
        let g = k as f32 / fade as f32;
        data[(in0 + k) * 2] *= g;
        data[(in0 + k) * 2 + 1] *= g;
    }
    let out_len = (frames - 1 - out0).min(fade);
    for k in 0..out_len {
        let g = (out_len - k) as f32 / out_len as f32;
        data[(out0 + k) * 2] *= g;
        data[(out0 + k) * 2 + 1] *= g;
    }
}

fn heard_of(d: &Data, now: Instant) -> (u64, u32) {
    let Some(at) = d.at.filter(|_| d.rate > 0) else { return (d.total, d.rate) };
    let into = (now.saturating_duration_since(at).as_secs_f64() * f64::from(d.rate)) as u64;
    (d.start + into.min(d.frames), d.rate)
}

/// `f32` slices as bytes, without a bytemuck dependency.
fn bytemuck_bytes(v: &[f32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), v.len() * 4) }
}

/// The strip's view state: the zoom (seconds shown), the pan back from the live edge, the selection in absolute
/// frames, and the drag-time hold that stops the scroll following the live edge while a selection is drawn. The
/// picture (`App`'s pull, `crate::views::rolling::pull`) fills the rest in: the view's frames, the last heard frame
/// and rate, the wave's rect and its per-column peaks.
#[derive(Default, Clone, Debug)]
pub struct Strip {
    /// Seconds the strip shows; 0 = the whole tape (the default, zoomed out).
    pub zoom: f32,
    /// Frames between the live edge and the view's right edge (0 = the newest frame shows at the right).
    pub pan: u64,
    /// The selection, in absolute frames (`from ..= to`).
    pub sel: Option<(u64, u64)>,
    /// The view's `from` while a selection is drawn (the scroll stands still), set at the press.
    pub hold: Option<u64>,
    /// The collapse arrow hid the strip.
    pub collapsed: bool,
    /// The capture stands still (Ctrl+click on the wave): the strip says so.
    pub paused: bool,
    /// The frames the tape holds (`oldest ..= heard`): columns of nothing outside it are not drawn.
    pub have: (u64, u64),
    /// The view's first frame, its length, the newest frame heard and the tape's rate (0 before the first push).
    pub from: u64,
    pub visible: u64,
    pub heard: u64,
    pub rate: u32,
    /// Where the wave drew last (the collapse arrow's strip left out); `None` while the strip is hidden.
    pub wave: Option<os_ui::Rect>,
    /// Per-column (min, max) of the view, older column first — the pull's picture.
    pub cols: Vec<(f32, f32)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const RATE: u32 = 1000; // a small rate keeps the tests' rings small

    fn tape(secs: f32) -> Rolling {
        let r = Rolling::default();
        r.sync(true, secs);
        r
    }

    fn block(r: &Rolling, frames: u32, at: f32, block: u64, rate: u32) {
        let stereo: Vec<f32> = (0..frames).flat_map(|i| [at + i as f32, -(at + i as f32)]).collect();
        // The device took the block long enough ago that it has played all of it, so `heard` covers it.
        let taken = Instant::now() - std::time::Duration::from_secs(2);
        r.push(&stereo, 2, block, taken, rate);
    }

    /// The tape keeps the newest frames across the wrap: what scan reads back is what was pushed last.
    #[test]
    fn the_tape_reads_back_across_the_wrap() {
        let r = tape(0.5);
        // 300 + 400 frames into a 500-frame tape: the oldest 200 are gone, the newest 500 remain.
        block(&r, 300, 1.0, 1, RATE);
        block(&r, 400, 4.0, 2, RATE);
        let (heard, rate) = r.heard(Instant::now());
        assert_eq!((heard, rate), (700, RATE));
        let mut cols = vec![(0.0f32, 0.0f32); 500];
        r.scan(200, 500, &mut cols);
        // Frame 200 is the oldest kept (the first block's frame 200); frame 699 the newest (the second block's
        // 4.0 + 399). The columns mix both channels: L and -L span -L .. L.
        assert_eq!(cols[0], (-201.0, 201.0), "the oldest kept frame");
        assert_eq!(cols[499], (-403.0, 403.0), "the newest frame");
        // Anything older reads as silence spans.
        let mut gone = vec![(1.0f32, 1.0f32); 10];
        r.scan(0, 10, &mut gone);
        assert!(gone.iter().all(|c| c.0 == 0.0 && c.1 == 0.0));
    }

    /// Zoomed out, the pyramid's peaks are the ring's own: each entry is read from the slot it was written
    /// to (`e`, not `e * PYR` — the wrong slot filled the zoomed-out wave with tall lines that were not in
    /// the audio, the more of the view the pyramid had to cover).
    #[test]
    fn the_pyramid_reads_what_the_ring_holds() {
        let r = tape(3.0); // 3072 frames, 12 pyramid entries
        // Two tapes' worth in 128-frame blocks: the ring wraps exactly once over, so every slot has been
        // written over. Each 256-frame entry carries its own size, so a wrong slot cannot pass unnoticed.
        let (mut f, mut b) = (0u64, 1u64);
        while f < 2 * 3072 {
            let n = 128usize;
            let amp = 0.1 + 0.06 * (f / 256 % 13) as f32;
            let stereo: Vec<f32> = (0..n)
                .flat_map(|i| {
                    let v = amp * (((f + i as u64) * 7) as f32 * 0.013).sin();
                    [v, -v]
                })
                .collect();
            r.push(&stereo, 2, b, Instant::now() - std::time::Duration::from_secs(2), RATE);
            f += n as u64;
            b += 1;
        }
        let (oldest, total, cap) = {
            let d = r.data.lock().expect("tape");
            (d.total - d.l.len() as u64, d.total, d.l.len() as u64)
        };
        assert_eq!((oldest, cap), (3072, 3072), "the tape wrapped once, on an entry's edge");
        // Views whose columns land on whole pyramid entries (no ragged ends): 512 frames or more a column.
        for (from, len, cols) in [(oldest, cap, 2), (oldest, cap, 4), (oldest, cap / 2, 1), (oldest + cap / 2, cap / 2, 1)] {
            // The columns' truth, walked from the ring itself; the lock is dropped before scan runs.
            let truth: Vec<(f32, f32)> = {
                let d = r.data.lock().expect("tape");
                (0..cols)
                    .map(|c| {
                        let a = (from + len * c as u64 / cols as u64).max(oldest);
                        let b = ((from + len * (c as u64 + 1) / cols as u64).max(a + 1)).min(total);
                        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                        for f in a..b {
                            let i = (f % cap) as usize;
                            lo = lo.min(d.l[i]).min(d.r[i]);
                            hi = hi.max(d.l[i]).max(d.r[i]);
                        }
                        if lo <= hi { (lo, hi) } else { (0.0, 0.0) }
                    })
                    .collect()
            };
            let mut got = vec![(0.0f32, 0.0f32); cols];
            r.scan(from, len, &mut got);
            for (c, g) in got.iter().enumerate() {
                assert!(
                    (g.0 - truth[c].0).abs() < 1e-5 && (g.1 - truth[c].1).abs() < 1e-5,
                    "column {c} of the view at {from} +{len}: scan {:?}, the ring holds {:?}",
                    (g.0, g.1),
                    truth[c]
                );
            }
        }
    }

    /// The streams of one device block sum, as the device hears them (the monitor's `Tap` rule).
    #[test]
    fn concurrent_streams_sum_into_one_block() {
        let r = tape(1.0);
        block(&r, 100, 1.0, 7, RATE);
        // The same device block again: the same frames, another stream's half.
        let stereo: Vec<f32> = vec![0.5; 200];
        r.push(&stereo, 2, 7, Instant::now(), RATE);
        let (heard, _) = r.heard(Instant::now());
        assert_eq!(heard, 100, "the same block is not counted twice");
        let mut cols = vec![(0.0f32, 0.0f32); 100];
        r.scan(0, 100, &mut cols);
        assert_eq!(cols[0], (-0.5, 1.5), "1.0 from the first stream + 0.5 from the second, L and -L mixed");
    }

    /// A selection renders as a float stereo WAV of the tape's rate.
    #[test]
    fn a_selection_renders_as_a_wav() {
        let r = tape(1.0);
        block(&r, 100, 2.0, 1, RATE);
        let wav = std::env::temp_dir().join(format!("rolling_test_{}.wav", std::process::id()));
        let n = r.render(10, 50, &wav).expect("render");
        assert_eq!(n, 50);
        let bytes = std::fs::read(&wav).expect("read");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(bytes.len(), 44 + 50 * 8, "44 bytes of header, 50 stereo f32 frames");
        std::fs::remove_file(&wav).ok();
    }

    /// Off, the tape keeps nothing and lets its memory go.
    #[test]
    fn off_the_tape_keeps_nothing() {
        let r = Rolling::default();
        r.sync(true, 1.0);
        block(&r, 100, 1.0, 1, RATE);
        r.sync(false, 1.0);
        block(&r, 100, 2.0, 2, RATE);
        let (heard, _) = r.heard(Instant::now());
        assert_eq!(heard, 0);
        assert!(!r.enabled());
    }

    // The record → drag out → playlist loop, measured (the drag-out came out sped up and noisier every time it
    // was repeated: each stage below has to carry the captured frames at the captured rate).

    /// `frames` of a `hz` sine, interleaved stereo.
    fn sine(frames: usize, hz: f64, rate: u32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(rate)).sin() as f32 * 0.5;
                [v, v]
            })
            .collect()
    }

    /// Frequency of a mono slice by its rising zero crossings (periods per second).
    fn hz_of(mono: &[f32], rate: u32) -> f64 {
        let periods = mono.iter().zip(mono.iter().skip(1)).filter(|&(a, b)| *a <= 0.0 && *b > 0.0).count();
        periods as f64 * f64::from(rate) / mono.len().max(1) as f64
    }

    /// The tape carries the capture at the captured rate: a selection renders as a WAV whose frames play the
    /// frequency that went in, and the sampler reads exactly what the render wrote.
    #[test]
    fn the_tape_round_trip_keeps_the_rate_and_the_frames() {
        const RATE: u32 = 48000;
        let r = Rolling::default();
        r.sync(true, 10.0);
        let wave = sine(RATE as usize, 1000.0, RATE);
        for (b, chunk) in wave.chunks(480 * 2).enumerate() {
            r.push(chunk, 2, b as u64 + 1, Instant::now() - std::time::Duration::from_secs(2), RATE);
        }
        let wav = std::env::temp_dir().join(format!("rolling_loop_{}.wav", std::process::id()));
        let n = r.render(12000, 24000, &wav).expect("render");
        assert_eq!(n, 24000, "the selection's frames, all of them");
        let s = os_engine::sample::load(&wav).expect("the drag-out loads as a sample");
        assert_eq!(s.rate, RATE, "the file's rate is the tape's");
        assert_eq!(s.frames(), 24000, "the file's frames are the selection's");
        assert!((hz_of(&s.left, RATE) - 1000.0).abs() < 5.0, "the sample plays at {} Hz, want 1000", hz_of(&s.left, RATE));
        let (frames, rate) = os_engine::sample::length(&wav).expect("the drag-out's header length");
        assert_eq!((frames, rate), (24000, RATE));
        std::fs::remove_file(&wav).ok();
    }

    /// A dragged-out clip plays on the playlist at the speed it was captured: a 1000 Hz sine stays 1000 Hz.
    #[test]
    fn a_dragged_clip_plays_at_its_captured_speed() {
        const RATE: u32 = 48000;
        let r = Rolling::default();
        r.sync(true, 10.0);
        let wave = sine(RATE as usize, 1000.0, RATE);
        for (b, chunk) in wave.chunks(480 * 2).enumerate() {
            r.push(chunk, 2, b as u64 + 1, Instant::now() - std::time::Duration::from_secs(2), RATE);
        }
        let wav = std::env::temp_dir().join(format!("rolling_clip_{}.wav", std::process::id()));
        r.render(0, 24000, &wav).expect("render");

        // The channel `add_audio_clip` makes (playlist.rs): an audio clip of the file, no stretch, no pitch.
        let mut project = crate::app::test_project();
        project.tempo = Some(130.0);
        let index = project.channels.iter().map(|c| c.index + 1).max().unwrap_or(0);
        let ticks = (0.5 * 130.0 / 60.0 * f64::from(project.ppq.max(1))).round() as u32;
        project.channels.push(os_model::Channel {
            index,
            kind: os_model::ChannelKind::AudioClip,
            sample_path: Some(wav.to_string_lossy().into_owned()),
            sound: os_model::ChannelSound { sampler_flags: Some(os_model::ChannelSound::AUDIO_CLIP_FLAGS), ..Default::default() },
            ..project.channels[0].clone()
        });
        project.arrangements = vec![os_model::Arrangement {
            clips: vec![os_model::Clip::new(0, ticks, 0, os_model::ClipSource::Channel(index))],
            ..Default::default()
        }];
        let mut lib = os_engine::SampleLibrary::default();
        let mut e = os_engine::Engine::new(&project, &mut lib, RATE, os_engine::PlayMode::Song);
        let mut out = Vec::new();
        e.render_ticks(usize::MAX, &mut out);
        assert!(out.len() >= 2 * 12000, "the clip's half second played, got {} frames", out.len() / 2);
        let hz = hz_of(&out[..2 * 12000].chunks_exact(2).map(|f| f[0]).collect::<Vec<_>>(), RATE);
        assert!((hz - 1000.0).abs() < 5.0, "the clip plays at {hz} Hz, want 1000 (speed {})", hz / 1000.0);
        std::fs::remove_file(&wav).ok();
    }

    /// A push that finds the data lock busy waits in the spool and lands in the tape with the next push: a busy UI
    /// (a scan, a drag-out) never takes a device block out of the tape (the lost blocks made every drag-out play
    /// short, and a little faster each time it was repeated).
    #[test]
    fn a_push_while_the_lock_is_busy_is_not_lost() {
        let r = tape(1.0);
        block(&r, 100, 1.0, 1, RATE);
        // The UI holds the data lock while the device pushes two blocks: they may only wait.
        let guard = r.data.lock().expect("locked");
        block(&r, 100, 2.0, 2, RATE);
        block(&r, 100, 3.0, 3, RATE);
        assert_eq!(r.pending.lock().expect("spool").len(), 2, "both blocks wait in the spool");
        drop(guard);
        // The next push flushes them in order, then writes its own.
        block(&r, 100, 4.0, 4, RATE);
        assert!(r.pending.lock().expect("spool").is_empty());
        let (heard, _) = r.heard(Instant::now());
        assert_eq!(heard, 400, "all four blocks are on the tape, none lost");
        // The spooled blocks' frames read back, in their places.
        let mut cols = vec![(0.0f32, 0.0f32); 400];
        r.scan(0, 400, &mut cols);
        assert_eq!(cols[150], (-52.0, 52.0), "the second block's frame 50 (2.0 + 50), summed L and -L");
        assert_eq!(cols[399], (-103.0, 103.0), "the last block's last frame");
    }

    /// The spool itself never grows past its cap: the oldest waiting block gives way.
    #[test]
    fn the_spool_keeps_its_cap() {
        let r = tape(1.0);
        let guard = r.data.lock().expect("locked");
        for b in 1..=(super::SPOOL + 16) as u64 {
            block(&r, 100, b as f32, b, RATE);
        }
        assert_eq!(r.pending.lock().expect("spool").len(), super::SPOOL);
        drop(guard);
        block(&r, 100, 999.0, 999, RATE);
        let (heard, _) = r.heard(Instant::now());
        assert_eq!(heard, (super::SPOOL + 1) as u64 * 100, "the kept waiting blocks and the one just pushed");
        assert_eq!(r.dropped.load(Ordering::Relaxed), 16, "the oldest waiting blocks gave way, counted");
    }

    /// A paused capture stands still: nothing lands on the tape until it resumes.
    #[test]
    fn a_paused_capture_keeps_nothing() {
        let r = tape(1.0);
        block(&r, 100, 1.0, 1, RATE);
        r.set_paused(true);
        assert!(r.paused());
        block(&r, 100, 2.0, 2, RATE);
        let (heard, _) = r.heard(Instant::now());
        assert_eq!(heard, 100, "the paused block is not on the tape");
        r.set_paused(false);
        block(&r, 100, 3.0, 3, RATE);
        let (heard, _) = r.heard(Instant::now());
        assert_eq!(heard, 200, "the capture went on where it stood");
    }

    /// A clip render trims the silence at either end and fades the edges over the asked milliseconds, from the
    /// nearest zero crossing, so a clip that starts mid-waveform does not click.
    #[test]
    fn a_clip_render_trims_and_fades() {
        const RATE: u32 = 8000;
        // A second of tape at its own rate: half silence, half a full-scale sine (a push sets the tape's rate).
        let r = Rolling::default();
        r.sync(true, 1.0);
        let mut stereo = vec![0.0f32; 8000 * 2];
        for (i, f) in stereo[4000 * 2..].chunks_exact_mut(2).enumerate() {
            let v = (2.0 * std::f64::consts::PI * 500.0 * i as f64 / f64::from(RATE)).sin() as f32;
            (f[0], f[1]) = (v, v);
        }
        r.push(&stereo, 2, 1, Instant::now() - std::time::Duration::from_secs(2), RATE);
        let wav = std::env::temp_dir().join(format!("rolling_trim_{}.wav", std::process::id()));
        let n = r.render_clip(0, 8000, &wav, true, 2.0).expect("render");
        assert_eq!(n, 4000, "the leading half second of silence is gone, the sine's frames stay");
        let s = os_engine::sample::load(&wav).expect("loads");
        assert_eq!(s.rate, RATE);
        // The fade is 16 frames (2 ms of 8000): the edges are held back, past it the wave is untouched.
        let first: f32 = s.left[..8].iter().map(|v| v.abs()).fold(0.0, f32::max);
        assert!(first < 0.75, "the fade-in holds the first frames back: {first}");
        let last: f32 = s.left[s.left.len() - 8..].iter().map(|v| v.abs()).fold(0.0, f32::max);
        assert!(last < 0.75, "the fade-out holds the last frames back: {last}");
        let peak: f32 = s.left.iter().map(|v| v.abs()).fold(0.0, f32::max);
        assert!(peak > 0.99, "the sine's own size is untouched past the fades: {peak}");
        // Without the trim, the same render keeps every frame.
        let n = r.render_clip(0, 8000, &wav, false, 0.0).expect("render");
        assert_eq!(n, 8000);
        std::fs::remove_file(&wav).ok();
    }
}
