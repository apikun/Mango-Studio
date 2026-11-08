# Benchmarks

`crates/os-bench` is Mango Studio's benchmark suite: it renders synthetic songs through
the engine, times every mixer-slot effect on its own, exercises the project model's
file I/O and note editing, and prints a terminal table with optional JSON and Markdown
reports. No fixture files, no network: every workload is built in code from a fixed
seed, so two runs on one machine do exactly the same work and the numbers can be
compared honestly.

## Running it

```sh
cargo run -p os-bench --release                       # full run (about 5 minutes)
cargo run -p os-bench --release -- --quick            # a short look (about 1 minute)
cargo run -p os-bench --release -- --filter engine/   # one group (engine, fx, model, wav)
cargo run -p os-bench --release -- --filter fx/vocodex
cargo run -p os-bench --release -- --json out.json --markdown out.md
cargo run -p os-bench --release -- --list             # all benchmark names
cargo run -p os-bench --release -- --seconds 30 --rate 96000
```

`--list` prints every benchmark with its workload. Iterations are adaptive: each
benchmark runs until it has spent a target slice of wall time in it (`--target-ms`,
default 300), at least `--min-iters` and at most `--max-iters` times. `--quick` lowers
all of them at once.

## What is measured

| Group | Benchmarks | What they tell you |
|---|---|---|
| `engine` | `open-<scene>` / `render-<scene>` for six scenes | The cost of building the engine over a project (samples pre-cached), and of rendering a fixed tick count of it. Scenes: `empty`, `tones-8`, `tones-32`, `fx-chain` (8 tones through 20 effect slots), `automation-8` (an automation clip sweeping a channel parameter), `sampler-8` (8 channels on a decoded WAV). |
| `fx` | one per mixer-slot effect (17) | Each effect alone, processing 1 s of interleaved stereo in place with a default state. This is the per-slot budget: a chain costs the sum of its slots. |
| `model` | `flp-save`, `flp-load`, `note-ops` | Saving a 16-channel, 64-pattern x 256-note project to FLP bytes, loading it back, and editing notes (clone, 2000 sorted inserts, thinning). |
| `wav` | `write-<n>s` | The WAV encoder on the export path. |

## How to read the numbers

- `min` is the least-noisy estimate of the true cost — compare `min` across runs.
- `mean` is what a sustained run feels like; `p95` and `max` show scheduler noise.
- `x rt` (engine, fx, wav) is the real-time multiplier: a 15 s song rendered in 1 s is
  15x rt. Above 1x it renders faster than it plays; live playback wants a multiple of
  headroom over 1x, because real sessions add hosted plugins on top.
- The JSON report carries every raw iteration time (µs) plus a `spread` (max/min), so
  percentiles can be re-cut offline.

Comparing runs fairly: same machine, `--release`, on mains power, background apps
closed, same `--seconds` and `--rate`. Never compare a sandbox or CI runner against a
desktop — virtualized CPU time is noisy.

## First findings (commit aef5fc87, shared Linux sandbox, release, 48 kHz, 15 s songs)

1. **The engine's floor is excellent.** A silent song renders at ~14,000x real time
   (925 µs for 15 s of audio): the mixer pass, tick clock and clip scheduling cost
   almost nothing on their own.
2. **Instrument channels are the whole CPU budget.** One tone channel takes the render
   from 14,000x to roughly 4,000x; eight channels land at **1.5x rt**, thirty-two at
   **0.4x rt**, and eight sampler voices (a 750 kB WAV) at **0.6x rt**. Probing the same
   scenes with 1, 2, 8 and 15 bars shows the cost scaling linearly with song length and
   steeply with channel count (1 channel: ~3.4 s for a 15-bar song; 8 channels: ~20 s),
   while a single note renders in ~400 µs. FX are not the cause (the fx-chain scene
   costs the same as plain tones), and the samples are decoded once (open benches are
   45-135 µs). The per-channel voice path — envelope, voicing or per-tick note handling
   — is where the time goes, and it is the first thing worth profiling for real-world
   sessions. Until then, the honest reading is: a handful of channels is fine, big
   racks are not, on this hardware.
3. **Effects span three orders of magnitude**, so slot count matters less than slot
   choice. Cheapest per second of audio: Parametric EQ 2 (21 µs), Balance (47 µs),
   Stereo Enhancer (48 µs), Soft Clipper (68 µs). Heaviest: Vocodex (19.4 ms), Maximus
   (9.8 ms), Reeverb 2 (5.3 ms), Love Philter (3.5 ms), Delay 3 (3.1 ms). A full chain
   of the cheap ones still costs less than one Vocodex.
4. **Transient Processor's default state idles it** (31 ns per second of audio): with
   every parameter at zero the effect sleeps. Its number is a floor, not a cost.
5. **Project I/O is not a concern.** A 16-channel, 16,000-note project saves to FLP in
   ~0.2 ms, loads back in ~0.26 ms, and 2,200 note edits take ~0.23 ms. The WAV writer
   runs at ~4,500x real time.

The sample report below is from the sandbox this was first written on; treat its
absolute numbers as illustrative and re-run on real hardware before comparing.

# Mango Studio benchmark report

| commit | rustc | profile | host | cpu | rate | song length | mode |
|---|---|---|---|---|---|---|---|
| aef5fc87 | unknown | release | linux-x86_64 | Intel(R) Xeon(R) Processor | 48000 Hz | 15 s | full |


## engine

| benchmark | work | iters | min | p50 | mean | p95 | max | throughput |
|---|---|---|---|---|---|---|---|---|
| engine/open-empty | 1 opens | 6 | 3.69µs | 3.92µs | 3.91µs | 4.44µs | 4.44µs | 255k/s |
| engine/render-empty | 720k frames @ 48000 Hz | 6 | 925.25µs | 978.40µs | 1.07ms | 1.61ms | 1.61ms | 671M/s (13972.2x rt) |
| engine/open-tones-8 | 1 opens | 6 | 45.26µs | 47.17µs | 50.67µs | 70.50µs | 70.50µs | 19.73k/s |
| engine/render-tones-8 | 720k frames @ 48000 Hz | 3 | 9.80s | 9.88s | 9.86s | 9.90s | 9.90s | 73.02k/s (1.5x rt) |
| engine/open-tones-32 | 1 opens | 6 | 134.52µs | 136.37µs | 139.17µs | 146.43µs | 146.43µs | 7.19k/s |
| engine/render-tones-32 | 720k frames @ 48000 Hz | 3 | 38.16s | 38.18s | 38.18s | 38.19s | 38.19s | 18.86k/s (0.4x rt) |
| engine/open-fx-chain | 1 opens | 6 | 61.77µs | 65.95µs | 67.07µs | 79.68µs | 79.68µs | 14.91k/s |
| engine/render-fx-chain | 720k frames @ 48000 Hz | 3 | 10.01s | 10.01s | 10.01s | 10.02s | 10.02s | 71.90k/s (1.5x rt) |
| engine/open-automation-8 | 1 opens | 6 | 44.93µs | 45.01µs | 46.00µs | 50.30µs | 50.30µs | 21.74k/s |
| engine/render-automation-8 | 720k frames @ 48000 Hz | 3 | 9.82s | 9.82s | 9.83s | 9.84s | 9.84s | 73.27k/s (1.5x rt) |
| engine/open-sampler-8 | 1 opens | 6 | 83.07µs | 88.08µs | 87.23µs | 95.62µs | 95.62µs | 11.46k/s |
| engine/render-sampler-8 | 720k frames @ 48000 Hz | 3 | 25.66s | 25.67s | 25.68s | 25.72s | 25.72s | 28.03k/s (0.6x rt) |

## fx

| benchmark | work | iters | min | p50 | mean | p95 | max | throughput |
|---|---|---|---|---|---|---|---|---|
| fx/fruity-parametric-eq-2 | 48.00k frames @ 48000 Hz | 6 | 20.75µs | 20.77µs | 20.80µs | 20.92µs | 20.92µs | 2.31G/s (48078.1x rt) |
| fx/fruity-limiter | 48.00k frames @ 48000 Hz | 6 | 61.16µs | 65.96µs | 400.35µs | 1.08ms | 1.08ms | 120M/s (2497.8x rt) |
| fx/fruity-reeverb-2 | 48.00k frames @ 48000 Hz | 6 | 5.31ms | 5.43ms | 5.40ms | 5.51ms | 5.51ms | 8.89M/s (185.1x rt) |
| fx/fruity-delay-3 | 48.00k frames @ 48000 Hz | 6 | 3.06ms | 3.11ms | 3.10ms | 3.15ms | 3.15ms | 15.50M/s (322.8x rt) |
| fx/fruity-soft-clipper | 48.00k frames @ 48000 Hz | 6 | 68.18µs | 68.57µs | 68.55µs | 69.42µs | 69.42µs | 700M/s (14588.6x rt) |
| fx/fruity-fast-dist | 48.00k frames @ 48000 Hz | 6 | 496.60µs | 505.31µs | 509.05µs | 542.65µs | 542.65µs | 94.29M/s (1964.4x rt) |
| fx/fruity-balance | 48.00k frames @ 48000 Hz | 6 | 46.70µs | 46.72µs | 46.73µs | 46.81µs | 46.81µs | 1.03G/s (21400.1x rt) |
| fx/fruity-stereo-enhancer | 48.00k frames @ 48000 Hz | 6 | 48.38µs | 48.43µs | 49.48µs | 53.98µs | 53.98µs | 970M/s (20210.7x rt) |
| fx/fruity-chorus | 48.00k frames @ 48000 Hz | 6 | 755.31µs | 764.42µs | 762.72µs | 766.15µs | 766.15µs | 62.93M/s (1311.1x rt) |
| fx/fruity-flanger | 48.00k frames @ 48000 Hz | 6 | 448.29µs | 457.25µs | 461.09µs | 489.83µs | 489.83µs | 104M/s (2168.8x rt) |
| fx/fruity-phaser | 48.00k frames @ 48000 Hz | 6 | 492.20µs | 493.11µs | 494.16µs | 498.71µs | 498.71µs | 97.13M/s (2023.6x rt) |
| fx/vocodex | 48.00k frames @ 48000 Hz | 6 | 19.36ms | 19.57ms | 19.99ms | 21.88ms | 21.88ms | 2.40M/s (50.0x rt) |
| fx/fruity-stereo-shaper | 48.00k frames @ 48000 Hz | 6 | 232.71µs | 233.02µs | 236.96µs | 256.65µs | 256.65µs | 203M/s (4220.1x rt) |
| fx/fruity-love-philter | 48.00k frames @ 48000 Hz | 6 | 3.53ms | 3.56ms | 3.56ms | 3.58ms | 3.58ms | 13.48M/s (280.9x rt) |
| fx/maximus | 48.00k frames @ 48000 Hz | 6 | 9.75ms | 9.83ms | 9.81ms | 9.84ms | 9.84ms | 4.90M/s (102.0x rt) |
| fx/transient-processor | 48.00k frames @ 48000 Hz | 6 | 31.00ns | 31.00ns | 34.17ns | 41.00ns | 41.00ns | 1405G/s (29268292.7x rt) |
| fx/fruity-filter | 48.00k frames @ 48000 Hz | 6 | 239.82µs | 240.79µs | 242.86µs | 254.11µs | 254.11µs | 198M/s (4117.6x rt) |

## model

| benchmark | work | iters | min | p50 | mean | p95 | max | throughput |
|---|---|---|---|---|---|---|---|---|
| model/flp-save | 447k bytes | 6 | 214.33µs | 222.05µs | 229.73µs | 255.12µs | 255.12µs | 1.95G/s |
| model/flp-load | 447k bytes | 6 | 259.43µs | 269.87µs | 273.77µs | 305.22µs | 305.22µs | 1.63G/s |
| model/note-ops | 2.13k notes | 6 | 234.13µs | 237.38µs | 240.69µs | 254.85µs | 254.85µs | 8.84M/s |

## wav

| benchmark | work | iters | min | p50 | mean | p95 | max | throughput |
|---|---|---|---|---|---|---|---|---|
| wav/write-15s | 720k frames @ 48000 Hz | 6 | 3.30ms | 3.31ms | 3.33ms | 3.41ms | 3.41ms | 216M/s (4509.9x rt) |

