// Copyright (C) 2026 mikobuntu
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Mango Studio: https://github.com/mikobuntu/Mango-Studio-Mikobuntu

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(windows)]
use std::sync::atomic::AtomicIsize;

use nih_plug::prelude::*;
use os_plugkit::vals::Vals;

use crate::dsp;
use crate::params::{PRESET_IDS, SLIDER_IDS, SfxrParams, WAVE};
use crate::view;
use crate::Shared;

pub struct SfxrEditor {
    params: Arc<SfxrParams>,
    shared: Arc<Shared>,
    scale: AtomicU32,
    #[cfg(windows)]
    hwnd: Arc<AtomicIsize>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    slot: os_plugkit::gui::Slot,
}

impl SfxrEditor {
    pub fn new(params: Arc<SfxrParams>, shared: Arc<Shared>) -> Self {
        Self {
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

impl Editor for SfxrEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn Any + Send> {
        let scale = f32::from_bits(self.scale.load(Ordering::Relaxed));
        match parent {
            #[cfg(windows)]
            ParentWindowHandle::Win32Hwnd(hwnd) => Box::new(win32::open(
                hwnd,
                scale,
                self.params.clone(),
                self.shared.clone(),
                context,
                self.hwnd.clone(),
            )),
            #[cfg(target_os = "linux")]
            ParentWindowHandle::X11Window(window) => Box::new(gui::open(
                window,
                scale,
                self.params.clone(),
                self.shared.clone(),
                context,
                &self.slot,
            )),
            #[cfg(target_os = "macos")]
            ParentWindowHandle::AppKitNsView(view) => {
                let scale = os_plugkit::gui::backing_scale(view as usize);
                Box::new(gui::open(
                    view as usize,
                    scale,
                    self.params.clone(),
                    self.shared.clone(),
                    context,
                    &self.slot,
                ))
            }
            _ => {
                let _ = (context, scale);
                Box::new(())
            }
        }
    }

    fn size(&self) -> (u32, u32) {
        (view::WIDTH, view::HEIGHT)
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

/// What a click or drag can act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Wave(usize),
    Preset(usize),
    Play,
    Param(&'static str),
}

fn target_at(x: f32, y: f32) -> Option<Target> {
    if let Some(i) = view::WAVE_RECTS.iter().position(|r| r.contains(x, y)) {
        return Some(Target::Wave(i));
    }
    if let Some(i) = view::PRESET_RECTS.iter().position(|r| r.contains(x, y)) {
        return Some(Target::Preset(i));
    }
    if view::PLAY_RECT.contains(x, y) {
        return Some(Target::Play);
    }
    view::param_at(x, y).map(Target::Param)
}

pub struct Logic {
    /// The parameters the vals' pointers point into: held here so they stay put for the editor's life.
    #[allow(dead_code)]
    pub params: Arc<SfxrParams>,
    pub shared: Arc<Shared>,
    pub vals: Vals,
    pub mouse: Option<(f32, f32)>,
    pub drag_param: Option<&'static str>,
    /// The status line's news (what the last click rolled).
    pub status: String,
}

impl Logic {
    pub fn new(params: Arc<SfxrParams>, shared: Arc<Shared>, context: Option<Arc<dyn GuiContext>>) -> Self {
        let vals = Vals::new(params.as_ref(), params.live.clone(), context);
        Logic { params, shared, vals, mouse: None, drag_param: None, status: String::new() }
    }

    pub fn display(&self) -> view::Display {
        view::Display {
            wave: self.vals.norm(WAVE),
            controls: std::array::from_fn(|index| {
                let id = SLIDER_IDS[index];
                view::Control { norm: self.vals.norm(id), text: self.vals.text(id) }
            }),
            mouse: self.mouse,
            status: self.status.clone(),
        }
    }

    pub fn set_drag_value(&mut self, _x: f32, y: f32) {
        let Some(id) = self.drag_param else { return };
        self.vals.set(id, view::param_norm_at(id, y));
    }

    /// A move: hover looks, and a drag follows.
    pub fn moved(&mut self, x: f32, y: f32) {
        self.mouse = Some((x, y));
        if self.drag_param.is_some() {
            self.set_drag_value(x, y);
        }
    }

    /// A press: a drag on a slider (a right press resets it to zero, as sfxr's does), a wave pick,
    /// a generator roll, or Play.
    pub fn down(&mut self, x: f32, y: f32, right: bool) {
        match target_at(x, y) {
            Some(Target::Param(id)) => {
                if right {
                    self.vals.edit_plain(id, 0.0);
                } else {
                    self.drag_param = Some(id);
                    self.vals.begin(id);
                    self.set_drag_value(x, y);
                }
            }
            Some(Target::Wave(i)) if !right => {
                self.vals.edit(WAVE, i as f32 / 3.0);
            }
            Some(Target::Preset(i)) if !right => self.generate(dsp::PRESETS[i]),
            Some(Target::Play) if !right => self.play(),
            _ => {}
        }
    }

    pub fn up(&mut self) {
        if let Some(id) = self.drag_param.take() {
            self.vals.end(id);
        }
    }

    /// Rolls a generator's patch onto the sliders (one edit each, so the host records it), tells so
    /// in the status line, and sounds it.
    pub fn generate(&mut self, preset: dsp::Preset) {
        let mut rng = dsp::Rng::from_clock();
        let patch = preset.roll(&mut rng);
        for id in PRESET_IDS {
            self.vals.edit_plain(id, patch.get(id));
        }
        self.vals.edit(WAVE, patch.wave as f32 / 3.0);
        self.status = format!("ROLLED {}", preset.name().to_uppercase());
        self.queue_preview();
    }

    /// Sounds the current sliders (Space or Enter too, as sfxr's window does).
    pub fn play(&mut self) {
        self.status.clear();
        self.queue_preview();
    }

    /// Hands the audio thread a preview note (A4).
    fn queue_preview(&self) {
        if let Ok(mut q) = self.shared.previews.lock() {
            q.push(view::PREVIEW_KEY);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod gui {
    use std::sync::Arc;

    use nih_plug::prelude::GuiContext;
    use os_plugkit::gui::{self as px, keysym, Button, Input, Window};
    use os_ui::Canvas;

    use super::Logic;
    use crate::params::SfxrParams;
    use crate::{Shared, view};

    struct Ed {
        win: Window,
        canvas: Canvas,
        buf: Vec<u32>,
        scale: f32,
        size: (u32, u32),
        logic: Logic,
    }

    pub fn open(
        parent: px::Parent,
        scale: f32,
        params: Arc<SfxrParams>,
        shared: Arc<Shared>,
        context: Arc<dyn GuiContext>,
        slot: &px::Slot,
    ) -> Option<px::EditorThread> {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let size = ((view::WIDTH as f32 * scale).round() as u32, (view::HEIGHT as f32 * scale).round() as u32);
        px::spawn(parent, size, slot, move |win| {
            let Ok(canvas) = Canvas::new(size.0, size.1, scale) else { return };
            let mut ed = Ed {
                win,
                canvas,
                buf: Vec::new(),
                scale,
                size,
                logic: Logic::new(params, shared, Some(context)),
            };
            ed.run();
        })
    }

    impl Ed {
        fn run(&mut self) {
            self.paint();
            while let Some(input) = self.win.next() {
                match input {
                    Input::Expose | Input::Tick => self.paint(),
                    Input::Move { x, y } => {
                        self.logic.moved(x / self.scale, y / self.scale);
                        self.paint();
                    }
                    Input::Leave => {
                        if self.logic.drag_param.is_none() {
                            self.logic.mouse = None;
                        }
                        self.paint();
                    }
                    Input::Down { x, y, btn, .. } => {
                        let (x, y) = (x / self.scale, y / self.scale);
                        self.logic.mouse = Some((x, y));
                        self.logic.down(x, y, btn == Button::Right);
                        self.paint();
                    }
                    Input::Up { .. } => self.logic.up(),
                    Input::Key { sym, text } => {
                        if text == Some(' ') || sym == keysym::RETURN || sym == keysym::KP_ENTER {
                            self.logic.play();
                            self.paint();
                        }
                    }
                    Input::Delta { .. } | Input::Wheel { .. } | Input::Drop { .. } => {}
                }
                if self.win.idle() {
                    self.paint();
                }
            }
        }

        fn paint(&mut self) {
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
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_RETURN, VK_SPACE};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    use super::{Logic, target_at};
    use crate::params::SfxrParams;
    use crate::{Shared, view};

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

    pub fn open(
        parent: *mut c_void,
        scale: f32,
        params: Arc<SfxrParams>,
        shared: Arc<Shared>,
        context: Arc<dyn GuiContext>,
        slot: Arc<AtomicIsize>,
    ) -> Handle {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let size = ((view::WIDTH as f32 * scale).round() as u32, (view::HEIGHT as f32 * scale).round() as u32);
        unsafe {
            let class = pw::register("MangoSfxrView", Some(wndproc));
            let win = Box::new(Win {
                hwnd: std::ptr::null_mut(),
                canvas: Canvas::new(size.0, size.1, scale).expect("Sfxr canvas"),
                pixels: vec![0; (size.0 * size.1) as usize],
                scale,
                size,
                logic: Logic::new(params, shared, Some(context)),
                slot: slot.clone(),
            });
            let title = pw::wide("Mango Sfxr");
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
                    let _ = view::paint(&mut win.canvas, &win.logic.display());
                    win.pixels.resize((win.size.0 * win.size.1) as usize, 0);
                    win.canvas.copy_to_xrgb(&mut win.pixels);
                    pw::blit(hdc, &win.pixels, win.size.0, win.size.1);
                    pw::end_paint(hwnd, &ps);
                    0
                }
                WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
                    SetFocus(hwnd);
                    let (x, y) = pw::xy(lp);
                    let (x, y) = (x as f32 / win.scale, y as f32 / win.scale);
                    win.logic.mouse = Some((x, y));
                    let right = msg == WM_RBUTTONDOWN;
                    let target = target_at(x, y);
                    if !right && matches!(target, Some(super::Target::Param(_))) {
                        SetCapture(hwnd);
                    }
                    win.logic.down(x, y, right);
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                    0
                }
                WM_MOUSEMOVE => {
                    let (x, y) = pw::xy(lp);
                    win.logic.moved(x as f32 / win.scale, y as f32 / win.scale);
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
                WM_KEYDOWN if wp as u16 == VK_SPACE || wp as u16 == VK_RETURN => {
                    win.logic.play();
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                    0
                }
                WM_TIMER | WM_CHANGED => {
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
