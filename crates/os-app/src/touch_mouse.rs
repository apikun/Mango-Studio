// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Android: touches are the mouse. One finger owns the pointer: it comes down as a move onto
//! the spot and a left press, moves as the finger does, and lifts as a release. A finger held
//! still is a right click — the app's menus are right-click menus, and touch has no second
//! button. Other fingers wait: multi-finger gestures (pinch to zoom, two-finger scroll) come
//! later. The mouse events go back through `window_event`, so the app's own logic — the
//! double-click timing, the drags, the scroll bars — is the exact desktop one. Not to be
//! confused with the Touch controller (`touch`), the MIDI pads window.

use std::time::{Duration, Instant};

use winit::event::{ElementState, MouseButton, Touch, TouchPhase, WindowEvent};

/// A finger held still this long is a right click.
const HOLD: Duration = Duration::from_millis(550);
/// A finger that travelled this far is a drag, not a hold (physical px).
const SLOP: f64 = 15.0;

/// The touch state between events (one per window; the main window is Android's only one).
#[derive(Default)]
pub struct TouchMouse {
    /// The finger that owns the pointer; other fingers' moves are ignored.
    owner: Option<u64>,
    /// Where the press sits, for the hold-vs-drag distance.
    down: Option<(f64, f64)>,
    /// When the press came down.
    at: Option<Instant>,
}

impl TouchMouse {
    /// The mouse events a touch stands for, in order.
    pub fn event(&mut self, t: Touch) -> Vec<WindowEvent> {
        let at = winit::dpi::PhysicalPosition::new(t.location.x, t.location.y);
        let move_to = WindowEvent::CursorMoved { device_id: t.device_id, position: at };
        match t.phase {
            // The first finger down: move onto the spot, then press.
            TouchPhase::Started if self.owner.is_none() => {
                self.owner = Some(t.id);
                self.down = Some((t.location.x, t.location.y));
                self.at = Some(Instant::now());
                vec![move_to, WindowEvent::MouseInput { device_id: t.device_id, state: ElementState::Pressed, button: MouseButton::Left }]
            }
            TouchPhase::Moved if self.owner == Some(t.id) => {
                if let Some((x0, y0)) = self.down {
                    let dx = x0 - t.location.x;
                    let dy = y0 - t.location.y;
                    if dx * dx + dy * dy > SLOP * SLOP {
                        self.down = None;
                        self.at = None;
                    }
                }
                vec![move_to]
            }
            // Up: a release; a held-still finger also right clicks, as a full click (the menus
            // open on the press and stay open: popups close on the next press, not the release).
            TouchPhase::Ended if self.owner == Some(t.id) => {
                self.owner = None;
                let long = self.at.is_some_and(|a| a.elapsed() >= HOLD);
                self.down = None;
                self.at = None;
                let mut v = vec![WindowEvent::MouseInput { device_id: t.device_id, state: ElementState::Released, button: MouseButton::Left }];
                if long {
                    let press_right = WindowEvent::MouseInput { device_id: t.device_id, state: ElementState::Pressed, button: MouseButton::Right };
                    let release_right = WindowEvent::MouseInput { device_id: t.device_id, state: ElementState::Released, button: MouseButton::Right };
                    v.extend([press_right, release_right]);
                }
                v
            }
            // A cancelled finger leaves the same way, without the right click.
            TouchPhase::Cancelled if self.owner == Some(t.id) => {
                self.owner = None;
                self.down = None;
                self.at = None;
                vec![WindowEvent::MouseInput { device_id: t.device_id, state: ElementState::Released, button: MouseButton::Left }]
            }
            _ => Vec::new(),
        }
    }
}
