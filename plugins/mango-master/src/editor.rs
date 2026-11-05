// Copyright (C) 2026 Artig3nce, apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The plugin editor: a child of the host's window (Win32, X11 on Linux, an NSView on macOS) that paints `view`, and the
//! editor logic, which is the same on all of them.

use std::any::Any;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
#[cfg(windows)]
use std::sync::atomic::AtomicIsize;

use nih_plug::prelude::*;
use os_plugkit::vals::Vals;

use crate::auto::{Solution, TARGETS};
use crate::params::{MasterParams, COMP_IDS, EQ_IDS};
use crate::{view, Shared, LISTEN_S};

/// Pixels of drag for a knob's whole range, and how much finer with Ctrl or Shift.
const KNOB_PX: f32 = 220.0;
const FINE: f32 = 0.1;
/// The wheel moves this part of the range per notch.
const WHEEL_PART: f32 = 0.02;

pub struct MasterEditor {
    params: Arc<MasterParams>,
    shared: Arc<Shared>,
    scale: AtomicU32,
    #[cfg(windows)]
    hwnd: Arc<AtomicIsize>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    slot: os_plugkit::gui::Slot,
}

impl MasterEditor {
    pub fn new(params: Arc<MasterParams>, shared: Arc<Shared>) -> Self {
        MasterEditor {
            params,
            shared,
            scale: AtomicU32::new(1f32.to_bits()),
            #[cfg(windows)]
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

impl Editor for MasterEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn Any + Send> {
        let scale = f32::from_bits(self.scale.load(Ordering::Relaxed));
        let (params, shared) = (self.params.clone(), self.shared.clone());
        match parent {
            #[cfg(windows)]
            ParentWindowHandle::Win32Hwnd(hwnd) => Box::new(win32::open(hwnd, scale, params, shared, context, self.hwnd.clone())),
            #[cfg(target_os = "linux")]
            ParentWindowHandle::X11Window(window) => Box::new(gui::open(window, scale, params, shared, context, &self.slot)),
            #[cfg(target_os = "macos")]
            ParentWindowHandle::AppKitNsView(v) => {
                let scale = os_plugkit::gui::backing_scale(v as usize);
                Box::new(gui::open(v as usize, scale, params, shared, context, &self.slot))
            }
            _ => {
                let _ = (context, scale, params, shared);
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

/// A knob being dragged: where it started.
#[derive(Clone, Copy)]
struct Drag {
    id: &'static str,
    y: f32,
    norm: f32,
}

pub struct Logic {
    /// The parameters the vals' pointers point into: held here so they stay put for the editor's life.
    #[allow(dead_code)]
    params: Arc<MasterParams>,
    pub shared: Arc<Shared>,
    pub vals: Vals,
    pub mouse: Option<(f32, f32)>,
    drag: Option<Drag>,
    pub status: String,
}

const READY: &str = "Press Auto Master, then play the loudest part of your song.";

impl Logic {
    pub fn new(params: Arc<MasterParams>, shared: Arc<Shared>, context: Option<Arc<dyn GuiContext>>) -> Self {
        let vals = Vals::new(params.as_ref(), params.live.clone(), context);
        Logic { params, shared, vals, mouse: None, drag: None, status: READY.into() }
    }

    fn target(&self) -> usize {
        (self.vals.plain("target") as usize).min(TARGETS.len() - 1)
    }

    /// Auto Master's button: start listening; while listening, finish if enough was heard, else stop.
    fn auto_pressed(&mut self) {
        let s = &self.shared;
        if s.solving.load(Ordering::Acquire) {
            return;
        }
        if s.listening.load(Ordering::Acquire) {
            if s.can_finish() {
                s.finish_listening(self.target());
                self.status = "Working out the settings...".into();
            } else {
                s.cancel_listening();
                self.status = "Stopped: it needs at least 3 seconds of your song.".into();
            }
        } else {
            s.start_listening();
            self.status = "Play your song now. Press the button again when you're done, or wait 30 seconds.".into();
        }
    }

    /// What the tick looks after: a capture that filled up, and an answer that arrived.
    pub fn poll(&mut self) {
        let listening = self.shared.listening.load(Ordering::Acquire);
        let held = self.shared.capture.try_lock().is_ok_and(|c| c.is_some());
        if !listening && held {
            self.shared.finish_listening(self.target());
            self.status = "Working out the settings...".into();
        }
        let answer = self.shared.solution.try_lock().ok().and_then(|mut s| s.take());
        match answer {
            Some(Some(s)) => self.apply(&s),
            Some(None) => self.status = "That was silence: play your song and try again.".into(),
            None => {}
        }
    }

    /// Moves every knob to Auto Master's answer, one undoable edit each.
    pub fn apply(&mut self, s: &Solution) {
        let set = s.settings;
        for (id, v) in EQ_IDS.iter().zip(set.eq_db) {
            self.vals.edit_plain(id, v);
        }
        for (id, v) in COMP_IDS.iter().zip(set.comp_db) {
            self.vals.edit_plain(id, v);
        }
        self.vals.edit_plain("ratio", set.ratio);
        self.vals.edit_plain("width", set.width * 100.0);
        self.vals.edit_plain("mono_bass", if set.mono_bass { 1.0 } else { 0.0 });
        self.vals.edit_plain("saturation", set.saturation * 100.0);
        self.vals.edit_plain("gain", set.gain_db);
        self.vals.edit_plain("ceiling", set.ceiling_db);
        self.shared.reset_meters.store(true, Ordering::Release);
        self.status = format!(
            "Set for {}: it measured {:.1} LUFS and now reaches {:.1} LUFS, peaks at {:.1} dB. Adjust anything by ear.",
            TARGETS[self.target()].name,
            s.before.lufs,
            s.lufs,
            s.true_peak_db
        );
    }

    fn toggle(&mut self, id: &str) {
        let on = self.vals.plain(id) > 0.5;
        self.vals.edit_plain(id, if on { 0.0 } else { 1.0 });
    }

    /// A press: left drags a knob or presses a button, right puts a knob back to its default.
    pub fn down(&mut self, x: f32, y: f32, right: bool) {
        self.mouse = Some((x, y));
        if let Some(k) = view::knob_at(x, y) {
            if right {
                self.vals.edit(k.id, self.vals.default_norm(k.id));
            } else {
                self.vals.begin(k.id);
                self.drag = Some(Drag { id: k.id, y, norm: self.vals.norm(k.id) });
            }
            return;
        }
        if right {
            return;
        }
        if view::AUTO.contains(x, y) {
            self.auto_pressed();
        } else if view::COMPARE.contains(x, y) {
            self.toggle("compare");
        } else if view::MONO_BASS.contains(x, y) {
            self.toggle("mono_bass");
        } else if view::RESET_METERS.contains(x, y) {
            self.shared.reset_meters.store(true, Ordering::Release);
        } else if let Some(i) = view::TARGETS_AT.iter().position(|r| r.contains(x, y)) {
            self.vals.edit_plain("target", i as f32);
            // Another target: the song Auto Master heard is worked out for it at once.
            if self.shared.has_heard() {
                self.shared.resolve(i);
                self.status = format!("Working out the settings for {}...", TARGETS[i].name);
            } else {
                self.status = format!("{} is chosen. Play your song for a few seconds, then press it again (or press Auto Master).", TARGETS[i].name);
            }
        }
    }

    /// A move: hover looks, and a drag follows. `fine`: Ctrl or Shift held.
    pub fn moved(&mut self, x: f32, y: f32, fine: bool) {
        self.mouse = Some((x, y));
        if let Some(d) = self.drag.as_mut() {
            let k = if fine { FINE } else { 1.0 };
            let travel = (d.y - y) / KNOB_PX * k;
            if fine {
                // From here on, relative to where fine dragging began.
                d.norm = (d.norm + travel).clamp(0.0, 1.0);
                d.y = y;
            }
            let v = if fine { d.norm } else { (d.norm + travel).clamp(0.0, 1.0) };
            let id = d.id;
            self.vals.set(id, v);
        }
    }

    pub fn up(&mut self) {
        if let Some(d) = self.drag.take() {
            self.vals.end(d.id);
        }
    }

    pub fn wheel(&mut self, x: f32, y: f32, notches: f32) {
        if let Some(k) = view::knob_at(x, y) {
            self.vals.edit(k.id, (self.vals.norm(k.id) + notches * WHEEL_PART).clamp(0.0, 1.0));
        }
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn display(&self) -> view::Display<'_> {
        let s = &self.shared;
        let target = self.target();
        view::Display {
            knobs: view::KNOBS.iter().map(|k| (self.vals.norm(k.id), self.vals.text(k.id))).collect(),
            target,
            targets: [TARGETS[0].name, TARGETS[1].name, TARGETS[2].name, TARGETS[3].name],
            mono_bass: self.vals.plain("mono_bass") > 0.5,
            compare: self.vals.plain("compare") > 0.5,
            listening: s.listening.load(Ordering::Acquire),
            solving: s.solving.load(Ordering::Acquire),
            listened: (s.captured_s.get(), LISTEN_S as f32),
            can_finish: s.can_finish(),
            status: &self.status,
            momentary: s.momentary.get(),
            integrated: s.integrated.get(),
            true_peak: s.true_peak.get(),
            reduction: s.reduction.get(),
            correlation: s.correlation.get(),
            ceiling: self.vals.plain("ceiling"),
            target_lufs: TARGETS[target].lufs,
            hover: self.mouse,
        }
    }
}

impl Drop for Logic {
    fn drop(&mut self) {
        self.up();
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod gui {
    use std::sync::Arc;

    use nih_plug::prelude::GuiContext;
    use os_plugkit::gui::{self as px, Button, Input, Window};
    use os_ui::Canvas;

    use super::Logic;
    use crate::params::MasterParams;
    use crate::{view, Shared};

    struct Ed {
        win: Window,
        canvas: Canvas,
        buf: Vec<u32>,
        scale: f32,
        size: (u32, u32),
        logic: Logic,
    }

    pub fn open(parent: px::Parent, scale: f32, params: Arc<MasterParams>, shared: Arc<Shared>, context: Arc<dyn GuiContext>, slot: &px::Slot) -> Option<px::EditorThread> {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let size = ((view::W as f32 * scale).round() as u32, (view::H as f32 * scale).round() as u32);
        px::spawn(parent, size, slot, move |win| {
            let Ok(canvas) = Canvas::new(size.0, size.1, scale) else { return };
            let mut ed = Ed { win, canvas, buf: Vec::new(), scale, size, logic: Logic::new(params, shared, Some(context)) };
            ed.run();
        })
    }

    impl Ed {
        fn run(&mut self) {
            self.paint();
            while let Some(input) = self.win.next() {
                match input {
                    Input::Expose => self.paint(),
                    Input::Tick => {
                        self.logic.poll();
                        self.logic.vals.expire();
                        self.paint();
                    }
                    Input::Move { x, y } => {
                        let (ctrl, shift, _) = self.win.mods();
                        self.logic.moved(x / self.scale, y / self.scale, ctrl || shift);
                        self.paint();
                    }
                    Input::Leave => {
                        if !self.logic.dragging() {
                            self.logic.mouse = None;
                        }
                        self.paint();
                    }
                    Input::Down { x, y, btn, .. } => {
                        self.logic.down(x / self.scale, y / self.scale, btn == Button::Right);
                        self.paint();
                    }
                    Input::Up { .. } => {
                        self.logic.up();
                        self.paint();
                    }
                    Input::Wheel { x, y, notches } => {
                        self.logic.wheel(x / self.scale, y / self.scale, notches);
                        self.paint();
                    }
                    Input::Delta { .. } | Input::Key { .. } | Input::Drop { .. } => {}
                }
            }
        }

        fn paint(&mut self) {
            if !self.win.idle() {
                return;
            }
            view::paint(&mut self.canvas, &self.logic.display());
            self.buf.resize((self.size.0 * self.size.1) as usize, 0);
            self.canvas.copy_to_xrgb(&mut self.buf);
            self.win.blit(&self.buf, self.size.0, self.size.1);
        }
    }
}

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::Arc;

    use nih_plug::prelude::GuiContext;
    use os_plugkit::win as pw;
    use os_ui::Canvas;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    use super::Logic;
    use crate::params::MasterParams;
    use crate::{view, Shared};

    const TIMER: usize = 1;
    const WM_CHANGED: u32 = WM_APP + 1;

    struct Win {
        hwnd: HWND,
        canvas: Canvas,
        pixels: Vec<u32>,
        scale: f32,
        size: (u32, u32),
        logic: Logic,
        slot: Arc<AtomicIsize>,
    }

    pub struct Handle(isize, Arc<AtomicIsize>);
    // SAFETY: the handle is only an HWND value; the wrapper drops it on the GUI thread.
    unsafe impl Send for Handle {}

    impl Drop for Handle {
        fn drop(&mut self) {
            self.1.store(0, Ordering::Release);
            unsafe { DestroyWindow(self.0 as HWND) };
        }
    }

    pub fn post_changed(hwnd: isize) {
        if hwnd != 0 {
            unsafe { PostMessageW(hwnd as HWND, WM_CHANGED, 0, 0) };
        }
    }

    pub fn open(parent: *mut c_void, scale: f32, params: Arc<MasterParams>, shared: Arc<Shared>, context: Arc<dyn GuiContext>, slot: Arc<AtomicIsize>) -> Handle {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let size = ((view::W as f32 * scale).round() as u32, (view::H as f32 * scale).round() as u32);
        unsafe {
            let class = pw::register("MangoMasterView", Some(wndproc));
            let win = Box::new(Win {
                hwnd: std::ptr::null_mut(),
                canvas: Canvas::new(size.0, size.1, scale).expect("Mango Master canvas"),
                pixels: vec![0; (size.0 * size.1) as usize],
                scale,
                size,
                logic: Logic::new(params, shared, Some(context)),
                slot: slot.clone(),
            });
            let title = pw::wide("Mango Master");
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
            SetTimer(hwnd, TIMER, 30, None);
            Handle(hwnd as isize, slot)
        }
    }

    unsafe fn state(hwnd: HWND) -> Option<&'static mut Win> {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Win;
        if ptr.is_null() { None } else { Some(unsafe { &mut *ptr }) }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_NCCREATE {
                let cs = lp as *const CREATESTRUCTW;
                let win = (*cs).lpCreateParams as *mut Win;
                (*win).hwnd = hwnd;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, win as isize);
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            if msg == WM_NCDESTROY {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Win;
                if !ptr.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    (*ptr).slot.store(0, Ordering::Release);
                    drop(Box::from_raw(ptr));
                }
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            let Some(win) = state(hwnd) else { return DefWindowProcW(hwnd, msg, wp, lp) };
            match msg {
                WM_PAINT => {
                    let (hdc, ps) = pw::begin_paint(hwnd);
                    view::paint(&mut win.canvas, &win.logic.display());
                    win.pixels.resize((win.size.0 * win.size.1) as usize, 0);
                    win.canvas.copy_to_xrgb(&mut win.pixels);
                    pw::blit(hdc, &win.pixels, win.size.0, win.size.1);
                    pw::end_paint(hwnd, &ps);
                    0
                }
                WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
                    SetFocus(hwnd);
                    let (x, y) = pw::xy(lp);
                    let right = msg == WM_RBUTTONDOWN;
                    win.logic.down(x as f32 / win.scale, y as f32 / win.scale, right);
                    if win.logic.dragging() {
                        SetCapture(hwnd);
                    }
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                    0
                }
                WM_MOUSEMOVE => {
                    let (x, y) = pw::xy(lp);
                    let (ctrl, shift, _) = pw::mods();
                    win.logic.moved(x as f32 / win.scale, y as f32 / win.scale, ctrl || shift);
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                    0
                }
                WM_LBUTTONUP | WM_RBUTTONUP => {
                    win.logic.up();
                    if msg == WM_LBUTTONUP {
                        ReleaseCapture();
                    }
                    0
                }
                WM_CAPTURECHANGED => {
                    win.logic.up();
                    0
                }
                WM_MOUSEWHEEL => {
                    let notches = f32::from((wp >> 16) as u16 as i16) / 120.0;
                    if let Some((x, y)) = win.logic.mouse {
                        win.logic.wheel(x, y, notches);
                    }
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                    0
                }
                WM_TIMER | WM_CHANGED => {
                    if msg == WM_TIMER {
                        win.logic.poll();
                        win.logic.vals.expire();
                    }
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                    0
                }
                WM_DESTROY => {
                    KillTimer(hwnd, TIMER);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto::{Report, Solution};
    use crate::dsp::Settings;

    fn logic() -> Logic {
        Logic::new(Arc::new(MasterParams::default()), Arc::new(Shared::default()), None)
    }

    fn centre(id: &str) -> (f32, f32) {
        let k = view::KNOBS.iter().find(|k| k.id == id).unwrap();
        (k.x, k.y)
    }

    #[test]
    fn no_two_controls_overlap_and_all_fit_the_window() {
        let knobs: Vec<_> = view::KNOBS.iter().collect();
        for (i, a) in knobs.iter().enumerate() {
            assert!(a.x > view::KNOB_R && a.x < view::W as f32 - view::KNOB_R && a.y < view::H as f32 - view::KNOB_R, "{}", a.id);
            for b in &knobs[i + 1..] {
                assert!((a.x - b.x).hypot(a.y - b.y) > 2.0 * view::KNOB_R, "{} {}", a.id, b.id);
            }
        }
        // Each parameter's knob is found where it's drawn.
        for k in knobs {
            assert_eq!(view::knob_at(k.x, k.y).map(|h| h.id), Some(k.id));
        }
    }

    #[test]
    fn dragging_a_knob_moves_it_and_right_click_resets_it() {
        let mut l = logic();
        let (x, y) = centre("eq_mid");
        let at = l.vals.norm("eq_mid");
        l.down(x, y, false);
        l.moved(x, y - 55.0, false);
        l.up();
        assert!((l.vals.norm("eq_mid") - at - 0.25).abs() < 0.01, "{}", l.vals.norm("eq_mid"));
        assert_eq!(l.params.settings().eq_db[2].round(), 6.0);
        l.down(x, y, true);
        assert_eq!(l.vals.plain("eq_mid"), 0.0);
        l.wheel(x, y, 5.0);
        assert!((l.vals.norm("eq_mid") - 0.6).abs() < 0.01);
    }

    #[test]
    fn buttons_switch_targets_compare_and_mono_bass() {
        let mut l = logic();
        let r = view::TARGETS_AT[2];
        l.down(r.cx(), r.cy(), false);
        assert_eq!(l.target(), 2);
        assert_eq!(l.display().target_lufs, TARGETS[2].lufs);
        l.down(view::COMPARE.cx(), view::COMPARE.cy(), false);
        assert!(l.params.comparing());
        l.down(view::COMPARE.cx(), view::COMPARE.cy(), false);
        assert!(!l.params.comparing());
        l.down(view::MONO_BASS.cx(), view::MONO_BASS.cy(), false);
        assert!(l.params.settings().mono_bass);
    }

    /// Another target works the settings out again from the song heard: Loud is louder than Streaming without listening again.
    #[test]
    fn another_target_is_worked_out_from_the_song_heard() {
        use crate::auto::Capture;
        let mut l = logic();
        let rate = l.shared.rate();
        let mut c = Capture::new(6.0, rate);
        let tone: Vec<f32> = (0..(5.0 * rate) as usize).map(|i| 0.1 * (i as f32 * 0.05).sin() + 0.05 * (i as f32 * 0.31).sin()).collect();
        c.push(&tone, &tone);
        // What merely played through (no Auto Master pressed) is enough.
        *l.shared.background.lock().unwrap() = Some(c);
        assert!(l.shared.heard.lock().unwrap().is_none());
        let mut gain_for = |l: &mut Logic, i: usize| {
            let r = view::TARGETS_AT[i];
            l.down(r.cx(), r.cy(), false);
            assert!(l.shared.solving.load(Ordering::Acquire) || l.shared.solution.lock().unwrap().is_some());
            while l.shared.solving.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            l.poll();
            l.vals.plain("gain")
        };
        let streaming = gain_for(&mut l, 0);
        let loud = gain_for(&mut l, 2);
        assert!(loud > streaming + 3.0, "streaming {streaming} dB, loud {loud} dB");
    }

    #[test]
    fn auto_master_starts_cancels_and_needs_enough_song() {
        let mut l = logic();
        let at = (view::AUTO.cx(), view::AUTO.cy());
        l.down(at.0, at.1, false);
        assert!(l.shared.listening.load(Ordering::Acquire));
        // Pressed again too soon: it stops and says why.
        l.down(at.0, at.1, false);
        assert!(!l.shared.listening.load(Ordering::Acquire));
        assert!(l.status.contains("3 seconds"), "{}", l.status);
        l.poll();
        assert!(!l.shared.solving.load(Ordering::Acquire), "nothing left to work on");
    }

    #[test]
    fn an_answer_moves_the_knobs_and_says_what_it_did() {
        let mut l = logic();
        let settings = Settings { eq_db: [1.0, -2.0, 0.5, 0.0, 3.0], comp_db: [-18.0, -20.0, -22.0], ratio: 2.5, width: 1.15, mono_bass: true, saturation: 0.1, gain_db: 7.5, ceiling_db: -1.0 };
        let before = Report { lufs: -20.0, true_peak_db: -6.0, bands_db: [-20.0; 5], side_db: -10.0 };
        *l.shared.solution.lock().unwrap() = Some(Some(Solution { settings, before, lufs: -14.0, true_peak_db: -1.0 }));
        l.poll();
        let got = l.params.settings();
        for (a, b) in got.eq_db.iter().zip(settings.eq_db).chain(got.comp_db.iter().zip(settings.comp_db)) {
            assert!((a - b).abs() < 0.06, "{a} {b}");
        }
        assert!((got.gain_db - 7.5).abs() < 0.06 && (got.width - 1.15).abs() < 0.01 && got.mono_bass && (got.saturation - 0.1).abs() < 0.01);
        assert!(l.status.contains("-14.0 LUFS"), "{}", l.status);
    }

    #[test]
    fn silence_gets_a_message_not_a_change() {
        let mut l = logic();
        *l.shared.solution.lock().unwrap() = Some(None);
        l.poll();
        assert!(l.status.contains("silence"));
        assert_eq!(l.params.settings(), Settings::default());
    }

    /// `ORANGE_MASTER_PNG=<file> cargo test -p mango-master -- --ignored render_editor`: the editor as a PNG.
    #[test]
    #[ignore]
    fn render_editor() {
        let Some(file) = std::env::var_os("ORANGE_MASTER_PNG") else { return };
        let mut l = logic();
        l.shared.momentary.set(-13.2);
        l.shared.integrated.set(-14.1);
        l.shared.true_peak.set(-1.1);
        l.shared.reduction.set(-2.4);
        l.shared.correlation.set(0.62);
        l.status = "Set for Streaming: it measured -21.4 LUFS and now reaches -14.0 LUFS, peaks at -1.1 dB. Adjust anything by ear.".into();
        for (id, v) in [("eq_low", 1.5), ("eq_high", 2.0), ("eq_mid", -1.0), ("comp_low", -18.0), ("comp_mid", -20.0), ("width", 115.0), ("gain", 7.5), ("saturation", 10.0)] {
            l.vals.edit_plain(id, v);
        }
        l.vals.edit_plain("mono_bass", 1.0);
        let mut c = os_ui::Canvas::new(view::W, view::H, 1.0).unwrap();
        view::paint(&mut c, &l.display());
        c.pixmap.save_png(file).unwrap();
    }
}
