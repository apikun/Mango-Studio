// Copyright (C) 2026 battlemountain2
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use nih_plug::prelude::*;
use os_hostlink::{PageIcon, Peer};
use os_plugkit::gui::{self, Button, Input};
use os_plugkit::vals::Vals;
use os_ui::Canvas;

use crate::params::WavetableParams;
use crate::{view, Shared};

pub struct WavetableEditor {
    params: Arc<WavetableParams>,
    shared: Arc<Shared>,
    scale: AtomicU32,
    slot: gui::Slot,
}

impl WavetableEditor {
    pub fn new(params: Arc<WavetableParams>, shared: Arc<Shared>) -> Self {
        Self {
            params,
            shared,
            scale: AtomicU32::new(1.0f32.to_bits()),
            slot: Default::default(),
        }
    }
}

impl Editor for WavetableEditor {
    fn spawn(&self, parent: ParentWindowHandle, context: Arc<dyn GuiContext>) -> Box<dyn Any + Send> {
        let scale = f32::from_bits(self.scale.load(Ordering::Relaxed));
        let (parent, scale) = match parent {
            #[cfg(target_os = "linux")]
            ParentWindowHandle::X11Window(window) => (window, scale),
            #[cfg(target_os = "macos")]
            ParentWindowHandle::AppKitNsView(view) => (view as usize, gui::backing_scale(view as usize)),
            _ => return Box::new(()),
        };

        let params = self.params.clone();
        let shared = self.shared.clone();
        let wake = self.slot.clone();
        let link = Peer::register(parent as u64, vec![PageIcon::Plugin], (view::W, view::H), move || gui::notify(&wake));
        let peer = link.clone();
        let size = (((view::W as f32) * scale).round() as u32, ((view::H as f32) * scale).round() as u32);

        let thread = gui::spawn(parent, size, &self.slot, move |window| {
            let Ok(canvas) = Canvas::new(size.0, size.1, scale) else {
                peer.unregister();
                return;
            };
            let logic = Logic::new(params, Some(context));
            Window {
                window,
                canvas,
                buffer: vec![0; (size.0 * size.1) as usize],
                size,
                scale,
                logic,
                shared,
                link: peer,
                dirty: true,
            }
            .run();
        });

        if thread.is_none() {
            link.unregister();
        }
        Box::new(thread)
    }

    fn size(&self) -> (u32, u32) {
        (view::W, view::H)
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        if !factor.is_finite() || !(0.5..=4.0).contains(&factor) {
            return false;
        }
        self.scale.store(factor.to_bits(), Ordering::Relaxed);
        true
    }

    fn param_value_changed(&self, _id: &str, _value: f32) {
        gui::notify(&self.slot);
    }
    fn param_modulation_changed(&self, _id: &str, _offset: f32) {
        gui::notify(&self.slot);
    }
    fn param_values_changed(&self) {
        gui::notify(&self.slot);
    }
}

pub struct Logic {
    pub vals: Vals,
    pub focus: Option<usize>,
    _params: Arc<WavetableParams>,
    dragging: bool,
}

impl Logic {
    pub fn new(params: Arc<WavetableParams>, context: Option<Arc<dyn GuiContext>>) -> Self {
        Self {
            vals: Vals::new(&*params, params.live.clone(), context),
            _params: params,
            focus: None,
            dragging: false,
        }
    }

    pub fn hit(x: f32, y: f32) -> Option<usize> {
        (0..view::TRACK_IDS.len()).find(|&i| view::track(i).outset(8.0).contains(x, y))
    }

    fn end(&mut self) {
        if self.dragging {
            if let Some(i) = self.focus {
                self.vals.end(view::TRACK_IDS[i]);
            }
        }
        self.dragging = false;
    }

    pub fn down(&mut self, x: f32, y: f32, reset: bool) {
        self.end();
        self.focus = Self::hit(x, y);

        if let Some(i) = self.focus {
            let id = view::TRACK_IDS[i];
            if reset {
                self.vals.edit(id, self.vals.default_norm(id));
            } else {
                self.vals.begin(id);
                self.dragging = true;
                self.moved(x);
            }
        } else if view::PREVIOUS.contains(x, y) {
            let cur = self.vals.plain("table") as usize;
            let next = (cur + 3 - 1) % 3;
            self.vals.edit_plain("table", next as f32);
        } else if view::NEXT.contains(x, y) || view::PRESET.contains(x, y) {
            let cur = self.vals.plain("table") as usize;
            let next = (cur + 1) % 3;
            self.vals.edit_plain("table", next as f32);
        } else if view::WAVETABLE_BOX.contains(x, y) {
            // Dragging across wavetable scrubs WT Pos
            let frac = ((x - view::WAVETABLE_BOX.x) / view::WAVETABLE_BOX.w).clamp(0.0, 1.0);
            self.vals.edit("wt_pos", frac);
        }
    }

    pub fn moved(&mut self, x: f32) {
        if self.dragging {
            if let Some(i) = self.focus {
                let rect = view::track(i);
                let norm = ((x - rect.x) / rect.w).clamp(0.0, 1.0);
                self.vals.set(view::TRACK_IDS[i], norm);
            }
        }
    }

    pub fn wheel(&mut self, x: f32, y: f32, notches: f32) {
        if let Some(i) = Self::hit(x, y) {
            self.focus = Some(i);
            let id = view::TRACK_IDS[i];
            let new_val = (self.vals.norm(id) + notches.signum() * 0.015).clamp(0.0, 1.0);
            self.vals.edit(id, new_val);
        }
    }
}

impl Drop for Logic {
    fn drop(&mut self) {
        self.end();
    }
}

struct Window {
    window: gui::Window,
    canvas: Canvas,
    buffer: Vec<u32>,
    size: (u32, u32),
    scale: f32,
    logic: Logic,
    shared: Arc<Shared>,
    link: Arc<Peer>,
    dirty: bool,
}

impl Window {
    fn run(mut self) {
        while let Some(input) = self.window.next() {
            match input {
                Input::Expose => self.dirty = true,
                Input::Tick => {
                    if self.logic.vals.expire() {
                        self.dirty = true;
                    }
                }
                Input::Move { x, y } => {
                    let (x, _y) = (x / self.scale, y / self.scale);
                    self.logic.moved(x);
                    self.dirty = true;
                }
                Input::Down { x, y, btn, .. } => {
                    let (x, y) = (x / self.scale, y / self.scale);
                    self.logic.down(x, y, btn == Button::Right);
                    self.dirty = true;
                }
                Input::Up { .. } => {
                    self.logic.end();
                    self.dirty = true;
                }
                Input::Wheel { x, y, notches } => {
                    let (x, y) = (x / self.scale, y / self.scale);
                    self.logic.wheel(x, y, notches);
                    self.dirty = true;
                }
                _ => {}
            }

            if self.dirty && self.window.idle() {
                self.paint();
            }
        }

        self.link.unregister();
    }

    fn paint(&mut self) {
        self.dirty = false;
        let voices = self.shared.voices.load(Ordering::Relaxed) as usize;
        view::paint(&mut self.canvas, &self.logic.vals, voices, self.logic.focus);

        let len = (self.size.0 * self.size.1) as usize;
        if self.buffer.len() != len {
            self.buffer.resize(len, 0);
        }
        self.canvas.copy_to_xrgb(&mut self.buffer);
        self.window.blit(&self.buffer, self.size.0, self.size.1);
    }
}
