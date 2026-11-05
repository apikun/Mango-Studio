// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The CPUs' deep sleep states (C-states), kept out of reach while the Audio settings' "Prevent CPU deep sleep
//! (C-states)" check is on (Linux): the kernel's PM QoS takes a maximum wakeup latency through `/dev/cpu_dma_latency`,
//! and a process holds its request only while the device's descriptor stays open — the request is registered at the
//! open, changed by what the process writes, and taken back the moment the descriptor closes ("To remove the user mode
//! request for a target value simply close the device node", Documentation/power/pm_qos_interface). The device takes
//! one 32-bit signed integer, the permitted wakeup latency in microseconds, in the machine's own byte order (a hex
//! string is accepted too); 0 asks for no latency at all, so no C-state deeper than the governor's shallowest is
//! entered. That is the difference between a wake-up that fits inside a 64-frame block (about 1.3 ms at 48 kHz) and
//! one that does not: a core that slept too deep comes back too late, the block is short, and the dropout (Xrun) is
//! heard. `crate::rt`'s real-time priority keeps the audio threads' turn coming; this keeps the turn worth waiting
//! for.
//!
//! The whole feature is one file descriptor and its lifetime, so the module is one guard in one global slot
//! (`Constraint` in a `Mutex<Option<_>>` behind a `OnceLock`): [`enable_low_latency`] opens the device write-only
//! (`O_CLOEXEC` — a child process, the plugin scanner or a renderer, inherits no say in the CPUs' sleep), writes the 0
//! and keeps the `File` in the guard; [`disable_low_latency`] takes the guard out of the slot, and its `Drop` closes
//! the file. Nothing else has to run for the constraint to go: the descriptor belongs to the process, so every end of
//! the process closes it — a normal return, `std::process::exit`, a panic that unwinds the run's end, a crash,
//! SIGKILL. The kernel cleans up what a dead process held; no handler, atexit or Drop of ours has to survive any of
//! those.
//!
//! Writing the device is the root's own on most systems (the node comes up root-owned; a udev rule lets a group in):
//! a refused toggle comes back as the kernel's own words in [`enable_low_latency`]'s error, and the Audio page's CPU
//! group says what to do about it (`views::settings`). The check itself is `crate::prefs::CPU_DEEP_SLEEP`; the toggle
//! holds or lets go the moment it flips (`App::cpu_deep_sleep`), and the start applies what was kept
//! (`main_impl`, Linux).
//!
//! # A UI event toggle callback
//!
//! The settings window's own path is `App::pref_toggle` → `App::cpu_deep_sleep`; any toggle callback of the same
//! shape reads the same:
//!
//! ```ignore
//! /// The check's new state is `on`: hold the constraint or let it go, and say how that went.
//! fn low_latency_toggled(&mut self, on: bool) {
//!     let said: Result<String, String> = if on {
//!         crate::cpu_dma_latency::enable_low_latency()
//!             .map(|()| "held: the CPUs stay out of their deep sleep states".to_string())
//!     } else {
//!         crate::cpu_dma_latency::disable_low_latency();
//!         Ok("released: the CPUs sleep as usual".to_string())
//!     };
//!     self.message = Some(match said {
//!         Ok(said) => format!("Prevent CPU deep sleep (C-states): {said}"),
//!         Err(e) => format!("Prevent CPU deep sleep (C-states): {e}"),
//!     });
//! }
//! ```

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::{Mutex, OnceLock};

/// The PM QoS device that takes the CPUs' maximum permitted wakeup latency, µs.
const DEVICE: &str = "/dev/cpu_dma_latency";

/// What the check asks for: no permitted wakeup latency at all. The kernel copies 4 bytes off the write when it is
/// that long (`cpu_latency_qos_write`, kernel/power/qos.c); a shorter write is read as a hex string instead.
const NO_LATENCY: i32 = 0;

/// The held constraint: the device's open descriptor, and so the request. While a `Constraint` exists the CPUs keep
/// out of their deep sleep states; the moment it is dropped the `File` closes and the kernel takes the request back.
struct Constraint {
    /// Opened write-only and close-on-exec; the 0 went out through it while it was still a local (`Constraint::hold`
    /// writes before the guard exists, so a failed write never leaves an opened-but-silent descriptor behind). Held
    /// for its `Drop` — the close is the release — and read for nothing.
    #[allow(dead_code)]
    file: File,
}

impl Constraint {
    /// Opens the device and asks for `NO_LATENCY`: the CPUs' permitted wakeup latency is none. Fails with the
    /// kernel's own words when the device is not there or not ours to write (a plain user's on most systems).
    fn hold() -> Result<Constraint, String> {
        let mut file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(DEVICE)
            .map_err(|e| format!("cannot open {DEVICE}: {e} (the device is the root's on most systems — the README's Permissions section)"))?;
        file.write_all(&payload()).map_err(|e| format!("cannot write {DEVICE}: {e}"))?;
        Ok(Constraint { file })
    }
}

impl Drop for Constraint {
    fn drop(&mut self) {
        // Nothing to write and nothing to close by hand: the kernel removes the request when the descriptor closes,
        // and `File` closes it as this drops. The impl is the type's point kept where a reader looks first — holding
        // the constraint *is* holding this value — and the seam where a release write (a value instead of the close,
        // should the kernel's interface ever want one) would go.
    }
}

/// The held constraint, where the toggles meet: `Some` while the device is open and the 0 is in it.
fn held() -> &'static Mutex<Option<Constraint>> {
    static CONSTRAINT: OnceLock<Mutex<Option<Constraint>>> = OnceLock::new();
    CONSTRAINT.get_or_init(|| Mutex::new(None))
}

/// The bytes the device takes for "no permitted wakeup latency": 0, as the machine's own 32-bit signed integer.
fn payload() -> [u8; 4] {
    NO_LATENCY.to_ne_bytes()
}

/// Keeps the CPUs out of their deep sleep states (C-states) until [`disable_low_latency`] or the process's end:
/// opens `/dev/cpu_dma_latency`, writes 0 µs and holds the descriptor. Holding it while it is held is still holding
/// it once (the kernel takes one request per descriptor, and the first is in place); `Ok` says the constraint is
/// held, the `Err` is the kernel's refusal (usually the device's permissions — see the module's docs).
pub fn enable_low_latency() -> Result<(), String> {
    let mut slot = held().lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        return Ok(());
    }
    *slot = Some(Constraint::hold()?);
    Ok(())
}

/// Lets the CPUs sleep again: takes the constraint out of the slot, whose `Drop` closes the descriptor and the
/// kernel's request with it. Letting go of nothing is nothing.
pub fn disable_low_latency() {
    *held().lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Whether this process holds the constraint now: the Audio page's CPU group reads it (with the check's setting) for
/// the line that says where the constraint stands.
pub fn is_enabled() -> bool {
    held().lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The payload the device takes: 0, in the machine's own bytes (zero is the same 4 bytes in either byte order,
    /// but the machine's own is what `write_all` sends).
    #[test]
    fn payload_is_zero_in_the_machine_s_bytes() {
        assert_eq!(payload(), 0i32.to_ne_bytes());
        assert_eq!(payload(), [0, 0, 0, 0]);
    }

    /// The whole toggle, against the device where it allows and the kernel's refusal where it does not (a plain user
    /// on most systems): an idle slot lets go cleanly, a refused hold leaves it idle and names the device, a granted
    /// one is one hold however often it is asked for, and letting go is heard at once. The one test on the global
    /// slot (the app's own toggles are the only other callers, and tests do not click the check), so nothing races it.
    #[test]
    fn the_toggle_holds_and_lets_go() {
        disable_low_latency();
        assert!(!is_enabled(), "an idle slot holds nothing");
        match enable_low_latency() {
            Ok(()) => {
                assert!(is_enabled(), "the constraint is held");
                enable_low_latency().expect("holding twice is still holding once");
                assert!(is_enabled(), "the first hold stays the only one");
            }
            Err(e) => {
                assert!(e.contains(DEVICE), "a refusal names the device: {e}");
                assert!(!is_enabled(), "a refused hold holds nothing");
            }
        }
        disable_low_latency();
        assert!(!is_enabled(), "letting go lets go");
        disable_low_latency();
        assert!(!is_enabled(), "letting go of nothing is nothing");
    }
}
