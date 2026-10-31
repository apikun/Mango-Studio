# Mango Wavetable Synthesizer ("Serum V2") — Architecture & Technical Specification

**Target Crate**: `plugins/lemon-wavetable`  
**Built-in CLAP ID**: `org.lemonstudio.lemon-wavetable`  
**VST3 Class ID**: `LemonStudioWave01` (16-byte UUID)  
**Category**: Instrument / Synthesizer  
**Status**: Specification Phase (Phase 1 Baseline)

---

## 1. Executive Summary & Design Philosophy

`Mango Wavetable` is a next-generation polyphonic wavetable synthesizer for Mango Studio, engineered as an open, modern successor to industry standards like Serum and Vital.

### Guiding Principles:
1. **Pristine Anti-Aliasing with Bounded CPU**: Mipmapped octave wavetables precomputed at table-load via FFT; real-time oscillator playback is $O(1)$ sample-lookup with zero runtime FFT overhead and zero audible aliasing across the entire 20 Hz – 20 kHz spectrum.
2. **Standard Serum Format Compatibility**: Direct import of 2,048-sample/frame single-cycle and multi-frame wavetables (`.wav`, 1 to 256 frames, float32/int24).
3. **Deep Modulation Architecture**: 4 Envelopes with tension curves, 8 vector-drawable breakpoint LFOs, and a 64-slot unipolar/bipolar modulation matrix.
4. **Zero Allocation in the Audio Thread**: All voice slots, phase accumulators, mipmap arrays, and modulation matrices are statically bounded or pre-allocated. No heap allocations, no locks, and no blocking I/O during DSP processing.
5. **Native Mango Studio Canvas GUI**: High-frame-rate 2D/3D animated wavetable rendering, draggable LFO nodes, and real-time visual feedback conforming to Mango Studio's softbuffer/UI kit (`Style::LEMON` and `Style::CITRUS`).

---

## 2. Audio Engine Architecture

```
                 +-------------------+
                 |    Wavetable A    |------+
                 | (Warp, Pos, Unison|      |
                 +-------------------+      |
                                            |
                 +-------------------+      v
                 |    Wavetable B    |--->[Filter 1 / 2]---+
                 | (Warp, Pos, Unison|    (Series/Parallel)|
                 +-------------------+      ^              |
                                            |              |
                 +-------------------+      |              |
                 |     Sub Osc       |------+              v
                 +-------------------+      |          [Master Amp]
                                            |              |
                 +-------------------+      |              v
                 |     Noise Osc     |------+       [FX Rack (10 slots)]
                 +-------------------+              (Distortion, OTT,
                                                     Chorus, Delay, Rev)
                                                           |
                                                           v
                                                     [Stereo Out]
```

### 2.1 Oscillators A and B

Each primary oscillator operates with independent phase, frequency, and wavetable memory.

- **Wavetable Representation**:
  - Cycle length: $N = 2,048$ samples per frame.
  - Multi-frame capacity: 1 to 256 frames per table.
  - Frame morphing: Linear crossfade and cubic Hermite position interpolation between consecutive frames.
- **Anti-Aliasing Strategy (Mipmapped Octave Tables)**:
  - For each wavetable frame, 11 mipmap levels are precomputed at load time using FFT bandlimiting:
    - Mipmap 0: Nyquist $\le 20,000\text{ Hz}$ (all harmonics up to 1024).
    - Mipmap 1: Nyquist $\le 10,000\text{ Hz}$ (up to 512 harmonics).
    - ...
    - Mipmap 10: Fundamentals only.
  - At runtime, the voice pitch chooses the appropriate mipmap level $L$ based on $\frac{F_{\text{voice}}}{F_s}$, selecting the highest harmonic band that does not exceed Nyquist.
  - Run-time cost: Identical to a single table lookup with fractional linear interpolation. Zero runtime FFT computation.
- **Warp Engine**:
  - **Sync**: Resets oscillator phase accumulator early at a multiple $k \in [1.0, 16.0]$, generating classic hard-sync formant screaming.
  - **Bend (+/-)**: Non-linear phase warp concentrating or spreading phase towards the cycle center ($f(\phi) = \phi^p$).
  - **PWM**: Phase stretching for pulse-width modulation on any arbitrary wave shape.
  - **Asym (+/-)**: Linear phase skewing left or right.
  - **Flip / Mirror**: Inverts or mirrors the second half of the 2,048-sample cycle.
  - **FM (Frequency Modulation)**: Linear through-zero phase modulation from Oscillator B to Oscillator A, Sub to A, or Noise to A.
  - **AM / RM**: Amplitude and Ring Modulation between oscillators.
- **Unison Engine**:
  - Voices per oscillator: 1 to 16 voices.
  - **Detune Depth**: Microtonal pitch divergence using exponential scaling ($2^{\pm \Delta/1200}$).
  - **Detune Blend**: Volume ratio between the centered fundamental voice and outer detuned voice pairs.
  - **Stereo Spread**: Pan divergence panning voice pairs evenly across the stereo field ($[-1.0, +1.0]$).
  - **Phase Randomization**: Initial start phase randomized between 0% (exact retrigger for punchy attack transients) and 100% (free-phase organic thickness).
  - **Stack Modes**: Unison pitch offsets (Linear, 12-Semitone/Octave, 5th, Power Chord).

### 2.2 Sub Oscillator
- Shapes: Pure Sine, Triangle, Sawtooth, Square, Pulse 25%, Noise.
- Octave transpose: $-2, -1, 0, +1, +2$ octaves relative to note.
- Level and Pan controls.
- **Direct Out toggle**: Allows bypassing the main filter and distortion stages to guarantee a pure, uncompromised sub-bass low end.

### 2.3 Noise Oscillator
- Sample-based texture engine loading one-shot or looping transient samples (attacks, vinyl crackle, analog hiss, breath).
- Pitch tracking toggle (Key Track).
- Tune (semitones/cents) and Phase lock.
- Stereo width and volume controls.

### 2.4 Dual Multi-Mode Filters (Filter 1 & Filter 2)
- Independent routing matrix:
  - Osc A $\rightarrow$ [Off / Flt 1 / Flt 2 / Both]
  - Osc B $\rightarrow$ [Off / Flt 1 / Flt 2 / Both]
  - Sub $\rightarrow$ [Off / Flt 1 / Flt 2 / Both]
  - Noise $\rightarrow$ [Off / Flt 1 / Flt 2 / Both]
  - Serial vs. Parallel routing switch between Filter 1 and Filter 2.
- Filter Topologies:
  1. **12 dB & 24 dB State-Variable Filters (SVF)**: Clean digital Lowpass, Highpass, Bandpass, and Peak with zero-delay feedback (ZDF).
  2. **Analog Saturated Ladder**: Transistor-modelled ladder lowpass with nonlinear soft clipping in the resonance feedback loop.
  3. **Comb Filter**: Tuned delay line with positive and negative feedback for metallic and physical modeling timbres.
  4. **Formant / Vowel Filter**: Multi-peak vocal cavity simulation morphing between vowels (A, E, I, O, U).
  5. **Phaser / Flanger Filter**: Multi-notch allpass cascade with dynamic sweeping.
- Filter Parameters: Cutoff (20 Hz – 20 kHz), Resonance (0.0 – 1.0, self-oscillating on Ladder), Drive (0 to +24 dB with warm saturation), Key Tracking (0% to 100%), and Stereo Spread (offsets cutoff between Left and Right channels).

---

## 3. Modulation Architecture

```
   [4x Multi-Stage Envelopes] --------+
                                      |
   [8x Vector Breakpoint LFOs] -------+---> [64-Slot Mod Matrix] ---> [All DSP Parameters]
                                      |     (Unipolar / Bipolar)      (Cutoff, Warp, WT Pos,
   [MIDI / Expressive Sources] -------+                                Pitch, Pan, FX Mix)
   (Velocity, Key, Wheel, Aftertouch)
```

### 3.1 Envelopes (Env 1 to Env 4)
- **Envelope 1**: Hardwired to Master Voice Amplitude (Amp Env) with safety release guarantees.
- **Envelopes 2, 3, 4**: Freely assignable modulation envelopes.
- Stages:
  - **Delay**: 0 to 10,000 ms pre-trigger silence.
  - **Attack**: 0.1 ms to 20,000 ms with adjustable curvature ($\tau \in [-1.0, +1.0]$: exponential, linear, logarithmic).
  - **Hold**: 0 to 10,000 ms peak sustain.
  - **Decay**: 1 ms to 20,000 ms with adjustable curvature.
  - **Sustain Level**: 0.0 to 1.0 ($- \infty$ to 0 dB).
  - **Release**: 1 ms to 30,000 ms with adjustable curvature.

### 3.2 Vector Breakpoint LFOs (LFO 1 to LFO 8)
- **Node-Based Curve Editor**:
  - Arbitrary number of points (up to 64 points per LFO).
  - Each segment between points has a tension control point (curving linearly, exponentially, or in an S-curve).
  - Grid snapping: 1/4, 1/8, 1/16, 1/32, 1/64, triplets.
- **Modes**:
  - **Trigger**: Restarts from node 0 on every new note-on.
  - **Envelope (One-Shot)**: Plays the curve from start to end once, holding the final node level like an envelope.
  - **Off (Free-Run)**: Continuous cyclical phase running across all voices.
- **Rate Synchronization**:
  - Tempo Synced: $1/128$ to 32 bars (including dotted and triplet divisions).
  - Free Hertz: $0.01\text{ Hz}$ to $200.0\text{ Hz}$ (capable of audio-rate AM modulation).

### 3.3 Modulation Matrix
- **64 Modulation Routing Slots**:
  - Source: Env 1–4, LFO 1–8, Velocity, KeyTrack, Mod Wheel, Pitch Bend, Aftertouch, Chaos/Random.
  - Destination: Any continuous parameter in the synthesizer (WT Pos A/B, Warp A/B, Filter Cutoff, Filter Drive, Unison Detune, Pitch, Panning, FX parameters).
  - Amount: Depth scaling with toggleable **Unipolar** ($[0, +1]$) or **Bipolar** ($[-1, +1]$) mode.
  - Curve: Remapping transfer function shaping the modulation response.
  - Aux / Secondary Source: Allows a secondary input (e.g. Mod Wheel or Macro knob) to scale the depth of the primary modulation source.

---

## 4. Built-in FX Rack (10 Re-Orderable Modules)

The output of the voice summing bus feeds a serial FX chain with user-reorderable slots:

1. **Hyper / Dimension**: Micro-delay chorus (Hyper) combined with Haas-effect psychoacoustic spatial widener (Dimension).
2. **Distortion**: Multi-algorithm saturation engine (Soft Clip, Hard Clip, Diode, Tube Warmth, Foldback/Wavefolder, Downsampler, Bitcrusher).
3. **Flanger**: Short modulated delay with feedback and phase inversion.
4. **Phaser**: 4 to 12-stage allpass filter cascade with stereo spread.
5. **Chorus**: Multi-voice lush BBD delay emulation with rate and depth controls.
6. **Delay**: Stereo, Ping-Pong, and Cross delay with tempo sync, high/low cut filters, and ducking.
7. **Multiband Compressor (OTT Mode)**: 3-band upward and downward dynamics processor directly reusing the battle-tested `os-dsp` Dariacore multiband compression algorithms.
8. **Reverb**: Studio algorithmic reverb with High/Low Damping, Pre-delay, Room Size, Decay Time, and Stereo Width.
9. **Parametric EQ**: 4-band equalizer (Low Shelf, 2x Peaking/Bell, High Shelf) with real-time interactive spectrum display.
10. **Post-Filter**: Secondary master filter stage for final sound sculpting.

---

## 5. Voice Lifecycle & Thread Safety

- **Voice Allocation**:
  - 32-voice polyphony limit per instance.
  - Voice Stealing: Oldest note or lowest amplitude release tail stolen when exceeding 32 voices.
  - Legato & Mono modes with glide/portamento time (0 to 5,000 ms) and configurable glide curves.
- **Thread Safety & Real-Time Invariants**:
  - Real-time audio rendering is completely non-blocking:
    - Zero `std::sync::Mutex` contention in the audio thread.
    - Zero heap allocation (`Vec::push`, `Box::new`, `String` formatting) inside `process()`.
    - Double-buffered or lock-free atomic parameter exchange for live GUI edits.
    - Wavetables are stored in shared immutable `Arc<WavetableData>` structures.

---

## 6. Graphical User Interface (Lemon Canvas)

The UI will be built natively on Mango Studio's softbuffer `Canvas` architecture:

```
+-----------------------------------------------------------------------------------+
|  [OSC A]  [OSC B]  [SUB]  [NOISE]  [FILTER]  |  [FX]  [MATRIX]  [GLOBAL]          |
+-----------------------------------------------------------------------------------+
|  +---------------------------+   +---------------------------+   +---------------+ |
|  |   OSC A (3D/2D Visual)    |   |   OSC B (3D/2D Visual)    |   |  FILTER 1     | |
|  | [~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~]   |   | [~ ~ ~ ~ ~ ~ ~ ~ ~ ~ ~]   |   | Cutoff: 1.2k  | |
|  | Pos: 42   Warp: Sync 2.4x |   | Pos: 12   Warp: FM(A) 30% |   | Res: 45%      | |
|  | Unison: 7 Detune: 0.14    |   | Unison: 1 Detune: 0.00    |   | Drive: +3dB   | |
|  +---------------------------+   +---------------------------+   +---------------+ |
+-----------------------------------------------------------------------------------+
|  [ENV 1] [ENV 2] [ENV 3] [ENV 4]  |  [LFO 1] [LFO 2] [LFO 3] [LFO 4]               |
|  +------------------------------+  +--------------------------------------------+ |
|  | Interactive ADSR Graph       |  | Interactive Multi-Point LFO Graph          | |
|  | A: 12ms D: 450ms S: 0.7 R:1s|  | Rate: 1/4 Sync  Mode: Trigger Nodes: 6     | |
|  +------------------------------+  +--------------------------------------------+ |
+-----------------------------------------------------------------------------------+
```

- **Top Navigation Bar**: Quick tab switching between Main Oscillators, FX Rack, Modulation Matrix, and Global Preferences.
- **Interactive 2D/3D Wavetable View**:
  - 2D mode: High-definition rendering of the active single-cycle waveform with modulated playhead scrubbing.
  - 3D mode: Isometric wireframe slice stack showing the entire wavetable progression with highlighted active slice.
- **Interactive Node Editors**:
  - Draggable breakpoint nodes with real-time curvature handles for envelopes and LFOs.
  - Hover highlights, keyboard modifiers (Shift for fine-tuning, Alt for curve reset).
- **Theme Awareness**:
  - Fully integrated with `os-theme`.
  - Automatically draws in **Lemon** (sleek flat neutral greys with lemon-yellow accent rings) or **Citrus** (classic 3D look), respecting whichever user theme palette is active.

---

## 7. Storage, Compatibility & File Formats

1. **Serum `.wav` Import**:
   - Parses RIFF WAV chunks (`fmt `, `data`, `clm `).
   - Validates $N = 2,048$ samples per frame.
   - Extracts frame counts automatically ($\text{frames} = \frac{\text{total samples}}{2048}$).
2. **Project Persistence (`.flp` / Plugin Wrapper)**:
   - Plugin state serialized as compact binary chunks in CLAP format (`builtin/lemon-wavetable.clap`).
   - All parameters, wavetable embedded binary blobs (for user-imported tables), LFO node vectors, and matrix routes are versioned and stored losslessly.
3. **Bundling**:
   - Integrated directly into the main Mango Studio release binary as an instant built-in channel instrument.
   - Also packaged as standalone `.clap` and `.vst3` bundles via `bundle-plugins`.

---

## 8. Implementation Milestones

- **Milestone 1 (Audio Core)**: 
  - Wavetable data structures, mipmap generator, and bandlimited single-voice oscillator playback.
  - Serum `.wav` file parser and default factory tables (Basic Shapes, Analog Saw, Modern Square).
- **Milestone 2 (Unison & Warp)**:
  - 16-voice unison engine with detune, blend, and stereo spread.
  - Sync, Bend, and FM warp modes.
- **Milestone 3 (Filters & Modulation)**:
  - ZDF State-Variable Filter (LP/HP/BP) with saturation.
  - 4 Envelopes + 8 LFO generators with tick-rate modulation evaluation.
  - Modulation matrix router.
- **Milestone 4 (GUI & Sound Canvas)**:
  - Native softbuffer vector UI: 2D/3D table visualizer, draggable LFO graph, and interactive ADSR sliders.
- **Milestone 5 (FX Rack & Integration)**:
  - OTT multiband dynamics, distortion, delay, reverb.
  - Full workspace integration into Channel Rack `+` and Project save/reload tests.
