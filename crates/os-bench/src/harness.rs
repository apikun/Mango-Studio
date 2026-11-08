// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The measurement harness: timing loop, statistics and the three report formats
//! (terminal table, JSON for machines, Markdown for issues and chat posts).

use std::time::Duration;

/// One benchmark's raw iteration times, in nanoseconds.
#[derive(Clone, Debug)]
pub struct Times(pub Vec<u64>);

/// Descriptive statistics over iteration times, in nanoseconds.
#[derive(Clone, Copy, Debug)]
pub struct Summary {
    pub min: f64,
    pub p50: f64,
    pub mean: f64,
    pub p95: f64,
    pub max: f64,
    /// Population standard deviation.
    pub sd: f64,
}

impl Summary {
    pub fn of(times: &[u64]) -> Summary {
        let mut v: Vec<f64> = times.iter().map(|&t| t as f64).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = v.len().max(1);
        let mean = v.iter().sum::<f64>() / n as f64;
        let var = v.iter().map(|t| (t - mean) * (t - mean)).sum::<f64>() / n as f64;
        // Nearest-rank percentiles on the sorted times.
        let pick = |p: f64| v[((p * (n - 1) as f64).round() as usize).min(n - 1)];
        Summary {
            min: v[0],
            p50: pick(0.50),
            mean,
            p95: pick(0.95),
            max: v[n - 1],
            sd: var.sqrt(),
        }
    }

    /// How many times slower the slowest run was than the fastest (noise indicator).
    pub fn spread(&self) -> f64 {
        if self.min <= 0.0 { f64::INFINITY } else { self.max / self.min }
    }
}

/// The work one iteration does, for throughput columns (`qty` per iteration).
#[derive(Clone, Copy, Debug)]
pub enum Work {
    /// Stereo frames rendered or processed (`rate` turns frames/s into x-realtime).
    Frames { qty: f64, rate: u32 },
    Bytes { qty: f64 },
    Notes { qty: f64 },
    Opens { qty: f64 },
}

impl Work {
    pub fn describe(&self) -> String {
        match *self {
            Work::Frames { qty, rate } => format!("{} frames @ {} Hz", fmt_qty(qty), rate),
            Work::Bytes { qty } => format!("{} bytes", fmt_qty(qty)),
            Work::Notes { qty } => format!("{} notes", fmt_qty(qty)),
            Work::Opens { qty } => format!("{} opens", fmt_qty(qty)),
        }
    }

    /// Throughput for the mean iteration time, as a human string.
    pub fn throughput(&self, mean_ns: f64) -> String {
        if mean_ns <= 0.0 {
            return "?".into();
        }
        match *self {
            Work::Frames { qty, rate } => {
                let fps = qty / (mean_ns / 1e9);
                let rt = if rate > 0 { fps / rate as f64 } else { 0.0 };
                format!("{}/s ({:.1}x rt)", fmt_qty(fps), rt)
            }
            Work::Bytes { qty } => format!("{}/s", fmt_qty(qty / (mean_ns / 1e9))),
            Work::Notes { qty } => format!("{}/s", fmt_qty(qty / (mean_ns / 1e9))),
            Work::Opens { qty } => format!("{}/s", fmt_qty(qty / (mean_ns / 1e9))),
        }
    }
}

/// One finished benchmark.
#[derive(Clone, Debug)]
pub struct BenchResult {
    pub name: String,
    pub group: &'static str,
    pub desc: String,
    pub warmup: usize,
    pub work: Work,
    pub times: Times,
    pub note: Option<String>,
}

impl BenchResult {
    pub fn summary(&self) -> Summary {
        Summary::of(&self.times.0)
    }
}

/// How many iterations a benchmark gets.
#[derive(Clone, Copy, Debug)]
pub struct RunCfg {
    pub warmup: usize,
    pub min_iters: usize,
    pub max_iters: usize,
    /// Keep iterating until this much wall time went into the benchmark (ms).
    pub target_ms: u64,
}

impl RunCfg {
    pub fn quick() -> RunCfg {
        RunCfg { warmup: 1, min_iters: 2, max_iters: 4, target_ms: 150 }
    }
    pub fn full() -> RunCfg {
        RunCfg { warmup: 1, min_iters: 3, max_iters: 6, target_ms: 300 }
    }
}

/// Times `f` for `cfg` iterations and returns the raw iteration times. `f` returns one
/// iteration's duration; everything it does counts, so setup belongs outside it.
pub fn measure(cfg: RunCfg, mut f: impl FnMut() -> Duration) -> Times {
    for _ in 0..cfg.warmup {
        f();
    }
    let mut times = Vec::new();
    let started = std::time::Instant::now();
    loop {
        times.push(f().as_nanos() as u64);
        if times.len() >= cfg.min_iters && started.elapsed().as_millis() as u64 >= cfg.target_ms {
            break;
        }
        if times.len() >= cfg.max_iters {
            break;
        }
    }
    Times(times)
}

/// `1234` -> `1.23k`, `5600000` -> `5.6M`, keeping decimals only under 100.
pub fn fmt_qty(v: f64) -> String {
    let (q, suffix) = if v >= 1e9 {
        (v / 1e9, "G")
    } else if v >= 1e6 {
        (v / 1e6, "M")
    } else if v >= 1e3 {
        (v / 1e3, "k")
    } else {
        (v, "")
    };
    let digits = if q >= 100.0 || suffix.is_empty() { 0 } else { 2 };
    format!("{:.*}{}", digits, q, suffix)
}

/// `1234` ns -> `1.23µs`, `1234000` ns -> `1.23ms`.
pub fn fmt_ns(ns: f64) -> String {
    let (q, unit) = if ns >= 1e9 {
        (ns / 1e9, "s")
    } else if ns >= 1e6 {
        (ns / 1e6, "ms")
    } else if ns >= 1e3 {
        (ns / 1e3, "µs")
    } else {
        (ns, "ns")
    };
    format!("{:.2}{}", q, unit)
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// The run's environment, printed in every report.
#[derive(Clone, Debug)]
pub struct Meta {
    pub commit: String,
    pub rustc: String,
    pub profile: &'static str,
    pub host: String,
    pub cpu: String,
    pub rate: u32,
    pub seconds: f64,
    pub quick: bool,
}

fn json_num(v: f64) -> String {
    if v.is_finite() { format!("{:.4}", v) } else { "null".to_string() }
}

fn work_qty(w: &Work) -> f64 {
    match *w {
        Work::Frames { qty, .. } | Work::Bytes { qty } | Work::Notes { qty } | Work::Opens { qty } => qty,
    }
}

/// The whole run, as JSON (raw iteration times included, for offline analysis).
pub fn to_json(meta: &Meta, results: &[BenchResult]) -> String {
    let mut out = String::new();
    out.push_str("{\n\"meta\": {");
    out.push_str(&format!("\n  \"commit\": \"{}\",", esc(&meta.commit)));
    out.push_str(&format!("\n  \"rustc\": \"{}\",", esc(&meta.rustc)));
    out.push_str(&format!("\n  \"profile\": \"{}\",", meta.profile));
    out.push_str(&format!("\n  \"host\": \"{}\",", esc(&meta.host)));
    out.push_str(&format!("\n  \"cpu\": \"{}\",", esc(&meta.cpu)));
    out.push_str(&format!("\n  \"rate\": {},", meta.rate));
    out.push_str(&format!("\n  \"seconds\": {},", meta.seconds));
    out.push_str(&format!("\n  \"quick\": {}", meta.quick));
    out.push_str("\n},\n\"results\": [");
    for (i, r) in results.iter().enumerate() {
        let s = r.summary();
        let times: Vec<String> = r.times.0.iter().map(|t| format!("{}", t / 1000)).collect();
        let note = r.note.as_deref().map(|n| format!("\"{}\"", esc(n))).unwrap_or_else(|| "null".into());
        out.push_str(&format!(
            "\n  {{\"name\": \"{}\", \"group\": \"{}\", \"desc\": \"{}\", \"warmup\": {}, \"iters\": {}, \
             \"work_qty\": {:.0}, \"work\": \"{}\", \"min_ns\": {}, \"p50_ns\": {}, \"mean_ns\": {}, \
             \"p95_ns\": {}, \"max_ns\": {}, \"sd_ns\": {}, \"spread\": {}, \"times_us\": [{}], \"note\": {}}}{}",
            esc(&r.name), r.group, esc(&r.desc), r.warmup, r.times.0.len(),
            work_qty(&r.work), esc(&r.work.describe()),
            json_num(s.min), json_num(s.p50), json_num(s.mean), json_num(s.p95), json_num(s.max),
            json_num(s.sd), json_num(s.spread()), times.join(", "), note,
            if i + 1 < results.len() { "," } else { "" },
        ));
    }
    out.push_str("\n]}\n");
    out
}

/// The whole run, as Markdown (paste-ready for issues, releases and chat).
pub fn to_markdown(meta: &Meta, results: &[BenchResult]) -> String {
    let mut out = String::new();
    out.push_str("# Mango Studio benchmark report\n\n");
    out.push_str(&format!(
        "| commit | rustc | profile | host | cpu | rate | song length | mode |\n|---|---|---|---|---|---|---|---|\n\
         | {} | {} | {} | {} | {} | {} Hz | {} s | {} |\n\n",
        meta.commit, esc(&meta.rustc), meta.profile, esc(&meta.host), esc(&meta.cpu), meta.rate, meta.seconds,
        if meta.quick { "quick" } else { "full" },
    ));
    let mut group = "";
    for r in results {
        if r.group != group {
            group = r.group;
            out.push_str(&format!("\n## {}\n\n", group));
            out.push_str("| benchmark | work | iters | min | p50 | mean | p95 | max | throughput |\n");
            out.push_str("|---|---|---|---|---|---|---|---|---|\n");
        }
        let s = r.summary();
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            esc(&r.name), esc(&r.work.describe()), r.times.0.len(),
            fmt_ns(s.min), fmt_ns(s.p50), fmt_ns(s.mean), fmt_ns(s.p95), fmt_ns(s.max),
            esc(&r.work.throughput(s.mean)),
        ));
    }
    out.push_str(
        "\n## How to read this\n\n\
        - `min` is the least-noisy estimate of the true cost; `mean` is what a sustained run feels like; \
        `p95`/`max` show scheduling noise. Compare `min` across runs on the same machine.\n\
        - `x rt` is the real-time multiplier: a 30 s song rendered in 1 s is 30x rt. Anything above ~1x per \
        core renders faster than it plays; live playback needs headroom, so look at `min` there.\n\
        - Iteration times (in µs) are in the JSON report (`--json`), so percentiles can be re-cut offline.\n",
    );
    out
}

/// The terminal table: one line per benchmark, groups separated by headers.
pub fn to_text(meta: &Meta, results: &[BenchResult]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "os-bench · commit {} · {} · {} · {} · {} Hz · {} s song · {}\n\n",
        meta.commit, meta.rustc, meta.profile, meta.host, meta.rate, meta.seconds,
        if meta.quick { "quick" } else { "full" },
    ));
    let (mut w, mut it, mut t1, mut t2, mut t3, mut tp) = (9usize, 5, 10, 10, 10, 22);
    let rows: Vec<(String, String, String, String, String, String, String)> = results
        .iter()
        .map(|r| {
            let s = r.summary();
            w = w.max(r.name.len());
            it = it.max(r.times.0.len().to_string().len());
            t1 = t1.max(fmt_ns(s.p50).len());
            t2 = t2.max(fmt_ns(s.p95).len());
            t3 = t3.max(fmt_ns(s.max).len());
            tp = tp.max(r.work.throughput(s.mean).len());
            (
                r.name.clone(),
                if r.desc.chars().count() > 60 {
                    let head: String = r.desc.chars().take(59).collect();
                    format!("{}…", head)
                } else {
                    r.desc.clone()
                },
                r.times.0.len().to_string(),
                fmt_ns(s.min),
                fmt_ns(s.p50),
                fmt_ns(s.mean),
                r.work.throughput(s.mean),
            )
        })
        .collect();
    out.push_str(&format!(
        "{:<w$}  {:<d$}  {:>it$}  {:>t1$}  {:>t2$}  {:>t3$}  {}\n",
        "benchmark", "work", "iters", "min", "p50", "mean", "throughput",
        w = w, d = 24, it = it, t1 = t1, t2 = t2, t3 = t3,
    ));
    let mut group = "";
    for (r, row) in results.iter().zip(&rows) {
        if r.group != group {
            group = r.group;
            out.push_str(&format!("-- {}\n", group));
        }
        out.push_str(&format!(
            "{:<w$}  {:<d$}  {:>it$}  {:>t1$}  {:>t2$}  {:>t3$}  {}\n",
            row.0, row.1, row.2, row.3, row.4, row.5, row.6,
            w = w, d = 24, it = it, t1 = t1, t2 = t2, t3 = t3,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_cut_the_sorted_times() {
        let s = Summary::of(&[100, 200, 300, 400, 500]);
        assert_eq!(s.min, 100.0);
        assert_eq!(s.p50, 300.0);
        assert_eq!(s.p95, 500.0);
        assert_eq!(s.max, 500.0);
        assert!((s.mean - 300.0).abs() < 1e-9);
        // Population sd of 100..500 step 100 is ~141.42.
        assert!((s.sd - 141.42135623730951).abs() < 1e-6);
    }

    #[test]
    fn times_format_in_readable_units() {
        assert_eq!(fmt_ns(500.0), "500.00ns");
        assert_eq!(fmt_ns(2500.0), "2.50µs");
        assert_eq!(fmt_ns(2_500_000.0), "2.50ms");
        assert_eq!(fmt_ns(2.5e9), "2.50s");
    }

    #[test]
    fn quantities_format_with_si_prefixes() {
        assert_eq!(fmt_qty(512.0), "512");
        assert_eq!(fmt_qty(1234.0), "1.23k");
        assert_eq!(fmt_qty(1_440_000.0), "1.44M");
        assert_eq!(fmt_qty(150_000.0), "150k");
    }

    #[test]
    fn throughput_reports_x_realtime() {
        // 24 000 frames (0.5 s) at 48 kHz processed in 1 ms = 500x realtime.
        let w = Work::Frames { qty: 24_000.0, rate: 48000 };
        assert_eq!(w.throughput(1e6), "24.00M/s (500.0x rt)");
    }

    #[test]
    fn measure_respects_the_iteration_floor() {
        let t = measure(RunCfg { warmup: 0, min_iters: 4, max_iters: 100, target_ms: 0 }, || {
            Duration::from_nanos(1)
        });
        assert_eq!(t.0.len(), 4);
    }

    #[test]
    fn json_escapes_and_carries_the_raw_times() {
        let meta = Meta {
            commit: "abc\"123".into(),
            rustc: "rustc 1.0".into(),
            profile: "debug",
            host: "linux".into(),
            cpu: "x".into(),
            rate: 48000,
            seconds: 1.0,
            quick: true,
        };
        let r = BenchResult {
            name: "engine/render".into(),
            group: "engine",
            desc: "8 tone channels".into(),
            warmup: 1,
            work: Work::Frames { qty: 24000.0, rate: 48000 },
            times: Times(vec![1_000_000, 2_000_000]),
            note: Some("a \"quoted\" note".into()),
        };
        let j = to_json(&meta, &[r]);
        assert!(j.contains("\"commit\": \"abc\\\"123\""));
        assert!(j.contains("a \\\"quoted\\\" note"));
        assert!(j.contains("\"times_us\": [1000, 2000]"));
        assert!(j.contains("\"iters\": 2"));
    }

    #[test]
    fn markdown_has_a_table_per_group() {
        let meta = Meta {
            commit: "abc".into(), rustc: "r".into(), profile: "release", host: "h".into(),
            cpu: "c".into(), rate: 48000, seconds: 8.0, quick: false,
        };
        let mk = |name: &str, group: &'static str| BenchResult {
            name: name.into(),
            group,
            desc: "d".into(),
            warmup: 1,
            work: Work::Opens { qty: 1.0 },
            times: Times(vec![1_000_000]),
            note: None,
        };
        let md = to_markdown(&meta, &[mk("a", "engine"), mk("b", "engine"), mk("c", "model")]);
        assert_eq!(md.matches("## engine").count(), 1);
        assert_eq!(md.matches("## model").count(), 1);
        assert!(md.contains("| a |"));
        assert!(md.contains("## How to read this"));
    }

    #[test]
    fn text_table_lines_up() {
        let meta = Meta {
            commit: "abc".into(), rustc: "r".into(), profile: "debug", host: "h".into(),
            cpu: "c".into(), rate: 48000, seconds: 8.0, quick: true,
        };
        let r = BenchResult {
            name: "engine/empty".into(),
            group: "engine",
            desc: "no channels".into(),
            warmup: 1,
            work: Work::Frames { qty: 24000.0, rate: 48000 },
            times: Times(vec![1_000_000]),
            note: None,
        };
        let t = to_text(&meta, &[r]);
        assert!(t.contains("engine/empty"));
        assert!(t.contains("-- engine"));
        assert!(t.contains("500.0x rt"));
    }
}
