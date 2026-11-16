// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The plugin editor: a child of the host's window (Win32, X11 on Linux, an NSView on macOS) that paints `view` with
//! the os-ui canvas, and the editor logic (platform independent): knob drags with the cursor held in place, the
//! reverse switch, the root key's menu, loading a sample, and a click on the well playing it. Parameter edits show
//! and sound at once (`os_plugkit::vals`).
//!
// spec: plugins/lemon-granulizer.md §3–§4, plugins/host-link.md

use std::any::Any;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::Arc;

use nih_plug::prelude::*;
use os_plugkit::MenuDef;
use os_plugkit::vals::Vals;
use os_ui::kit::menu::{Mark, MenuItem, MenuStyle};

use crate::params::{self, GranParams, NAMES};
use crate::view::{self, Target, ViewState};
use crate::Shared;

pub struct GranEditor {
    pub params: Arc<GranParams>,
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

impl GranEditor {
    pub fn new(params: Arc<GranParams>, shared: Arc<Shared>) -> Self {
        GranEditor {
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

impl Editor for GranEditor {
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
        (view::W, view::H)
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
/// The velocity a click on the well plays the sample at.
const PREVIEW_VELOCITY: f32 = 100.0 / 127.0;

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
    /// The root key the note plays against.
    Root(i32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuReq {
    Param(String),
    Root,
    About,
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
    pub params: Arc<GranParams>,
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
}

impl Logic {
    pub fn new(params: Arc<GranParams>, shared: Arc<Shared>, ctx: Option<Arc<dyn GuiContext>>) -> Self {
        let vals = Vals::new(&*params, params.live.clone(), ctx);
        Logic { vals, params, shared, view: ViewState::default(), hits: Vec::new(), drag: None, hint: String::new(), copied: None, hosted: false, playing: None, peaks: (u64::MAX, Vec::new()) }
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

    fn state(&self) -> params::State {
        self.params.state()
    }

    fn frames(&self) -> usize {
        self.shared.sample().map_or(0, |s| s.frames())
    }

    fn hint_for(&self, t: Option<&Target>) -> String {
        match t {
            Some(Target::Knob(id)) => format!("{}: {}", Self::name(id), self.vals.text(id)),
            Some(Target::Switch(id)) => format!("{}\t{}", Self::name(id), if self.vals.norm(id) > 0.5 { "on" } else { "off" }),
            Some(Target::Root) => format!("Root key: {}\tthe note plays against it", view::key_name(self.state().root)),
            Some(Target::Load) => "Load a sample".into(),
            Some(Target::Well) => {
                if self.frames() == 0 {
                    "No sample: LOAD opens one".into()
                } else {
                    let secs = self.shared.sample().map_or(0.0, |s| s.frames() as f64 / f64::from(s.rate.max(1)));
                    format!("Sample\t{secs:.2} s")
                }
            }
            Some(Target::Logo) => "Mango Granulizer".into(),
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
            (Target::Root, Btn::Left | Btn::Right) => acts.push(Act::Menu(MenuReq::Root, view::ROOT_BOX.x, view::ROOT_BOX.bottom())),
            (Target::Load, Btn::Left) => acts.push(Act::OpenDialog),
            (Target::Well, Btn::Left) if has => {
                let key = u8::try_from(self.state().root).ok().filter(|&k| k < 128);
                self.view.pressed = Some(Target::Well);
                self.play(key);
            }
            (Target::Logo, Btn::Left | Btn::Right) => acts.push(Act::Menu(MenuReq::About, x, y)),
            _ => {}
        }
        self.hint = self.hint_for(Some(&t));
        acts
    }

    /// Mouse movement during a locked drag (logical px since the last event).
    pub fn delta(&mut self, _dx: f32, dy: f32, m: Mods) -> Vec<Act> {
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
        if let Some(d) = self.drag.take() {
            self.vals.end(&d.id);
        }
        self.view.pressed = None;
        vec![Act::Repaint]
    }

    pub fn moved(&mut self, x: f32, y: f32, _m: Mods) -> Vec<Act> {
        self.view.mouse = Some((x, y));
        if self.drag.is_none() {
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

    /// Wheel: a knob moves a step (10 cents on the pitch, 1 elsewhere; ctrl takes a tenth of the pitch's).
    pub fn wheel(&mut self, x: f32, y: f32, notches: f32, m: Mods) -> Vec<Act> {
        let Some(t) = self.hit(x, y) else { return vec![] };
        let d = notches.signum();
        match &t {
            Target::Knob(id) => {
                let step = if id == params::PITCH_ID { if m.ctrl { 1.0 } else { 10.0 } } else { 1.0 };
                let plain = self.vals.plain(id) + d * step;
                self.vals.edit_plain(id, plain);
            }
            Target::Switch(id) => self.vals.edit(id, if d > 0.0 { 1.0 } else { 0.0 }),
            _ => return vec![],
        }
        self.hint = self.hint_for(Some(&t));
        vec![Act::Repaint]
    }

    pub fn menu(&self, req: &MenuReq) -> MenuDef<Cmd> {
        let mut m = MenuDef::new(MenuStyle::Context);
        match req {
            MenuReq::Param(id) => {
                m.head(&Self::name(id), Cmd::None);
                m.add(MenuItem::item("Reset"), Cmd::Reset(id.clone()));
                m.sep(Cmd::None);
                m.add(MenuItem::item("Copy value"), Cmd::CopyValue(id.clone()));
                m.add(MenuItem::item("Paste value").enabled(self.copied.is_some()), Cmd::PasteValue(id.clone()));
                m.sep(Cmd::None);
                m.add(MenuItem::item("Create automation clip").enabled(self.hosted), Cmd::Automate(id.clone()));
            }
            MenuReq::Root => {
                let root = self.state().root;
                m.head("Root key", Cmd::None);
                for oct in 0..=10 {
                    let mut sub = MenuDef::new(MenuStyle::Context);
                    for k in oct * 12..(oct * 12 + 12).min(128) {
                        sub.add(MenuItem::item(&view::key_name(k)).mark(Mark::Radio(k == root)), Cmd::Root(k));
                    }
                    m.submenu(&format!("Octave {oct}"), sub, Cmd::None);
                }
            }
            MenuReq::About => {
                m.head("Mango Granulizer", Cmd::None);
                m.add(MenuItem::item(&format!("Version {}", env!("CARGO_PKG_VERSION"))).enabled(false), Cmd::None);
                m.add(MenuItem::item("Lemon Studio's recreation of the Granulizer").enabled(false), Cmd::None);
            }
        }
        m
    }

    pub fn pick(&mut self, req: &MenuReq, cmd: Cmd, x: f32, y: f32) -> Vec<Act> {
        match cmd {
            Cmd::None => {}
            Cmd::Reset(id) => self.reset(&id),
            Cmd::CopyValue(id) => self.copied = Some(self.vals.norm(&id)),
            Cmd::PasteValue(id) => {
                if let Some(v) = self.copied {
                    self.vals.edit(&id, v);
                }
            }
            Cmd::TypeIn(id) => return vec![Act::TypeIn(id, x, y)],
            Cmd::Automate(id) => return NAMES.iter().position(|(i, _)| *i == id).map(Act::Automate).into_iter().collect(),
            Cmd::Root(k) => self.shared.set_root(k, &self.params),
        }
        let _ = req;
        vec![Act::Repaint]
    }

    /// The value text a type-in box starts with.
    pub fn value_text(&self, id: &str) -> String {
        self.vals.text(id)
    }

    /// Applies a typed value.
    pub fn typed(&mut self, id: &str, text: &str) {
        if let Some(n) = self.vals.parse(id, text) {
            self.vals.edit(id, n);
        }
    }

    /// Loads a sample (the open dialog's file).
    pub fn load(&mut self, path: &str) {
        if let Err(e) = self.shared.load(path, &self.params) {
            self.hint = e;
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
        let hint = if self.hosted { String::new() } else { self.hint.clone() };
        self.peaks();
        let look = view::Look { state: &st, frames, peaks: &self.peaks.1 };
        self.hits = view::paint(c, palette, &self.vals, &self.view, &look, &hint);
    }
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
    use crate::params::GranParams;
    use crate::view;
    use crate::Shared;

    const TIMER: usize = 1;
    const WM_CHANGED: u32 = WM_APP + 1;

    struct Win {
        hwnd: HWND,
        canvas: Canvas,
        buf: Vec<u32>,
        scale: f32,
        size: (u32, u32),
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

    pub fn open(parent: *mut c_void, scale: f32, params: Arc<GranParams>, shared: Arc<Shared>, ctx: Arc<dyn GuiContext>, slot: Arc<AtomicIsize>) -> Handle {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let px = |v: u32| (v as f32 * scale).round() as u32;
        let size = (px(view::W), px(view::H));
        shared.editor_open.store(true, Ordering::Relaxed);
        unsafe {
            let class = pw::register("MangoGranulizerView", Some(wndproc));
            let win = Box::new(Win {
                hwnd: std::ptr::null_mut(),
                canvas: Canvas::new(size.0, size.1, scale).expect("canvas"),
                buf: vec![0; (size.0 * size.1) as usize],
                scale,
                size,
                logic: Logic::new(params, shared.clone(), Some(ctx)),
                palette: os_plugkit::palette(),
                last_hint: String::new(),
                lock: CursorLock::default(),
                tracking: false,
                slot: slot.clone(),
                last_sig: 0,
                last_raw: u64::MAX,
            });
            let title = pw::wide("Mango Granulizer");
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
                            self.logic.load(&path);
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
                Cmd::Metrics => os_hostlink::pack_size(view::W, view::H),
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
            self.buf.resize((self.size.0 * self.size.1) as usize, 0);
            self.canvas.copy_to_xrgb(&mut self.buf);
            pw::blit(hdc, &self.buf, self.size.0, self.size.1);
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
        let title = pw::wide("Sample for the granulizer");
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
    use crate::params::GranParams;
    use crate::view;
    use crate::Shared;

    struct Ed {
        win: Window,
        canvas: Canvas,
        buf: Vec<u32>,
        scale: f32,
        size: (u32, u32),
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

    pub fn open(parent: px::Parent, scale: f32, params: Arc<GranParams>, shared: Arc<Shared>, ctx: Arc<dyn GuiContext>, slot: &px::Slot) -> Handle {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let size = ((view::W as f32 * scale).round() as u32, (view::H as f32 * scale).round() as u32);
        let sh = shared.clone();
        let wake = slot.clone();
        let link = Peer::register(parent as u64, vec![PageIcon::Sample], (view::W, view::H), move || px::notify(&wake));
        let peer = link.clone();
        let thread = px::spawn(parent, size, slot, move |win| {
            let Ok(canvas) = Canvas::new(size.0, size.1, scale) else { return };
            let logic = Logic::new(params, shared, Some(ctx));
            let ed = Ed { win, canvas, buf: Vec::new(), scale, size, logic, palette: os_plugkit::palette(), last_hint: String::new(), dirty: true, link: peer };
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
                            self.logic.load(&path);
                        }
                        self.dirty = true;
                    }
                }
            }
        }

        fn paint(&mut self) {
            self.dirty = false;
            self.logic.paint(&mut self.canvas, &self.palette);
            self.buf.resize((self.size.0 * self.size.1) as usize, 0);
            self.canvas.copy_to_xrgb(&mut self.buf);
            self.win.blit(&self.buf, self.size.0, self.size.1);
        }
    }

    /// The open sample dialog: the desktop's own (through its portal on Linux; on macOS AppKit's).
    fn file_dialog(win: &Window) -> Option<String> {
        if os_plugkit::no_modal() {
            return None;
        }
        let path = win.dialog(move || {
            rfd::FileDialog::new()
                .set_title("Sample for the granulizer")
                .add_filter("Audio files", &["wav", "flac", "ogg", "wv", "aif", "aiff", "mp3"])
                .pick_file()
        });
        path.flatten().map(|p| p.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::REVERSE_ID;
    use crate::tests::wav;

    fn logic() -> Logic {
        let mut l = Logic::new(Arc::new(GranParams::default()), Arc::new(Shared::default()), None);
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
        let p = wav("editor", 0.5);
        let mut l = logic();
        l.load(&p.to_string_lossy());
        paint(&mut l);
        (l, p)
    }

    #[test]
    fn every_parameter_has_a_control() {
        let l = logic();
        for (id, _) in NAMES {
            assert!(l.hits.iter().any(|h| matches!(&h.t, Target::Knob(i) | Target::Switch(i) if i == id)), "{id}");
        }
        for t in [Target::Load, Target::Well, Target::Root, Target::Logo] {
            centre(&l, &t);
        }
    }

    #[test]
    fn knobs_switches_and_hints() {
        let mut l = logic();
        let (x, y) = centre(&l, &Target::Knob(params::GRAIN_ID.into()));
        assert!(l.down(x, y, Btn::Left, Mods::default(), false).contains(&Act::Lock));
        // 200 px = 5..2000 ms: 4 px up adds about 40.
        for _ in 0..4 {
            l.delta(0.0, -1.0, Mods::default());
        }
        l.up(Btn::Left);
        let grain = l.params.values().grain;
        assert!((88..=92).contains(&grain), "4 px of drag on the grain: {grain}");
        assert!(l.hint.starts_with("Grain: "), "{}", l.hint);
        l.wheel(x, y, 1.0, Mods::default());
        assert_eq!(l.params.values().grain, grain + 1);
        let (x, y) = centre(&l, &Target::Knob(params::PITCH_ID.into()));
        l.wheel(x, y, 1.0, Mods::default());
        assert_eq!(l.params.values().pitch, 10);
        assert_eq!(l.hint, "Pitch shift: +10 cents");
        l.wheel(x, y, 1.0, Mods { ctrl: true, ..Mods::default() });
        assert_eq!(l.params.values().pitch, 11);
        let (x, y) = centre(&l, &Target::Switch(REVERSE_ID.into()));
        l.down(x, y, Btn::Left, Mods::default(), false);
        assert!(l.params.values().reverse);
        assert_eq!(l.hint, "Reverse\ton");
        assert_eq!(l.pick(&MenuReq::Param(REVERSE_ID.into()), Cmd::Automate(REVERSE_ID.into()), 0.0, 0.0), vec![Act::Automate(8)]);
        // A copy across knobs.
        l.pick(&MenuReq::Param(params::GRAIN_ID.into()), Cmd::CopyValue(params::GRAIN_ID.into()), 0.0, 0.0);
        let grain = l.params.values().grain;
        let (x, y) = centre(&l, &Target::Knob(params::SPACING_ID.into()));
        l.wheel(x, y, -10.0, Mods::default());
        l.pick(&MenuReq::Param(params::SPACING_ID.into()), Cmd::PasteValue(params::SPACING_ID.into()), 0.0, 0.0);
        assert_eq!(l.params.values().spacing, grain);
    }

    #[test]
    fn the_root_menu_sets_the_key_the_note_plays_against() {
        let (mut l, p) = loaded();
        l.pick(&MenuReq::Root, Cmd::Root(36), 0.0, 0.0);
        assert_eq!(l.state().root, 36);
        assert_eq!(l.shared.kit.lock().unwrap().as_ref().unwrap().root, 36);
        assert!(l.menu(&MenuReq::Root).find("C3").is_some());
        assert_eq!(l.hint_for(Some(&Target::Root)), "Root key: C3\tthe note plays against it");
        // A click on the well plays the root key; letting go ends the note.
        l.down(view::WAVE.cx(), view::WAVE.cy(), Btn::Left, Mods::default(), false);
        assert_eq!(*l.shared.previews.lock().unwrap(), vec![(36, PREVIEW_VELOCITY, true)]);
        l.up(Btn::Left);
        assert_eq!(l.shared.previews.lock().unwrap().last(), Some(&(36, 0.0, false)));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn values_type_in_and_the_sample_loads() {
        let (mut l, p) = loaded();
        l.typed(params::PITCH_ID, "-200 cents");
        assert_eq!(l.params.values().pitch, -200);
        assert_eq!(l.value_text(params::GRAIN_ID), "50ms");
        l.typed(params::GRAIN_ID, "250ms");
        assert_eq!(l.params.values().grain, 250);
        // A file the granulizer cannot read leaves the sample as it is and the hint says so.
        l.load("Z:\\nowhere.wav");
        assert!(l.hint.contains("nowhere"), "{}", l.hint);
        assert!(l.state().path.ends_with(".wav"), "{}", l.state().path);
        let _ = std::fs::remove_file(p);
    }

    /// `GRAN_PNG=<dir> cargo test -p lemon-granulizer -- --ignored`: the editor, empty and with a sample, as PNGs.
    #[test]
    #[ignore]
    fn render_editor() {
        let Some(dir) = std::env::var_os("GRAN_PNG") else { return };
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
        save(&mut l, "gran_empty", 1.0);
        let path = std::env::var("GRAN_SAMPLE").ok();
        let p = wav("render", 0.5);
        l.load(path.as_deref().unwrap_or(&p.to_string_lossy()));
        l.vals.edit(params::RANDOM_ID, 0.35);
        l.vals.edit(params::HOLD_ID, 0.6);
        l.hint = "Sample\t0.10 s".into();
        save(&mut l, "gran_sample", 1.0);
        save(&mut l, "gran_sample_x2", 2.0);
        let _ = std::fs::remove_file(p);
    }
}
