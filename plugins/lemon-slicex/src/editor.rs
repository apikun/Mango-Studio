// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The plugin editor: a child of the host's window (Win32, X11 on Linux, an NSView on macOS) that paints `view` with
//! the os-ui canvas, and the editor logic (platform independent): knob drags with the cursor held in place, the
//! switches, the loop's tempo and length, loading a loop, the slicing and dump menus, choosing and playing a slice,
//! its key and reverse switch. Parameter edits show and sound at once (`os_plugkit::vals`); the loop's edits change
//! the saved state the voices play.
//!
// spec: plugins/lemon-slicex.md §3–§5, plugins/host-link.md

use std::any::Any;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::Arc;

use nih_plug::prelude::*;
use os_hostlink::NoteOut;
use os_plugkit::vals::Vals;
use os_plugkit::MenuDef;
use os_ui::kit::menu::{Mark, MenuItem, MenuStyle};

use crate::ext::{Ext, Source};
use crate::extctl::{self, Ctl};
use crate::params::{self, SlicexParams, NAMES};
use crate::state::{State, H_BEATS, H_BPM};
use crate::view::{self, Target, ViewState};
use crate::{dump, slicing, Shared};

pub struct SlicexEditor {
    pub params: Arc<SlicexParams>,
    pub shared: Arc<Shared>,
    /// DPI scale as f32 bits.
    pub scale: AtomicU32,
    /// The open editor window (0 = closed): parameter changes post a repaint to it.
    #[cfg_attr(not(windows), allow(dead_code))]
    hwnd: Arc<AtomicIsize>,
    /// Linux and macOS: the open editor's thread waker, for the same.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    slot: os_plugkit::gui::Slot,
}

impl SlicexEditor {
    pub fn new(params: Arc<SlicexParams>, shared: Arc<Shared>) -> Self {
        SlicexEditor {
            params,
            shared,
            scale: AtomicU32::new(1f32.to_bits()),
            hwnd: Arc::new(AtomicIsize::new(0)),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            slot: Default::default(),
        }
    }

    fn notify(&self) {
        #[cfg(windows)]
        win32::post_changed(self.hwnd.load(Ordering::Acquire));
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        os_plugkit::gui::notify(&self.slot);
    }
}

impl Editor for SlicexEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn Any + Send> {
        let scale = f32::from_bits(self.scale.load(Ordering::Relaxed));
        match parent {
            #[cfg(windows)]
            ParentWindowHandle::Win32Hwnd(hwnd) => {
                Box::new(win32::open(hwnd, scale, self.params.clone(), self.shared.clone(), context, self.hwnd.clone()))
            }
            #[cfg(target_os = "linux")]
            ParentWindowHandle::X11Window(window) => Box::new(gui::open(window, scale, self.params.clone(), self.shared.clone(), context, &self.slot)),
            // macOS hosts set no scale (sizes are points): the editor paints at its window's backing scale.
            #[cfg(target_os = "macos")]
            ParentWindowHandle::AppKitNsView(view) => {
                let scale = os_plugkit::gui::backing_scale(view as usize);
                Box::new(gui::open(view as usize, scale, self.params.clone(), self.shared.clone(), context, &self.slot))
            }
            _ => {
                let _ = (context, scale);
                Box::new(())
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        let x = self.params.ext();
        view::window_size(x.view, x.keys)
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        self.scale.store(factor.to_bits(), Ordering::Relaxed);
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {
        self.notify();
    }
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {
        self.notify();
    }
    fn param_values_changed(&self) {
        self.notify();
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Laws

/// Pixels of drag for a knob's whole range.
pub const KNOB_PX: f32 = 200.0;
const FINE: f32 = 0.25;
/// The velocity a click on the well plays its slice at.
const PREVIEW_VELOCITY: f32 = 100.0 / 127.0;

// ---------------------------------------------------------------------------------------------------------------
// Menus

/// The slicing menu's entries (spec: plugins/fruity-slicer.md §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slicing {
    BuiltIn,
    /// Dull, Medium, Sharp.
    Auto(usize),
    /// Slices to a beat: 6, 4, 3, 2, 1.
    Beats(u32),
    None,
    ZeroCross,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    None,
    Reset(String),
    CopyValue(String),
    PasteValue(String),
    TypeIn(String),
    Automate(String),
    Slice(Slicing),
    /// Dump: Original length (true) or Normal.
    Dump(bool),
    /// The chosen slice's root key (−1 = its own).
    Root(i32),
    /// File > New: an empty Slicex. Load sample into deck A (0) or B (1).
    NewLoop,
    LoadDeck(u8),
    /// Regions > Delete: the chosen slice goes. Assign trigger notes to all: every slice plays on its own key.
    DeleteSlice,
    DefaultKeys,
    /// Select previous (-1) or next (1) region.
    Select(i32),
    /// The options menu: layering mode, crossfade curve, the switches, the pad and the articulator.
    SetLayer(u8),
    SetFade(u8),
    ToggleSameDeck,
    ToggleAutoDump,
    CentrePad,
    ResetArt,
    /// The chosen slice's reverse switch; its name typed in.
    ToggleReverse,
    Rename,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuReq {
    /// The options button's menu (layering, crossfade, pad, articulator).
    Options,
    /// The chosen slice's menu (right click on the wave): reverse, rename, trigger note, delete.
    Slice,
    /// One of the toolbar's menus (`view::MENUS`).
    Toolbar(u8),
    Param(String),
    Slicing,
    Dump,
    Key,
    About,
}

// ---------------------------------------------------------------------------------------------------------------
// Editor logic

/// The type-in ids of the loop's tempo and length (not parameters).
pub const BPM_ID: &str = "bpm";
pub const BEATS_ID: &str = "beats";
/// The type-in id of the chosen slice's name.
pub const SLICE_NAME_ID: &str = "slice-name";

/// The envelope graph's points that can be dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
enum GraphPt {
    /// The peak: its time is the attack.
    Peak,
    /// The middle of the decay: its height is the decay's bend.
    Mid,
    /// The end of the decay: its time and height are the decay's length and the sustain level.
    End,
    /// The "S" tag: the end's time alone.
    Tag,
}

/// Something the window has to do after an event.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Repaint,
    /// Hide the cursor and hold it in place (knob drags).
    Lock,
    Menu(MenuReq, f32, f32),
    TypeIn(String, f32, f32),
    /// Ask the host for an automation clip of parameter `index` (host link).
    Automate(usize),
    OpenDialog,
    /// Send these notes to the host for the channel's current pattern (host link `Notes`).
    Notes(Vec<NoteOut>),
    /// The window changes to this size (a view button); the platform's window follows.
    Resize(u32, u32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Btn {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug)]
struct Drag {
    id: String,
    v: f32,
}

pub struct Logic {
    pub vals: Vals,
    pub params: Arc<SlicexParams>,
    pub shared: Arc<Shared>,
    pub view: ViewState,
    pub hits: Vec<view::Hit>,
    drag: Option<Drag>,
    /// Hint text (a tab before the right-aligned part).
    pub hint: String,
    copied: Option<f32>,
    pub hosted: bool,
    /// The key a click on the well is playing.
    playing: Option<u8>,
    /// The waveform's columns and the kit version they were made for.
    peaks: (u64, Vec<(f32, f32)>),
    /// A knob of the added panels being dragged (its control and position), and whether the pad is.
    ext_drag: Option<(Ctl, f32)>,
    pad_drag: bool,
    /// The deck the open dialog's file goes to (0 = A, 1 = B).
    load_deck: u8,
    /// A point of the envelope graph being dragged.
    graph_drag: Option<GraphPt>,
    /// The host's side of the window, for a size change.
    pub ctx: Option<Arc<dyn GuiContext>>,
}

impl Logic {
    pub fn new(params: Arc<SlicexParams>, shared: Arc<Shared>, ctx: Option<Arc<dyn GuiContext>>) -> Self {
        let vals = Vals::new(&*params, params.live.clone(), ctx.clone());
        Logic { vals, params, shared, view: ViewState::default(), hits: Vec::new(), drag: None, hint: String::new(), copied: None, hosted: false, playing: None, peaks: (u64::MAX, Vec::new()), ext_drag: None, pad_drag: false, load_deck: 0, graph_drag: None, ctx }
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn hit(&self, x: f32, y: f32) -> Option<Target> {
        self.hits.iter().rev().find(|h| h.r.contains(x, y)).map(|h| h.t.clone())
    }

    fn name(id: &str) -> String {
        NAMES.iter().find(|(i, _)| *i == id).map_or_else(|| id.to_string(), |(_, n)| n.to_string())
    }

    fn state(&self) -> State {
        self.params.state()
    }

    fn frames(&self) -> usize {
        self.shared.sample().map_or(0, |s| s.frames())
    }

    fn chosen(&self, st: &State) -> usize {
        self.view.chosen.min(st.slices.len().saturating_sub(1))
    }

    /// The loop changed: store it and play it.
    fn commit(&mut self, st: &State) {
        self.shared.set_state(&self.params, st);
    }

    /// The window's size now (logical px): what the view shows.
    pub fn size(&self) -> (u32, u32) {
        let x = self.params.ext();
        view::window_size(x.view, x.keys)
    }

    /// Changes what Slicex adds and plays the result.
    fn edit_ext(&mut self, f: impl FnOnce(&mut Ext)) {
        let mut x = self.params.ext();
        f(&mut x);
        self.params.set_ext(x);
        self.shared.rebuild(&self.params);
    }

    /// The pad follows the pointer at (x, y).
    fn pad_to(&mut self, x: f32, y: f32) {
        let p = view::PAD;
        self.params.set_pad(((x - p.x - 4.0) / (p.w - 8.0)).clamp(0.0, 1.0), ((p.bottom() - 4.0 - y) / (p.h - 8.0)).clamp(0.0, 1.0));
    }

    /// The envelope's point `pt` follows the pointer at (x, y).
    fn graph_to(&mut self, pt: GraphPt, x: f32, y: f32) {
        let sel = self.view.sel;
        let g = view::GRAPH;
        let secs = (f64::from((x - g.x) / g.w) * view::ENV_AXIS_S).clamp(0.0, view::ENV_AXIS_S);
        let level = ((g.bottom() - 17.0 - y) / (g.h - 21.0)).clamp(0.0, 1.0);
        self.edit_ext(|e| {
            let env = &mut e.artic[sel.art].env[sel.tgt.index()];
            let unit = env.unit_seconds(0.5);
            let (att, _, _) = env.seconds(0.5);
            match pt {
                GraphPt::Peak => {
                    let hold = f64::from(env.att + env.dec) * unit;
                    env.dec = ((hold - secs).max(0.0) / unit).min(f64::from(crate::ext::ENV_MAX_MS)) as f32;
                    env.att = (secs.min(hold) / unit).min(f64::from(crate::ext::ENV_MAX_MS)) as f32;
                }
                GraphPt::Tag | GraphPt::End => {
                    env.dec = ((secs - att).max(0.0) / unit).min(f64::from(crate::ext::ENV_MAX_MS)) as f32;
                    if pt == GraphPt::End {
                        env.sus = level;
                    }
                }
                GraphPt::Mid => {
                    // The level at the decay's middle fixes the bend: sus + (1 - sus) * 0.5^bend.
                    if env.sus < 0.98 {
                        let r = ((level - env.sus) / (1.0 - env.sus)).clamp(0.01, 0.99);
                        env.bend = (r.ln() / 0.5f32.ln()).clamp(0.4, 6.0);
                    }
                }
            }
        });
    }

    /// A press on a control of the added panels.
    fn ext_down(&mut self, c: Ctl, x: f32, y: f32, btn: Btn, m: Mods) -> Vec<Act> {
        let sel = self.view.sel;
        if btn == Btn::Right {
            match c {
                Ctl::Edit(7) if self.frames() > 0 => return vec![Act::Menu(MenuReq::Slicing, x, y)],
                Ctl::Edit(8) => return vec![Act::Menu(MenuReq::Dump, x, y)],
                _ => {}
            }
        }
        if btn != Btn::Left && !(c.is_knob() && btn == Btn::Middle) {
            return vec![];
        }
        let chosen = self.view.chosen;
        if c == Ctl::Amt && sel.src == Source::Env {
            let e = self.params.ext().artic[sel.art].env[sel.tgt.index()];
            let geo = view::env_geometry(&e, view::GRAPH);
            let near = |p: (f32, f32)| (p.0 - x).hypot(p.1 - y) <= 9.0;
            let pt = if geo.tag.outset(3.0).contains(x, y) {
                Some(GraphPt::Tag)
            } else if near(geo.end) {
                Some(GraphPt::End)
            } else if near(geo.mid) {
                Some(GraphPt::Mid)
            } else if near(geo.peak) {
                Some(GraphPt::Peak)
            } else {
                None
            };
            if pt.is_some() {
                self.graph_drag = pt;
                self.view.pressed = Some(Target::Ext(c));
                return vec![Act::Repaint];
            }
        }
        if c.is_knob() {
            if m.alt || btn == Btn::Middle {
                let d = extctl::default_norm(c, &sel);
                self.edit_ext(|e| extctl::set(e, &sel, c, d));
                return vec![];
            }
            self.ext_drag = Some((c, extctl::get(&self.params.ext(), &sel, c)));
            self.view.pressed = Some(Target::Ext(c));
            return vec![Act::Lock];
        }
        match c {
            Ctl::Pad => {
                self.pad_drag = true;
                self.view.pressed = Some(Target::Ext(c));
                self.pad_to(x, y);
            }
            Ctl::Art(i) => {
                self.view.sel.art = i;
                if !self.state().slices.is_empty() {
                    self.edit_ext(|e| e.set_art(chosen, i));
                }
            }
            Ctl::Mode(n) => self.edit_ext(|e| e.artic[sel.art].mode = n),
            Ctl::Slope(n) => self.edit_ext(|e| e.artic[sel.art].slope = n),
            Ctl::Tgt(i) => self.view.sel.tgt = crate::ext::ALL_TARGETS[i.min(crate::ext::TARGETS - 1)],
            Ctl::Src(i) => self.view.sel.src = crate::ext::ALL_SOURCES[i.min(crate::ext::SOURCES - 1)],
            Ctl::Tempo => self.edit_ext(|e| {
                let t = sel.tgt.index();
                if sel.src == Source::Lfo {
                    let l = &mut e.artic[sel.art].lfo[t];
                    l.tempo = !l.tempo;
                    l.rate = if l.tempo { 1.0 } else { 2.0 };
                } else {
                    let v = &mut e.artic[sel.art].env[t];
                    v.tempo = !v.tempo;
                }
            }),
            Ctl::LfoShape(_) => self.edit_ext(|e| {
                let l = &mut e.artic[sel.art].lfo[sel.tgt.index()];
                l.shape = (l.shape + 1) % 5;
            }),
            Ctl::Menu(i) => return vec![Act::Menu(MenuReq::Toolbar(i), x, y)],
            Ctl::Options => return vec![Act::Menu(MenuReq::Options, x, y)],
            Ctl::Edit(7) => {
                self.slice(Slicing::Auto(1));
                return vec![Act::Repaint];
            }
            Ctl::Edit(8) => return self.dump(false).map(Act::Notes).into_iter().collect(),
            Ctl::Row(3) | Ctl::Row(5) => {
                let n = self.state().slices.len() as i32;
                if n > 0 {
                    let d = if c == Ctl::Row(5) { -1 } else { 1 };
                    self.view.chosen = (self.view.chosen as i32 + d).clamp(0, n - 1) as usize;
                }
            }
            Ctl::Loop | Ctl::Edit(_) | Ctl::Info(_) | Ctl::Row(_) => {}
            Ctl::View(n) => {
                self.edit_ext(|e| e.view = n.min(3));
                let (w, h) = self.size();
                return vec![Act::Resize(w, h), Act::Repaint];
            }
            Ctl::Keys => {
                self.edit_ext(|e| e.keys = !e.keys);
                let (w, h) = self.size();
                return vec![Act::Resize(w, h), Act::Repaint];
            }
            Ctl::Smooth => self.edit_ext(|e| e.smooth = !e.smooth),
            Ctl::Layer(n) => self.edit_ext(|e| e.layer = n.min(6)),
            Ctl::SameDeck => self.edit_ext(|e| e.same_deck = !e.same_deck),
            Ctl::Crossfade(n) => self.edit_ext(|e| e.fade = n.min(4)),
            Ctl::AutoDump => self.edit_ext(|e| e.auto_dump = !e.auto_dump),
            Ctl::Key(k) => {
                self.view.pressed = Some(Target::Ext(c));
                let st = self.state();
                if let Some(i) = st.slice_for(i32::from(k)) {
                    self.view.chosen = i;
                }
                self.play(Some(k));
            }
            Ctl::PlaySlice => {
                let st = self.state();
                let i = self.chosen(&st);
                if let Some(k) = u8::try_from(st.key_of(i)).ok().filter(|&k| k < 128).filter(|_| !st.slices.is_empty()) {
                    self.view.pressed = Some(Target::Ext(c));
                    self.play(Some(k));
                }
            }
            Ctl::Stop => self.play(None),
            Ctl::DeckToggle(d) => self.edit_ext(|e| {
                // A deck goes on or off; both off leaves the other one on.
                if d == 0 {
                    e.a_on = !e.a_on || !e.b_on;
                } else {
                    e.b_on = !e.b_on || !e.a_on;
                }
            }),
            Ctl::LoadDeck(n) => {
                self.load_deck = n.min(1);
                return vec![Act::OpenDialog];
            }
            _ => {}
        }
        vec![Act::Repaint]
    }

    fn ext_hint(&self, c: Ctl) -> String {
        let (x, sel) = (self.params.ext(), self.view.sel);
        let tab = |on: bool| if on { "on" } else { "off" };
        let a = &x.artic[sel.art];
        match c {
            k if k.is_knob() => format!("{}: {}", k.caption(), extctl::text(&x, &sel, k)),
            Ctl::Pad => format!("Modulation pad\tX {:.0}%  Y {:.0}%", x.mod_x * 100.0, x.mod_y * 100.0),
            Ctl::Smooth => format!("Smooth pad moves\t{}", tab(x.smooth)),
            Ctl::Art(i) => format!("Articulator {}\tgives it to the chosen slice", i + 1),
            Ctl::Mode(n) => format!("Filter: {}", crate::ext::FILTER_MODES[usize::from(n).min(4)]),
            Ctl::Slope(n) => format!("Filter slope\t{} dB", 12 * u32::from(n)),
            Ctl::Tgt(i) => format!("Target: {}", crate::ext::ALL_TARGETS[i.min(5)].name()),
            Ctl::Src(i) => format!("Source: {}", crate::ext::ALL_SOURCES[i.min(5)].name()),
            Ctl::Tempo => format!("Times in beats\t{}", tab(if sel.src == Source::Lfo { a.lfo[sel.tgt.index()].tempo } else { a.env[sel.tgt.index()].tempo })),
            Ctl::LfoShape(_) => "LFO shape: click for the next".into(),
            Ctl::Layer(n) => format!("Drum layering\t{}", crate::ext::LAYERS[usize::from(n).min(6)]),
            Ctl::SameDeck => format!("Deck B plays deck A's slice number\t{}", tab(x.same_deck)),
            Ctl::Crossfade(n) => format!("Crossfade\t{}", crate::ext::FADES[usize::from(n).min(4)]),
            Ctl::AutoDump => format!("Auto-dump\t{}", tab(x.auto_dump)),
            Ctl::Options => "Options".into(),
            Ctl::RandLvl | Ctl::LfoLvl => format!("{}\t{}", c.caption(), extctl::text(&x, &sel, c)),
            _ => String::new(),
        }
    }

    fn hint_for(&self, t: Option<&Target>) -> String {
        let st = self.state();
        match t {
            Some(Target::Knob(id)) => format!("{}: {}", Self::name(id), self.vals.text(id)),
            Some(Target::Switch(id)) => format!("{}\t{}", Self::name(id), if self.vals.norm(id) > 0.5 { "on" } else { "off" }),
            Some(Target::Bpm) => format!("Loop tempo: {:.3} BPM", st.bpm()),
            Some(Target::Beats) => format!("Loop length: {:.2} beats", st.beats()),
            Some(Target::Load) => "Load a loop".into(),
            Some(Target::Slicing) => format!("Slicing\t{} slices", st.slices.len()),
            Some(Target::Dump) => {
                if self.hosted { "Dump to piano roll".into() } else { "Dump to piano roll\tin Lemon Studio".into() }
            }
            Some(Target::Well) => {
                let i = self.view.mouse.map_or(0, |(x, _)| slice_at(&st, view::frame_at(x, self.frames())));
                match st.slices.get(i) {
                    Some(s) => format!("Slice {}{}\t{}", i + 1, if s.name.is_empty() { String::new() } else { format!(": {}", s.name()) }, view::key_name(st.key_of(i))),
                    None => "No slices".into(),
                }
            }
            Some(Target::Key) => {
                let i = self.chosen(&st);
                match st.slices.get(i) {
                    Some(s) => format!("Slice key: {}{}", view::key_name(st.key_of(i)), if s.root < 0 { " (default)" } else { "" }),
                    None => String::new(),
                }
            }
            Some(Target::Rev) => {
                let i = self.chosen(&st);
                format!("Reverse slice\t{}", if st.slices.get(i).is_some_and(|s| s.reversed()) { "on" } else { "off" })
            }
            Some(Target::Logo) => "Mango Slicex".into(),
            Some(Target::Ext(c)) => self.ext_hint(*c),
            None => String::new(),
        }
    }

    fn reset(&mut self, id: &str) {
        let d = self.vals.default_norm(id);
        self.vals.edit(id, d);
    }

    fn play(&mut self, key: Option<u8>) {
        if let Ok(mut q) = self.shared.previews.lock() {
            if let Some(k) = self.playing.take() {
                q.push((k, 0.0, false));
            }
            if let Some(k) = key {
                q.push((k, PREVIEW_VELOCITY, true));
                self.playing = Some(k);
            }
        }
    }

    pub fn down(&mut self, x: f32, y: f32, btn: Btn, m: Mods, _dbl: bool) -> Vec<Act> {
        self.view.mouse = Some((x, y));
        let Some(t) = self.hit(x, y) else { return vec![] };
        let mut acts = vec![Act::Repaint];
        let has = self.frames() > 0;
        match (&t, btn) {
            (Target::Ext(c), _) => {
                let mut a = self.ext_down(*c, x, y, btn, m);
                acts.append(&mut a);
            }
            (Target::Knob(id) | Target::Switch(id), Btn::Left) if m.alt => self.reset(id),
            (Target::Knob(id) | Target::Switch(id), Btn::Middle) => self.reset(id),
            (Target::Knob(id), Btn::Left) => {
                self.vals.begin(id);
                self.drag = Some(Drag { id: id.clone(), v: self.vals.norm(id) });
                self.view.pressed = Some(t.clone());
                acts.push(Act::Lock);
            }
            (Target::Switch(id), Btn::Left) => {
                let on = self.vals.norm(id) > 0.5;
                self.vals.edit(id, if on { 0.0 } else { 1.0 });
            }
            (Target::Knob(id) | Target::Switch(id), Btn::Right) => acts.push(Act::Menu(MenuReq::Param(id.clone()), x, y)),
            (Target::Bpm, Btn::Left | Btn::Right) if has => acts.push(Act::TypeIn(BPM_ID.into(), x, y)),
            (Target::Beats, Btn::Left | Btn::Right) if has => acts.push(Act::TypeIn(BEATS_ID.into(), x, y)),
            (Target::Load, Btn::Left) => {
                self.load_deck = 0;
                acts.push(Act::OpenDialog)
            }
            (Target::Slicing, Btn::Left | Btn::Right) if has => acts.push(Act::Menu(MenuReq::Slicing, x, y)),
            (Target::Dump, Btn::Left | Btn::Right) => acts.push(Act::Menu(MenuReq::Dump, x, y)),
            (Target::Well, Btn::Left) if has => {
                let st = self.state();
                let i = slice_at(&st, view::frame_at(x, self.frames()));
                if i < st.slices.len() {
                    self.view.chosen = i;
                    self.view.pressed = Some(Target::Well);
                    let key = u8::try_from(st.key_of(i)).ok().filter(|&k| k < 128);
                    self.play(key);
                }
            }
            (Target::Well, Btn::Right) if has => {
                let st = self.state();
                let i = slice_at(&st, view::frame_at(x, self.frames()));
                if i < st.slices.len() {
                    self.view.chosen = i;
                    acts.push(Act::Menu(MenuReq::Slice, x, y));
                }
            }
            (Target::Key, Btn::Left | Btn::Right) if !self.state().slices.is_empty() => acts.push(Act::Menu(MenuReq::Key, view::KEY_BOX.x, view::KEY_BOX.bottom())),
            (Target::Rev, Btn::Left) => {
                let mut st = self.state();
                let i = self.chosen(&st);
                if let Some(s) = st.slices.get_mut(i) {
                    s.flags ^= 1;
                    self.commit(&st);
                }
            }
            (Target::Logo, Btn::Left | Btn::Right) => acts.push(Act::Menu(MenuReq::About, x, y)),
            _ => {}
        }
        self.hint = self.hint_for(Some(&t));
        acts
    }

    /// Mouse movement during a locked drag (logical px since the last event).
    pub fn delta(&mut self, _dx: f32, dy: f32, m: Mods) -> Vec<Act> {
        if let Some((c, v)) = self.ext_drag {
            let v = (v - dy * if m.ctrl { FINE } else { 1.0 } / KNOB_PX).clamp(0.0, 1.0);
            let sel = self.view.sel;
            self.edit_ext(|e| extctl::set(e, &sel, c, v));
            self.ext_drag = Some((c, v));
            self.hint = self.ext_hint(c);
            return vec![Act::Repaint];
        }
        let Some(Drag { id, v }) = self.drag.clone() else { return vec![] };
        let f = if m.ctrl { FINE } else { 1.0 };
        let v = (v - dy * f / KNOB_PX).clamp(0.0, 1.0);
        self.vals.set(&id, v);
        self.drag = Some(Drag { id, v });
        let t = self.view.pressed.clone();
        self.hint = self.hint_for(t.as_ref());
        vec![Act::Repaint]
    }

    pub fn up(&mut self, _btn: Btn) -> Vec<Act> {
        self.play(None);
        self.ext_drag = None;
        self.pad_drag = false;
        self.graph_drag = None;
        if let Some(d) = self.drag.take() {
            self.vals.end(&d.id);
        }
        self.view.pressed = None;
        vec![Act::Repaint]
    }

    pub fn moved(&mut self, x: f32, y: f32, _m: Mods) -> Vec<Act> {
        self.view.mouse = Some((x, y));
        if let Some(pt) = self.graph_drag {
            self.graph_to(pt, x, y);
            return vec![Act::Repaint];
        }
        if self.pad_drag {
            self.pad_to(x, y);
            self.hint = self.ext_hint(Ctl::Pad);
            return vec![Act::Repaint];
        }
        if self.drag.is_none() && self.ext_drag.is_none() {
            let t = self.hit(x, y);
            self.hint = self.hint_for(t.as_ref());
        }
        vec![Act::Repaint]
    }

    pub fn leave(&mut self) -> Vec<Act> {
        self.view.mouse = None;
        self.hint.clear();
        vec![Act::Repaint]
    }

    /// Wheel: a knob moves a step (10 cents / 1 ms; ctrl: 1), a switch flips, the tempo and length move by one.
    pub fn wheel(&mut self, x: f32, y: f32, notches: f32, m: Mods) -> Vec<Act> {
        let Some(t) = self.hit(x, y) else { return vec![] };
        let d = notches.signum();
        match &t {
            Target::Ext(c) if c.is_knob() => {
                let (sel, v) = (self.view.sel, extctl::get(&self.params.ext(), &self.view.sel, *c));
                let step = if m.ctrl { 0.01 } else { 0.05 };
                let c = *c;
                self.edit_ext(|e| extctl::set(e, &sel, c, v + d * step));
            }
            Target::Knob(id) => {
                let step = if m.ctrl || id != params::PITCH_ID { 1.0 } else { 10.0 };
                let plain = self.vals.plain(id) + d * step;
                self.vals.edit_plain(id, plain);
            }
            Target::Switch(id) => self.vals.edit(id, if d > 0.0 { 1.0 } else { 0.0 }),
            Target::Bpm if self.frames() > 0 => {
                let st = self.state();
                self.set_tempo(Some(st.bpm() + f64::from(d)), None);
            }
            Target::Beats if self.frames() > 0 => {
                let st = self.state();
                self.set_tempo(None, Some(st.beats() + f64::from(d)));
            }
            _ => return vec![],
        }
        self.hint = self.hint_for(Some(&t));
        vec![Act::Repaint]
    }

    /// Sets the loop's tempo or length; the other follows, so the loop keeps its length in seconds (spec:
    /// plugins/fruity-slicer.md §1.1).
    pub fn set_tempo(&mut self, bpm: Option<f64>, beats: Option<f64>) {
        let mut st = self.state();
        let secs = self.shared.sample().map(|s| s.frames() as f64 / f64::from(s.rate.max(1))).filter(|s| *s > 0.0);
        let Some(secs) = secs else { return };
        let (bpm, beats) = match (bpm, beats) {
            (Some(t), _) => {
                let t = t.clamp(10.0, 522.0);
                (t, t * secs / 60.0)
            }
            (None, Some(b)) => {
                let b = b.clamp(0.25, 512.0);
                (b * 60.0 / secs, b)
            }
            (None, None) => return,
        };
        st.set_f32(H_BPM, bpm as f32);
        st.set_f32(H_BEATS, beats as f32);
        self.commit(&st);
    }

    pub fn menu(&self, req: &MenuReq) -> MenuDef<Cmd> {
        let mut m = MenuDef::new(MenuStyle::Context);
        match req {
            MenuReq::Options => {
                let x = self.params.ext();
                m.head("Layering", Cmd::None);
                for (i, name) in crate::ext::LAYERS.iter().enumerate() {
                    m.add(MenuItem::item(name).mark(Mark::Radio(usize::from(x.layer) == i)), Cmd::SetLayer(i as u8));
                }
                m.sep(Cmd::None);
                m.head("Crossfade", Cmd::None);
                for (i, name) in crate::ext::FADES.iter().enumerate() {
                    m.add(MenuItem::item(name).mark(Mark::Radio(usize::from(x.fade) == i)), Cmd::SetFade(i as u8));
                }
                m.sep(Cmd::None);
                m.add(MenuItem::item("Same deck").mark(Mark::Check(x.same_deck)), Cmd::ToggleSameDeck);
                m.add(MenuItem::item("Auto-dump").mark(Mark::Check(x.auto_dump)), Cmd::ToggleAutoDump);
                m.sep(Cmd::None);
                m.add(MenuItem::item("Centre the modulation pad"), Cmd::CentrePad);
                m.add(MenuItem::item("Reset this articulator"), Cmd::ResetArt);
                m.sep(Cmd::None);
                for n in ["Assign both decks to the keyboard", "Layer the decks by name"] {
                    m.add(MenuItem::item(n).enabled(false), Cmd::None);
                }
            }
            MenuReq::Slice => {
                let st = self.state();
                let i = self.chosen(&st);
                let s = st.slices.get(i);
                m.head(&format!("Slice {}", i + 1), Cmd::None);
                m.add(MenuItem::item("Reverse").mark(Mark::Check(s.is_some_and(|s| s.reversed()))), Cmd::ToggleReverse);
                m.add(MenuItem::item("Rename..."), Cmd::Rename);
                let root = s.map_or(-1, |s| s.root);
                let mut keys = MenuDef::new(MenuStyle::Context);
                keys.add(MenuItem::item(&format!("Default ({})", view::key_name(60 + i as i32))).mark(Mark::Radio(root < 0)), Cmd::Root(-1));
                for oct in 0..=10 {
                    let mut sub = MenuDef::new(MenuStyle::Context);
                    for k in oct * 12..(oct * 12 + 12).min(128) {
                        sub.add(MenuItem::item(&view::key_name(k)).mark(Mark::Radio(k == root)), Cmd::Root(k));
                    }
                    keys.submenu(&format!("Octave {oct}"), sub, Cmd::None);
                }
                m.submenu("Trigger note", keys, Cmd::None);
                m.sep(Cmd::None);
                m.add(MenuItem::item("Delete").enabled(st.slices.len() > 1), Cmd::DeleteSlice);
            }
            MenuReq::Toolbar(i) => self.toolbar_menu(&mut m, usize::from(*i)),
            MenuReq::Param(id) => {
                m.head(&Self::name(id), Cmd::None);
                m.add(MenuItem::item("Reset"), Cmd::Reset(id.clone()));
                m.sep(Cmd::None);
                m.add(MenuItem::item("Copy value"), Cmd::CopyValue(id.clone()));
                m.add(MenuItem::item("Paste value").enabled(self.copied.is_some()), Cmd::PasteValue(id.clone()));
                if [params::PITCH_ID, params::ATTACK_ID, params::DECAY_ID].contains(&id.as_str()) {
                    m.add(MenuItem::item("Type in value..."), Cmd::TypeIn(id.clone()));
                }
                m.sep(Cmd::None);
                m.add(MenuItem::item("Create automation clip").enabled(self.hosted), Cmd::Automate(id.clone()));
            }
            MenuReq::Slicing => {
                let has_cues = self.state().path().to_lowercase().ends_with(".wav");
                m.head("Slicing", Cmd::None);
                m.add(MenuItem::item("Use sample built-in slicing").enabled(has_cues), Cmd::Slice(Slicing::BuiltIn));
                m.sep(Cmd::None);
                for (i, name) in ["Dull auto-slicing", "Medium auto-slicing", "Sharp auto-slicing"].into_iter().enumerate() {
                    m.add(MenuItem::item(name), Cmd::Slice(Slicing::Auto(i)));
                }
                m.sep(Cmd::None);
                for (n, name) in [(6, "1/6 beat"), (4, "1/4 beat"), (3, "1/3 beat"), (2, "1/2 beat"), (1, "Beat")] {
                    m.add(MenuItem::item(name), Cmd::Slice(Slicing::Beats(n)));
                }
                m.sep(Cmd::None);
                m.add(MenuItem::item("No slicing"), Cmd::Slice(Slicing::None));
                m.sep(Cmd::None);
                m.add(MenuItem::item("Zero-cross check slices"), Cmd::Slice(Slicing::ZeroCross));
            }
            MenuReq::Dump => {
                let ok = self.hosted && self.frames() > 0 && !self.state().slices.is_empty();
                m.head("Dump to piano roll", Cmd::None);
                m.add(MenuItem::item("Normal").enabled(ok), Cmd::Dump(false));
                m.add(MenuItem::item("Original length").enabled(ok), Cmd::Dump(true));
            }
            MenuReq::Key => {
                let st = self.state();
                let i = self.chosen(&st);
                let root = st.slices.get(i).map_or(-1, |s| s.root);
                m.head("Slice key", Cmd::None);
                m.add(MenuItem::item(&format!("Default ({})", view::key_name(60 + i as i32))).mark(Mark::Radio(root < 0)), Cmd::Root(-1));
                m.sep(Cmd::None);
                for oct in 0..=10 {
                    let mut sub = MenuDef::new(MenuStyle::Context);
                    for k in oct * 12..(oct * 12 + 12).min(128) {
                        sub.add(MenuItem::item(&view::key_name(k)).mark(Mark::Radio(k == root)), Cmd::Root(k));
                    }
                    m.submenu(&format!("Octave {oct}"), sub, Cmd::None);
                }
            }
            MenuReq::About => {
                m.head("Mango Slicex", Cmd::None);
                m.add(MenuItem::item(&format!("Version {}", env!("CARGO_PKG_VERSION"))).enabled(false), Cmd::None);
                m.add(MenuItem::item("Lemon Studio's recreation of Slicex").enabled(false), Cmd::None);
            }
        }
        m
    }

    /// The toolbar menu `i` as the original has it. What the sample editor does not do yet is listed but greyed.
    fn toolbar_menu(&self, m: &mut MenuDef<Cmd>, i: usize) {
        let st = self.state();
        let has = self.frames() > 0 && !st.slices.is_empty();
        let off = |m: &mut MenuDef<Cmd>, names: &[&str]| {
            for n in names {
                m.add(MenuItem::item(n).enabled(false), Cmd::None);
            }
        };
        let sub_off = |m: &mut MenuDef<Cmd>, name: &str, names: &[&str]| {
            let mut s = MenuDef::new(MenuStyle::Context);
            for n in names {
                s.add(MenuItem::item(n).enabled(false), Cmd::None);
            }
            m.submenu(name, s, Cmd::None);
        };
        match i {
            0 => {
                m.add(MenuItem::item("New"), Cmd::NewLoop);
                m.add(MenuItem::item("Load sample A..."), Cmd::LoadDeck(0));
                m.add(MenuItem::item("Load sample B..."), Cmd::LoadDeck(1));
                m.sep(Cmd::None);
                off(m, &["Save sample as...", "Export regions", "Export display as", "Recent projects"]);
            }
            1 => {
                m.add(MenuItem::item("Edit properties").enabled(false), Cmd::None);
                m.sep(Cmd::None);
                let mut r = MenuDef::new(MenuStyle::Context);
                r.add(MenuItem::item("16 bit integer").mark(Mark::Radio(false)).enabled(false), Cmd::None);
                r.add(MenuItem::item("32 bit float").mark(Mark::Radio(true)).enabled(false), Cmd::None);
                m.submenu("Resolution", r, Cmd::None);
                let mut c = MenuDef::new(MenuStyle::Context);
                c.add(MenuItem::item("Mono").mark(Mark::Radio(false)).enabled(false), Cmd::None);
                c.add(MenuItem::item("Stereo").mark(Mark::Radio(true)).enabled(false), Cmd::None);
                m.submenu("Channels", c, Cmd::None);
            }
            2 => off(
                m,
                &["Undo", "Undo history", "Cut", "Copy", "Paste insert", "Paste replace", "Paste mix", "Paste stretch", "Clear", "Delete", "Trim", "Insert silence", "Click-free (smooth) editing"],
            ),
            3 => {
                sub_off(m, "Envelope", &["Cancel all envelopes", "Add points at selection", "Add points at regions", "Flip vertically"]);
                sub_off(m, "Amp", &["Mixdown amp envelopes", "Amp tool", "Reverse polarity", "Normalize", "Fade in", "Fade out", "Declick in", "Declick out"]);
                sub_off(m, "Waveform", &["Center"]);
                sub_off(m, "Time", &["Reverse", "Time stretch / pitch shift", "Drum (loop) stretch", "Claw machine", "Scratch"]);
                sub_off(m, "Channels", &["Swap channels", "Convert left or right to mono"]);
                sub_off(m, "Synthesis", &["Generate noise"]);
                sub_off(m, "Scripting", &["Run script", "Run last script"]);
                sub_off(m, "Noise", &["Acquire noise threshold", "Gate noise", "Trim side noise", "Trim all noise and slice up"]);
                sub_off(m, "Spectral", &["Convolution reverb", "Blur", "Equalize", "Clean up (denoise)"]);
                sub_off(m, "Regions", &["Normalize all regions", "Declick in all regions", "Declick out all regions", "Perfect all regions", "Tune loop"]);
                sub_off(m, "Analysis", &["Convert to score and dump to piano roll"]);
                sub_off(m, "Sequencing", &["Send to Playlist as audio clip", "Send to selected channel"]);
            }
            4 => {
                off(m, &["Add region", "Add marker", "Set loop", "Set first downbeat"]);
                m.add(MenuItem::item("Delete").enabled(has), Cmd::DeleteSlice);
                m.sep(Cmd::None);
                m.add(MenuItem::item("Assign trigger notes to all").enabled(has), Cmd::DefaultKeys);
                m.sep(Cmd::None);
                let mut a = MenuDef::new(MenuStyle::Context);
                for (n, name) in ["Dull (few slices)", "Medium", "Sharp (many slices)"].into_iter().enumerate() {
                    a.add(MenuItem::item(name).enabled(self.frames() > 0), Cmd::Slice(Slicing::Auto(n)));
                }
                m.submenu("Auto slice", a, Cmd::None);
                let mut g = MenuDef::new(MenuStyle::Context);
                for (n, name) in [(4, "Small grid"), (2, "Medium grid"), (1, "Large grid")] {
                    g.add(MenuItem::item(name).enabled(self.frames() > 0), Cmd::Slice(Slicing::Beats(n)));
                }
                m.submenu("Grid slicing", g, Cmd::None);
                let cues = st.path().to_lowercase().ends_with(".wav");
                m.add(MenuItem::item("Use the sample's own markers").enabled(cues), Cmd::Slice(Slicing::BuiltIn));
                m.add(MenuItem::item("No slicing").enabled(self.frames() > 0), Cmd::Slice(Slicing::None));
                m.sep(Cmd::None);
                m.add(MenuItem::item("Zero-cross check all regions").enabled(has), Cmd::Slice(Slicing::ZeroCross));
                off(m, &["Detect beats", "Detect pitch regions", "Freeze all"]);
            }
            5 => off(m, &["Audio", "Spectrum", "Dual view", "Regions", "Loop"]),
            6 => off(m, &["Snap to grid", "Snap to regions", "Snap to samples", "Snap to zero-crossing", "Snap to pitch period"]),
            7 => {
                m.add(MenuItem::item("Select previous region").enabled(has), Cmd::Select(-1));
                m.add(MenuItem::item("Select next region").enabled(has), Cmd::Select(1));
                m.sep(Cmd::None);
                off(m, &["Deselect", "Select before", "Select after", "Select zoomed part"]);
            }
            _ => off(m, &["Zoom in 2x", "Zoom out 2x", "Zoom out full", "Zoom on selection"]),
        }
    }

    pub fn pick(&mut self, _req: &MenuReq, cmd: Cmd, x: f32, y: f32) -> Vec<Act> {
        match cmd {
            Cmd::None => {}
            Cmd::NewLoop => {
                let st = crate::state::State::default();
                if let Ok(mut l) = self.shared.loaded_path.lock() {
                    l.clear();
                }
                if let Ok(mut cur) = self.shared.sample.lock() {
                    *cur = None;
                }
                if let Ok(mut p) = self.params.sample_path.write() {
                    p.clear();
                }
                self.view.chosen = 0;
                self.commit(&st);
            }
            Cmd::LoadDeck(n) => {
                self.load_deck = n.min(1);
                return vec![Act::OpenDialog];
            }
            Cmd::DeleteSlice => {
                let mut st = self.state();
                let i = self.chosen(&st);
                if i < st.slices.len() && st.slices.len() > 1 {
                    st.slices.remove(i);
                    self.commit(&st);
                }
            }
            Cmd::SetLayer(n) => self.edit_ext(|e| e.layer = n.min(6)),
            Cmd::SetFade(n) => self.edit_ext(|e| e.fade = n.min(4)),
            Cmd::ToggleSameDeck => self.edit_ext(|e| e.same_deck = !e.same_deck),
            Cmd::ToggleAutoDump => self.edit_ext(|e| e.auto_dump = !e.auto_dump),
            Cmd::CentrePad => self.edit_ext(|e| (e.mod_x, e.mod_y) = (0.5, 0.5)),
            Cmd::ResetArt => {
                let a = self.view.sel.art;
                self.edit_ext(|e| e.artic[a] = crate::ext::Artic::default());
            }
            Cmd::ToggleReverse => {
                let mut st = self.state();
                let i = self.chosen(&st);
                if let Some(s) = st.slices.get_mut(i) {
                    s.flags ^= 1;
                    self.commit(&st);
                }
            }
            Cmd::Rename => return vec![Act::TypeIn(SLICE_NAME_ID.into(), x, y)],
            Cmd::DefaultKeys => {
                let mut st = self.state();
                for s in &mut st.slices {
                    s.root = -1;
                }
                self.commit(&st);
            }
            Cmd::Select(d) => {
                let n = self.state().slices.len() as i32;
                if n > 0 {
                    self.view.chosen = (self.view.chosen as i32 + d).clamp(0, n - 1) as usize;
                }
            }
            Cmd::Reset(id) => self.reset(&id),
            Cmd::CopyValue(id) => self.copied = Some(self.vals.norm(&id)),
            Cmd::PasteValue(id) => {
                if let Some(v) = self.copied {
                    self.vals.edit(&id, v);
                }
            }
            Cmd::TypeIn(id) => return vec![Act::TypeIn(id, x, y)],
            Cmd::Automate(id) => return NAMES.iter().position(|(i, _)| *i == id).map(Act::Automate).into_iter().collect(),
            Cmd::Slice(s) => self.slice(s),
            Cmd::Dump(original) => return self.dump(original).map(Act::Notes).into_iter().collect(),
            Cmd::Root(k) => {
                let mut st = self.state();
                let i = self.chosen(&st);
                if let Some(s) = st.slices.get_mut(i) {
                    s.root = k;
                    self.commit(&st);
                }
            }
        }
        vec![Act::Repaint]
    }

    /// Cuts the loop again (the slicing menu).
    pub fn slice(&mut self, how: Slicing) {
        let Some(s) = self.shared.sample() else { return };
        let mut st = self.state();
        let frames = s.frames();
        st.slices = match how {
            Slicing::BuiltIn => {
                let info = std::fs::read(crate::sample_file(&self.params)).map(|b| slicing::wav_info(&b)).unwrap_or_default();
                let b = slicing::builtin(&info, frames);
                if b.is_empty() {
                    return;
                }
                b
            }
            Slicing::Auto(i) => slicing::auto(&s.left, &s.right, s.rate, slicing::AUTO_DB[i.min(2)]),
            Slicing::Beats(n) => {
                let beats = if st.beats() > 0.0 { st.beats() } else { f64::from(slicing::guess_tempo(frames, s.rate).0) };
                slicing::by_beats(frames, beats, n)
            }
            Slicing::None => {
                let path = st.path().replace('\\', "/");
                let name = path.rsplit('/').next().unwrap_or("").to_string();
                let stem = name.rsplit_once('.').map_or(name.as_str(), |(a, _)| a).to_string();
                slicing::whole(&stem)
            }
            Slicing::ZeroCross => {
                let mut sl = st.slices.clone();
                slicing::zero_cross(&mut sl, &s.left, &s.right, s.rate);
                sl
            }
        };
        self.view.chosen = self.view.chosen.min(st.slices.len().saturating_sub(1));
        self.commit(&st);
    }

    /// The notes of a dump: Normal, or Original length at the host's tempo (the loop's own when the host gave none).
    pub fn dump(&self, original: bool) -> Option<Vec<NoteOut>> {
        let s = self.shared.sample()?;
        let st = self.state();
        let notes = if original {
            let tempo = self.shared.tempo().unwrap_or_else(|| if st.bpm() > 0.0 { st.bpm() } else { 120.0 });
            dump::original(&st, s.frames(), s.rate, tempo)
        } else {
            dump::normal(&st, s.frames())
        };
        (!notes.is_empty()).then_some(notes)
    }

    /// The value text a type-in box starts with.
    pub fn value_text(&self, id: &str) -> String {
        let st = self.state();
        match id {
            BPM_ID => format!("{:.3}", st.bpm()),
            BEATS_ID => format!("{:.2}", st.beats()),
            SLICE_NAME_ID => st.slices.get(self.chosen(&st)).map(|s| s.name()).unwrap_or_default(),
            _ => self.vals.text(id),
        }
    }

    /// Applies a typed value.
    pub fn typed(&mut self, id: &str, text: &str) {
        let num = || text.trim().trim_end_matches(|c: char| c.is_alphabetic() || c.is_whitespace()).parse::<f64>().ok();
        match id {
            BPM_ID => self.set_tempo(num(), None),
            BEATS_ID => self.set_tempo(None, num()),
            SLICE_NAME_ID => {
                let mut st = self.state();
                let i = self.chosen(&st);
                if let Some(s) = st.slices.get_mut(i) {
                    s.name = text.trim().as_bytes().to_vec();
                    self.commit(&st);
                }
            }
            _ => {
                if let Some(n) = self.vals.parse(id, text) {
                    self.vals.edit(id, n);
                }
            }
        }
    }

    /// Loads a loop (the open dialog's file).
    pub fn load(&mut self, path: &str) -> Vec<Act> {
        let deck = std::mem::take(&mut self.load_deck);
        let r = if deck == 1 {
            self.shared.load_b(path, &self.params)
        } else {
            self.view.chosen = 0;
            self.shared.load(path, &self.params)
        };
        match r {
            // A loop loaded into deck A goes to the piano roll at once when AUTO-DUMP is on.
            Ok(()) if deck == 0 && self.hosted && self.params.ext().auto_dump => self.dump(false).map(Act::Notes).into_iter().collect(),
            Ok(()) => Vec::new(),
            Err(e) => {
                self.hint = e;
                Vec::new()
            }
        }
    }

    /// The waveform's columns for the well (made again when the sample changes).
    fn peaks(&mut self) -> &[(f32, f32)] {
        let v = self.shared.kit_version.load(Ordering::Acquire);
        if self.peaks.0 != v {
            let cols = view::WAVE.w as usize;
            self.peaks = (v, self.shared.sample().map_or_else(Vec::new, |s| peaks(&s.left, &s.right, cols)));
        }
        &self.peaks.1
    }

    /// Paints the editor; `hint` is drawn when the host does not show it.
    pub fn paint(&mut self, c: &mut os_ui::Canvas, palette: &os_theme::Palette) {
        let st = self.state();
        let frames = self.frames();
        let can_dump = self.hosted && frames > 0 && !st.slices.is_empty();
        self.view.chosen = self.chosen(&st);
        // A value shows over the wave while it is dragged; no tooltips otherwise.
        let hint = if self.hosted || (self.drag.is_none() && self.ext_drag.is_none()) { String::new() } else { self.hint.clone() };
        self.peaks();
        let ext = self.params.ext();
        let a_name = std::path::Path::new(&st.path()).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let b_name = if ext.deck_b.is_empty() { String::new() } else { crate::state::State::parse(&ext.deck_b).map(|s| std::path::Path::new(&s.path()).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).unwrap_or_default() };
        let rate = self.shared.sample().map_or(0, |s| s.rate);
        let look = view::Look { state: &st, frames, rate, peaks: &self.peaks.1, can_dump, ext: &ext, decks: [a_name, b_name] };
        self.hits = view::paint(c, palette, &self.vals, &self.view, &look, &hint);
    }
}

/// The slice the frame is in (the last one starting at or before it).
pub fn slice_at(st: &State, frame: usize) -> usize {
    st.slices.iter().rposition(|s| s.start as usize <= frame).unwrap_or(0)
}

/// The low and high of the summed channels in `cols` columns.
pub fn peaks(left: &[f32], right: &[f32], cols: usize) -> Vec<(f32, f32)> {
    let n = left.len().min(right.len());
    if n == 0 || cols == 0 {
        return Vec::new();
    }
    (0..cols)
        .map(|c| {
            let (a, b) = (c * n / cols, ((c + 1) * n / cols).max(c * n / cols + 1).min(n));
            (a..b).fold((0.0f32, 0.0f32), |(lo, hi), i| {
                let v = (left[i] + right[i]) * 0.5;
                (lo.min(v), hi.max(v))
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------
// The window

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::Arc;

    use nih_plug::prelude::GuiContext;
    use os_plugkit::win::{self as pw, CursorLock};
    use os_theme::Palette;
    use os_ui::Canvas;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OPENFILENAMEW};
    use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    use super::{Act, Btn, Logic, Mods};
    use crate::params::SlicexParams;
    use crate::view;
    use crate::Shared;

    const TIMER: usize = 1;
    const WM_CHANGED: u32 = WM_APP + 1;

    struct Win {
        hwnd: HWND,
        canvas: Canvas,
        buf: Vec<u32>,
        scale: f32,
        /// The window's size, and the canvas's (the full layout; a smaller view shows its top rows).
        size: (u32, u32),
        full: (u32, u32),
        logic: Logic,
        palette: Palette,
        last_hint: String,
        lock: CursorLock,
        tracking: bool,
        slot: Arc<AtomicIsize>,
        /// What was painted last (repaint when it changes): the values, the saved state.
        last_sig: u64,
        last_raw: u64,
    }

    /// Owns the window; dropping it closes the editor.
    pub struct Handle(isize, Arc<AtomicIsize>, Arc<Shared>);

    // SAFETY: the handle is only an HWND value; the wrapper drops it on the GUI thread.
    unsafe impl Send for Handle {}

    impl Drop for Handle {
        fn drop(&mut self) {
            self.1.store(0, Ordering::Release);
            self.2.editor_open.store(false, Ordering::Relaxed);
            unsafe { DestroyWindow(self.0 as HWND) };
        }
    }

    pub fn post_changed(hwnd: isize) {
        if hwnd != 0 {
            unsafe { PostMessageW(hwnd as HWND, WM_CHANGED, 0, 0) };
        }
    }

    fn mods() -> Mods {
        let (ctrl, shift, alt) = pw::mods();
        Mods { ctrl, shift, alt }
    }

    pub fn open(parent: *mut c_void, scale: f32, params: Arc<SlicexParams>, shared: Arc<Shared>, ctx: Arc<dyn GuiContext>, slot: Arc<AtomicIsize>) -> Handle {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let px = |v: u32| (v as f32 * scale).round() as u32;
        let full = (px(view::W), px(view::H));
        let now = { let x = params.ext(); view::window_size(x.view, x.keys) };
        let size = (px(now.0), px(now.1));
        shared.editor_open.store(true, Ordering::Relaxed);
        unsafe {
            let class = pw::register("LemonSlicexView", Some(wndproc));
            let win = Box::new(Win {
                hwnd: std::ptr::null_mut(),
                canvas: Canvas::new(full.0, full.1, scale).expect("canvas"),
                buf: vec![0; (full.0 * full.1) as usize],
                scale,
                size,
                full,
                logic: Logic::new(params, shared.clone(), Some(ctx)),
                palette: os_plugkit::palette(),
                last_hint: String::new(),
                lock: CursorLock::default(),
                tracking: false,
                slot: slot.clone(),
                last_sig: 0,
                last_raw: u64::MAX,
            });
            let title = pw::wide("Mango Slicex");
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                0,
                0,
                size.0 as i32,
                size.1 as i32,
                parent as HWND,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                Box::into_raw(win) as *const c_void,
            );
            slot.store(hwnd as isize, Ordering::Release);
            SetTimer(hwnd, TIMER, 50, None);
            Handle(hwnd as isize, slot, shared)
        }
    }

    unsafe fn state(hwnd: HWND) -> Option<&'static mut Win> {
        let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Win;
        if p.is_null() { None } else { Some(unsafe { &mut *p }) }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_NCCREATE {
                let cs = lp as *const CREATESTRUCTW;
                let w = (*cs).lpCreateParams as *mut Win;
                (*w).hwnd = hwnd;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, w as isize);
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            if msg == WM_NCDESTROY {
                let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Win;
                if !p.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    (*p).slot.store(0, Ordering::Release);
                    drop(Box::from_raw(p));
                }
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            let Some(w) = state(hwnd) else { return DefWindowProcW(hwnd, msg, wp, lp) };
            if msg == os_hostlink::message() {
                return w.host_link(wp, lp);
            }
            match msg {
                WM_PAINT => {
                    w.paint();
                    0
                }
                WM_ERASEBKGND => 1,
                WM_TIMER | WM_CHANGED => {
                    w.poll();
                    0
                }
                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_RBUTTONDBLCLK | WM_MBUTTONDOWN => {
                    SetFocus(hwnd);
                    let (x, y) = w.mouse(lp);
                    let btn = match msg {
                        WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => Btn::Right,
                        WM_MBUTTONDOWN => Btn::Middle,
                        _ => Btn::Left,
                    };
                    let acts = w.logic.down(x, y, btn, mods(), msg == WM_LBUTTONDBLCLK);
                    if !acts.contains(&Act::Lock) {
                        SetCapture(hwnd);
                    }
                    w.run(acts);
                    0
                }
                WM_MOUSEMOVE => {
                    if !w.tracking {
                        let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                        TrackMouseEvent(&mut tme);
                        w.tracking = true;
                    }
                    if let Some((dx, dy)) = w.lock.delta() {
                        if dx != 0 || dy != 0 {
                            let acts = w.logic.delta(dx as f32 / w.scale, dy as f32 / w.scale, mods());
                            w.run(acts);
                        }
                        return 0;
                    }
                    let (x, y) = w.mouse(lp);
                    let acts = w.logic.moved(x, y, mods());
                    w.run(acts);
                    0
                }
                WM_MOUSELEAVE => {
                    w.tracking = false;
                    if !w.lock.locked() && !w.logic.dragging() {
                        let acts = w.logic.leave();
                        w.run(acts);
                    }
                    0
                }
                WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
                    w.lock.unlock();
                    ReleaseCapture();
                    let acts = w.logic.up(Btn::Left);
                    w.run(acts);
                    0
                }
                WM_CAPTURECHANGED => {
                    if w.logic.dragging() && lp as HWND != hwnd {
                        w.lock.unlock();
                        let acts = w.logic.up(Btn::Left);
                        w.run(acts);
                    }
                    0
                }
                WM_MOUSEWHEEL => {
                    let mut r: RECT = std::mem::zeroed();
                    GetWindowRect(hwnd, &mut r);
                    let (sx, sy) = pw::xy(lp);
                    let (x, y) = ((sx - r.left) as f32 / w.scale, (sy - r.top) as f32 / w.scale);
                    let notches = ((wp >> 16) & 0xffff) as u16 as i16 as f32 / 120.0;
                    let acts = w.logic.wheel(x, y, notches, mods());
                    w.run(acts);
                    0
                }
                WM_DESTROY => {
                    KillTimer(hwnd, TIMER);
                    w.lock.unlock();
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
    }

    impl Win {
        fn mouse(&self, l: LPARAM) -> (f32, f32) {
            let (x, y) = pw::xy(l);
            (x as f32 / self.scale, y as f32 / self.scale)
        }

        fn screen(&self, x: f32, y: f32) -> (i32, i32) {
            pw::to_screen(self.hwnd, (x * self.scale) as i32, (y * self.scale) as i32)
        }

        fn invalidate(&self) {
            unsafe { InvalidateRect(self.hwnd, std::ptr::null(), 0) };
        }

        /// Timer / parameter change: expire confirmed overlay values; repaint when something changed.
        fn poll(&mut self) {
            let gone = self.logic.vals.expire();
            let sig = self.logic.vals.signature();
            let raw = self.logic.params.raw_version.load(Ordering::Acquire);
            if gone || sig != self.last_sig || raw != self.last_raw {
                self.invalidate();
            }
            if self.logic.hint != self.last_hint {
                self.last_hint = self.logic.hint.clone();
                if self.logic.hosted {
                    os_hostlink::to_host(self.hwnd as isize, os_hostlink::Cmd::Changed, 0);
                }
            }
        }

        fn run(&mut self, acts: Vec<Act>) {
            let mut repaint = false;
            for a in acts {
                match a {
                    Act::Repaint => repaint = true,
                    Act::Lock => self.lock.lock(self.hwnd),
                    Act::Menu(req, x, y) => {
                        let def = self.logic.menu(&req);
                        let (sx, sy) = self.screen(x, y);
                        if let Some(cmd) = pw::popup(self.hwnd, sx, sy, &def, &self.palette, self.scale) {
                            let acts = self.logic.pick(&req, cmd, x, y);
                            self.run(acts);
                        }
                        repaint = true;
                    }
                    Act::TypeIn(id, x, y) => {
                        let (sx, sy) = self.screen(x, y);
                        let cur = self.logic.value_text(&id);
                        if let Some(t) = pw::type_in(self.hwnd, sx, sy, &cur, self.scale) {
                            self.logic.typed(&id, &t);
                        }
                        repaint = true;
                    }
                    Act::Automate(i) => {
                        os_hostlink::to_host(self.hwnd as isize, os_hostlink::Cmd::Automate, i as isize);
                    }
                    Act::OpenDialog => {
                        if let Some(path) = file_dialog(self.hwnd) {
                            let acts = self.logic.load(&path);
                            self.act(acts);
                        }
                        repaint = true;
                    }
                    Act::Notes(notes) => {
                        let token = os_hostlink::post_notes(notes);
                        if os_hostlink::to_host(self.hwnd as isize, os_hostlink::Cmd::Notes, token) == 0 {
                            os_hostlink::take_notes(token);
                        }
                    }
                    Act::Resize(w, h) => {
                        self.size = ((w as f32 * self.scale).round() as u32, (h as f32 * self.scale).round() as u32);
                        unsafe { SetWindowPos(self.hwnd, std::ptr::null_mut(), 0, 0, self.size.0 as i32, self.size.1 as i32, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE) };
                        if self.logic.hosted {
                            os_hostlink::to_host(self.hwnd as isize, os_hostlink::Cmd::Changed, 0);
                        } else if let Some(c) = &self.logic.ctx {
                            c.request_resize();
                        }
                        repaint = true;
                    }
                }
            }
            if repaint {
                self.invalidate();
            }
        }

        /// Answers the host (spec `plugins/host-link.md`): one page, the hint line. Unknown commands return 0.
        unsafe fn host_link(&mut self, wp: WPARAM, lp: LPARAM) -> LRESULT {
            use os_hostlink::{Cmd, PageIcon};
            let Some(cmd) = Cmd::from_usize(wp) else { return 0 };
            match cmd {
                Cmd::Hello => {
                    self.logic.hosted = true;
                    self.invalidate();
                    os_hostlink::VERSION as LRESULT
                }
                Cmd::PageCount => 1,
                Cmd::PageIcon => PageIcon::Sample as LRESULT,
                Cmd::GetPage | Cmd::SetPage => 0,
                Cmd::Metrics => {
                    let (w, h) = self.logic.size();
                    os_hostlink::pack_size(w, h)
                }
                Cmd::Hint => {
                    if lp != 0 {
                        // SAFETY: the host passes a `HintBuf` of its own process (spec: host-link.md).
                        unsafe { &mut *(lp as *mut os_hostlink::HintBuf) }.set(&self.logic.hint);
                    }
                    1
                }
                Cmd::Changed | Cmd::Note | Cmd::Automate | Cmd::Audio | Cmd::Resizable | Cmd::SetSize | Cmd::Notes => 0,
            }
        }

        fn paint(&mut self) {
            let (hdc, ps) = pw::begin_paint(self.hwnd);
            self.logic.paint(&mut self.canvas, &self.palette);
            self.last_sig = self.logic.vals.signature();
            self.last_raw = self.logic.params.raw_version.load(Ordering::Acquire);
            self.buf.resize((self.full.0 * self.full.1) as usize, 0);
            self.canvas.copy_to_xrgb(&mut self.buf);
            pw::blit(hdc, &self.buf[..(self.size.0 * self.size.1) as usize], self.size.0, self.size.1);
            pw::end_paint(self.hwnd, &ps);
        }
    }

    /// The open sample dialog; `None` when cancelled or in tests that allow no modal loops.
    fn file_dialog(owner: HWND) -> Option<String> {
        if os_plugkit::no_modal() {
            return None;
        }
        let mut file = vec![0u16; 1024];
        let filter: Vec<u16> = "Audio files\0*.wav;*.flac;*.ogg;*.wv;*.aif;*.aiff;*.mp3\0All files\0*.*\0\0".encode_utf16().collect();
        let title = pw::wide("Loop to slice");
        unsafe {
            let mut ofn: OPENFILENAMEW = std::mem::zeroed();
            ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
            ofn.hwndOwner = owner;
            ofn.lpstrFilter = filter.as_ptr();
            ofn.lpstrFile = file.as_mut_ptr();
            ofn.nMaxFile = file.len() as u32;
            ofn.lpstrTitle = title.as_ptr();
            ofn.Flags = OFN_NOCHANGEDIR | OFN_FILEMUSTEXIST;
            if GetOpenFileNameW(&mut ofn) == 0 {
                return None;
            }
        }
        let end = file.iter().position(|&c| c == 0).unwrap_or(file.len());
        Some(String::from_utf16_lossy(&file[..end]))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod gui {
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use nih_plug::prelude::GuiContext;
    use os_hostlink::{Cmd, PageIcon, Peer};
    use os_plugkit::gui::{self as px, Button, Input, Window};
    use os_theme::Palette;
    use os_ui::Canvas;

    use super::{Act, Btn, Logic, Mods};
    use crate::params::SlicexParams;
    use crate::view;
    use crate::Shared;

    struct Ed {
        win: Window,
        canvas: Canvas,
        buf: Vec<u32>,
        scale: f32,
        /// The window's size, and the canvas's (the full layout; a smaller view shows its top rows).
        size: (u32, u32),
        full: (u32, u32),
        logic: Logic,
        palette: Palette,
        last_hint: String,
        dirty: bool,
        link: Arc<Peer>,
    }

    impl Drop for Ed {
        fn drop(&mut self) {
            self.link.unregister();
        }
    }

    /// Owns the editor thread; dropping it closes the editor.
    pub struct Handle(Option<px::EditorThread>, Arc<Shared>);

    impl Drop for Handle {
        fn drop(&mut self) {
            self.0.take();
            self.1.editor_open.store(false, Ordering::Relaxed);
        }
    }

    pub fn open(parent: px::Parent, scale: f32, params: Arc<SlicexParams>, shared: Arc<Shared>, ctx: Arc<dyn GuiContext>, slot: &px::Slot) -> Handle {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let full = ((view::W as f32 * scale).round() as u32, (view::H as f32 * scale).round() as u32);
        let now = { let x = params.ext(); view::window_size(x.view, x.keys) };
        let size = ((now.0 as f32 * scale).round() as u32, (now.1 as f32 * scale).round() as u32);
        let sh = shared.clone();
        let wake = slot.clone();
        let link = Peer::register(parent as u64, vec![PageIcon::Sample], now, move || px::notify(&wake));
        let peer = link.clone();
        let thread = px::spawn(parent, size, slot, move |win| {
            let Ok(canvas) = Canvas::new(full.0, full.1, scale) else { return };
            let logic = Logic::new(params, shared, Some(ctx));
            let ed = Ed { win, canvas, buf: Vec::new(), scale, size, full, logic, palette: os_plugkit::palette(), last_hint: String::new(), dirty: true, link: peer };
            ed.run();
        });
        if thread.is_none() {
            link.unregister();
        }
        sh.editor_open.store(thread.is_some(), Ordering::Relaxed);
        Handle(thread, sh)
    }

    fn btn(b: Button) -> Btn {
        match b {
            Button::Left => Btn::Left,
            Button::Right => Btn::Right,
            Button::Middle => Btn::Middle,
        }
    }

    impl Ed {
        fn run(mut self) {
            while let Some(input) = self.win.next() {
                match input {
                    Input::Expose => self.dirty = true,
                    Input::Tick => self.poll(),
                    Input::Down { x, y, btn: b, dbl } => {
                        let (x, y) = (x / self.scale, y / self.scale);
                        let acts = self.logic.down(x, y, btn(b), self.mods(), dbl && b == Button::Left);
                        self.act(acts);
                    }
                    Input::Move { x, y } => {
                        let acts = self.logic.moved(x / self.scale, y / self.scale, self.mods());
                        self.act(acts);
                    }
                    Input::Delta { dx, dy } => {
                        let acts = self.logic.delta(dx / self.scale, dy / self.scale, self.mods());
                        self.act(acts);
                    }
                    Input::Up { .. } => {
                        self.win.unlock();
                        let acts = self.logic.up(Btn::Left);
                        self.act(acts);
                    }
                    Input::Leave => {
                        if !self.win.locked() && !self.logic.dragging() {
                            let acts = self.logic.leave();
                            self.act(acts);
                        }
                    }
                    Input::Wheel { x, y, notches } => {
                        let acts = self.logic.wheel(x / self.scale, y / self.scale, notches, self.mods());
                        self.act(acts);
                    }
                    Input::Key { .. } | Input::Drop { .. } => {}
                }
                if self.dirty && self.win.idle() {
                    self.paint();
                }
            }
        }

        fn mods(&self) -> Mods {
            let (ctrl, shift, alt) = self.win.mods();
            Mods { ctrl, shift, alt }
        }

        fn screen(&self, x: f32, y: f32) -> (i32, i32) {
            self.win.to_root((x * self.scale) as i32, (y * self.scale) as i32)
        }

        fn poll(&mut self) {
            self.logic.vals.expire();
            self.dirty = true;
            if self.logic.hint != self.last_hint {
                self.last_hint.clone_from(&self.logic.hint);
                self.link.set_hint(&self.logic.hint);
                self.link.to_host(Cmd::Changed, 0);
            }
            self.logic.hosted = self.link.hosted();
        }

        fn act(&mut self, acts: Vec<Act>) {
            for a in acts {
                match a {
                    Act::Repaint => self.dirty = true,
                    Act::Lock => self.win.lock(),
                    Act::Menu(req, x, y) => {
                        let def = self.logic.menu(&req);
                        let (sx, sy) = self.screen(x, y);
                        if let Some(cmd) = self.win.popup(sx, sy, &def, &self.palette, self.scale) {
                            let acts = self.logic.pick(&req, cmd, x, y);
                            self.act(acts);
                        }
                        self.dirty = true;
                    }
                    Act::TypeIn(id, x, y) => {
                        let (sx, sy) = self.screen(x, y);
                        let cur = self.logic.value_text(&id);
                        if let Some(t) = self.win.type_in(sx, sy, &cur, self.scale) {
                            self.logic.typed(&id, &t);
                        }
                        self.dirty = true;
                    }
                    Act::Automate(i) => {
                        self.link.to_host(Cmd::Automate, i as isize);
                    }
                    Act::OpenDialog => {
                        if let Some(path) = file_dialog(&self.win) {
                            let acts = self.logic.load(&path);
                            self.act(acts);
                        }
                        self.dirty = true;
                    }
                    Act::Notes(notes) => {
                        let token = os_hostlink::post_notes(notes);
                        if self.link.to_host(Cmd::Notes, token) == 0 {
                            os_hostlink::take_notes(token);
                        }
                    }
                    Act::Resize(w, h) => {
                        self.size = ((w as f32 * self.scale).round() as u32, (h as f32 * self.scale).round() as u32);
                        self.win.resize(self.size.0, self.size.1);
                        // Lemon Studio's frame follows the link; other hosts the request.
                        self.link.set_size(w, h);
                        if let Some(c) = &self.logic.ctx {
                            c.request_resize();
                        }
                        self.dirty = true;
                    }
                }
            }
        }

        fn paint(&mut self) {
            self.dirty = false;
            self.logic.paint(&mut self.canvas, &self.palette);
            self.buf.resize((self.full.0 * self.full.1) as usize, 0);
            self.canvas.copy_to_xrgb(&mut self.buf);
            self.win.blit(&self.buf[..(self.size.0 * self.size.1) as usize], self.size.0, self.size.1);
        }
    }

    /// The open sample dialog: the desktop's own (through its portal on Linux; on macOS AppKit's).
    fn file_dialog(win: &Window) -> Option<String> {
        if os_plugkit::no_modal() {
            return None;
        }
        let path = win.dialog(move || {
            rfd::FileDialog::new()
                .set_title("Loop to slice")
                .add_filter("Audio files", &["wav", "flac", "ogg", "wv", "aif", "aiff", "mp3"])
                .pick_file()
        });
        path.flatten().map(|p| p.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::loop_file;

    fn logic() -> Logic {
        let mut l = Logic::new(Arc::new(SlicexParams::default()), Arc::new(Shared::default()), None);
        paint(&mut l);
        l
    }

    fn paint(l: &mut Logic) {
        let mut c = os_ui::Canvas::new(view::W, view::H, 1.0).unwrap();
        l.paint(&mut c, &os_plugkit::palette());
    }

    fn centre(l: &Logic, t: &Target) -> (f32, f32) {
        let h = l.hits.iter().rev().find(|h| &h.t == t).unwrap_or_else(|| panic!("no hit {t:?}"));
        (h.r.cx(), h.r.cy())
    }

    fn loaded() -> (Logic, std::path::PathBuf) {
        let p = loop_file("editor", &[]);
        let mut l = logic();
        l.load(&p.to_string_lossy());
        paint(&mut l);
        (l, p)
    }

    #[test]
    fn every_parameter_has_a_control() {
        let l = logic();
        // The window has the original's four master faders; attack, decay and the switches stay plugin parameters only.
        assert!(l.hits.iter().any(|h| matches!(&h.t, Target::Knob(i) if i == params::PITCH_ID)));
        for t in [Target::Well, Target::Bpm, Target::Beats, Target::Logo, Target::Ext(Ctl::Master), Target::Ext(Ctl::RandLvl), Target::Ext(Ctl::LfoLvl), Target::Ext(Ctl::AutoDump), Target::Ext(Ctl::Crossfade(2)), Target::Ext(Ctl::Menu(0)), Target::Ext(Ctl::Menu(8)), Target::Ext(Ctl::Edit(7)), Target::Ext(Ctl::Edit(8)), Target::Ext(Ctl::PlaySlice)] {
            centre(&l, &t);
        }
    }

    #[test]
    fn knobs_switches_and_hints() {
        let mut l = logic();
        let (x, y) = centre(&l, &Target::Knob(params::PITCH_ID.into()));
        l.wheel(x, y, 1.0, Mods::default());
        assert_eq!(l.params.values().pitch, 10);
        assert_eq!(l.hint, "Pitch shift: +10 cents");
        assert!(l.down(x, y, Btn::Left, Mods::default(), false).contains(&Act::Lock));
        // 200 px = the whole range: 5 px up = +60 cents.
        for _ in 0..5 {
            l.delta(0.0, -1.0, Mods::default());
        }
        l.up(Btn::Left);
        assert_eq!(l.params.values().pitch, 70);
        // The master level faders move what Slicex adds.
        let (x, y) = centre(&l, &Target::Ext(Ctl::LfoLvl));
        l.down(x, y, Btn::Left, Mods::default(), false);
        for _ in 0..20 {
            l.delta(0.0, 1.0, Mods::default());
        }
        l.up(Btn::Left);
        assert!((l.params.ext().lfo_level - 0.9).abs() < 1e-3, "{}", l.params.ext().lfo_level);
        assert_eq!(l.pick(&MenuReq::Param("pitch".into()), Cmd::Automate("pitch".into()), 0.0, 0.0), vec![Act::Automate(0)]);
    }

    #[test]
    fn slicing_choosing_and_the_slices_settings() {
        let (mut l, p) = loaded();
        assert_eq!(l.state().slices.len(), 4);
        l.pick(&MenuReq::Slicing, Cmd::Slice(Slicing::Beats(2)), 0.0, 0.0);
        let st = l.state();
        assert_eq!(st.slices.len(), 8);
        assert_eq!(st.slices[2].name(), "3 - Beat 2");
        l.pick(&MenuReq::Slicing, Cmd::Slice(Slicing::None), 0.0, 0.0);
        assert_eq!(l.state().slices[0].name(), p.file_stem().unwrap().to_string_lossy());
        l.pick(&MenuReq::Slicing, Cmd::Slice(Slicing::Auto(1)), 0.0, 0.0);
        paint(&mut l);
        // A click on the well's third quarter chooses slice 3 and plays its key; letting go ends the note.
        let x = view::WAVE.x + view::WAVE.w * 0.6;
        l.down(x, view::WAVE.cy(), Btn::Left, Mods::default(), false);
        assert_eq!(l.view.chosen, 2);
        assert_eq!(*l.shared.previews.lock().unwrap(), vec![(62, PREVIEW_VELOCITY, true)]);
        l.up(Btn::Left);
        assert_eq!(l.shared.previews.lock().unwrap().last(), Some(&(62, 0.0, false)));
        assert_eq!(l.hint, "Slice 3\tD5");
        // A right click on the wave opens the slice's menu: its trigger note, reverse and name.
        let x = view::WAVE.x + view::WAVE.w * 0.6;
        let acts = l.down(x, view::WAVE.cy(), Btn::Right, Mods::default(), false);
        assert!(acts.contains(&Act::Menu(MenuReq::Slice, x, view::WAVE.cy())));
        assert!(l.menu(&MenuReq::Slice).find("Delete").is_some());
        l.pick(&MenuReq::Slice, Cmd::Root(36), 0.0, 0.0);
        l.pick(&MenuReq::Slice, Cmd::ToggleReverse, 0.0, 0.0);
        assert_eq!(l.pick(&MenuReq::Slice, Cmd::Rename, 1.0, 2.0), vec![Act::TypeIn(SLICE_NAME_ID.into(), 1.0, 2.0)]);
        l.typed(SLICE_NAME_ID, "Snare");
        assert_eq!(l.state().slices[2].name(), "Snare");
        let st = l.state();
        assert_eq!((st.slices[2].root, st.slices[2].reversed()), (36, true));
        assert_eq!(l.shared.kit.lock().unwrap().as_ref().unwrap().keys[36], Some(2));
        assert!(l.menu(&MenuReq::Key).find("C3").is_some());
        let _ = std::fs::remove_file(p);
    }

    /// Deck B loads a loop; layering picks the decks that sound, SAME DECK plays B's slice with A's number, and an
    /// articulator button assigns the chosen slice.
    #[test]
    fn decks_layers_and_articulators() {
        let (mut l, p) = loaded();
        let q = loop_file("deck-b", &[]);
        assert!(l.pick(&MenuReq::Toolbar(0), Cmd::LoadDeck(1), 0.0, 0.0).contains(&Act::OpenDialog));
        l.load(&q.to_string_lossy());
        paint(&mut l);
        assert!(l.shared.kit.lock().unwrap().as_ref().unwrap().b.is_some());
        let (mut a, mut b) = (crate::dsp::Engine::new(44100.0), crate::dsp::Engine::new(44100.0));
        let kit = l.shared.kit.lock().unwrap().clone().unwrap();
        a.note_on(Some(&kit), 61, 1.0, None);
        assert_eq!(a.sounding().count(), 2, "both decks play");
        l.edit_ext(|e| e.b_on = false);
        let kit = l.shared.kit.lock().unwrap().clone().unwrap();
        b.note_on(Some(&kit), 61, 1.0, None);
        assert_eq!(b.sounding().count(), 1, "deck A alone");
        // Articulator 3 for the chosen slice.
        l.view.chosen = 1;
        let (x, y) = centre(&l, &Target::Ext(Ctl::Art(2)));
        l.down(x, y, Btn::Left, Mods::default(), false);
        assert_eq!((l.view.sel.art, l.params.ext().art_of(1)), (2, 2));
        let _ = (std::fs::remove_file(p), std::fs::remove_file(q));
    }

    /// The view buttons: each asks for its window size, the bands under the top strip move up with their controls, and
    /// a hidden band answers nothing.
    #[test]
    fn the_view_buttons_resize_and_move_the_bands() {
        let (mut l, p) = loaded();
        let (x, y) = centre(&l, &Target::Ext(Ctl::View(2)));
        let acts = l.down(x, y, Btn::Left, Mods::default(), false);
        assert!(acts.contains(&Act::Resize(view::W, view::H - 214)), "{acts:?}");
        paint(&mut l);
        // The region settings are gone; the wave moved up 214 px.
        assert!(!l.hits.iter().any(|h| h.t == Target::Ext(Ctl::Art(0))));
        let (_, wy) = centre(&l, &Target::Well);
        assert_eq!(wy, view::WAVE.cy() - 214.0);
        // Only the master panel, then everything again with the keyboard off.
        let (x, y) = centre(&l, &Target::Ext(Ctl::View(3)));
        assert!(l.down(x, y, Btn::Left, Mods::default(), false).contains(&Act::Resize(view::W, 112)));
        paint(&mut l);
        assert!(!l.hits.iter().any(|h| h.t == Target::Well || h.t == Target::Load));
        let (x, y) = centre(&l, &Target::Ext(Ctl::View(0)));
        l.down(x, y, Btn::Left, Mods::default(), false);
        paint(&mut l);
        let (x, y) = centre(&l, &Target::Ext(Ctl::Keys));
        assert!(l.down(x, y, Btn::Left, Mods::default(), false).contains(&Act::Resize(view::W, view::H - 66)));
        assert_eq!(l.size(), (view::W, view::H - 66));
        let _ = std::fs::remove_file(p);
    }

    /// The envelope graph's points follow the pointer: the S tag moves the decay's end in time, the end point also sets
    /// the sustain level, the middle point bends the decay, the peak sets the attack.
    #[test]
    fn the_envelopes_points_can_be_dragged() {
        let mut l = logic();
        let env = |l: &Logic| l.params.ext().artic[0].env[crate::ext::Target::Vol.index()];
        let g = view::GRAPH;
        let at = |s: f64| g.x + (s / view::ENV_AXIS_S) as f32 * g.w;
        // The S tag: from 1.5 s to 1 s, the sustain level stays.
        let geo = view::env_geometry(&env(&l), g);
        l.down(geo.tag.cx(), geo.tag.cy(), Btn::Left, Mods::default(), false);
        l.moved(at(1.0), geo.tag.cy(), Mods::default());
        l.up(Btn::Left);
        assert!((env(&l).dec - 1000.0).abs() < 8.0, "{}", env(&l).dec);
        assert_eq!(env(&l).sus, 0.0);
        // The end point: also up to half the level.
        paint(&mut l);
        let geo = view::env_geometry(&env(&l), g);
        let mid_y = g.bottom() - 17.0 - 0.5 * (g.h - 21.0);
        l.down(geo.end.0, geo.end.1, Btn::Left, Mods::default(), false);
        l.moved(at(2.0), mid_y, Mods::default());
        l.up(Btn::Left);
        assert!((env(&l).dec - 2000.0).abs() < 8.0 && (env(&l).sus - 0.5).abs() < 0.02, "{:?}", env(&l));
        // The middle point: a straight decay is halfway between the levels.
        paint(&mut l);
        let geo = view::env_geometry(&env(&l), g);
        let straight = g.bottom() - 17.0 - 0.75 * (g.h - 21.0);
        l.down(geo.mid.0, geo.mid.1, Btn::Left, Mods::default(), false);
        l.moved(geo.mid.0, straight, Mods::default());
        l.up(Btn::Left);
        assert!((env(&l).bend - 1.0).abs() < 0.1, "{}", env(&l).bend);
        // Elsewhere on the graph a vertical drag still sets the source's amount.
        paint(&mut l);
        assert!(l.down(g.x + 5.0, g.bottom() - 40.0, Btn::Left, Mods::default(), false).contains(&Act::Lock));
        l.up(Btn::Left);
    }

    #[test]
    fn tempo_and_length_move_together() {
        let (mut l, p) = loaded();
        // 2 s at 4 beats, 120 BPM; 9 beats make 270 BPM; 60 BPM make 2 beats.
        l.typed(BEATS_ID, "9");
        assert_eq!((l.state().beats(), l.state().bpm()), (9.0, 270.0));
        l.typed(BPM_ID, "60 bpm");
        assert_eq!((l.state().beats(), l.state().bpm()), (2.0, 60.0));
        assert_eq!(l.value_text(BPM_ID), "60.000");
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn the_dump_is_offered_when_hosted() {
        let (mut l, p) = loaded();
        assert!(!l.menu(&MenuReq::Dump).items.iter().any(|i| matches!(i, MenuItem::Item { enabled: true, .. })));
        l.hosted = true;
        assert_eq!(l.menu(&MenuReq::Dump).find("Normal"), Some(Cmd::Dump(false)));
        let acts = l.pick(&MenuReq::Dump, Cmd::Dump(false), 0.0, 0.0);
        let Some(Act::Notes(n)) = acts.first() else { panic!("{acts:?}") };
        let got: Vec<(f64, f64, u8)> = n.iter().map(|n| (n.start, n.end, n.key)).collect();
        assert_eq!(got, [(0.0, 1.0, 60), (1.0, 2.0, 61), (2.0, 3.0, 62), (3.0, 4.0, 63)]);
        // Original length at the host's 60 BPM: the 2 s loop takes 2 beats.
        l.shared.tempo.store(60f64.to_bits(), Ordering::Relaxed);
        let Some(Act::Notes(n)) = l.pick(&MenuReq::Dump, Cmd::Dump(true), 0.0, 0.0).first().cloned() else { panic!() };
        assert_eq!(n.last().map(|n| n.end), Some(2.0));
        let _ = std::fs::remove_file(p);
    }

    /// `SLICER_PNG=<dir> cargo test -p lemon-slicex -- --ignored`: the editor, empty and with a loop, as PNGs.
    #[test]
    #[ignore]
    fn render_editor() {
        let Some(dir) = std::env::var_os("SLICER_PNG") else { return };
        let save = |l: &mut Logic, name: &str, scale: f32| {
            let (w, h) = ((view::W as f32 * scale) as u32, (view::H as f32 * scale) as u32);
            let mut c = os_ui::Canvas::new(w, h, scale).unwrap();
            l.paint(&mut c, &os_plugkit::palette());
            let mut px = vec![0u32; (w * h) as usize];
            c.copy_to_xrgb(&mut px);
            // The window shows the top rows of the canvas.
            let wh = (l.size().1 as f32 * scale) as u32;
            px.truncate((w * wh) as usize);
            let mut pm = tiny_skia::Pixmap::new(w, wh).unwrap();
            for (o, p) in pm.data_mut().chunks_mut(4).zip(&px) {
                o.copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]);
            }
            pm.save_png(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
        };
        let mut l = logic();
        save(&mut l, "slicer_empty", 1.0);
        let path = std::env::var("SLICER_LOOP").ok();
        let p = loop_file("render", &[]);
        l.load(path.as_deref().unwrap_or(&p.to_string_lossy()));
        l.view.chosen = 1;
        l.hint = "Slice 2\tC#5".into();
        save(&mut l, "slicer_loop", 1.0);
        save(&mut l, "slicer_loop_x2", 2.0);
        for (name, view, keys) in [("no_editor", 1u8, true), ("no_regions", 2, true), ("compact", 3, true), ("no_keys", 0, false)] {
            l.edit_ext(|e| {
                e.view = view;
                e.keys = keys;
            });
            save(&mut l, &format!("slicex_{name}"), 1.0);
        }
        let _ = std::fs::remove_file(p);
    }
}
