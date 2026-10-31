// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Hover tooltips: after the pointer rests over a control, [`kit::tooltip`] shows its hint in a bubble at the
//! cursor. The floating hint bar keeps repeating the hint as before; the bubble is for when it is off, and for
//! reading at the control instead of across the window.
//!
//! The bubble is up while the pointer stays still and keeps its control, and any move, press or key takes it back
//! and restarts the rest ([`App::tooltip_pause`]). The app's tick drives the timer ([`App::tooltip_tick`]), as the
//! rest happens without mouse events; while it is up, a tick that finds the hint changed (the CPU panel's does)
//! rewords it. Nothing shows one while a drag, a press, a menu, a popup or a dialog has the pointer's attention,
//! and a menu row never gets one (the hint bar and the menus speak for themselves).
//!
//! Showing and hiding repaints the whole frame: a bubble is a rare, one-frame change next to what else repaints
//! the frame, and its rounded shadow over anything is not worth a damage bookkeeping of its own. It paints over
//! the windows and menus but under the message boxes (`views::paint`), takes no hit of its own (clicks pass under
//! it to what it names), and the hint is split by [`split_hint`]: a short `: ` tail becomes the caption line, the
//! way the EQ's hint reads as a name with its value under it.

use std::time::{Duration, Instant};

use os_ui::{kit, Rect};

use crate::app::{App, State, Ui};

/// How long the pointer rests over a control before its tooltip shows.
pub const DELAY: Duration = Duration::from_millis(500);

/// From the cursor to the bubble's top left: out from under the pointer, below-right of it (the kit's own keeps it
/// inside the window, lifting it over the cursor at the bottom edge).
const OFFSET: (f32, f32) = (14.0, 22.0);

/// The longest `: ` tail a hint may have to become the caption line (a longer one is part of the name, if any).
const CAPTION_MAX: usize = 24;

/// The bubble while it is up: the hovered control's hint, split into name and caption, and where the cursor rested.
#[derive(Debug, Clone, PartialEq)]
pub struct Tip {
    pub name: String,
    pub caption: String,
    /// Where the cursor was when the bubble came up (the bubble sits [`OFFSET`] away from it).
    pub at: (f32, f32),
}

/// The hint's name and caption: a short `: ` tail becomes the dim caption ("EQ band 2 (peaking) level" /
/// "9.0dB", "Target mixer track" / "Master", "Solo" / "not available yet"); a hint without one stays a single
/// line, as one that is all name does.
pub fn split_hint(hint: &str) -> (String, String) {
    match hint.rsplit_once(": ") {
        Some((name, caption)) if !name.is_empty() && caption.chars().count() <= CAPTION_MAX => {
            (name.to_string(), caption.to_string())
        }
        _ => (hint.to_string(), String::new()),
    }
}

impl App {
    /// The pointer moved, a press or a key came: the bubble goes, and the rest starts over. Nothing but the take
    /// marks the frame when no bubble was up, so the moves and keys that never show one cost nothing.
    pub(crate) fn tooltip_pause(&mut self) {
        self.tip_rest = Instant::now();
        if self.state.tooltip.take().is_some() {
            self.invalidate();
        }
    }

    /// The tooltip's timer at `now` (the tick's clock; tests pass their own): shows, rewords or takes the bubble,
    /// as resting over the hovered control and what it says say.
    pub fn tooltip_tick(&mut self, now: Instant) {
        let want = now.duration_since(self.tip_rest) >= DELAY
            && self.tip_control
            && !self.hint.is_empty()
            && self.drag.is_none()
            && self.pressed.is_none()
            && self.armed.is_none()
            && self.armed_right.is_none()
            && self.hold.is_none()
            && self.state.menu_open.is_none()
            && !self.modal_open()
            && !self.loading();
        let at = self.mouse;
        let (name, caption) = split_hint(&self.hint);
        match (want, &self.state.tooltip) {
            (true, Some(tip)) if tip.name == name && tip.caption == caption && tip.at == at => {}
            (true, _) => {
                self.state.tooltip = Some(Tip { name, caption, at });
                self.invalidate();
            }
            (false, Some(_)) => {
                self.state.tooltip = None;
                self.invalidate();
            }
            (false, None) => {}
        }
    }
}

/// The bubble over the frame's last overlay but the message boxes (`views::paint`): nothing registers it, so a
/// click under it lands on what it names.
pub fn paint(ui: &mut Ui, s: &State) {
    if let Some(tip) = &s.tooltip {
        let bounds = Rect::new(0.0, 0.0, s.screen.0, s.screen.1);
        kit::tooltip(ui.c, ui.t, (tip.at.0 + OFFSET.0, tip.at.1 + OFFSET.1), &tip.name, &tip.caption, bounds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_splits_at_a_short_tail() {
        assert_eq!(split_hint("EQ band 2 (peaking) level: 9.0dB"), ("EQ band 2 (peaking) level".into(), "9.0dB".into()));
        assert_eq!(split_hint("Solo: not available yet"), ("Solo".into(), "not available yet".into()));
        assert_eq!(split_hint("Target mixer track: Master"), ("Target mixer track".into(), "Master".into()));
    }

    #[test]
    fn hint_stays_whole_without_a_short_tail() {
        let (n, c) = split_hint("Piano roll (F7)");
        assert_eq!((n.as_str(), c.as_str()), ("Piano roll (F7)", ""));
        let (n, c) = split_hint("Tempo 130.000 BPM");
        assert_eq!((n.as_str(), c.as_str()), ("Tempo 130.000 BPM", ""));
        // A long tail is part of the name: a bubble of one line, however wide.
        let (n, c) = split_hint("Compute: the whole song, all its channels and every effect");
        assert_eq!((n.as_str(), c.as_str()), ("Compute: the whole song, all its channels and every effect", ""));
        let (n, c) = split_hint("");
        assert_eq!((n.as_str(), c.as_str()), ("", ""));
    }
}
