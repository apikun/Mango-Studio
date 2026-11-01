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
use os_engine::sample::Sample;
use os_hostlink::NoteOut;
use os_plugkit::vals::Vals;
use os_plugkit::MenuDef;
use os_ui::kit::menu::{Mark, MenuItem, MenuStyle};

use crate::edit::{self, Clip, Range, Shift};
use crate::ext::{Ext, Source};
use crate::extctl::{self, Ctl, Sel};
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
    /// The region the region settings show.
    Choose(usize),
    /// A sample edit, a selection change, a zoom, and the wave editor's switches.
    Edit(Ed),
    SaveAs(SaveKind),
    Sel(SelOp),
    Zoom(ZoomOp),
    SetSnap(u8),
    ToggleRegions,
    ToggleClickFree,
    /// Add or remove a marker at the selection's start; add the selection as a region.
    Marker,
    AddRegion,
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
    /// The region list of the region settings.
    Regions,
    /// The Run script button's menu: the region presets.
    Script,
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

/// The sample edits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ed {
    Undo,
    Normalize,
    NormalizeEach,
    FadeIn,
    FadeOut,
    DeclickIn,
    DeclickOut,
    Reverse,
    Polarity,
    Center,
    Swap,
    Mono(Option<bool>),
    Stereo,
    Clear,
    Delete,
    Trim,
    InsertSilence,
    Cut,
    Copy,
    PasteInsert,
    PasteReplace,
    PasteMix,
    Noise,
    DrumStretch,
    /// Every region on its own: normalized, declicked in, declicked out, or all the declicks and the level.
    AllNormalize,
    AllDeclickIn,
    AllDeclickOut,
    AllPerfect,
    /// The loop as an audio clip in the host's playlist.
    Send,
}

/// What a selection change does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelOp {
    Prev,
    Next,
    Deselect,
    Before,
    After,
    Visible,
}

/// What a zoom does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoomOp {
    In,
    Out,
    Full,
    Selection,
}

/// A press on the wave that is still going: it may become a selection or move a marker.
#[derive(Clone, Copy, Debug, PartialEq)]
enum WaveDrag {
    /// Nothing moved yet: a click that plays the slice; moving more than a few px makes it a selection.
    Pending { x0: f32, frame0: usize },
    Select { frame0: usize },
    /// Marker (slice start) `0` follows the pointer.
    Marker(usize),
}

/// What the save dialog saves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SaveKind {
    /// The whole loop as one file.
    Sample,
    /// Each slice as a file of its own.
    Regions,
}

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
    /// The save dialog, and what a file it picks gets.
    Save(SaveKind),
    /// This audio file goes to the host's playlist as an audio clip.
    Send(String),
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
    peaks: (u64, (f64, f64), Vec<(f32, f32)>),
    /// A knob of the added panels being dragged (its control and position), and whether the pad is.
    ext_drag: Option<(Ctl, f32)>,
    pad_drag: bool,
    /// The deck the open dialog's file goes to (0 = A, 1 = B).
    load_deck: u8,
    /// A point of the envelope graph being dragged.
    graph_drag: Option<GraphPt>,
    /// A press on the wave that is going on.
    wave_drag: Option<WaveDrag>,
    /// Earlier states of the loop (its audio and its saved state), for Undo.
    history: Vec<(Arc<Sample>, State)>,
    /// Cut or copied audio.
    clip: Option<Clip>,
    /// The file the loop's edits are kept in (a new one at the first edit).
    edit_file: Option<std::path::PathBuf>,
    /// The frame last clicked in the wave.
    last_frame: Option<usize>,
    /// The loop button: a played slice plays again when it ends.
    loop_on: bool,
    /// What the last edit or action said.
    pub said: String,
    /// The host's side of the window, for a size change.
    pub ctx: Option<Arc<dyn GuiContext>>,
}

impl Logic {
    pub fn new(params: Arc<SlicexParams>, shared: Arc<Shared>, ctx: Option<Arc<dyn GuiContext>>) -> Self {
        let vals = Vals::new(&*params, params.live.clone(), ctx.clone());
        Logic { vals, params, shared, view: ViewState::default(), hits: Vec::new(), drag: None, hint: String::new(), copied: None, hosted: false, playing: None, peaks: (u64::MAX, (0.0, 1.0), Vec::new()), ext_drag: None, pad_drag: false, load_deck: 0, graph_drag: None, wave_drag: None, history: Vec::new(), clip: None, edit_file: None, last_frame: None, loop_on: false, said: String::new(), ctx }
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
        self.view.sel.slice = self.view.chosen;
        let sel = self.view.sel;
        // The region settings' two small boxes: a click takes the next value, a right click the one before (or the slice's
        // own articulator).
        if let Ctl::RegArt(row) = c {
            let (i, row) = (self.view.chosen, usize::from(row));
            let cur = self.params.ext().art_for(i, row);
            match btn {
                Btn::Left => self.edit_ext(|e| e.region_mut(i).art[row] = ((cur + 1) % crate::ext::ARTICS) as u8),
                Btn::Right => self.edit_ext(|e| e.region_mut(i).art[row] = crate::ext::ART_OWN),
                Btn::Middle => {}
            }
            return vec![Act::Repaint];
        }
        if c == Ctl::RegCut {
            let i = self.view.chosen;
            let d = match btn {
                Btn::Left => 1,
                Btn::Right => 8,
                Btn::Middle => 0,
            };
            self.edit_ext(|e| {
                let r = e.region_mut(i);
                r.cut = if btn == Btn::Middle { 0 } else { (r.cut + d) % 9 };
            });
            return vec![Act::Repaint];
        }
        if c == Ctl::RegPick {
            return vec![Act::Menu(MenuReq::Regions, x, y)];
        }
        if btn == Btn::Right {
            if let Ctl::Edit(i) = c {
                return self.edit_button(i, btn, x, y);
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
            Ctl::Edit(i) => return self.edit_button(i, btn, x, y),
            Ctl::Options => return vec![Act::Menu(MenuReq::Options, x, y)],
            Ctl::Edit(7) => {
                self.slice(Slicing::Auto(1));
                return vec![Act::Repaint];
            }
            Ctl::Edit(8) => return self.dump(false).map(Act::Notes).into_iter().collect(),
            Ctl::Row(0) => return vec![Act::Menu(MenuReq::Toolbar(8), x, y)],
            Ctl::Row(1) => self.run_zoom(ZoomOp::Full),
            Ctl::Row(2) => self.toggle_marker(),
            Ctl::Row(3) => self.run_sel(SelOp::Prev),
            Ctl::Row(4) => self.run_sel(SelOp::Next),
            Ctl::Row(5) => self.edit_ext(|e| e.show_regions = !e.show_regions),
            Ctl::Row(6) => self.edit_ext(|e| e.freeze = !e.freeze),
            Ctl::Row(7) => self.edit_ext(|e| e.snap = (e.snap + 1) % 3),
            Ctl::Row(8) => self.edit_ext(|e| e.slide = !e.slide),
            Ctl::Info(_) => self.edit_ext(|e| e.click_free = !e.click_free),
            Ctl::Scroll(d) => self.scroll(d),
            Ctl::Loop => self.loop_on = !self.loop_on,
            Ctl::Edit(_) | Ctl::Row(_) => {}
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

    /// The sample edit buttons: a click does the edit; a right click the other one (or the menu).
    fn edit_button(&mut self, i: u8, btn: Btn, x: f32, y: f32) -> Vec<Act> {
        let right = btn == Btn::Right;
        // Undo works on what was done; the rest need a loop.
        if self.frames() == 0 && i != 0 {
            self.say("Load a loop first: drag one here, or File > Load sample");
            return vec![Act::Repaint];
        }
        if i == 8 && !self.hosted && !right {
            self.say("Dump score sends the slices to the piano roll of Mango Studio");
            return vec![Act::Repaint];
        }
        match (i, right) {
            (0, _) => self.run_edit(Ed::Undo),
            (1, false) => self.run_edit(Ed::Normalize),
            (1, true) => self.run_edit(Ed::NormalizeEach),
            (2, false) => self.run_edit(Ed::FadeIn),
            (2, true) => self.run_edit(Ed::DeclickIn),
            (3, false) => self.run_edit(Ed::FadeOut),
            (3, true) => self.run_edit(Ed::DeclickOut),
            (4, _) => vec![Act::Menu(MenuReq::Script, x, y)],
            (5, _) => self.run_edit(Ed::DrumStretch),
            (6, false) => {
                self.toggle_marker();
                vec![Act::Repaint]
            }
            (6, true) => {
                self.add_region();
                vec![Act::Repaint]
            }
            (7, false) => {
                self.slice(Slicing::Auto(1));
                vec![Act::Repaint]
            }
            (7, true) if self.frames() > 0 => vec![Act::Menu(MenuReq::Slicing, x, y)],
            (8, false) => self.dump(false).map(Act::Notes).into_iter().collect(),
            (8, true) => vec![Act::Menu(MenuReq::Dump, x, y)],
            (9, _) => vec![Act::Save(SaveKind::Sample)],
            (10, _) => self.run_edit(Ed::Send),
            _ => Vec::new(),
        }
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
            Ctl::RegKnob(r, w) => format!("{}\t{}", c.caption(), extctl::text(&x, &Sel { slice: self.view.chosen, ..sel }, Ctl::RegKnob(r, w))),
            Ctl::RegArt(r) => format!("Articulator of {}\tclick: next, right-click: the slice's own", ["AMP", "FILTER", "SPEED", "START"][usize::from(r.min(3))]),
            Ctl::RegCut => "Cut group\tslices of one group stop each other".into(),
            Ctl::RegPick => "Choose the region".into(),
            Ctl::RandLvl | Ctl::LfoLvl => format!("{}\t{}", c.caption(), extctl::text(&x, &sel, c)),
            Ctl::LoadDeck(n) => format!("Load a loop into deck {}", if n == 0 { 'A' } else { 'B' }),
            Ctl::Key(k) => format!("Key {}", view::key_name(i32::from(k))),
            Ctl::PlaySlice => "Play the chosen slice".into(),
            Ctl::Stop => "Stop".into(),
            Ctl::DeckToggle(d) => format!("Deck {} sounds\t{}", if d == 0 { 'A' } else { 'B' }, tab(if d == 0 { x.a_on } else { x.b_on })),
            Ctl::View(n) => format!("View\t{}", ["everything", "without the audio editor", "without the region settings", "master panel only"][usize::from(n).min(3)]),
            Ctl::Keys => format!("Keyboard\t{}", if x.keys { "shown" } else { "hidden" }),
            Ctl::Menu(i) => format!("{} menu", view::MENUS[usize::from(i).min(8)]),
            Ctl::Edit(i) => match i {
                0 => "Undo".into(),
                1 => "Normalize\tright-click: each region".into(),
                2 => "Fade in\tright-click: declick in".into(),
                3 => "Fade out\tright-click: declick out".into(),
                4 => "Run script\tthe region presets".into(),
                5 => "Drum (loop) stretch\tto the project's tempo".into(),
                6 => "Add or remove a marker\tright-click: add the selection as a region".into(),
                7 => "Auto-slice\tmedium; right-click for more".into(),
                8 => "Dump score\tto the piano roll; right-click for more".into(),
                9 => "Save the loop as a wave file".into(),
                _ => "Send the loop to the playlist\tas an audio clip".into(),
            },
            Ctl::Info(_) => format!("Click-free editing\t{}", tab(x.click_free)),
            Ctl::Scroll(d) => format!("Scroll {}", if d < 0 { "left" } else { "right" }),
            Ctl::Loop => format!("Loop\t{}", tab(self.loop_on)),
            Ctl::Row(i) => ["Zoom menu", "Whole loop in view", "Add or remove a marker", "Previous region", "Next region", "Show the regions", "Freeze the loop", "Snap", "Slide: a moved marker takes the later ones"][usize::from(i).min(8)].to_string(),
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
                let i = self.view.mouse.map_or(0, |(x, _)| slice_at(&st, view::frame_at(x, self.frames(), self.view.zoom)));
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
                let (frames, zoom) = (self.frames(), self.view.zoom);
                let frame = view::frame_at(x, frames, zoom);
                self.last_frame = Some(frame);
                self.view.pressed = Some(Target::Well);
                // A press on a slice's start takes the marker (unless the loop is frozen).
                let ext = self.params.ext();
                let near = st.slices.iter().position(|s| ext.show_regions && (view::x_of(s.start, frames, zoom) - x).abs() <= 4.0);
                if let Some(i) = near.filter(|_| !ext.freeze) {
                    self.view.chosen = i;
                    self.snapshot();
                    self.wave_drag = Some(WaveDrag::Marker(i));
                } else {
                    let i = slice_at(&st, frame);
                    if i < st.slices.len() {
                        self.view.chosen = i;
                        let key = u8::try_from(st.key_of(i)).ok().filter(|&k| k < 128);
                        self.play(key);
                    }
                    self.wave_drag = Some(WaveDrag::Pending { x0: x, frame0: frame });
                }
            }
            (Target::Well, Btn::Right) if has => {
                let st = self.state();
                let i = slice_at(&st, view::frame_at(x, self.frames(), self.view.zoom));
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
        // The play button keeps its slice going (it ends by itself, or again and again with the loop button on).
        if self.view.pressed.as_ref() != Some(&Target::Ext(Ctl::PlaySlice)) {
            self.play(None);
        }
        self.ext_drag = None;
        self.pad_drag = false;
        self.graph_drag = None;
        if let Some(WaveDrag::Select { .. }) = self.wave_drag.take() {
            if self.view.range.is_some_and(|(a, b)| a == b) {
                self.view.range = None;
            }
        }
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
        if let Some(d) = self.wave_drag {
            self.wave_to(d, x);
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
            Target::Well if self.frames() > 0 => {
                // The wheel zooms at the pointer; with shift it scrolls.
                if m.shift {
                    self.scroll(-d as i8);
                } else {
                    self.zoom_at(x, if d > 0.0 { 0.7 } else { 1.0 / 0.7 });
                }
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
            MenuReq::Regions => {
                let st = self.state();
                let chosen = self.chosen(&st);
                m.head("Regions", Cmd::None);
                for (i, s) in st.slices.iter().enumerate() {
                    let name = if s.name.is_empty() { format!("Marker #{}", i + 1) } else { s.name() };
                    m.add(MenuItem::item(&name).mark(Mark::Radio(i == chosen)), Cmd::Choose(i));
                }
            }
            MenuReq::Script => {
                let has = self.frames() > 0 && !self.state().slices.is_empty();
                m.head("Run script", Cmd::None);
                for (n, e) in [("Normalize all regions", Ed::AllNormalize), ("Declick in all regions", Ed::AllDeclickIn), ("Declick out all regions", Ed::AllDeclickOut), ("Perfect all regions", Ed::AllPerfect)] {
                    m.add(MenuItem::item(n).enabled(has), Cmd::Edit(e));
                }
            }
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

    /// The toolbar menu `i`: what each does, on the selection (the whole loop with none).
    fn toolbar_menu(&self, m: &mut MenuDef<Cmd>, i: usize) {
        let st = self.state();
        let ext = self.params.ext();
        let frames = self.frames();
        let has = frames > 0;
        let slices = has && !st.slices.is_empty();
        let sel = self.view.range.is_some_and(|(a, b)| a != b);
        let item = |m: &mut MenuDef<Cmd>, name: &str, on: bool, c: Cmd| m.add(MenuItem::item(name).enabled(on), c);
        let sub = |m: &mut MenuDef<Cmd>, name: &str, items: &[(&str, bool, Cmd)]| {
            let mut s = MenuDef::new(MenuStyle::Context);
            for (n, on, c) in items {
                s.add(MenuItem::item(n).enabled(*on), c.clone());
            }
            m.submenu(name, s, Cmd::None);
        };
        match i {
            0 => {
                item(m, "New", has, Cmd::NewLoop);
                item(m, "Load sample A...", true, Cmd::LoadDeck(0));
                item(m, "Load sample B...", true, Cmd::LoadDeck(1));
                m.sep(Cmd::None);
                item(m, "Save sample as...", has, Cmd::SaveAs(SaveKind::Sample));
                item(m, "Export regions...", slices, Cmd::SaveAs(SaveKind::Regions));
                if self.hosted {
                    m.sep(Cmd::None);
                    item(m, "Send to the playlist as an audio clip", has, Cmd::Edit(Ed::Send));
                }
            }
            1 => {
                item(m, "Mono (the two channels mixed)", has, Cmd::Edit(Ed::Mono(None)));
                item(m, "Stereo (the left channel on both)", has, Cmd::Edit(Ed::Stereo));
            }
            2 => {
                item(m, "Undo", !self.history.is_empty(), Cmd::Edit(Ed::Undo));
                m.sep(Cmd::None);
                item(m, "Cut", sel, Cmd::Edit(Ed::Cut));
                item(m, "Copy", sel, Cmd::Edit(Ed::Copy));
                item(m, "Paste insert", self.clip.is_some(), Cmd::Edit(Ed::PasteInsert));
                item(m, "Paste replace", self.clip.is_some(), Cmd::Edit(Ed::PasteReplace));
                item(m, "Paste mix", self.clip.is_some(), Cmd::Edit(Ed::PasteMix));
                m.sep(Cmd::None);
                item(m, "Clear", has, Cmd::Edit(Ed::Clear));
                item(m, "Delete", sel, Cmd::Edit(Ed::Delete));
                item(m, "Trim", sel, Cmd::Edit(Ed::Trim));
                item(m, "Insert silence", has, Cmd::Edit(Ed::InsertSilence));
                m.sep(Cmd::None);
                m.add(MenuItem::item("Click-free (smooth) editing").mark(Mark::Check(ext.click_free)), Cmd::ToggleClickFree);
            }
            3 => {
                sub(
                    m,
                    "Amp",
                    &[
                        ("Normalize", has, Cmd::Edit(Ed::Normalize)),
                        ("Fade in", has, Cmd::Edit(Ed::FadeIn)),
                        ("Fade out", has, Cmd::Edit(Ed::FadeOut)),
                        ("Declick in", has, Cmd::Edit(Ed::DeclickIn)),
                        ("Declick out", has, Cmd::Edit(Ed::DeclickOut)),
                        ("Reverse polarity", has, Cmd::Edit(Ed::Polarity)),
                    ],
                );
                sub(m, "Waveform", &[("Center", has, Cmd::Edit(Ed::Center))]);
                sub(m, "Time", &[("Reverse", has, Cmd::Edit(Ed::Reverse)), ("Drum (loop) stretch", has, Cmd::Edit(Ed::DrumStretch))]);
                sub(
                    m,
                    "Channels",
                    &[
                        ("Swap channels", has, Cmd::Edit(Ed::Swap)),
                        ("Convert left to mono", has, Cmd::Edit(Ed::Mono(Some(true)))),
                        ("Convert right to mono", has, Cmd::Edit(Ed::Mono(Some(false)))),
                        ("Mix to mono", has, Cmd::Edit(Ed::Mono(None))),
                    ],
                );
                sub(m, "Synthesis", &[("Generate noise", has, Cmd::Edit(Ed::Noise))]);
                sub(
                    m,
                    "Regions",
                    &[
                        ("Normalize all regions", slices, Cmd::Edit(Ed::AllNormalize)),
                        ("Declick in all regions", slices, Cmd::Edit(Ed::AllDeclickIn)),
                        ("Declick out all regions", slices, Cmd::Edit(Ed::AllDeclickOut)),
                        ("Perfect all regions", slices, Cmd::Edit(Ed::AllPerfect)),
                    ],
                );
            }
            4 => {
                item(m, "Add region", sel, Cmd::AddRegion);
                item(m, "Add marker (or take the one there away)", has, Cmd::Marker);
                item(m, "Delete", slices && st.slices.len() > 1, Cmd::DeleteSlice);
                m.sep(Cmd::None);
                item(m, "Assign trigger notes to all", slices, Cmd::DefaultKeys);
                m.sep(Cmd::None);
                sub(
                    m,
                    "Auto slice",
                    &[("Dull (few slices)", has, Cmd::Slice(Slicing::Auto(0))), ("Medium", has, Cmd::Slice(Slicing::Auto(1))), ("Sharp (many slices)", has, Cmd::Slice(Slicing::Auto(2)))],
                );
                sub(m, "Grid slicing", &[("Small grid", has, Cmd::Slice(Slicing::Beats(4))), ("Medium grid", has, Cmd::Slice(Slicing::Beats(2))), ("Large grid", has, Cmd::Slice(Slicing::Beats(1)))]);
                item(m, "Detect beats", has, Cmd::Slice(Slicing::Beats(1)));
                item(m, "Use the sample's own markers", st.path().to_lowercase().ends_with(".wav"), Cmd::Slice(Slicing::BuiltIn));
                item(m, "No slicing", has, Cmd::Slice(Slicing::None));
                m.sep(Cmd::None);
                item(m, "Zero-cross check all regions", slices, Cmd::Slice(Slicing::ZeroCross));
            }
            5 => {
                m.add(MenuItem::item("Regions").mark(Mark::Check(ext.show_regions)), Cmd::ToggleRegions);
            }
            6 => {
                for (n, name) in crate::ext::SNAPS.iter().enumerate() {
                    m.add(MenuItem::item(name).mark(Mark::Radio(usize::from(ext.snap) == n)), Cmd::SetSnap(n as u8));
                }
            }
            7 => {
                item(m, "Select previous region", slices, Cmd::Sel(SelOp::Prev));
                item(m, "Select next region", slices, Cmd::Sel(SelOp::Next));
                m.sep(Cmd::None);
                item(m, "Deselect", sel, Cmd::Sel(SelOp::Deselect));
                item(m, "Select before", has, Cmd::Sel(SelOp::Before));
                item(m, "Select after", has, Cmd::Sel(SelOp::After));
                item(m, "Select zoomed part", has, Cmd::Sel(SelOp::Visible));
            }
            _ => {
                item(m, "Zoom in 2x", has, Cmd::Zoom(ZoomOp::In));
                item(m, "Zoom out 2x", has, Cmd::Zoom(ZoomOp::Out));
                item(m, "Zoom out full", has, Cmd::Zoom(ZoomOp::Full));
                item(m, "Zoom on selection", sel, Cmd::Zoom(ZoomOp::Selection));
            }
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
                self.view.range = None;
                self.view.zoom = (0.0, 1.0);
                self.history.clear();
                self.edit_file = None;
                self.commit(&st);
            }
            Cmd::LoadDeck(n) => {
                self.load_deck = n.min(1);
                return vec![Act::OpenDialog];
            }
            Cmd::DeleteSlice => {
                self.snapshot();
                let mut st = self.state();
                let i = self.chosen(&st);
                if i < st.slices.len() && st.slices.len() > 1 {
                    st.slices.remove(i);
                    self.commit(&st);
                }
            }
            Cmd::Choose(i) => self.view.chosen = i.min(self.state().slices.len().saturating_sub(1)),
            Cmd::Edit(e) => return self.run_edit(e),
            Cmd::SaveAs(k) => return vec![Act::Save(k)],
            Cmd::Sel(op) => self.run_sel(op),
            Cmd::Zoom(op) => self.run_zoom(op),
            Cmd::SetSnap(n) => self.edit_ext(|e| e.snap = n.min(2)),
            Cmd::ToggleRegions => self.edit_ext(|e| e.show_regions = !e.show_regions),
            Cmd::ToggleClickFree => self.edit_ext(|e| e.click_free = !e.click_free),
            Cmd::Marker => self.toggle_marker(),
            Cmd::AddRegion => self.add_region(),
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
                self.snapshot();
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
        if self.locked() {
            return;
        }
        self.snapshot();
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

    // ------------------------------------------------------------------------------------------------------------
    // The sample editor

    fn say(&mut self, s: &str) {
        self.said = s.to_string();
        self.hint = s.to_string();
    }

    fn locked(&mut self) -> bool {
        let f = self.params.ext().freeze;
        if f {
            self.say("The loop is frozen: switch Freeze off to edit it");
        }
        f
    }

    /// The state now and the audio, put on the history (Undo takes them back).
    fn snapshot(&mut self) {
        if let Some(s) = self.shared.sample() {
            let st = self.state();
            self.history.push((s, st));
            if self.history.len() > 20 {
                self.history.remove(0);
            }
        }
    }

    /// The loop's audio is `s` and its saved state `st` from now on: the audio goes to its edit file.
    fn put_audio(&mut self, s: Sample, mut st: State) {
        let dir = edit_dir();
        let _ = std::fs::create_dir_all(&dir);
        let file = self
            .edit_file
            .get_or_insert_with(|| {
                let path = st.path().replace('\\', "/");
                let name = path.rsplit('/').next().unwrap_or("loop");
                let stem = name.rsplit_once('.').map_or(name, |(a, _)| a);
                let stem = stem.split("-edit-").next().unwrap_or(stem);
                let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
                dir.join(format!("{stem}-edit-{n:x}.wav"))
            })
            .clone();
        if std::fs::write(&file, edit::wav_bytes(&s)).is_err() {
            self.say("The edit could not be kept in a file");
        }
        let path = file.to_string_lossy().into_owned();
        st.set_path(&path);
        if let Ok(mut p) = self.params.sample_path.write() {
            *p = path.clone();
        }
        self.shared.set_sample(Some(Arc::new(s)), &path);
        self.commit(&st);
    }

    /// An edit of the loop and its slices: `f` changes the audio and the state; the old ones go on the history.
    fn edit_custom(&mut self, name: &str, f: impl FnOnce(&Sample, &mut State) -> Sample) -> bool {
        if self.locked() {
            return false;
        }
        let Some(s) = self.shared.sample() else { return false };
        self.snapshot();
        let mut st = self.state();
        let new = f(&s, &mut st);
        self.put_audio(new, st);
        self.say(name);
        true
    }

    /// An edit of the selection (the whole loop with none): `f` gives the new audio and how the slice starts move.
    fn edit_with(&mut self, name: &str, f: impl FnOnce(&Sample, Range) -> (Sample, Shift)) -> bool {
        let range = self.view.range;
        let ok = self.edit_custom(name, |s, st| {
            let r = edit::range_or_all(s, range);
            let (new, shift) = f(s, r);
            let last = (new.frames() as u32).saturating_sub(1);
            for sl in &mut st.slices {
                sl.start = shift.map(sl.start).min(last);
            }
            st.slices.dedup_by_key(|sl| sl.start);
            new
        });
        if ok && self.shared.sample().is_some_and(|s| Some(s.frames()) != self.view.range.map(|r| r.1.max(r.0))) {
            // A change of length leaves the old frame numbers meaningless.
            let len = self.frames();
            if self.view.range.is_some_and(|(a, b)| a.max(b) > len) {
                self.view.range = None;
            }
        }
        ok
    }

    fn edit_same(&mut self, name: &str, f: impl FnOnce(&Sample, Range) -> Sample) -> bool {
        self.edit_with(name, |s, r| (f(s, r), Shift::None))
    }

    /// The slices as frame ranges.
    fn regions(st: &State, frames: usize) -> Vec<Range> {
        (0..st.slices.len()).map(|i| ((st.slices[i].start as usize).min(frames), st.slices.get(i + 1).map_or(frames, |s| (s.start as usize).min(frames)))).filter(|(a, b)| b > a).collect()
    }

    /// Where a paste or a marker goes: the selection's start, else the frame last clicked.
    fn at(&self) -> usize {
        self.view.range.map(|(a, b)| a.min(b)).or(self.last_frame).unwrap_or(0)
    }

    pub fn run_edit(&mut self, e: Ed) -> Vec<Act> {
        let click = self.params.ext().click_free;
        match e {
            Ed::Undo => match self.history.pop() {
                Some((s, st)) => {
                    self.view.range = None;
                    self.put_audio_plain(s, st);
                    self.say("Undone");
                }
                None => self.say("Nothing to undo"),
            },
            Ed::Normalize => {
                self.edit_same("Normalize", edit::normalize);
            }
            Ed::NormalizeEach => {
                let frames = self.frames();
                let regions = Self::regions(&self.state(), frames);
                self.edit_custom("Normalize regions", |s, _| edit::normalize_each(s, &regions));
            }
            Ed::FadeIn => {
                self.edit_same("Fade in", edit::fade_in);
            }
            Ed::FadeOut => {
                self.edit_same("Fade out", edit::fade_out);
            }
            Ed::DeclickIn => {
                self.edit_same("Declick in", edit::declick_in);
            }
            Ed::DeclickOut => {
                self.edit_same("Declick out", edit::declick_out);
            }
            Ed::Reverse => {
                self.edit_same("Reverse", edit::reverse);
            }
            Ed::Polarity => {
                self.edit_same("Reverse polarity", edit::reverse_polarity);
            }
            Ed::Center => {
                self.edit_same("Center", edit::center);
            }
            Ed::Swap => {
                self.edit_same("Swap channels", edit::swap_channels);
            }
            Ed::Mono(from) => {
                self.edit_same("Mono", |s, r| edit::to_mono(s, r, from));
            }
            Ed::Stereo => {
                self.edit_same("Stereo", |s, r| edit::to_mono(s, r, Some(true)));
            }
            Ed::Clear => {
                self.edit_same("Clear", edit::clear);
            }
            Ed::Noise => {
                self.edit_same("Noise", edit::noise);
            }
            Ed::Delete => {
                if self.view.range.is_none() {
                    self.say("Select what to delete first");
                } else {
                    self.edit_with("Delete", |s, r| {
                        let (o, sh) = edit::delete(s, r);
                        (if click { edit::smooth_join(&o, r.0) } else { o }, sh)
                    });
                }
            }
            Ed::Trim => {
                if self.view.range.is_none() {
                    self.say("Select what to keep first");
                } else {
                    self.edit_with("Trim", edit::trim);
                }
            }
            Ed::InsertSilence => {
                let n = self.view.range.map_or(self.shared.sample().map_or(100, |s| s.rate as usize / 10), |(a, b)| a.max(b) - a.min(b));
                let at = self.at();
                self.edit_with("Insert silence", |s, _| edit::insert_silence(s, at, n));
            }
            Ed::Copy => match (self.shared.sample(), self.view.range.filter(|(a, b)| a != b)) {
                (Some(s), Some(r)) => {
                    self.clip = Some(edit::copy(&s, edit::range_or_all(&s, Some(r))));
                    self.say("Copied");
                }
                _ => self.say("Select what to copy first"),
            },
            Ed::Cut => match (self.shared.sample(), self.view.range.filter(|(a, b)| a != b)) {
                (Some(s), Some(r)) => {
                    let r = edit::range_or_all(&s, Some(r));
                    self.clip = Some(edit::copy(&s, r));
                    self.edit_with("Cut", |s, r| {
                        let (o, sh) = edit::delete(s, r);
                        (if click { edit::smooth_join(&o, r.0) } else { o }, sh)
                    });
                }
                _ => self.say("Select what to cut first"),
            },
            Ed::PasteInsert | Ed::PasteReplace | Ed::PasteMix => {
                let Some(c) = self.clip.clone() else {
                    self.say("Nothing copied yet");
                    return vec![Act::Repaint];
                };
                let at = self.at();
                match e {
                    Ed::PasteInsert => self.edit_with("Paste insert", |s, _| {
                        let (o, sh) = edit::paste_insert(s, at, &c);
                        (if click { edit::smooth_join(&edit::smooth_join(&o, at), at + c.left.len()) } else { o }, sh)
                    }),
                    Ed::PasteReplace => self.edit_same("Paste replace", |s, _| edit::paste_replace(s, at, &c)),
                    _ => self.edit_same("Paste mix", |s, _| edit::paste_mix(s, at, &c)),
                };
            }
            Ed::DrumStretch => {
                let Some(tempo) = self.shared.tempo() else {
                    self.say("Play the project once so Slicex knows its tempo");
                    return vec![Act::Repaint];
                };
                let bpm = self.state().bpm();
                if bpm <= 0.0 || (bpm / tempo - 1.0).abs() < 1e-3 {
                    self.say("The loop is at the project's tempo already");
                    return vec![Act::Repaint];
                }
                let factor = bpm / tempo;
                self.edit_custom("Drum (loop) stretch", |s, st| {
                    let starts: Vec<u32> = st.slices.iter().map(|x| x.start).collect();
                    let (o, new) = edit::drum_stretch(s, &starts, factor);
                    for (sl, n) in st.slices.iter_mut().zip(new) {
                        sl.start = n;
                    }
                    st.set_f32(H_BPM, tempo as f32);
                    o
                });
            }
            Ed::AllNormalize | Ed::AllDeclickIn | Ed::AllDeclickOut | Ed::AllPerfect => {
                let frames = self.frames();
                let regions = Self::regions(&self.state(), frames);
                let name = match e {
                    Ed::AllNormalize => "Normalize all regions",
                    Ed::AllDeclickIn => "Declick in all regions",
                    Ed::AllDeclickOut => "Declick out all regions",
                    _ => "Perfect all regions",
                };
                self.edit_custom(name, |s, _| {
                    regions.iter().fold(s.clone(), |acc, &r| match e {
                        Ed::AllNormalize => edit::normalize(&acc, r),
                        Ed::AllDeclickIn => edit::declick_in(&acc, r),
                        Ed::AllDeclickOut => edit::declick_out(&acc, r),
                        _ => edit::normalize(&edit::declick_out(&edit::declick_in(&acc, r), r), r),
                    })
                });
            }
            Ed::Send => {
                let Some(s) = self.shared.sample() else { return vec![] };
                let dir = edit_dir();
                let _ = std::fs::create_dir_all(&dir);
                let file = dir.join("sent.wav");
                if std::fs::write(&file, edit::wav_bytes(&s)).is_ok() {
                    return vec![Act::Send(file.to_string_lossy().into_owned())];
                }
                self.say("The loop could not be written for the playlist");
            }
        }
        vec![Act::Repaint]
    }

    /// `put_audio` for Undo: no new history, no new file name.
    fn put_audio_plain(&mut self, s: Arc<Sample>, st: State) {
        self.put_audio((*s).clone(), st);
    }

    /// What the playlist hear about: said once the host answered.
    pub fn sent(&mut self, ok: bool) {
        self.say(if ok { "Sent to the playlist" } else { "Only Mango Studio takes the loop as an audio clip" });
    }

    /// The name the save dialog starts with.
    pub fn save_name(&self, kind: SaveKind) -> String {
        let path = self.state().path().replace('\\', "/");
        let name = path.rsplit('/').next().unwrap_or("loop");
        let stem = name.rsplit_once('.').map_or(name, |(a, _)| a);
        let stem = stem.split("-edit-").next().unwrap_or(stem);
        match kind {
            SaveKind::Sample => format!("{stem}.wav"),
            SaveKind::Regions => format!("{stem}.wav"),
        }
    }

    /// Writes the loop (or each region, as `name-01.wav`...) to the file the save dialog picked.
    pub fn save_to(&mut self, kind: SaveKind, path: &str) {
        let Some(s) = self.shared.sample() else { return };
        match kind {
            SaveKind::Sample => {
                let ok = std::fs::write(path, edit::wav_bytes(&s)).is_ok();
                self.say(if ok { "Saved" } else { "The file could not be written" });
            }
            SaveKind::Regions => {
                let st = self.state();
                let base = path.strip_suffix(".wav").unwrap_or(path).to_string();
                let regions = Self::regions(&st, s.frames());
                let mut n = 0;
                for (i, &(a, b)) in regions.iter().enumerate() {
                    let piece = Sample { rate: s.rate, left: s.left[a..b].to_vec(), right: s.right[a..b].to_vec() };
                    if std::fs::write(format!("{base}-{:02}.wav", i + 1), edit::wav_bytes(&piece)).is_ok() {
                        n += 1;
                    }
                }
                self.say(&format!("Saved {n} regions"));
            }
        }
    }

    // -- the selection and the view of the wave

    /// A frame moved to what the snap setting wants: the nearest slice start, or zero crossing.
    fn snap(&self, f: usize) -> usize {
        let Some(s) = self.shared.sample() else { return f };
        match self.params.ext().snap {
            1 => {
                let st = self.state();
                let (z0, z1) = view::zoom_of(self.view.zoom);
                let reach = (((z1 - z0) * s.frames() as f64) * 8.0 / f64::from(view::WAVE.w)) as usize;
                st.slices.iter().map(|x| x.start as usize).filter(|&x| x.abs_diff(f) <= reach).min_by_key(|&x| x.abs_diff(f)).unwrap_or(f)
            }
            2 => {
                let reach = (s.rate / 200) as usize;
                let (a, b) = (f.saturating_sub(reach), (f + reach).min(s.frames().saturating_sub(1)));
                (a..b).filter(|&i| (s.left[i] <= 0.0) != (s.left[i + 1] <= 0.0)).min_by_key(|&i| i.abs_diff(f)).unwrap_or(f)
            }
            _ => f,
        }
    }

    /// The wave drag follows the pointer at x.
    fn wave_to(&mut self, d: WaveDrag, x: f32) {
        let (frames, zoom) = (self.frames(), self.view.zoom);
        let f = self.snap(view::frame_at(x, frames, zoom));
        match d {
            WaveDrag::Pending { x0, frame0 } => {
                if (x - x0).abs() > 3.0 {
                    self.play(None);
                    self.wave_drag = Some(WaveDrag::Select { frame0 });
                    self.view.range = Some((frame0.min(f), frame0.max(f)));
                }
            }
            WaveDrag::Select { frame0 } => self.view.range = Some((frame0.min(f), frame0.max(f))),
            WaveDrag::Marker(i) => {
                let mut st = self.state();
                let n = st.slices.len();
                let Some(old) = st.slices.get(i).map(|s| s.start) else { return };
                let new = if i == 0 { f.min(st.slices.get(1).map_or(frames, |s| s.start as usize).saturating_sub(1)) } else { f }.min(frames.saturating_sub(1)) as u32;
                if self.params.ext().slide {
                    // The later markers keep their distance.
                    let delta = i64::from(new) - i64::from(old);
                    let last = st.slices[n - 1].start as i64 + delta;
                    if last < frames as i64 && (i == 0 || i64::from(st.slices[i - 1].start) < i64::from(new)) {
                        for s in &mut st.slices[i..] {
                            s.start = (i64::from(s.start) + delta).max(0) as u32;
                        }
                    }
                } else {
                    let lo = if i == 0 { 0 } else { st.slices[i - 1].start + 1 };
                    let hi = st.slices.get(i + 1).map_or(frames as u32, |s| s.start).saturating_sub(1);
                    if lo <= hi {
                        st.slices[i].start = new.clamp(lo, hi);
                    }
                }
                self.commit(&st);
            }
        }
    }

    pub fn run_sel(&mut self, op: SelOp) {
        let frames = self.frames();
        let st = self.state();
        if frames == 0 {
            return;
        }
        let regions = Self::regions(&st, frames);
        let at = self.at();
        match op {
            SelOp::Deselect => self.view.range = None,
            SelOp::Before => self.view.range = Some((0, at)),
            SelOp::After => self.view.range = Some((at, frames)),
            SelOp::Visible => {
                let (z0, z1) = view::zoom_of(self.view.zoom);
                self.view.range = Some(((z0 * frames as f64) as usize, (z1 * frames as f64) as usize));
            }
            SelOp::Prev | SelOp::Next => {
                if regions.is_empty() {
                    return;
                }
                let n = regions.len() as i32;
                let i = (self.chosen(&st) as i32 + if op == SelOp::Next { 1 } else { -1 }).clamp(0, n - 1) as usize;
                self.view.chosen = i;
                self.view.range = Some(regions[i]);
            }
        }
    }

    pub fn run_zoom(&mut self, op: ZoomOp) {
        let frames = self.frames();
        if frames == 0 {
            return;
        }
        let (z0, z1) = view::zoom_of(self.view.zoom);
        let mid = (z0 + z1) / 2.0;
        self.view.zoom = match op {
            ZoomOp::Full => (0.0, 1.0),
            ZoomOp::In => self.zoom_span(mid, (z1 - z0) / 2.0),
            ZoomOp::Out => self.zoom_span(mid, (z1 - z0) * 2.0),
            ZoomOp::Selection => match self.view.range.filter(|(a, b)| a != b) {
                Some((a, b)) => (a.min(b) as f64 / frames as f64, a.max(b) as f64 / frames as f64),
                None => (z0, z1),
            },
        };
    }

    /// A view `span` wide (a fraction of the loop) centred on `mid`, kept inside the loop.
    fn zoom_span(&self, mid: f64, span: f64) -> (f64, f64) {
        let span = span.clamp(0.002, 1.0);
        let a = (mid - span / 2.0).clamp(0.0, 1.0 - span);
        (a, a + span)
    }

    /// Zooms by `k` (under 1: in) around the frame under x.
    pub fn zoom_at(&mut self, x: f32, k: f64) {
        let (z0, z1) = view::zoom_of(self.view.zoom);
        let f = f64::from(((x - view::WAVE.x) / view::WAVE.w).clamp(0.0, 1.0));
        let at = z0 + f * (z1 - z0);
        let span = ((z1 - z0) * k).clamp(0.002, 1.0);
        let a = (at - f * span).clamp(0.0, 1.0 - span);
        self.view.zoom = (a, a + span);
    }

    /// Scrolls the view by a tenth of what it shows; `dir` -1 is left.
    pub fn scroll(&mut self, dir: i8) {
        let (z0, z1) = view::zoom_of(self.view.zoom);
        let span = z1 - z0;
        let a = (z0 + f64::from(dir) * span / 4.0).clamp(0.0, 1.0 - span);
        self.view.zoom = (a, a + span);
    }

    /// Adds a marker at the selection's start (or the last click), or takes away the one that is there.
    pub fn toggle_marker(&mut self) {
        if self.locked() {
            return;
        }
        let frames = self.frames();
        if frames == 0 {
            return;
        }
        let at = self.at().min(frames - 1) as u32;
        let reach = (frames / 2000).max(1) as u32;
        self.snapshot();
        let mut st = self.state();
        if let Some(i) = st.slices.iter().position(|s| s.start.abs_diff(at) <= reach) {
            if st.slices.len() > 1 {
                st.slices.remove(i);
            }
        } else {
            let i = st.slices.iter().position(|s| s.start > at).unwrap_or(st.slices.len());
            st.slices.insert(i, crate::state::Slice::at(at));
            self.view.chosen = i;
        }
        self.commit(&st);
    }

    /// The selection as a region: a marker at each end.
    pub fn add_region(&mut self) {
        let Some((a, b)) = self.view.range.filter(|(a, b)| a != b) else {
            self.say("Select the region first");
            return;
        };
        if self.locked() {
            return;
        }
        self.snapshot();
        let mut st = self.state();
        for at in [a.min(b) as u32, a.max(b) as u32] {
            if at < self.frames() as u32 && !st.slices.iter().any(|s| s.start == at) {
                let i = st.slices.iter().position(|s| s.start > at).unwrap_or(st.slices.len());
                st.slices.insert(i, crate::state::Slice::at(at));
            }
        }
        self.commit(&st);
    }

    /// The loop button keeps a slice playing: called as the window ticks.
    pub fn tick(&mut self) {
        if self.loop_on
            && let Some(k) = self.playing
            && !self.shared.voices.load(Ordering::Relaxed)
        {
            self.play(Some(k));
        }
    }

    /// Loads a loop (the open dialog's file).
    pub fn load(&mut self, path: &str) -> Vec<Act> {
        let deck = std::mem::take(&mut self.load_deck);
        let r = if deck == 1 {
            self.shared.load_b(path, &self.params)
        } else {
            self.view.chosen = 0;
            self.view.range = None;
            self.view.zoom = (0.0, 1.0);
            self.history.clear();
            self.edit_file = None;
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
        let zoom = view::zoom_of(self.view.zoom);
        if self.peaks.0 != v || self.peaks.1 != zoom {
            let cols = view::WAVE.w as usize;
            self.peaks = (
                v,
                zoom,
                self.shared.sample().map_or_else(Vec::new, |s| {
                    let n = s.frames();
                    let (a, b) = (((zoom.0 * n as f64) as usize).min(n), ((zoom.1 * n as f64).ceil() as usize).min(n));
                    peaks(&s.left[a..b], &s.right[a..b], cols)
                }),
            );
        }
        &self.peaks.2
    }

    /// Paints the editor; `hint` is drawn when the host does not show it.
    pub fn paint(&mut self, c: &mut os_ui::Canvas, palette: &os_theme::Palette) {
        let st = self.state();
        let frames = self.frames();
        let can_dump = self.hosted && frames > 0 && !st.slices.is_empty();
        self.view.chosen = self.chosen(&st);
        self.view.sel.slice = self.view.chosen;
        // A value shows over the wave while it is dragged; no tooltips otherwise.
        let hint = if self.hosted || (self.drag.is_none() && self.ext_drag.is_none()) { String::new() } else { self.hint.clone() };
        self.peaks();
        let ext = self.params.ext();
        let a_name = std::path::Path::new(&st.path()).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let b_name = if ext.deck_b.is_empty() { String::new() } else { crate::state::State::parse(&ext.deck_b).map(|s| std::path::Path::new(&s.path()).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).unwrap_or_default() };
        let rate = self.shared.sample().map_or(0, |s| s.rate);
        let look = view::Look { state: &st, frames, rate, peaks: &self.peaks.2, can_dump, hosted: self.hosted, ext: &ext, decks: [a_name, b_name] };
        self.hits = view::paint(c, palette, &self.vals, &self.view, &look, &hint);
    }
}

/// Where the edited loops are kept: Mango Studio's own folder (tests use a temporary one).
fn edit_dir() -> std::path::PathBuf {
    if cfg!(test) {
        std::env::temp_dir().join(format!("lemon-slicex-test-{}", std::process::id()))
    } else {
        os_paths::user_dir().map(|d| d.join("Slicex")).unwrap_or_else(|| std::env::temp_dir().join("lemon-slicex"))
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
    use windows_sys::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OPENFILENAMEW};
    use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
    use windows_sys::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
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
            DragAcceptFiles(hwnd, 1);
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
                WM_DROPFILES => {
                    // A file dropped on the window (from Explorer, or the browser through the app) opens as the load button's does.
                    let drop = wp as HDROP;
                    let mut buf = vec![0u16; 1024];
                    let n = DragQueryFileW(drop, 0, buf.as_mut_ptr(), buf.len() as u32) as usize;
                    DragFinish(drop);
                    if n > 0 {
                        let acts = w.logic.load(&String::from_utf16_lossy(&buf[..n]));
                        w.act(acts);
                        w.invalidate();
                    }
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
            self.logic.tick();
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
                        if let Some(path) = file_dialog(self.hwnd, None) {
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
                    Act::Save(kind) => {
                        let name = self.logic.save_name(kind);
                        if let Some(path) = file_dialog(self.hwnd, Some(&name)) {
                            self.logic.save_to(kind, &path);
                        }
                        repaint = true;
                    }
                    Act::Send(path) => {
                        os_hostlink::post_audio(&path, os_hostlink::AudioTo::Playlist);
                        let ok = os_hostlink::to_host(self.hwnd as isize, os_hostlink::Cmd::Audio, os_hostlink::AudioTo::Playlist as isize) == 1;
                        if !ok {
                            os_hostlink::take_audio();
                        }
                        self.logic.sent(ok);
                        repaint = true;
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
    fn file_dialog(owner: HWND, save: Option<&str>) -> Option<String> {
        if os_plugkit::no_modal() {
            return None;
        }
        let mut file = vec![0u16; 1024];
        if let Some(name) = save {
            for (o, c) in file.iter_mut().zip(name.encode_utf16().take(1000)) {
                *o = c;
            }
        }
        let filter: Vec<u16> = if save.is_some() { "WAV files\0*.wav\0\0" } else { "Audio files\0*.wav;*.flac;*.ogg;*.wv;*.aif;*.aiff;*.mp3\0All files\0*.*\0\0" }.encode_utf16().collect();
        let title = pw::wide(if save.is_some() { "Save sample as" } else { "Loop to slice" });
        let ext = pw::wide("wav");
        unsafe {
            let mut ofn: OPENFILENAMEW = std::mem::zeroed();
            ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
            ofn.hwndOwner = owner;
            ofn.lpstrFilter = filter.as_ptr();
            ofn.lpstrFile = file.as_mut_ptr();
            ofn.nMaxFile = file.len() as u32;
            ofn.lpstrTitle = title.as_ptr();
            ofn.lpstrDefExt = ext.as_ptr();
            ofn.Flags = OFN_NOCHANGEDIR | if save.is_some() { OFN_OVERWRITEPROMPT } else { OFN_FILEMUSTEXIST };
            let ok = if save.is_some() { GetSaveFileNameW(&mut ofn) } else { GetOpenFileNameW(&mut ofn) };
            if ok == 0 {
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
        link.accept_drops();
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
                    Input::Drop { path } => {
                        let acts = self.logic.load(&path);
                        self.act(acts);
                        self.dirty = true;
                    }
                    Input::Key { .. } => {}
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
            self.logic.tick();
            // A sample dropped on the window (from the browser) opens as the load button's file would.
            for path in self.link.take_dropped() {
                let acts = self.logic.load(&path);
                self.act(acts);
            }
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
                        if let Some(path) = file_dialog(&self.win, None) {
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
                    Act::Save(kind) => {
                        let name = self.logic.save_name(kind);
                        if let Some(path) = file_dialog(&self.win, Some(name)) {
                            self.logic.save_to(kind, &path);
                        }
                        self.dirty = true;
                    }
                    Act::Send(path) => {
                        os_hostlink::post_audio(&path, os_hostlink::AudioTo::Playlist);
                        let ok = self.link.hosted() && self.link.to_host(Cmd::Audio, os_hostlink::AudioTo::Playlist as isize) == 1;
                        if !ok {
                            os_hostlink::take_audio();
                        }
                        self.logic.sent(ok);
                        self.dirty = true;
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
    fn file_dialog(win: &Window, save: Option<String>) -> Option<String> {
        if os_plugkit::no_modal() {
            return None;
        }
        let path = win.dialog(move || {
            let d = rfd::FileDialog::new();
            match save {
                Some(name) => d.set_title("Save sample as").set_file_name(name).add_filter("WAV files", &["wav"]).save_file(),
                None => d.set_title("Loop to slice").add_filter("Audio files", &["wav", "flac", "ogg", "wv", "aif", "aiff", "mp3"]).pick_file(),
            }
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

    /// Edits work on the selection (the whole loop without one), can be undone, move the slices when they change the
    /// length, and are kept in a file the saved state points to.
    #[test]
    fn editing_the_loop_and_undoing() {
        let (mut l, p) = loaded();
        let peak = |l: &Logic| l.shared.sample().unwrap().left.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let before = peak(&l);
        l.run_edit(Ed::Normalize);
        assert!((peak(&l) - 1.0).abs() < 1e-6 && peak(&l) > before);
        assert!(l.state().path().contains("-edit-"), "{}", l.state().path());
        assert!(std::path::Path::new(&l.state().path()).exists());
        l.run_edit(Ed::Undo);
        assert!((peak(&l) - before).abs() < 1e-6);
        // A selection by dragging over the wave: no slice plays from it, the edit works on it alone.
        paint(&mut l);
        let (x0, x1, y) = (view::WAVE.x + 40.0, view::WAVE.x + 200.0, view::WAVE.cy());
        l.down(x0, y, Btn::Left, Mods::default(), false);
        l.moved(x1, y, Mods::default());
        l.up(Btn::Left);
        let (a, b) = l.view.range.expect("selected");
        assert!(a < b && l.shared.previews.lock().unwrap().last().is_some_and(|p| !p.2), "the click's note ended");
        let frames = l.frames();
        l.run_edit(Ed::Delete);
        assert_eq!(l.frames(), frames - (b - a));
        assert!(l.state().slices.iter().all(|s| (s.start as usize) < l.frames()));
        l.run_edit(Ed::Undo);
        assert_eq!(l.frames(), frames);
        // Cut, copy and paste.
        l.view.range = Some((100, 300));
        l.run_edit(Ed::Copy);
        l.view.range = Some((1000, 1000));
        l.run_edit(Ed::PasteInsert);
        assert_eq!(l.frames(), frames + 200);
        // Frozen: the edits refuse.
        l.edit_ext(|e| e.freeze = true);
        assert!(!l.edit_same("Reverse", edit::reverse));
        let _ = std::fs::remove_file(p);
    }

    /// Markers move by dragging (the later ones stay, or follow with Slide), the marker button adds and takes away, and
    /// the wave zooms and scrolls.
    #[test]
    fn markers_zoom_and_loop() {
        let (mut l, p) = loaded();
        paint(&mut l);
        let n = l.state().slices.len();
        let y = view::WAVE.cy();
        let frames = l.frames();
        let x1 = view::x_of(l.state().slices[1].start, frames, l.view.zoom);
        l.down(x1, y, Btn::Left, Mods::default(), false);
        l.moved(x1 + 20.0, y, Mods::default());
        l.up(Btn::Left);
        let (s1, s2) = (l.state().slices[1].start, l.state().slices[2].start);
        assert!(s1 > 22050 && s2 == 44100, "{s1} {s2}");
        // The marker button: one at the selection's start, again to take it away.
        l.view.range = Some((10_000, 10_100));
        l.toggle_marker();
        assert_eq!(l.state().slices.len(), n + 1);
        l.toggle_marker();
        assert_eq!(l.state().slices.len(), n);
        // Undo puts the first drag back.
        l.run_edit(Ed::Undo);
        // Zoom in with the wheel, scroll with the arrows, back out in full.
        l.wheel(view::WAVE.cx(), y, 1.0, Mods::default());
        assert!(l.view.zoom.1 - l.view.zoom.0 < 1.0);
        let z = l.view.zoom;
        l.scroll(1);
        assert!(l.view.zoom.0 > z.0 || z.1 >= 1.0);
        l.run_zoom(ZoomOp::Full);
        assert_eq!(l.view.zoom, (0.0, 1.0));
        // The loop button plays the slice again once it ended.
        l.loop_on = true;
        l.play(Some(60));
        l.shared.previews.lock().unwrap().clear();
        l.shared.voices.store(false, Ordering::Relaxed);
        l.tick();
        assert!(l.shared.previews.lock().unwrap().iter().any(|p| p.0 == 60 && p.2), "played again");
        let _ = std::fs::remove_file(p);
    }

    /// The region settings: a knob moves the chosen slice's own setting, ART picks its articulator (a right click the
    /// slice's own), CUT the cut group, and the list chooses the region; they are saved with the articulators.
    #[test]
    fn region_settings_belong_to_the_chosen_slice() {
        let (mut l, p) = loaded();
        paint(&mut l);
        l.view.chosen = 1;
        paint(&mut l);
        let (x, y) = centre(&l, &Target::Ext(Ctl::RegKnob(0, 0)));
        l.down(x, y, Btn::Left, Mods::default(), false);
        for _ in 0..40 {
            l.delta(0.0, 1.0, Mods::default());
        }
        l.up(Btn::Left);
        let ext = l.params.ext();
        assert!(ext.region(1).amp < 0.7 && ext.region(0).amp == 0.8, "{:?}", ext.region(1));
        let (x, y) = centre(&l, &Target::Ext(Ctl::RegArt(2)));
        l.down(x, y, Btn::Left, Mods::default(), false);
        l.up(Btn::Left);
        assert_eq!(l.params.ext().art_for(1, 2), 1, "the next articulator");
        l.down(x, y, Btn::Right, Mods::default(), false);
        assert_eq!(l.params.ext().region(1).art[2], crate::ext::ART_OWN);
        let (x, y) = centre(&l, &Target::Ext(Ctl::RegCut));
        l.down(x, y, Btn::Left, Mods::default(), false);
        l.down(x, y, Btn::Left, Mods::default(), false);
        assert_eq!(l.params.ext().region(1).cut, 2);
        assert!(l.menu(&MenuReq::Regions).find("Marker #3").is_some());
        l.pick(&MenuReq::Regions, Cmd::Choose(2), 0.0, 0.0);
        assert_eq!(l.view.chosen, 2);
        let _ = std::fs::remove_file(p);
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
