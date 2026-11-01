// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The Mango Beat editor: the platform-independent logic (parameter drags, the envelope points, the slot and grid
//! menus) and the `Editor` the host spawns; `window` carries the per-platform child windows.
//!
// spec: plugins/mango-beat.md (§2 controls, §3 hints, §4 interaction)

use std::any::Any;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use nih_plug::prelude::*;
use os_plugkit::vals::Vals;
use os_plugkit::MenuDef;
use os_ui::kit::menu::{MenuItem, MenuStyle};

use crate::grid::{Curve, GridState, Mode, Pt};
use crate::params::{BeatParams, DEFS};
use crate::view::{self, Target, ViewState};
use crate::Shared;

pub struct BeatEditor {
    pub params: Arc<BeatParams>,
    pub shared: Arc<Shared>,
    /// DPI scale as f32 bits.
    pub scale: AtomicU32,
}

impl BeatEditor {
    pub fn new(params: Arc<BeatParams>, shared: Arc<Shared>) -> Self {
        BeatEditor { params, shared, scale: AtomicU32::new(1f32.to_bits()) }
    }
}

impl Editor for BeatEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn Any + Send> {
        let scale = f32::from_bits(self.scale.load(Ordering::Relaxed));
        match parent {
            #[cfg(windows)]
            ParentWindowHandle::Win32Hwnd(hwnd) => {
                Box::new(crate::window::win32::open(hwnd, scale, self.params.clone(), self.shared.clone(), context))
            }
            #[cfg(target_os = "linux")]
            ParentWindowHandle::X11Window(win) => {
                Box::new(crate::window::gui::open(win, scale, self.params.clone(), self.shared.clone(), context))
            }
            // macOS hosts set no scale (sizes are points): the editor paints at its window's backing scale.
            #[cfg(target_os = "macos")]
            ParentWindowHandle::AppKitNsView(nsview) => {
                let scale = os_plugkit::gui::backing_scale(nsview as usize);
                Box::new(crate::window::gui::open(nsview as usize, scale, self.params.clone(), self.shared.clone(), context))
            }
            // Elsewhere no editor opens yet.
            _ => {
                let _ = (context, scale);
                Box::new(())
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        (view::W, view::H)
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        self.scale.store(factor.to_bits(), Ordering::Relaxed);
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {
        repaint::notify_editor();
    }
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {
        repaint::notify_editor();
    }
    fn param_values_changed(&self) {
        repaint::notify_editor();
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Laws (spec §4)

/// Pixels of drag for a dial's whole range.
pub const KNOB_PX: f32 = 200.0;
const FINE: f32 = 0.25;
/// Pixels of drag for a point across the whole grid.
pub const POINT_PX: f32 = 260.0;
/// Pixels of drag for a point over the grid's height.
pub const POINT_VPX: f32 = 170.0;
/// Pixels of drag for a tension handle.
const TENSION_PX: f32 = 80.0;

// ---------------------------------------------------------------------------------------------------------------
// Menus

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    None,
    Reset(String),
    CopyValue(String),
    PasteValue(String),
    TypeIn(String),
    Automate(String),
    /// A point moves to another shape (section, slot, point, mode).
    Shape(usize, usize, usize, Mode),
    /// A point goes away.
    DeletePoint(usize, usize, usize),
    /// A section's grid resets to its default line.
    ResetSection(usize),
    /// Slot clipboard: copy, paste and clear one section's slot.
    CopySlot(usize, usize),
    PasteSlot(usize, usize),
    ClearSlot(usize, usize),
    About,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuReq {
    Param(String),
    Point(usize, usize, usize),
    Section(usize, usize),
    About,
}

// ---------------------------------------------------------------------------------------------------------------
// Editor logic

/// Something the window has to do after an event.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Repaint,
    /// Hide the cursor and hold it in place (dial and point drags).
    Lock,
    Menu(MenuReq, f32, f32),
    TypeIn(String, f32, f32),
    /// Ask the host for an automation clip of parameter `index` (host link).
    Automate(usize),
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

/// A drag: a parameter (its normalised value and pixels for the range), a grid point (its beat and value at the
/// start and the pixels moved so far) or a tension handle.
#[derive(Clone, Debug)]
enum Drag {
    Param { id: String, v: f32, px: f32, across: bool },
    Point { sec: usize, slot: usize, i: usize, beat: f32, value: f32, dx: f32, dy: f32 },
    Tension { sec: usize, slot: usize, i: usize, t: f32, dy: f32 },
}

/// Every parameter id in the host's order.
pub fn ids() -> impl Iterator<Item = &'static str> {
    DEFS.iter().map(|d| d.id)
}

pub struct Logic {
    pub vals: Vals,
    pub params: Arc<BeatParams>,
    pub shared: Arc<Shared>,
    pub view: ViewState,
    pub hits: Vec<view::Hit>,
    drag: Option<Drag>,
    /// Hint text.
    pub hint: String,
    copied: Option<f32>,
    /// A copied slot (the section menus paste it).
    copied_slot: Option<Curve>,
    /// The grids the editor shows (its copy of the shared ones).
    pub grids: GridState,
    grids_seen: u64,
    pub hosted: bool,
}

impl Logic {
    pub fn new(params: Arc<BeatParams>, shared: Arc<Shared>, ctx: Option<Arc<dyn GuiContext>>) -> Self {
        let vals = Vals::new(&*params, params.live.clone(), ctx);
        let (grids, seen) = params.grid.load_and_seen();
        Logic {
            vals,
            params,
            shared,
            view: ViewState::default(),
            hits: Vec::new(),
            drag: None,
            hint: String::new(),
            copied: None,
            copied_slot: None,
            grids,
            grids_seen: seen,
            hosted: false,
        }
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn hit(&self, x: f32, y: f32) -> Option<(Target, os_ui::Rect)> {
        self.hits.iter().rev().find(|h| h.r.contains(x, y)).map(|h| (h.t.clone(), h.r))
    }

    /// The grids moved outside the editor (a preset, a project load): the copy follows.
    pub fn sync_grids(&mut self) {
        let v = self.params.grid.version();
        if v != self.grids_seen {
            let (g, seen) = self.params.grid.load_and_seen();
            self.grids = g;
            self.grids_seen = seen;
        }
    }

    fn hint_for(&mut self, t: Option<&Target>) -> String {
        t.map(|t| view::hint(t, &self.vals, &self.grids)).unwrap_or_default()
    }

    fn range(id: &str) -> (i32, i32) {
        crate::params::index(id).map_or((0, 1), |k| crate::params::RANGES[k])
    }

    fn edit(&mut self, id: &str, norm: f32) {
        self.vals.edit(id, norm);
    }

    fn reset(&mut self, id: &str) {
        let d = self.vals.default_norm(id);
        self.edit(id, d);
    }

    fn begin_param(&mut self, id: &str, px: f32, across: bool, t: Target) -> Vec<Act> {
        self.vals.begin(id);
        self.drag = Some(Drag::Param { id: id.into(), v: self.vals.norm(id), px, across });
        self.view.pressed = Some(t);
        vec![Act::Repaint, Act::Lock]
    }

    /// Puts the editor's grids in the shared state (the sound hears them next block) and repaints.
    fn grids_changed(&mut self) -> Vec<Act> {
        self.params.grid.store(self.grids.clone());
        vec![Act::Repaint]
    }

    /// The shown slot of a section, by the parameters.
    fn shown_slot(&self, sec: usize) -> usize {
        let id = if sec == view::TIME { "time_slot" } else { "vol_slot" };
        (self.vals.plain(id).round() as i32).clamp(0, 3) as usize
    }

    fn curve_mut(&mut self, sec: usize, slot: usize) -> &mut Curve {
        if sec == view::TIME {
            &mut self.grids.time[slot.min(3)]
        } else {
            &mut self.grids.volume[slot.min(3)]
        }
    }

    /// A point's neighbours' beats (its x stays between them; the first point pins at 0).
    fn beat_bounds(curve: &Curve, i: usize) -> (f32, f32) {
        let lo = if i == 0 { 0.0 } else { curve.pts[i - 1].x };
        let hi = curve.pts.get(i + 1).map_or(1.0, |p| p.x);
        (lo, hi)
    }

    /// Snaps a beat position to the grid's twenty-fourths (a free drag with Shift).
    fn snap_beat(x: f32, free: bool) -> f32 {
        if free {
            return x.clamp(0.0, 1.0);
        }
        ((x * 24.0).round() / 24.0).clamp(0.0, 1.0)
    }

    pub fn down(&mut self, x: f32, y: f32, btn: Btn, m: Mods, _dbl: bool) -> Vec<Act> {
        self.view.mouse = Some((x, y));
        self.sync_grids();
        let Some((t, _)) = self.hit(x, y) else { return vec![] };
        let id = t.id().to_string();
        let mut acts = vec![Act::Repaint];
        match (&t, btn) {
            (Target::Logo, Btn::Left | Btn::Right) => acts.push(Act::Menu(MenuReq::About, x, y)),
            (Target::Logo, _) => {}
            (_, Btn::Middle) if !id.is_empty() => self.reset(&id),
            (_, Btn::Middle) => {}
            (Target::Knob(_), Btn::Right) => acts.push(Act::Menu(MenuReq::Param(id), x, y)),
            (Target::Knob(_), Btn::Left) if m.alt => self.reset(&id),
            (Target::Knob(_), Btn::Left) => acts = self.begin_param(&id, KNOB_PX, false, t.clone()),
            (Target::Slot(sec, slot), Btn::Right) => acts.push(Act::Menu(MenuReq::Section(*sec, *slot), x, y)),
            (Target::Slot(sec, slot), Btn::Left) => {
                let pid = if *sec == view::TIME { "time_slot" } else { "vol_slot" };
                let n = self.vals.plain_to_norm(pid, *slot as f32);
                self.edit(pid, n);
                self.view.active = Some((*sec, *slot, 0));
            }
            (Target::Grid(sec), Btn::Right) => acts.push(Act::Menu(MenuReq::Section(*sec, self.shown_slot(*sec)), x, y)),
            (Target::Grid(sec), Btn::Left) => {
                // A click on the empty grid plants a point there (on the beat's twenty-fourths).
                let sec = *sec;
                let slot = self.shown_slot(sec);
                let (beat, value) = view::value_at(sec, x, y);
                let beat = Self::snap_beat(beat, m.shift);
                let curve = self.curve_mut(sec, slot);
                curve.pts.push(Pt::new(beat, value, 0.0, Mode::Single as u8));
                curve.normalize();
                let i = curve.pts.iter().position(|p| (p.x - beat).abs() < 1e-4).unwrap_or(0);
                let (px, py) = (curve.pts[i].x, curve.pts[i].y);
                self.view.active = Some((sec, slot, i));
                self.drag = Some(Drag::Point { sec, slot, i, beat: px, value: py, dx: 0.0, dy: 0.0 });
                self.view.pressed = Some(t.clone());
                acts.push(Act::Lock);
                acts.extend(self.grids_changed());
            }
            (Target::Point(sec, slot, i), Btn::Right) => acts.push(Act::Menu(MenuReq::Point(*sec, *slot, *i), x, y)),
            (Target::Point(sec, slot, i), Btn::Left) if m.alt => {
                // Alt takes the point away (when one can be spared).
                let (sec, slot, i) = (*sec, *slot, *i);
                let curve = self.curve_mut(sec, slot);
                if i < curve.pts.len() && curve.pts.len() > 1 {
                    curve.pts.remove(i);
                    curve.normalize();
                    self.view.active = None;
                    acts.extend(self.grids_changed());
                }
            }
            (Target::Point(sec, slot, i), Btn::Left) => {
                let (sec, slot, i) = (*sec, *slot, *i);
                let curve = self.curve_of(sec, slot).unwrap_or_default();
                if let Some(p) = curve.pts.get(i).copied() {
                    self.view.active = Some((sec, slot, i));
                    // The tension ring (drawn at the segment's middle) drags the shape, not the point.
                    let ring = if i > 0 {
                        curve.pts.get(i - 1).is_some_and(|a| {
                            Mode::ALL.get(p.m as usize).is_some_and(|m| m.has_tension()) && {
                                let mid = (a.x + p.x) / 2.0;
                                let hy = view::gy(sec, curve.value_at(f64::from(mid)).unwrap_or(0.5));
                                let hx = view::gx(sec, mid);
                                (x - hx).abs() <= 7.0 && (y - hy).abs() <= 7.0
                            }
                        })
                    } else {
                        false
                    };
                    if ring {
                        self.drag = Some(Drag::Tension { sec, slot, i, t: p.t, dy: 0.0 });
                    } else {
                        self.drag = Some(Drag::Point { sec, slot, i, beat: p.x, value: p.y, dx: 0.0, dy: 0.0 });
                    }
                    self.view.pressed = Some(t.clone());
                    acts.push(Act::Lock);
                }
            }
        }
        self.hint = self.hint_for(Some(&t));
        acts
    }

    /// Mouse movement during a locked drag (logical px since the last event).
    pub fn delta(&mut self, dx: f32, dy: f32, m: Mods) -> Vec<Act> {
        let Some(d) = self.drag.clone() else { return vec![] };
        let f = if m.ctrl { FINE } else { 1.0 };
        match d {
            Drag::Param { id, v, px, across } => {
                let moved = if across { dx } else { -dy };
                let v = (v + moved * f / px).clamp(0.0, 1.0);
                self.vals.set(&id, v);
                self.drag = Some(Drag::Param { id, v, px, across });
            }
            Drag::Point { sec, slot, i, beat, value, dx: acc_x, dy: acc_y } => {
                let acc_x = acc_x + dx * f;
                let acc_y = acc_y + dy * f;
                // Shift locks the axis that moved most first (as the playlist's points); the beat lands on the
                // twenty-fourths unless the drag holds Shift from the start.
                let lock = ((acc_x.abs() > 3.0 || acc_y.abs() > 3.0)).then(|| acc_x.abs() >= acc_y.abs());
                let mut x = beat + acc_x / POINT_PX;
                let mut y = (value - acc_y / POINT_VPX).clamp(0.0, 1.0);
                if m.shift {
                    x = Self::snap_beat(x, true);
                } else {
                    x = Self::snap_beat(x, false);
                }
                if lock == Some(false) {
                    x = beat;
                }
                if lock == Some(true) {
                    y = value;
                }
                let curve = self.curve_mut(sec, slot);
                if i < curve.pts.len() {
                    let (lo, hi) = Self::beat_bounds(curve, i);
                    curve.pts[i].x = if i == 0 { 0.0 } else { x.clamp(lo, hi) };
                    curve.pts[i].y = y;
                }
                self.drag = Some(Drag::Point { sec, slot, i, beat, value, dx: acc_x, dy: acc_y });
                let t = self.view.pressed.clone();
                self.hint = self.hint_for(t.as_ref());
                return self.grids_changed();
            }
            Drag::Tension { sec, slot, i, t, dy: acc_y } => {
                let acc_y = acc_y + dy * f;
                let curve = self.curve_mut(sec, slot);
                if i < curve.pts.len() {
                    curve.pts[i].t = (t - acc_y / TENSION_PX).clamp(-1.0, 1.0);
                }
                self.drag = Some(Drag::Tension { sec, slot, i, t, dy: acc_y });
                let t = self.view.pressed.clone();
                self.hint = self.hint_for(t.as_ref());
                return self.grids_changed();
            }
        }
        let t = self.view.pressed.clone();
        self.hint = self.hint_for(t.as_ref());
        vec![Act::Repaint]
    }

    pub fn up(&mut self, _btn: Btn) -> Vec<Act> {
        let Some(d) = self.drag.take() else { return vec![] };
        if let Drag::Param { id, .. } = d {
            self.vals.end(&id);
        }
        self.view.pressed = None;
        vec![Act::Repaint]
    }

    pub fn moved(&mut self, x: f32, y: f32, _m: Mods) -> Vec<Act> {
        self.view.mouse = Some((x, y));
        if self.drag.is_none() {
            let t = self.hit(x, y).map(|h| h.0);
            self.hint = self.hint_for(t.as_ref());
        }
        vec![Act::Repaint]
    }

    pub fn leave(&mut self) -> Vec<Act> {
        self.view.mouse = None;
        self.hint.clear();
        vec![Act::Repaint]
    }

    /// Wheel: one step per notch (1/128 of the range for the settings with many steps).
    pub fn wheel(&mut self, x: f32, y: f32, notches: f32, _m: Mods) -> Vec<Act> {
        let Some((t, _)) = self.hit(x, y).filter(|(t, _)| matches!(t, Target::Knob(_))) else { return vec![] };
        let id = t.id().to_string();
        let (lo, hi) = Self::range(&id);
        let steps = (hi - lo) as f32;
        let step = if steps > 128.0 { (steps / 128.0).round() } else { 1.0 };
        let cur = self.vals.plain(&id);
        let plain = (cur + notches.signum() * step).clamp(lo as f32, hi as f32);
        self.vals.edit_plain(&id, plain);
        self.hint = self.hint_for(Some(&t));
        vec![Act::Repaint]
    }

    fn curve_of(&self, sec: usize, slot: usize) -> Option<Curve> {
        Some(if sec == view::TIME { self.grids.time[slot.min(3)].clone() } else { self.grids.volume[slot.min(3)].clone() })
    }

    pub fn menu(&self, req: &MenuReq) -> MenuDef<Cmd> {
        match req {
            MenuReq::Param(id) => {
                let mut m = MenuDef::new(MenuStyle::Context);
                m.head(&view::hint(&Target::Knob(id.clone()), &self.vals, &self.grids), Cmd::None);
                m.add(MenuItem::item("Reset"), Cmd::Reset(id.clone()));
                m.sep(Cmd::None);
                m.add(MenuItem::item("Copy value"), Cmd::CopyValue(id.clone()));
                m.add(MenuItem::item("Paste value").enabled(self.copied.is_some()), Cmd::PasteValue(id.clone()));
                m.add(MenuItem::item("Type in value..."), Cmd::TypeIn(id.clone()));
                // Automation belongs to the host: offered when Mango Studio hosts the editor (host link).
                m.sep(Cmd::None);
                m.add(MenuItem::item("Create automation clip").enabled(self.hosted), Cmd::Automate(id.clone()));
                m.sep(Cmd::None);
                m.add(MenuItem::item("About Mango Beat..."), Cmd::About);
                m
            }
            MenuReq::Point(sec, slot, i) => {
                let mut m = MenuDef::new(MenuStyle::Context);
                m.head(&view::point_hint(*sec, *slot, *i, &self.grids), Cmd::None);
                let cur = self
                    .curve_of(*sec, *slot)
                    .and_then(|c| c.pts.get(*i).copied())
                    .map(|p| Mode::ALL.get(p.m as usize).copied().unwrap_or(Mode::Single));
                for mode in Mode::ALL {
                    let name = if cur == Some(mode) { format!("{}  (the shape)", mode.name()) } else { mode.name().to_string() };
                    m.add(MenuItem::item(&name), Cmd::Shape(*sec, *slot, *i, mode));
                }
                m.sep(Cmd::None);
                let removable = self.curve_of(*sec, *slot).is_some_and(|c| c.pts.len() > 1);
                m.add(MenuItem::item("Delete point").enabled(removable), Cmd::DeletePoint(*sec, *slot, *i));
                m.add(MenuItem::item("Reset section"), Cmd::ResetSection(*sec));
                m
            }
            MenuReq::Section(sec, slot) => {
                let mut m = MenuDef::new(MenuStyle::Context);
                m.head(
                    &format!("{} {}", if *sec == view::TIME { "Time" } else { "Volume" }, view::slot_name(*slot)),
                    Cmd::None,
                );
                m.add(MenuItem::item("Copy slot"), Cmd::CopySlot(*sec, *slot));
                m.add(MenuItem::item("Paste slot").enabled(self.copied_slot.is_some()), Cmd::PasteSlot(*sec, *slot));
                m.sep(Cmd::None);
                m.add(MenuItem::item("Clear slot"), Cmd::ClearSlot(*sec, *slot));
                m.add(MenuItem::item("Reset section"), Cmd::ResetSection(*sec));
                m
            }
            MenuReq::About => {
                let mut m = MenuDef::new(MenuStyle::Context);
                m.head("Mango Beat", Cmd::None);
                m.add(MenuItem::item(&format!("Version {}", env!("CARGO_PKG_VERSION"))).enabled(false), Cmd::None);
                m.add(MenuItem::item("A Gross Beat-style time manipulator").enabled(false), Cmd::None);
                m
            }
        }
    }

    pub fn pick(&mut self, _req: &MenuReq, cmd: Cmd, x: f32, y: f32) -> Vec<Act> {
        match cmd {
            Cmd::None => {}
            Cmd::Reset(id) => self.reset(&id),
            Cmd::CopyValue(id) => self.copied = Some(self.vals.norm(&id)),
            Cmd::PasteValue(id) => {
                if let Some(v) = self.copied {
                    self.edit(&id, v);
                }
            }
            Cmd::TypeIn(id) => return vec![Act::TypeIn(id, x, y)],
            Cmd::Automate(id) => return ids().position(|i| i == id).map(Act::Automate).into_iter().collect(),
            Cmd::About => return vec![Act::Menu(MenuReq::About, x, y)],
            Cmd::Shape(sec, slot, i, mode) => {
                let curve = self.curve_mut(sec, slot);
                let n = curve.pts.len();
                if let Some(p) = curve.pts.get_mut(i.min(n.saturating_sub(1))) {
                    p.m = mode as u8;
                }
                return self.grids_changed();
            }
            Cmd::DeletePoint(sec, slot, i) => {
                let curve = self.curve_mut(sec, slot);
                if i < curve.pts.len() && curve.pts.len() > 1 {
                    curve.pts.remove(i);
                    curve.normalize();
                }
                self.view.active = None;
                return self.grids_changed();
            }
            Cmd::ResetSection(sec) => {
                if sec == view::TIME {
                    self.grids.time = Default::default();
                } else {
                    self.grids.volume = Default::default();
                }
                return self.grids_changed();
            }
            Cmd::CopySlot(sec, slot) => {
                self.copied_slot = self.curve_of(sec, slot);
            }
            Cmd::PasteSlot(sec, slot) => {
                if let Some(c) = self.copied_slot.clone() {
                    *self.curve_mut(sec, slot) = c;
                    return self.grids_changed();
                }
            }
            Cmd::ClearSlot(sec, slot) => {
                *self.curve_mut(sec, slot) = Curve::default();
                return self.grids_changed();
            }
        }
        vec![Act::Repaint]
    }

    /// The value text a type-in box starts with.
    pub fn value_text(&self, id: &str) -> String {
        self.vals.text(id)
    }

    /// Applies a typed value.
    pub fn typed(&mut self, id: &str, text: &str) {
        if let Some(n) = self.vals.parse(id, text) {
            self.edit(id, n);
        }
    }

    /// Paints the editor; `hint` is drawn when the host does not show it.
    pub fn paint(&mut self, c: &mut os_ui::Canvas, palette: &os_theme::Palette) {
        self.sync_grids();
        let hint = if self.hosted { String::new() } else { self.hint.clone() };
        self.hits = view::paint(c, palette, &self.vals, &self.view, &self.grids, &hint);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The windows and the repaint path

#[cfg(windows)]
pub(crate) static HWND_SLOT: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) static WAKE_SLOT: std::sync::OnceLock<os_plugkit::gui::Slot> = std::sync::OnceLock::new();

/// The parameter changes repaint the open editor, on every platform.
mod repaint {
    pub(crate) fn notify_editor() {
        #[cfg(windows)]
        crate::window::win32::post_changed(super::HWND_SLOT.load(std::sync::atomic::Ordering::Acquire));
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if let Some(slot) = super::WAKE_SLOT.get() {
            os_plugkit::gui::notify(slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{TIME_MIX, VOL_SLOT};

    fn logic() -> Logic {
        let mut l = Logic::new(Arc::new(BeatParams::default()), Arc::new(Shared::default()), None);
        paint(&mut l);
        l
    }

    fn paint(l: &mut Logic) {
        let mut c = os_ui::Canvas::new(view::W, view::H, 1.0).unwrap();
        l.paint(&mut c, &os_plugkit::palette());
    }

    fn centre(l: &Logic, t: &Target) -> (f32, f32) {
        // The FIRST hit with the target (a point's own circle: its tension ring, drawn later, must not win).
        let h = l.hits.iter().find(|h| &h.t == t).unwrap_or_else(|| panic!("no hit {t:?}"));
        (h.r.cx(), h.r.cy())
    }

    /// Every parameter has a control (the slot parameters via their sections' buttons), and no two hit regions
    /// overlap.
    #[test]
    fn every_parameter_has_a_control_and_controls_do_not_overlap() {
        let l = logic();
        for (k, d) in DEFS.iter().enumerate() {
            if k == crate::params::TIME_SLOT || k == crate::params::VOL_SLOT {
                let sec = if k == crate::params::TIME_SLOT { view::TIME } else { view::VOL };
                assert!((0..4).any(|slot| l.hits.iter().any(|h| h.t == Target::Slot(sec, slot))), "{}", d.id);
            } else {
                assert!(l.hits.iter().any(|h| h.t.id() == d.id), "{}", d.id);
            }
        }
        assert_eq!(ids().count(), crate::params::PARAMS);
        for sec in [view::TIME, view::VOL] {
            for slot in 0..4 {
                assert!(l.hits.iter().any(|h| h.t == Target::Slot(sec, slot)), "slot {sec}/{slot}");
            }
        }
        for (i, a) in l.hits.iter().enumerate() {
            assert!(a.r.x >= 0.0 && a.r.y >= 0.0 && a.r.x + a.r.w <= view::W as f32 && a.r.y + a.r.h <= view::H as f32, "{:?}", a.t);
            for b in &l.hits[i + 1..] {
                let apart = a.r.x + a.r.w <= b.r.x || b.r.x + b.r.w <= a.r.x || a.r.y + a.r.h <= b.r.y || b.r.y + b.r.h <= a.r.y;
                assert!(apart, "{:?} overlaps {:?}", a.t, b.t);
            }
        }
    }

    /// A dial drags, wheels and resets; a slot click switches the section's slot; the hint follows.
    #[test]
    fn dials_drag_and_slots_switch() {
        let mut l = logic();
        let t = Target::Knob("time_mix".into());
        let (x, y) = centre(&l, &t);
        let acts = l.down(x, y, Btn::Left, Mods::default(), false);
        assert!(acts.contains(&Act::Lock));
        for _ in 0..100 {
            l.delta(0.0, -1.0, Mods::default());
        }
        l.up(Btn::Left);
        assert_eq!(l.params.v(TIME_MIX), 10000);
        assert_eq!(l.hint, "Time mix: 100.0%");
        l.wheel(x, y, -1.0, Mods::default());
        assert_eq!(l.vals.plain("time_mix"), 9922.0, "a 1/128 step of the range per notch");
        l.down(x, y, Btn::Left, Mods { alt: true, ..Mods::default() }, false);
        assert_eq!(l.vals.plain("time_mix"), 10000.0);
        let acts = l.down(x, y, Btn::Right, Mods::default(), false);
        assert!(matches!(acts.last(), Some(Act::Menu(MenuReq::Param(id), _, _)) if id == "time_mix"));
        let (x, y) = centre(&l, &Target::Slot(view::VOL, 2));
        l.down(x, y, Btn::Left, Mods::default(), false);
        assert_eq!(l.params.v(VOL_SLOT), 2);
        assert_eq!(l.hint, "Volume slot: C");
    }

    /// A click on the empty grid plants a point; the point drags (snapped to the twenty-fourths) and alt takes it
    /// away; the editor's grids reach the shared state.
    #[test]
    fn points_are_planted_dragged_and_deleted() {
        let mut l = logic();
        let (gx, gy) = (view::GRIDS[view::TIME].x + 120.0, view::GRIDS[view::TIME].cy());
        l.down(gx, gy, Btn::Left, Mods::default(), false);
        assert_eq!(l.grids.time[0].pts.len(), 1, "one point planted");
        assert!(matches!(l.drag, Some(Drag::Point { .. })));
        l.up(Btn::Left);
        let seen = l.params.grid.version();
        assert!(seen > 0, "the shared grids heard the edit");
        paint(&mut l);
        // Plant a second point, then drag IT a quarter beat across (the first point pins at 0).
        let (x2, y2) = (view::GRIDS[view::TIME].x + 300.0, view::GRIDS[view::TIME].y + 40.0);
        l.down(x2, y2, Btn::Left, Mods::default(), false);
        l.up(Btn::Left);
        assert_eq!(l.grids.time[0].pts.len(), 2);
        paint(&mut l);
        let (px, py) = centre(&l, &Target::Point(view::TIME, 0, 1));
        l.down(px, py, Btn::Left, Mods::default(), false);
        for _ in 0..65 {
            l.delta(1.0, 0.0, Mods::default());
        }
        l.up(Btn::Left);
        let p = l.grids.time[0].pts[1];
        assert!((p.x - 0.75).abs() < 1e-3, "snapped to the twenty-fourths: {}", p.x);
        // Alt removes the first point (the survivor pins at 0).
        paint(&mut l);
        let (px, py) = centre(&l, &Target::Point(view::TIME, 0, 0));
        l.down(px, py, Btn::Left, Mods { alt: true, ..Mods::default() }, false);
        l.up(Btn::Left);
        assert_eq!(l.grids.time[0].pts.len(), 1);
        assert_eq!(l.grids.time[0].pts[0].x, 0.0, "the survivor pins at 0");
        assert!(l.params.grid.version() > seen);
    }

    /// The point and section menus offer the shapes, delete, copy and clear; picking them lands in the grids.
    #[test]
    fn menus_edit_the_grids() {
        let mut l = logic();
        let (gx, gy) = (view::GRIDS[view::TIME].x + 60.0, view::GRIDS[view::TIME].cy());
        l.down(gx, gy, Btn::Left, Mods::default(), false);
        l.up(Btn::Left);
        let m = l.menu(&MenuReq::Point(view::TIME, 0, 0));
        assert!(m.find("Hold").is_some(), "the shape list is there");
        assert!(m.find("Delete point").is_some(), "the item is there even when disabled");
        l.pick(&MenuReq::Point(view::TIME, 0, 0), Cmd::DeletePoint(view::TIME, 0, 0), 0.0, 0.0);
        assert_eq!(l.grids.time[0].pts.len(), 1, "a lone point stays");
        l.grids.time[0].pts.push(crate::grid::Pt::new(0.5, 0.5, 0.0, 0));
        l.pick(&MenuReq::Point(view::TIME, 0, 0), Cmd::Shape(view::TIME, 0, 0, Mode::Hold), 0.0, 0.0);
        assert_eq!(l.grids.time[0].pts[0].m, Mode::Hold as u8);
        l.pick(&MenuReq::Point(view::TIME, 0, 0), Cmd::DeletePoint(view::TIME, 0, 1), 0.0, 0.0);
        assert_eq!(l.grids.time[0].pts.len(), 1);
        l.pick(&MenuReq::Section(view::TIME, 0), Cmd::ClearSlot(view::TIME, 0), 0.0, 0.0);
        assert!(l.grids.time[0].pts.is_empty(), "cleared to the default line");
        // The About menu comes from the wordmark.
        let (x, y) = centre(&l, &Target::Logo);
        assert!(matches!(l.down(x, y, Btn::Left, Mods::default(), false).last(), Some(Act::Menu(MenuReq::About, _, _))));
        assert_eq!(l.hint, "Mango Beat");
    }

    /// The editor follows grids that moved outside it (a preset loading), by letter on the slot and in decibels
    /// on the point.
    #[test]
    fn grids_from_outside_reach_the_editor() {
        let mut l = logic();
        let mut g = GridState::default();
        g.volume[3] = crate::grid::Curve::constant(0.25);
        l.params.grid.store(g);
        l.sync_grids();
        assert_eq!(l.grids.volume[3].pts[0].y, 0.25);
        let (x, y) = centre(&l, &Target::Slot(view::VOL, 3));
        l.down(x, y, Btn::Left, Mods::default(), false);
        assert_eq!(l.hint, "Volume slot: D");
        paint(&mut l);
        let (px, py) = centre(&l, &Target::Point(view::VOL, 3, 0));
        l.moved(px, py, Mods::default());
        assert!(l.hint.contains("-12"), "the hint reads the gate in dB: {}", l.hint);
    }

    /// `BEAT_PNG=<dir> cargo test -p mango-beat -- --ignored`: the editor at a few settings, as PNGs.
    #[test]
    #[ignore]
    fn render_editor() {
        let Some(dir) = std::env::var_os("BEAT_PNG") else { return };
        let save = |l: &mut Logic, name: &str, scale: f32| {
            let (w, h) = ((view::W as f32 * scale) as u32, (view::H as f32 * scale) as u32);
            let mut c = os_ui::Canvas::new(w, h, scale).unwrap();
            l.paint(&mut c, &os_plugkit::palette());
            let mut px = vec![0u32; (w * h) as usize];
            c.copy_to_xrgb(&mut px);
            let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
            for (o, p) in pm.data_mut().chunks_mut(4).zip(&px) {
                o.copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]);
            }
            pm.save_png(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
        };
        let mut l = logic();
        save(&mut l, "beat_default", 1.0);
        save(&mut l, "beat_default_x2", 2.0);
        // A half-time time grid and a trance gate.
        l.grids.time[0] = crate::grid::Curve { pts: vec![crate::grid::Pt::new(0.0, 0.375, 0.6, 0), crate::grid::Pt::new(1.0, 0.0, 0.0, 12)] };
        l.grids.volume[0] = crate::dsp::gate(2);
        l.hint = "Time A, point 1: beat 12/24, -2.0x".into();
        save(&mut l, "beat_halftime_gate", 1.0);
    }
}
