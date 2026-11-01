// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Lemon Studio's small generators as VST3 / CLAP plugins (nih-plug): Lemon Kick, Lemon Boom, Lemon Bass, Lemon
//! Acid, Mango Pluck, Mango Mini, Mango DX, Mango Autogun, Mango Drums, Mango Keys, Mango Slayer and Mango Dashboard. Each is a
//! handful of settings over a small synthesizer, so they share everything but their description: the plugin, its
//! parameters and saved state ([`params`]), the Pith editor ([`editor`], [`view`]). A description ([`GenSpec`])
//! gives the settings, their value texts, the editor's cards and controls, and the sound. The plugin modules
//! (`plugins/lemon-kick` and the others) export one each, the way the small effects do. Mango Surface is here too:
//! eight faders that sound nothing, a performance links them to anything. And Mango Flex: two hundred fifty-six
//! named presets with four macros over each, and the echo and space of the preset behind them.
//!
// spec: plugins/lemon-small-generators.md, plugins/plugin-formats.md
#![allow(semicolon_in_expressions_from_non_local_macros)]

pub mod editor;
pub mod params;
pub mod view;

pub mod acid;
pub mod monk;
pub mod surface;
pub mod flex;
pub mod boobass;
pub mod fruit_kick;
pub mod lemon_boom;
pub mod autogun;
pub mod drums;
pub mod dx;
pub mod keys;
pub mod mini;
pub mod pluck;
pub mod slayer;
pub mod dashboard;

use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use nih_plug::prelude::*;
use os_ui::{Canvas, Color, Rect};

use params::GenParams;

/// The most settings a generator here has.
pub const MAX: usize = 16;

/// One setting: the plugin's saved state holds it as int `k` (its place in [`GenSpec::settings`], unless
/// [`GenSpec::fields`] maps it).
pub struct Setting {
    /// The parameter's id (host automation and saved sessions).
    pub id: &'static str,
    /// The name hosts and the hint show.
    pub name: &'static str,
    /// The stored range, also the knob's.
    pub range: (i32, i32),
    /// A new instance's value.
    pub start: i32,
    /// Where a drag lands when it comes near and a wheel step stops (the centre of a bipolar knob, unity gain).
    pub magnet: Option<i32>,
}

/// A control of the editor.
pub enum Control {
    /// A Pith dial of radius `r` centred at (`cx`, `cy`), its caption under it (the value text, in lemon, when the
    /// caption is empty).
    Knob {
        id: &'static str,
        cx: f32,
        cy: f32,
        r: f32,
        caption: &'static str,
        bipolar: bool,
    },
    /// A radio list: one row per value from the range's minimum, 16 px apart from `top`, a toggle at `x` and the name
    /// beside it.
    Choice {
        id: &'static str,
        x: f32,
        top: f32,
        names: &'static [&'static str],
    },
    /// A vertical fader over `top..bot` at `x`, its fill in `col` from the bottom (from the middle when `bipolar`),
    /// `label` over it and ticks at `marks` (normalised).
    Fader {
        id: &'static str,
        x: f32,
        top: f32,
        bot: f32,
        label: &'static str,
        col: Color,
        bipolar: bool,
        marks: &'static [f32],
    },
    /// A button centred at (`cx`, `cy`) that shows the value's envelope shape; a click steps to the next value.
    Shape { id: &'static str, cx: f32, cy: f32 },
    /// A tile: its own card over `x0..x1`, `y0..y1`, a small dial at the left, the caption in a field at the top right
    /// and the value text under it.
    Tile {
        id: &'static str,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        caption: &'static str,
    },
}

impl Control {
    pub fn id(&self) -> &'static str {
        match self {
            Control::Knob { id, .. }
            | Control::Choice { id, .. }
            | Control::Fader { id, .. }
            | Control::Shape { id, .. }
            | Control::Tile { id, .. } => id,
        }
    }
}

/// Where the wordmark goes: the lemon mark and LEMON beside the name on one line centred at `y`, or (`stacked`) LEMON
/// over the name, its top at `y`. `anchor` is the share of its width left of `x` (1 = right edge on `x`).
pub struct Mark {
    pub x: f32,
    pub y: f32,
    pub anchor: f32,
    pub stacked: bool,
}

/// An outlined card under a ruled heading (no heading when it is empty).
pub struct Card {
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
    pub heading: &'static str,
}

/// A well that pictures the settings.
pub struct Display {
    pub r: Rect,
    pub draw: fn(&mut Canvas, Rect, &[i32]),
}

/// The sound of a generator, made for a sample rate from the settings in stored order. It plays the notes the host
/// sends: a note that starts cuts the one it replaces when the instrument is monophonic, and one that releases ends
/// when its tail is over (`take_ended` tells the host, spec: audio/voices.md §4).
pub trait Instrument: Send {
    /// New settings: they act from this block on.
    fn set(&mut self, v: &[i32]);
    fn set_sample_rate(&mut self, rate: u32);
    /// Forgets what the sound holds, as at the start of a song.
    fn reset(&mut self);
    /// A note starts (`voice_id` is the host's, echoed on its end; `None` for a preview).
    fn note_on(&mut self, key: u8, velocity: f32, voice_id: Option<i32>);
    /// A note's key is up.
    fn note_off(&mut self, key: u8, voice_id: Option<i32>);
    /// A note stops at once (the Choke of the host, a cut group).
    fn choke(&mut self, key: u8);
    /// Every note stops (CC 120 / 123, Stop).
    fn all_off(&mut self);
    /// A sounding voice's tuning moves by `cents` (portamento and slides; ignored when the instrument does not
    /// follow them).
    fn tune(&mut self, _key: u8, _voice_id: Option<i32>, _cents: f64) {}
    /// Fills the next `left` and `right` samples.
    fn render(&mut self, left: &mut [f32], right: &mut [f32]);
    /// The voices that ended since last called: (key, voice id), for the host's VoiceTerminated.
    fn take_ended(&mut self) -> Vec<(u8, Option<i32>)>;
    /// Whether notes are sounding or in their release.
    fn has_voices(&self) -> bool;
}

/// Everything that makes one of the generators.
pub struct GenSpec {
    /// "Lemon Kick": the plugin's name.
    pub name: &'static str,
    /// The wordmark's word: "KICK".
    pub word: &'static str,
    /// The About menu's line: "Lemon Studio's kick".
    pub about: &'static str,
    /// The settings in stored order.
    pub settings: &'static [Setting],
    /// The version the saved state starts with (an int before the settings), if it has one. A stored state with a
    /// higher version is not read (spec §5), unless `any_version`.
    pub version: Option<i32>,
    /// Every version of the saved state is read the same way.
    pub any_version: bool,
    /// The int (after the version) each setting is, when that is not its place: an int left out is kept as stored.
    /// Empty: setting k is int k.
    pub fields: &'static [usize],
    /// The bytes a new instance's saved state has after the settings (kept as stored otherwise).
    pub tail: &'static [u8],
    /// Editor size in logical pixels.
    pub size: (u32, u32),
    pub cards: &'static [Card],
    pub controls: &'static [Control],
    pub display: Option<Display>,
    pub mark: Mark,
    /// The hint well (`None`: the hint goes in the display's top-left corner); on the wordmark's line it stops 16 px
    /// short of it.
    pub hint: Option<Rect>,
    /// Captions that belong to no control: (centre x, top, text).
    pub labels: &'static [(f32, f32, &'static str)],
    /// A setting's value text.
    pub text: fn(usize, i32) -> String,
    /// A typed value for a setting.
    pub parse: fn(usize, &str) -> Option<i32>,
    /// The instrument, made for a sample rate from the starting settings.
    pub make: fn(u32, &[i32]) -> Box<dyn Instrument>,
}

impl GenSpec {
    /// Setting index of an id.
    pub fn index(&self, id: &str) -> Option<usize> {
        self.settings.iter().position(|s| s.id == id)
    }

    /// The settings a new instance starts on.
    pub fn start(&self) -> Vec<i32> {
        self.settings.iter().map(|s| s.start).collect()
    }
}

/// What makes a plugin type of one of the generators: its description and its ids.
pub trait Kind: 'static + Send + Sync {
    const SPEC: &'static GenSpec;
    const CLAP_ID: &'static str;
    const CLAP_DESCRIPTION: &'static str;
    const CLAP_FEATURES: &'static [ClapFeature];
    const VST3_CLASS_ID: [u8; 16];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory];
}

/// What the audio thread and the editor share.
pub struct Shared {
    pub rate: AtomicU32,
    /// Set while an editor is open.
    pub editor_open: AtomicBool,
}

impl Default for Shared {
    fn default() -> Self {
        Shared {
            rate: AtomicU32::new(44100),
            editor_open: AtomicBool::new(false),
        }
    }
}

/// One of the generators as a plugin.
pub struct SmallGen<K: Kind> {
    params: Arc<GenParams>,
    shared: Arc<Shared>,
    /// Made in `initialize` (for the sample rate).
    dsp: Option<Box<dyn Instrument>>,
    raw: Vec<u8>,
    raw_seen: u64,
    kind: PhantomData<K>,
}

impl<K: Kind> Default for SmallGen<K> {
    fn default() -> Self {
        SmallGen {
            params: Arc::new(GenParams::new(K::SPEC)),
            shared: Arc::new(Shared::default()),
            dsp: None,
            raw: params::default_state(K::SPEC),
            raw_seen: u64::MAX,
            kind: PhantomData,
        }
    }
}

impl<K: Kind> SmallGen<K> {
    /// The saved state's copy (never blocks: keeps the last one while the editor writes).
    fn read_raw(&mut self) {
        let v = self.params.raw_version.load(Ordering::Acquire);
        if v == self.raw_seen {
            return;
        }
        if let Ok(b) = self.params.raw.try_read() {
            self.raw.clone_from(&b);
            self.raw_seen = v;
        }
    }

    /// The settings the sound plays (the first `n` values; no allocation).
    fn settings(&mut self) -> [i32; MAX] {
        self.read_raw();
        self.params.settings(&self.raw)
    }
}

impl<K: Kind> Plugin for SmallGen<K> {
    const NAME: &'static str = K::SPEC.name;
    const VENDOR: &'static str = "Mango Studio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(editor::GenEditor::new(
            K::SPEC,
            self.params.clone(),
            self.shared.clone(),
        )))
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        config: &BufferConfig,
        _ctx: &mut impl InitContext<Self>,
    ) -> bool {
        let rate = config.sample_rate.round() as u32;
        self.shared.rate.store(rate, Ordering::Relaxed);
        self.raw_seen = u64::MAX;
        let s = self.settings();
        let v = &s[..K::SPEC.settings.len()];
        match &mut self.dsp {
            Some(d) => {
                d.set_sample_rate(rate);
                d.set(v);
            }
            None => self.dsp = Some((K::SPEC.make)(rate, v)),
        }
        true
    }

    fn reset(&mut self) {
        if let Some(d) = &mut self.dsp {
            d.reset();
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        ctx: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let s = self.settings();
        let Some(dsp) = &mut self.dsp else {
            return ProcessStatus::Normal;
        };
        dsp.set(&s[..K::SPEC.settings.len()]);
        let len = buffer.samples();
        let out = buffer.as_slice();
        for ch in out.iter_mut() {
            ch.fill(0.0);
        }
        if out.len() < 2 {
            return ProcessStatus::Normal;
        }
        let mut at = 0;
        while let Some(ev) = ctx.next_event() {
            let t = (ev.timing() as usize).min(len);
            if t > at {
                let (left, right) = out.split_at_mut(1);
                dsp.render(&mut left[0][at..t], &mut right[0][at..t]);
                at = t;
            }
            match ev {
                NoteEvent::NoteOn {
                    note,
                    velocity,
                    voice_id,
                    ..
                } => dsp.note_on(note.clamp(0, 127) as u8, velocity.clamp(0.0, 1.0), voice_id),
                NoteEvent::NoteOff { note, voice_id, .. } => {
                    dsp.note_off(note.clamp(0, 127) as u8, voice_id)
                }
                NoteEvent::Choke { note, .. } => dsp.choke(note.clamp(0, 127) as u8),
                // Portamento and slide notes (spec: audio/channel-voices.md): the host's tuning of a key's voices.
                NoteEvent::PolyTuning {
                    note,
                    voice_id,
                    tuning,
                    ..
                } => dsp.tune(note.clamp(0, 127) as u8, voice_id, f64::from(tuning)),
                NoteEvent::MidiCC { cc: 120 | 123, .. } => dsp.all_off(),
                _ => {}
            }
        }
        if at < len {
            let (left, right) = out.split_at_mut(1);
            dsp.render(&mut left[0][at..len], &mut right[0][at..len]);
        }
        // Voice ends go back to the host, at the end of the block (spec: audio/voices.md §4).
        let last = (len as u32).saturating_sub(1);
        for (note, voice_id) in dsp.take_ended() {
            ctx.send_event(NoteEvent::VoiceTerminated {
                timing: last,
                voice_id,
                channel: 0,
                note,
            });
        }
        if dsp.has_voices() {
            ProcessStatus::KeepAlive
        } else {
            ProcessStatus::Normal
        }
    }
}

impl<K: Kind> ClapPlugin for SmallGen<K> {
    const CLAP_ID: &'static str = K::CLAP_ID;
    const CLAP_DESCRIPTION: Option<&'static str> = Some(K::CLAP_DESCRIPTION);
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = K::CLAP_FEATURES;
}

impl<K: Kind> Vst3Plugin for SmallGen<K> {
    const VST3_CLASS_ID: [u8; 16] = K::VST3_CLASS_ID;
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = K::VST3_SUBCATEGORIES;
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const SPECS: [&GenSpec; 10] = [
        fruit_kick::SPEC,
        lemon_boom::SPEC,
        boobass::SPEC,
        acid::SPEC,
        pluck::SPEC,
        mini::SPEC,
        dx::SPEC,
        autogun::SPEC,
        drums::SPEC,
        keys::SPEC,
    ];

    /// Every generator: ids unique, starts inside the ranges, a control per setting, the plugin ids distinct.
    #[test]
    fn the_generators_are_described_whole() {
        for s in SPECS {
            assert!(s.settings.len() <= MAX, "{}", s.name);
            for (k, set) in s.settings.iter().enumerate() {
                assert_eq!(s.index(set.id), Some(k), "{} {}", s.name, set.id);
                assert!(
                    (set.range.0..=set.range.1).contains(&set.start),
                    "{} {}",
                    s.name,
                    set.id
                );
                if let Some(m) = set.magnet {
                    assert!(
                        (set.range.0..=set.range.1).contains(&m),
                        "{} {}",
                        s.name,
                        set.id
                    );
                }
                assert_eq!(
                    s.controls.iter().filter(|c| c.id() == set.id).count(),
                    1,
                    "{} {}",
                    s.name,
                    set.id
                );
                // Every value has a text, and typing it in gives a value with the same text.
                for v in set.range.0..=set.range.1 {
                    let t = (s.text)(k, v);
                    let back = (s.parse)(k, &t)
                        .unwrap_or_else(|| panic!("{} {} {v}: {t}", s.name, set.id));
                    assert_eq!((s.text)(k, back), t, "{} {} {v}", s.name, set.id);
                }
            }
            assert!(s.name.starts_with("Mango "), "{}", s.name);
            // The instrument starts and renders silence at rest.
            let mut d = (s.make)(44100, &s.start());
            let mut l = vec![0.0f32; 480];
            let mut r = vec![0.0f32; 480];
            d.note_on(60, 1.0, None);
            d.render(&mut l, &mut r);
            assert!(
                l.iter().any(|x| x.abs() > 0.001),
                "{}: a note sounds",
                s.name
            );
            d.all_off();
            while d.has_voices() {
                d.render(&mut l, &mut r);
            }
            l.iter_mut().for_each(|x| *x = 0.0);
            r.iter_mut().for_each(|x| *x = 0.0);
            d.render(&mut l, &mut r);
            assert!(
                l.iter().all(|x| x.abs() < 0.001) && r.iter().all(|x| x.abs() < 0.001),
                "{}: silence after the tail",
                s.name
            );
        }
        let ids = [
            <fruit_kick::LemonKick as ClapPlugin>::CLAP_ID,
            <lemon_boom::LemonBoom as ClapPlugin>::CLAP_ID,
            <boobass::LemonBass as ClapPlugin>::CLAP_ID,
            <acid::LemonAcid as ClapPlugin>::CLAP_ID,
            <pluck::MangoPluck as ClapPlugin>::CLAP_ID,
            <mini::MangoMini as ClapPlugin>::CLAP_ID,
            <dx::MangoDx as ClapPlugin>::CLAP_ID,
            <autogun::MangoAutogun as ClapPlugin>::CLAP_ID,
            <drums::MangoDrums as ClapPlugin>::CLAP_ID,
            <keys::MangoKeys as ClapPlugin>::CLAP_ID,
        ];
        let classes = [
            <fruit_kick::LemonKick as Vst3Plugin>::VST3_CLASS_ID,
            <lemon_boom::LemonBoom as Vst3Plugin>::VST3_CLASS_ID,
            <boobass::LemonBass as Vst3Plugin>::VST3_CLASS_ID,
            <acid::LemonAcid as Vst3Plugin>::VST3_CLASS_ID,
            <pluck::MangoPluck as Vst3Plugin>::VST3_CLASS_ID,
            <mini::MangoMini as Vst3Plugin>::VST3_CLASS_ID,
            <dx::MangoDx as Vst3Plugin>::VST3_CLASS_ID,
            <autogun::MangoAutogun as Vst3Plugin>::VST3_CLASS_ID,
            <drums::MangoDrums as Vst3Plugin>::VST3_CLASS_ID,
            <keys::MangoKeys as Vst3Plugin>::VST3_CLASS_ID,
        ];
        for i in 0..ids.len() {
            for j in i + 1..ids.len() {
                assert_ne!(ids[i], ids[j]);
                assert_ne!(classes[i], classes[j]);
            }
        }
        assert_eq!(<fruit_kick::LemonKick as Plugin>::NAME, "Mango Kick");
        assert_eq!(<pluck::MangoPluck as Plugin>::NAME, "Mango Pluck");
        assert_eq!(<mini::MangoMini as Plugin>::NAME, "Mango Mini");
        assert_eq!(<dx::MangoDx as Plugin>::NAME, "Mango DX");
        assert_eq!(<autogun::MangoAutogun as Plugin>::NAME, "Mango Autogun");
        assert_eq!(<drums::MangoDrums as Plugin>::NAME, "Mango Drums");
        assert_eq!(<keys::MangoKeys as Plugin>::NAME, "Mango Keys");
    }

    /// The saved state round-trips through the plugin state, and the editor's live edits reach the sound.
    #[test]
    fn state_and_edits_reach_the_sound() {
        for spec in SPECS {
            let b = params::default_state(spec);
            assert_eq!(b.len(), params::state_len(spec) + spec.tail.len());
            assert_eq!(params::values_of(spec, &b), spec.start());
            let blob = params::plugin_state_from_fl(spec, &b);
            assert_eq!(params::fl_from_plugin_state(spec, &blob), Some(b.clone()));
            // An edit lands in the state and reaches the sound.
            let mut p = SmallGen::<fruit_kick::KickKind> {
                params: Arc::new(GenParams::new(spec)),
                ..Default::default()
            };
            let id = p.params.p[0].as_ptr();
            p.params.live.set(id, 1.0);
            let s = p.settings();
            assert_eq!(
                s[0], spec.settings[0].range.1,
                "{}: the live edit wins",
                spec.name
            );
        }
    }
}
