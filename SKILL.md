---
name: mango-studio
description: >-
  Build, test and extend Mango Studio — an FL Studio-style digital audio workstation
  written in Rust (a fork of Lemon Studio) for Windows, Linux and Android. Use when
  working in this repository: it maps the workspace, states the conventions, gives
  recipes for common changes and lists the traps that break builds.
---

# Mango Studio agent guide

## What this is

Mango Studio is an FL Studio-style DAW: channel rack, piano roll, playlist, mixer with
effect slots, a plugin system, and project files compatible with FL Studio's `.flp`.
It is a fork of Lemon Studio (`ItsAlphaNeon/Lemon-Studio`), which itself recreates FL
Studio behavior from measured specs. The design north star is **FL Studio parity**:
when code and behavior disagree, the spec files under `docs/` win — most modules cite
the spec they implement in doc comments (e.g. `spec: audio/timing.md`).

Everything is Rust, edition 2024, GPL-3.0-or-later, one workspace. No GPU requirement:
the UI is a custom software renderer.

## Workspace map

| Crate | Role |
|---|---|
| `crates/os-model` | The project document: `Project`, `Channel`, `Pattern`, `Note`, `Clip`, `MixerTrack`, `Automation`, `Target` (automation destinations). No I/O, no audio. |
| `crates/os-flp` | FL Studio's binary `.flp`: `FlpFile::parse`, `project::load`, `project::create` (write). Byte-exact round trips are tested. |
| `crates/os-engine` | The tick-based audio engine: `Engine` (build from a `Project` + `SampleLibrary`), mixer, built-in slot effects (`effects/`), automation evaluation, offline render (`render_all`, `render_ticks`), WAV writer. |
| `crates/os-dsp` | DSP primitives: filters, curves, the time-stretch bindings (needs libclang at build time). |
| `crates/os-ui` | The custom software UI toolkit: canvas (tiny-skia), text (Windows: GDI ClearType; Linux/macOS: outline engine with bundled Barlow faces), knobs, menus. |
| `crates/os-theme` | Color palettes. |
| `crates/os-app` | The application binary (`mangostudio`): views (`src/views/`), transport, device I/O (WASAPI/ALSA via the vendored cpal), plugin hosting (`os-host`, `os-clap`, `os-vst3`, `os-vst2` wrappers), export, history/undo. |
| `crates/os-paths` | User/system folders per platform. |
| `crates/os-smf` | Standard MIDI file read/write. |
| `crates/os-android` | Android activity wrapper (the app builds as a lib for it). |
| `plugins/*` | ~55 instrument and effect plugins (nih-plug crates: CLAP + VST3). Built-ins named `lemon-*` (Lemon Studio recreations) and `mango-*` (Mango additions). |
| `tools/*` | Utilities: `os-render` (FLP → WAV headless), `os-play`, `flp-dump`, `bundle-app`, `bundle-plugins`, `notices`, `watermark`. |
| `crates/os-bench` | The benchmark suite; see `docs/benchmarks.md` before trusting or quoting numbers. |

The version lives in the root `Cargo.toml` under `[workspace.package]`; crates use
`version.workspace = true`. GitHub releases are tagged `vX.Y.Z` and built by CI.

## Commands

```sh
cargo build --workspace --locked          # what CI does
cargo test  --workspace --locked          # CI runs this on Windows, Linux and macOS
cargo run -p os-app --release             # the app
cargo run -p os-render -- <project.flp> out.wav   # headless render
cargo run -p os-bench --release           # benchmarks (docs/benchmarks.md)
```

- Always pass `--locked`; `Cargo.lock` is committed. Adding a workspace member or a
  dependency updates the lock — commit that update in the same change.
- `cpal` is patched from `vendor/cpal` (`[patch.crates-io]` in the root `Cargo.toml`):
  an Android latency change. Do not remove the patch.
- Linux build needs `libasound2-dev libwayland-dev libclang-dev pkg-config` (libclang
  generates the time-stretcher's bindings; this is exactly what CI installs).
- Android is `cargo check --workspace --target aarch64-linux-android` in CI — check
  only. Keep it that way: no new system dependencies on any platform.

## Architecture in one pass

**Model → engine → app.** `os-model` is the document. `os-engine` turns a document +
sample library into audio, tick by tick (a tick is one beat slice: `rate·60/(tempo·ppq)`
frames). `os-app` owns devices, hosting and UI, and edits the model.

**Engine.** `Engine::new(&project, &mut library, rate, PlayMode::Song | Pattern(u16))`
builds all channel/slot state up front. `render_ticks(n, &mut out)` appends interleaved
stereo f32; `render_all` renders to the first quiet tick after the song's end (the
export path). Hosted instruments/effects sit behind `Arc<Mutex<…>>` (`SharedInstrument`,
`SharedEffect`); the audio thread only ever `try_lock`s — a busy plugin passes silence
rather than blocking, and a tick waits at most `BUSY_WAIT` (2 ms) in all. Slot effects
without hosting are plain `Effect` objects built by `effects::from_slot(name, state,
rate)` from the project's saved plugin bytes. Automation runs per tick
(`engine/param_auto.rs`), sampled every 64 frames, delivered to instruments/effects by
`Target` id (`os_model::target`).

**Plugins.** Each `plugins/*` crate is a nih-plug plugin. The app compiles them with
the `linked` feature (no exported entry points — they would clash when linked
together) and lists them in `crates/os-app/src/plugins.rs::builtin()`. A plugin has a
params struct, a DSP core (e.g. `Citrus` with `render(left, right)`), and a view.

**Views.** Every window is a module in `crates/os-app/src/views/` exposing
`pub fn paint(ui: &mut Ui, s: &State, r: Rect, f: &FrameInfo)`. There is no registry:
`views/mod.rs::paint` lays out the screen and dispatches to each view by window kind.
Mouse/keyboard handling lives in the same view files; global actions dispatch through
`Action`s handled in `app.rs`.

**Undo.** `os-app/src/history.rs`: one list of named steps; an edit records the state
it is about to change and undo/redo swaps before/after.

## Conventions (CI and reviewers enforce these)

- Tests live in the same file, in a `#[cfg(test)] mod tests` at the bottom. Names are
  lowercase behavior sentences: `arrows_walk_the_rows_and_hear_the_samples`,
  `states_round_trip_and_a_later_version_is_not_read`. Each test gets a doc comment
  citing the spec it checks.
- Comments and identifiers in English; comments explain *why* and cite specs
  (`spec: audio/mixer.md "..."`).
- Header on every file:
  `// Copyright (C) 2026 ItsAlphaNeon` + `// SPDX-License-Identifier: GPL-3.0-or-later`
  + the Lemon Studio URL line.
- Keep the warning count at zero for code you touch.
- Behavior changes need a test; spec-visible changes may need a spec update under
  `docs/` in the same PR.

## Recipes

**Add a built-in effect (mixer slot).** Create `crates/os-engine/src/effects/<name>.rs`
with `struct X { … }`, `fn from_state(state: &[u8], sample_rate: u32) -> Option<Self>`
(parsing the project's saved bytes) and `impl Effect for X` (`process(&mut self, buf:
&mut [f32])`, interleaved stereo in place; optionally `latency`, `reset`,
`set_tempo`). Register the name in `effects/mod.rs::from_slot`. Add round-trip +
rendering tests like the neighbors have.

**Add a plugin channel instrument.** New `plugins/<name>` nih-plug crate, added to the
workspace members and to `crates/os-app/Cargo.toml` with `features = ["linked"]`, then
to `plugins.rs::builtin()`. Params + DSP + view; the host attaches it with
`Engine::set_instrument`.

**Add an automation target.** Extend `Target` in `crates/os-model/src/lib.rs`
(encode in the id layout there, decode in `Target::decode`), then consume it in
`crates/os-engine/src/engine/param_auto.rs::run_param_automation`. Note:
`SlotMix`/`SlotOn` exist as FLP init controls but are not engine automation targets
yet — check `automation_target()` in `app.rs` before promising UI automation.

**Add a view change.** Find the window in `crates/os-app/src/views/`, edit `paint` and
its input handlers, keep layout logic in the view (not `app.rs`), test tree/menu
shapes in the view's tests.

**Add a benchmark.** A scenario builder in `crates/os-bench/src/scenarios.rs`
(deterministic, `Project` in code), a `Def` in `suites.rs` (setup outside the timed
closure, one iteration = one `f()` call), name it `group/name`. Update
`docs/benchmarks.md` if it changes conclusions.

**Render headless.** `tools/os-render`: `FlpFile::parse` → `project::load` →
`SampleLibrary::new(None)` → `Engine::new` → `follow_markers` → `render_all` →
`wav::write_f32_stereo`. Any headless audio check should follow that shape.

## Pitfalls

- **Forgetting `linked`:** building plugin crates without it exports CLAP/VST3 entry
  points that collide at link time inside the app.
- **Blocking the audio thread:** never lock, allocate heavily or wait in a `process`
  path. `try_lock` + pass-through, always.
- **Fonts are Latin-only** (Barlow on Linux/macOS, Cuprum on Windows): CJK/Cyrillic
  renders as boxes and non-UTF-8 names become U+FFFD. No font fallback chain yet.
- **Paths with spaces break mp3lame builds** (LAME's libtool): keep build directories
  space-free or point `CARGO_TARGET_DIR` elsewhere.
- **Android:** no new system deps; the NDK job is check-only.
- **Don't trust single benchmark runs**; read `docs/benchmarks.md` for methodology and
  current findings before quoting numbers.
- **Specs over intuition:** an FL Studio behavior question is answered by `docs/` and
  the manual, not by what seems reasonable.

## Release process

Tag `vX.Y.Z` on GitHub. `release.yml` builds Windows x64 (zip), Linux x64 (zip) and
Android arm64 (APK, signed the same as previous releases so it updates in place).
Release notes are written in plain words, grouped by area ("Sound and instruments",
"Playing and editing", "Fixes"), each bullet naming what landed and the idea behind
it, with issue numbers.
