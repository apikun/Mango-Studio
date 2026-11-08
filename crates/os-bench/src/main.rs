// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! `os-bench` — Mango Studio's benchmark suite. Renders synthetic songs through the
//! engine, times every mixer-slot effect, exercises the project model's file I/O and
//! note editing, and reports a terminal table plus optional JSON and Markdown files:
//!
//! ```text
//! cargo run -p os-bench --release                       # full run, terminal table
//! cargo run -p os-bench --release -- --quick            # short run for a quick look
//! cargo run -p os-bench --release -- --filter engine/   # one group
//! cargo run -p os-bench --release -- --json out.json --markdown out.md
//! ```

mod harness;
mod scenarios;
mod suites;

use std::time::Instant;

use harness::{fmt_ns, BenchResult, Meta, RunCfg};

struct Args {
    list: bool,
    filters: Vec<String>,
    quick: bool,
    seconds: f64,
    rate: u32,
    json: Option<String>,
    markdown: Option<String>,
    cfg: RunCfg,
}

fn usage() -> String {
    "usage: os-bench [--list] [--filter <substr>]... [--quick] [--seconds <s>] [--rate <hz>]
                 [--json <path>] [--markdown <path>] [--warmup <n>] [--min-iters <n>]
                 [--max-iters <n>] [--target-ms <ms>]

  --list          print the benchmark names and exit
  --filter        only run benchmarks whose name contains <substr> (repeatable)
  --quick         fewer, shorter iterations (also caps --seconds at 8)
  --seconds       song length the engine benches render (default 15)"
        .into()
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        list: false,
        filters: vec![],
        quick: false,
        seconds: 15.0,
        rate: 48000,
        json: None,
        markdown: None,
        cfg: RunCfg::full(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| -> Result<String, String> {
            it.next().ok_or_else(|| format!("{} needs a value\n{}", name, usage()))
        };
        match arg.as_str() {
            "--list" => a.list = true,
            "--quick" => a.quick = true,
            "--filter" => a.filters.push(value("--filter")?.to_lowercase()),
            "--seconds" => a.seconds = value("--seconds")?.parse().map_err(|_| "--seconds needs a number")?,
            "--rate" => a.rate = value("--rate")?.parse().map_err(|_| "--rate needs a number")?,
            "--json" => a.json = Some(value("--json")?),
            "--markdown" => a.markdown = Some(value("--markdown")?),
            "--warmup" => a.cfg.warmup = value("--warmup")?.parse().map_err(|_| "--warmup needs a number")?,
            "--min-iters" => a.cfg.min_iters = value("--min-iters")?.parse().map_err(|_| "--min-iters needs a number")?,
            "--max-iters" => a.cfg.max_iters = value("--max-iters")?.parse().map_err(|_| "--max-iters needs a number")?,
            "--target-ms" => a.cfg.target_ms = value("--target-ms")?.parse().map_err(|_| "--target-ms needs a number")?,
            "--help" | "-h" => return Err(usage()),
            other => return Err(format!("unknown argument {}\n{}", other, usage())),
        }
    }
    if !(1000.0..=192000.0).contains(&(a.rate as f64)) {
        return Err("--rate wants something between 8000 and 192000".into());
    }
    if a.seconds < 1.0 || a.seconds > 600.0 {
        return Err("--seconds wants 1..600".into());
    }
    if a.quick {
        a.seconds = a.seconds.min(8.0);
    }
    Ok(a)
}

fn cpu_model() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(info) = std::fs::read_to_string("/proc/cpuinfo") {
            for line in info.lines() {
                if let Some(rest) = line.strip_prefix("model name") {
                    if let Some((_, v)) = rest.split_once(':') {
                        return v.trim().to_string();
                    }
                }
            }
        }
    }
    "unknown".into()
}

fn git_output(args: &[&str]) -> Option<String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn meta(args: &Args) -> Meta {
    Meta {
        commit: git_output(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into()),
        rustc: rustc_version(),
        profile: if cfg!(debug_assertions) { "debug" } else { "release" },
        host: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        cpu: cpu_model(),
        rate: args.rate,
        seconds: args.seconds,
        quick: args.quick,
    }
}

fn rustc_version() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    for exe in ["rustc".to_string(), format!("{home}/.cargo/bin/rustc")] {
        if let Ok(o) = std::process::Command::new(&exe).arg("--version").output() {
            if o.status.success() {
                return String::from_utf8_lossy(&o.stdout).trim().to_string();
            }
        }
    }
    "unknown".into()
}

fn main() {
    if let Err(e) = run() {
        eprintln!("os-bench: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let opts = suites::Options { rate: args.rate, seconds: args.seconds };

    // The stand-in sample the sampler scenes play; removed again at the end.
    let sample = std::env::temp_dir().join(format!("os-bench-sample-{}.wav", std::process::id()));
    scenarios::write_sine_wav(&sample, args.rate, 2.0).map_err(|e| format!("{}: {}", sample.display(), e))?;

    let defs = suites::build(&opts, &sample);
    if args.list {
        for d in &defs {
            println!("{:<36} {:<6} {}", d.name, d.group, d.desc);
        }
        std::fs::remove_file(&sample).ok();
        return Ok(());
    }
    let defs: Vec<_> = defs
        .into_iter()
        .filter(|d| args.filters.is_empty() || args.filters.iter().any(|f| d.name.to_lowercase().contains(f)))
        .collect();
    if defs.is_empty() {
        std::fs::remove_file(&sample).ok();
        return Err("no benchmark matches the filter".into());
    }

    let mut meta = meta(&args);
    meta.rustc = rustc_version();
    println!("{}", harness::to_text(&meta, &[])); // header line only
    println!("running {} benchmark{}\n", defs.len(), if defs.len() == 1 { "" } else { "s" });

    let cfg = if args.quick { RunCfg::quick() } else { args.cfg };
    let mut results = Vec::new();
    for d in defs {
        let mut d = d;
        let started = Instant::now();
        let times = harness::measure(cfg, &mut d.f);
        let r = BenchResult {
            name: d.name.clone(),
            group: d.group,
            desc: d.desc.clone(),
            warmup: args.cfg.warmup,
            work: d.work,
            times,
            note: d.note.clone(),
        };
        let s = r.summary();
        println!(
            "{:<36} {:>10} {:>10} {:>10}  {:<24} ({:.1}s)",
            r.name,
            fmt_ns(s.min),
            fmt_ns(s.mean),
            fmt_ns(s.max),
            r.work.throughput(s.mean),
            started.elapsed().as_secs_f64(),
        );
        results.push(r);
    }

    println!();
    println!("{}", harness::to_text(&meta, &results));
    if let Some(path) = &args.json {
        std::fs::write(path, harness::to_json(&meta, &results)).map_err(|e| format!("{}: {}", path, e))?;
        println!("wrote {}", path);
    }
    if let Some(path) = &args.markdown {
        std::fs::write(path, harness::to_markdown(&meta, &results)).map_err(|e| format!("{}: {}", path, e))?;
        println!("wrote {}", path);
    }

    std::fs::remove_file(&sample).ok();
    Ok(())
}
