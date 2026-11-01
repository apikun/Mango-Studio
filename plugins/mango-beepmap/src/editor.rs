// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(windows)]
use std::sync::atomic::AtomicIsize;

use nih_plug::prelude::*;
use os_plugkit::vals::Vals;

use crate::params::{CONTROL_IDS, BeepMapParams};
use crate::{Shared, view};

pub struct BeepMapEditor {
    params: Arc<BeepMapParams>,
    shared: Arc<Shared>,
    scale: AtomicU32,
    #[cfg(windows)]
    hwnd: Arc<AtomicIsize>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    slot: os_plugkit::gui::Slot,
}

impl BeepMapEditor {
    pub fn new(params: Arc<BeepMapParams>, shared: Arc<Shared>) -> Self {
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

impl Editor for BeepMapEditor {
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

struct Logic {
    params: Arc<BeepMapParams>,
    shared: Arc<Shared>,
    vals: Vals,
    mouse: Option<(f32, f32)>,
    drag_param: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Load,
    Param(&'static str),
}

impl Logic {
    fn new(params: Arc<BeepMapParams>, shared: Arc<Shared>, context: Arc<dyn GuiContext>) -> Self {
        let vals = Vals::new(params.as_ref(), params.live.clone(), Some(context));
        Self { params, shared, vals, mouse: None, drag_param: None }
    }

    fn display(&self) -> view::Display {
        let mut d = self.shared.display();
        d.controls = std::array::from_fn(|index| {
            let id = CONTROL_IDS[index];
            view::Control { norm: self.vals.norm(id), text: self.vals.text(id) }
        });
        d
    }

    fn set_drag_value(&mut self, _x: f32, y: f32) {
        let Some(id) = self.drag_param else { return };
        if let Some(index) = CONTROL_IDS.iter().position(|candidate| *candidate == id) {
            self.vals.set(id, view::knob_norm_at(index, y));
        }
    }

    fn begin_drag(&mut self, id: &'static str, x: f32, y: f32) {
        self.drag_param = Some(id);
        self.vals.begin(id);
        self.set_drag_value(x, y);
    }

    fn end_drag(&mut self) {
        if let Some(id) = self.drag_param.take() {
            self.vals.end(id);
        }
    }

    fn load(&self, path: &str) {
        if let Err(e) = self.shared.load(path, &self.params) {
            eprintln!("Mango BeepMap: {e}");
        }
    }
}

fn target_at(x: f32, y: f32) -> Option<Target> {
    if view::LOAD.contains(x, y) {
        Some(Target::Load)
    } else {
        CONTROL_IDS.iter().enumerate().find_map(|(index, id)| view::knob_region(index).contains(x, y).then_some(Target::Param(id)))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod gui {
    use std::sync::Arc;

    use nih_plug::prelude::GuiContext;
    use os_plugkit::gui::{self as px, Input, Window};
    use os_ui::Canvas;

    use super::{Logic, Target, target_at};
    use crate::params::BeepMapParams;
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
        params: Arc<BeepMapParams>,
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
                logic: Logic::new(params, shared, context),
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
                        let (x, y) = (x / self.scale, y / self.scale);
                        self.logic.mouse = Some((x, y));
                        if self.logic.drag_param.is_some() {
                            self.logic.set_drag_value(x, y);
                        }
                        self.paint();
                    }
                    Input::Leave => {
                        if self.logic.drag_param.is_none() {
                            self.logic.mouse = None;
                        }
                        self.paint();
                    }
                    Input::Down { x, y, btn: os_plugkit::gui::Button::Left, .. } => {
                        let (x, y) = (x / self.scale, y / self.scale);
                        self.logic.mouse = Some((x, y));
                        match target_at(x, y) {
                            Some(Target::Load) => self.open_file(),
                            Some(Target::Param(id)) => self.logic.begin_drag(id, x, y),
                            None => {}
                        }
                        self.paint();
                    }
                    Input::Up { .. } => self.logic.end_drag(),
                    Input::Down { .. } | Input::Delta { .. } | Input::Wheel { .. } | Input::Key { .. } | Input::Drop { .. } => {}
                }
                if self.win.idle() {
                    self.paint();
                }
            }
        }

        fn open_file(&mut self) {
            if os_plugkit::no_modal() {
                return;
            }
            let path = self.win.dialog(|| {
                rfd::FileDialog::new()
                    .set_title("Load an image for BeepMap")
                    .add_filter("PNG image", &["png"])
                    .pick_file()
            });
            if let Some(Some(path)) = path {
                self.logic.load(&path.to_string_lossy());
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
    use windows_sys::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OPENFILENAMEW};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    use super::{Logic, Target, target_at};
    use crate::params::BeepMapParams;
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
        params: Arc<BeepMapParams>,
        shared: Arc<Shared>,
        context: Arc<dyn GuiContext>,
        slot: Arc<AtomicIsize>,
    ) -> Handle {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let size = ((view::WIDTH as f32 * scale).round() as u32, (view::HEIGHT as f32 * scale).round() as u32);
        unsafe {
            let class = pw::register("MangoBeepMapView", Some(wndproc));
            let win = Box::new(Win {
                hwnd: std::ptr::null_mut(),
                canvas: Canvas::new(size.0, size.1, scale).expect("BeepMap canvas"),
                pixels: vec![0; (size.0 * size.1) as usize],
                scale,
                size,
                logic: Logic::new(params, shared, context),
                slot: slot.clone(),
            });
            let title = pw::wide("Mango BeepMap");
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
                WM_LBUTTONDOWN => {
                    SetFocus(hwnd);
                    let (x, y) = pw::xy(lp);
                    let (x, y) = (x as f32 / win.scale, y as f32 / win.scale);
                    win.logic.mouse = Some((x, y));
                    match target_at(x, y) {
                        Some(Target::Load) => win.open_file(hwnd),
                        Some(Target::Param(id)) => {
                            win.logic.begin_drag(id, x, y);
                            SetCapture(hwnd);
                        }
                        None => {}
                    }
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                    0
                }
                WM_MOUSEMOVE => {
                    let (x, y) = pw::xy(lp);
                    let (x, y) = (x as f32 / win.scale, y as f32 / win.scale);
                    win.logic.mouse = Some((x, y));
                    if win.logic.drag_param.is_some() {
                        win.logic.set_drag_value(x, y);
                    }
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                    0
                }
                WM_LBUTTONUP => {
                    if win.logic.drag_param.is_some() {
                        win.logic.end_drag();
                        ReleaseCapture();
                    }
                    0
                }
                WM_CAPTURECHANGED => {
                    win.logic.end_drag();
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

    impl Win {
        fn open_file(&mut self, owner: HWND) {
            if os_plugkit::no_modal() {
                return;
            }
            let mut file = vec![0u16; 32_768];
            let filter: Vec<u16> = "PNG image\0*.png\0All files\0*.*\0\0".encode_utf16().collect();
            let title = pw::wide("Load an image for BeepMap");
            unsafe {
                let mut dialog: OPENFILENAMEW = std::mem::zeroed();
                dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
                dialog.hwndOwner = owner;
                dialog.lpstrFilter = filter.as_ptr();
                dialog.lpstrFile = file.as_mut_ptr();
                dialog.nMaxFile = file.len() as u32;
                dialog.lpstrTitle = title.as_ptr();
                dialog.Flags = OFN_NOCHANGEDIR | OFN_FILEMUSTEXIST;
                if GetOpenFileNameW(&mut dialog) == 0 {
                    return;
                }
            }
            let end = file.iter().position(|&c| c == 0).unwrap_or(file.len());
            self.logic.load(&String::from_utf16_lossy(&file[..end]));
        }
    }
}
