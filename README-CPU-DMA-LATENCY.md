# CPU DMA Latency (Prevent CPU Deep Sleep) — Feature README

*Low-latency optimization for Mango Studio on Linux: an Audio settings toggle that keeps the CPUs out of their deep power-saving sleep states (C-states), so the audio callback is never left waiting for a sleeping core at small buffer sizes — no more Xruns (dropouts) that are the idle governor's fault rather than the mix's.*

This document covers what the feature does, how it is implemented, what changed in the source tree, how to apply the patch, how to build and test, how to grant the toggle the device permissions it needs, and the git workflow to get it pushed as a pull request.

---

## 1. What the feature is

Writing a 32-bit signed integer (`0`) to `/dev/cpu_dma_latency` registers a PM QoS request with the kernel: the maximum permitted CPU wakeup latency becomes 0 µs, so no C-state deeper than the governor's shallowest state is ever entered. The request lives exactly as long as the file descriptor stays open — the kernel removes it the moment the descriptor closes (kernel documentation, `Documentation/power/pm_qos_interface`):

> "To register the default PM QoS target for the CPU latency QoS, the process must open /dev/cpu_dma_latency. … As long as the device node is held open that process has a registered request on the parameter. … To remove the user mode request for a target value simply close the device node."

Mango Studio's audio threads already run at real-time priority (`rt.rs`, rtkit). This feature removes the *other* half of the small-buffer equation: a core that slept in a deep C-state takes tens or hundreds of microseconds to wake, which at a 64-frame buffer (1.3 ms at 48 kHz) is a meaningful slice of the block's budget — and when the wake-up lands late, the block is late, and the device plays a gap. With the constraint held at 0, wake-up is immediate.

**User-visible behaviour:**

- **Settings ▸ Audio ▸ CPU ▸ "Prevent CPU deep sleep (C-states)"** — a new check, live on Linux (it does not appear on Windows, macOS or Android, where the device does not exist).
- Turning it on holds the constraint immediately; the hint bar reports the outcome, e.g.
  `Prevent CPU deep sleep (C-states): held: the CPUs stay out of their deep sleep states`
- Turning it off releases it immediately:
  `Prevent CPU deep sleep (C-states): released: the CPUs sleep as usual`
- If the device refuses (it is root-owned on most systems), the kernel's own error is shown instead:
  `Prevent CPU deep sleep (C-states): cannot open /dev/cpu_dma_latency: Permission denied (the device is the root's on most systems — the README's Permissions section)`
- A line under the check (on the Audio page, while the check is on) says where the constraint stands: held, or refused.
- The choice is kept in the user's `settings.txt` (key `cpu_dma_latency`) and re-applied at every start.

---

## 2. Files in this package

| Path | What it is |
| --- | --- |
| `Mango-Studio-main/` | The complete source tree, patched, as a git repository: `main` holds the pristine upstream source, `feat/Linux_Disable_C-States` holds the change (checked out). |
| `mango-studio-cpu-dma-latency.patch` | The change as a unified diff — applies to upstream `main` (0.6.1, commit `f258195`, 2026-11-01) with `git apply`, or as a commit via `git am`. |
| `README-CPU-DMA-LATENCY.md` | This document (a copy also lives at the patched tree's root). |

---

## 3. How it is implemented

### 3.1 The module — `crates/os-app/src/cpu_dma_latency.rs` (new, Linux-only)

One small, self-contained module, declared in `main.rs` behind `#[cfg(target_os = "linux")]` exactly like the existing `rt` module. It uses only `std` and `libc`.

```rust
pub fn enable_low_latency() -> Result<(), String>;  // open /dev/cpu_dma_latency, write 0, hold the descriptor
pub fn disable_low_latency();                       // drop the guard: the descriptor closes, the kernel releases
pub fn is_enabled() -> bool;                        // does this process hold the constraint right now?
```

Design points, in the order a reviewer will ask about them:

- **Global manager.** A function-local `static CONSTRAINT: OnceLock<Mutex<Option<Constraint>>>` (`std::sync::OnceLock`, stable since 1.70) — initialized on first use, no lazy crate, no unsafe. The `Mutex<Option<_>>` is the whole state machine: `Some(guard)` = the device is open and the 0 is in it; `None` = released.
- **RAII.** `Constraint` wraps the open `std::fs::File`. Dropping it drops the `File`, which closes the descriptor, which removes the kernel request. `impl Drop for Constraint` is deliberately explicit: it documents the invariant ("holding the constraint *is* holding this value") and marks the seam where a release write would go, should the kernel interface ever grow one. The `File` itself is never read — `#[allow(dead_code)]` says so at the field.
- **Every exit is covered.** The descriptor belongs to the process, not to any thread: a normal return, `std::process::exit`, a panic that unwinds to the end of the run, a crash, or `SIGKILL` — in every one of those cases the kernel closes the process's descriptors and reclaims the PM QoS request. There is no cleanup handler to fail, no atexit to skip, nothing to poison.
- **Thread-safe.** `enable_low_latency` takes the mutex, checks for an existing guard (holding twice is still holding once — the kernel takes one request per descriptor, and the first is already in place), opens and writes *before* publishing the guard, so a failed open or write never leaves an opened-but-silent descriptor behind. Poisoning follows the crate's existing idiom (`lock().unwrap_or_else(|e| e.into_inner())`), which cannot lose the invariant here: the only state is the guard itself.
- **Close-on-exec.** The file is opened with `std::os::unix::fs::OpenOptionsExt::custom_flags(libc::O_CLOEXEC)`: a child process (the plugin scanner, a renderer, `--render`) inherits no authority over the CPUs' sleep.
- **The payload.** `0i32.to_ne_bytes()` — the kernel's `cpu_latency_qos_write` (kernel/power/qos.c) copies exactly `sizeof(s32)` raw bytes when the write is that long (shorter writes are parsed as a hex string instead). Native byte order is what the interface means by "an s32 value"; for 0 both byte orders coincide, and the test asserts the payload both ways.
- **Error style.** Errors are the kernel's own error text plus one clause of context, as `Result<(), String>` — the crate's existing convention (`rt::promote`).

### 3.2 The toggle — `prefs.rs` + `views/settings.rs`

The feature follows the crate's own pattern for settings-window checks (as documented at the top of `prefs.rs`):

1. `prefs.rs` declares `CPU_DEEP_SLEEP: Check { key: "cpu_dma_latency", label: "Prevent CPU deep sleep (C-states)", default: false }` behind `#[cfg(target_os = "linux")]`, and `checks()` chains it in on Linux only (the same way the test-only `TEST_CHECK` is chained in). The declaration tests (`declarations_match_the_settings_pages`) pass on every platform: the check exists and is painted where `cfg!(target_os = "linux")`, and does not exist elsewhere.
2. `views/settings.rs` adds the check to the existing **CPU** group on the Audio page (beside "Smart disable" / "Align tick lengths"), plus a status `Text` line that appears while the check is on — "Holding /dev/cpu_dma_latency at 0 µs: …" when the constraint is actually held, "The device refused …" when the setting is on but the device said no.
3. `App::pref_toggle` (in `prefs.rs`) gains one cfg-gated hook: for this key it calls `App::cpu_deep_sleep(on)`, which invokes the module and puts the outcome in the hint bar (`state.message`) — including the kernel's own words on refusal. The settings LED itself always shows the *kept choice*; the line under the check shows the *actual state*, so a permission problem is visible even after a restart.
4. `main_impl` (`main_body.rs`) applies the kept setting once at every Linux start, before the event loop and after the one-shot child processes (the plugin scanner and its kind exit before this point, and `O_CLOEXEC` means children could not inherit the descriptor anyway).

### 3.3 Platform gating map

| Platform | Module compiled | Check visible | Toggle effective |
| --- | --- | --- | --- |
| Linux desktop | yes (`cfg(target_os = "linux")`) | yes | yes |
| Windows / macOS | no | no | no |
| Android | no (the app builds as a lib there; nothing references the module) | no | no |

This is why `cargo build --workspace --all-targets` and CI stay green on all three desktop platforms of the existing matrix (Windows, Linux, macOS).

---

## 4. Permissions — why the toggle can say "Permission denied", and what to do about it

Reading the kernel source (`kernel/power/qos.c`) settles it: **there is no capability check** (`CAP_SYS_NICE` or otherwise) in the device's open or write path. What governs access is simply the device node's file mode, which on most distributions comes up owned by `root` with no write for group/other. A normal user's `enable` therefore fails with `EACCES` ("Permission denied") — the feature reports it instead of pretending.

Options, from quickest to most durable:

1. **Test it right away (sudo).**
   ```bash
   sudo ./target/release/mangostudio
   ```
   Toggle the check on. If it holds, everything below is only about making that comfortable.

2. **A udev rule (recommended).** Let a dedicated group write the device node:
   ```bash
   echo 'KERNEL=="cpu_dma_latency", GROUP="audio", MODE="0660"' | \
     sudo tee /etc/udev/rules.d/70-cpu-dma-latency.rules
   sudo udevadm control --reload
   sudo udevadm trigger --subsystem-match=misc
   sudo usermod -aG audio "$USER"   # if you are not in the audio group yet
   ```
   Log out and back in (group membership applies at login), then start Mango Studio normally. *Caveat, stated plainly:* this lets any process running as that group hold the CPUs awake, which trades idle power for latency. Use a group you actually administer (e.g. `audio`); do not use mode `0666`.

3. **What does not work:** `setcap cap_sys_nice+ep` on the executable. Because the kernel does no capability check on this device, capabilities are irrelevant — the file mode is everything. (This README deliberately does not recommend it.)

**Troubleshooting**

| Message | Cause | Fix |
| --- | --- | --- |
| `cannot open /dev/cpu_dma_latency: Permission denied …` | Node is root-owned (the common case) | udev rule above, or sudo for a one-off test |
| `cannot open /dev/cpu_dma_latency: No such file or directory` | Kernel built without CPU latency QoS, or a container/VM without the device | none — the toggle stays off (and the check still remembers your choice) |
| `cannot write /dev/cpu_dma_latency: …` | Opened but the write failed (very unusual) | Report it with the kernel log; the module surfaces the exact text |

**Note:** the check keeps your choice even when the device refuses. The line under the check on the Audio page shows the mismatch ("The device refused …"), so after fixing permissions you can see it flip to "Holding …" — either after restarting the program (the start applies the kept setting) or by toggling the check off and on again.

---

## 5. Applying the patch

You can either use the shipped tree directly (it is already patched, on the `feat/Linux_Disable_C-States` branch) or apply the patch to your own checkout.

**Option A — use the tree as shipped.** Unzip, enter `Mango-Studio-main/`, and you are on the feature branch with the change committed. Continue at §6.

**Option B — patch your own checkout (your `Mango-Studio-Mikobuntu` fork, say).**

The patch carries its own commit message, so `git am` is the one-step way — it applies the diff *and* creates the commit:

```bash
cd ~/Mango-Studio-Mikobuntu
git fetch upstream                     # upstream = apikun/Mango-Studio
git switch feat/Linux_Disable_C-States # your branch (add -c if it does not exist yet)
git am --check /path/to/mango-studio-cpu-dma-latency.patch   # dry-run: silence means it fits
git am /path/to/mango-studio-cpu-dma-latency.patch
git log --oneline -1                   # the commit, message intact
```

The plain-diff route works too, if you would rather stage and commit it yourself (message: §10.1):

```bash
git apply --check /path/to/mango-studio-cpu-dma-latency.patch   # dry-run: silence means it fits
git apply /path/to/mango-studio-cpu-dma-latency.patch
git status --short                     # expect: 4 modified files + 2 new files (see §8)
```

The commit arrives already authored as you (`mikobuntu <335217+mikobuntu@users.noreply.github.com>`), so there is nothing to re-own afterwards. Check with `git log -1 --format='%an <%ae>'`.

**Starting over — wiping a previous patch attempt** (harmless on a feature branch that holds nothing else):

```bash
cd ~/Mango-Studio-Mikobuntu
git am --abort                        # only if git am is mid-flight (conflict, etc.)
git switch feat/Linux_Disable_C-States
git fetch upstream
git reset --hard upstream/main        # the branch goes back to pristine main — previous attempts gone
git status --short                    # must print nothing
git clean -fdn                        # optional: peek at untracked strays; run with -fd to delete them
```

If the branch was already pushed in its broken state, push again with `--force-with-lease` after re-applying (it is your own fork's branch — safe): `git push --force-with-lease origin feat/Linux_Disable_C-States`.

---

## 6. Building

Toolchain: any recent stable Rust (the workspace is edition 2024; the new module uses only features stable since 1.70). The Linux build needs the same packages CI installs (`/.github/workflows/ci.yml`):

```bash
# Debian/Ubuntu
sudo apt-get update
sudo apt-get install -y --no-install-recommends libasound2-dev libwayland-dev libclang-dev pkg-config

cd Mango-Studio-main
cargo build --release --locked
```

The binary lands at `target/release/mangostudio`. The first build takes a while (the workspace is large; `--locked` uses the committed `Cargo.lock` so dependency versions match CI exactly).

Windows and macOS builds are untouched by this change (everything new is compiled out); `cargo build --release` works there as before.

---

## 7. Testing

```bash
# Just the new module's tests (fast, no device needed):
cargo test -p os-app --lib cpu_dma_latency

# The settings/prefs side (page wiring, declarations, toggling):
cargo test -p os-app --lib prefs
cargo test -p os-app --lib settings
```

What the module's two tests assert:

- `payload_is_zero_in_the_machine_s_bytes` — the written payload is exactly `0i32.to_ne_bytes()` (and, for zero, `[0, 0, 0, 0]` in either byte order).
- `the_toggle_holds_and_lets_go` — the full state machine: an idle manager lets go cleanly; a refused hold names the device and holds nothing; a granted hold is idempotent; release is immediate and repeatable. It passes both with and without write access to the device, so it is safe on CI runners and developer machines alike.

The prefs/settings tests run unchanged on all platforms (CI's Windows/Linux/macOS matrix stays green): on non-Linux the check is compiled out, and on Linux the declarations test sees it painted on the Audio page's CPU group, exactly where the declarations test looks for it.

---

## 8. What changed (manifest)

| File | Change |
| --- | --- |
| `crates/os-app/src/cpu_dma_latency.rs` | **new** — the manager module (≈170 lines with docs and tests) |
| `crates/os-app/src/main.rs` | +2 lines — `#[cfg(target_os = "linux")] mod cpu_dma_latency;` |
| `crates/os-app/src/prefs.rs` | +30 lines — the `CPU_DEEP_SLEEP` check declaration, its Linux-only listing in `checks()`, the `pref_toggle` hook and `App::cpu_deep_sleep` (hint-bar messaging) |
| `crates/os-app/src/views/settings.rs` | +28 lines −4 — the Audio ▸ CPU group builds the check plus its state line (Linux), unchanged elsewhere |
| `crates/os-app/src/main_body.rs` | +9 lines — `main_impl` applies the kept setting at every Linux start |
| `README-CPU-DMA-LATENCY.md` | **new** — this document |

Nothing else in the workspace (engine, plugins, tools, vendor) is touched.

---

## 9. Verifying the feature at runtime

With the program running and the check on:

```bash
# 1. The descriptor is open (a literal "cpu_dma_latency" in the process's fds):
ls -l /proc/"$(pidof mangostudio)"/fd | grep cpu_dma_latency

# 2. The CPUs report (nearly) no idle residency in the deeper states while audio plays:
sudo turbostat --interval 5        # compare C-state residency with the check off
#   or
sudo powertop                      # "Usage; Events/s" per C-state
```

Then the audible test: set **Buffer length** to 64 or 128 frames on the Audio page, play the heaviest project you have, and compare the CPU panel's dropout counter (right-click the panel ▸ the dropouts reading) with the check off and on. Idle-power cost is the flip side: expect measurably higher idle consumption while the check is on — that is the point of the setting, and it is why it is a setting.

---

## 10. Git workflow — review, commit, push, pull request

The shipped repository already has the commit (on `feat/Linux_Disable_C-States`); if you applied the patch with `git am` (§5, Option B), your checkout has it too — skip straight to §10.2. What follows is the fork round-trip: push the branch to your fork (`origin`), open the PR against upstream (`upstream` = apikun/Mango-Studio).

### 10.1 If you staged the change by hand (the `git apply` route)

```bash
cd ~/Mango-Studio-Mikobuntu
git switch feat/Linux_Disable_C-States        # your branch
git add crates/os-app/src/cpu_dma_latency.rs \
        crates/os-app/src/main.rs \
        crates/os-app/src/prefs.rs \
        crates/os-app/src/views/settings.rs \
        crates/os-app/src/main_body.rs \
        README-CPU-DMA-LATENCY.md
git commit   # message: see below
```

Commit message used in the shipped tree (keep it, or reword freely):

```
audio: the Audio settings' CPU deep sleep check (Linux)

Settings ▸ Audio ▸ CPU grows "Prevent CPU deep sleep (C-states)": while it is
on, the program holds the kernel's PM QoS CPU latency constraint through
/dev/cpu_dma_latency at 0, so the CPUs keep out of their deep sleep states and
their wake-up latency stops eating into small buffers' blocks (dropouts).

The module (cpu_dma_latency.rs) is one guard in one global slot: a
Mutex<Option<Constraint>> behind a OnceLock, the guard holding the open device
file (O_CLOEXEC, write-only, one raw 0i32 in the machine's byte order).
Dropping the guard closes the descriptor and the kernel takes the request back;
the descriptor belongs to the process, so every end of the process — exit,
panic, crash, SIGKILL — releases it. Windows, macOS and Android never see the
check (the device does not exist there); the kept choice re-applies at every
Linux start.
```

### 10.2 Push your fork's branch and open the pull request

```bash
# The commit already carries your identity — confirm it:
git log -1 --format='%an <%ae>'                # → mikobuntu <335217+mikobuntu@users.noreply.github.com>

# Review once more, exactly what will go out:
git log --oneline upstream/main..feat/Linux_Disable_C-States
git diff upstream/main...feat/Linux_Disable_C-States --stat

# Push the branch to your fork (origin = Mango-Studio-Mikobuntu):
git push -u origin feat/Linux_Disable_C-States
# (add --force-with-lease instead if you are replacing an earlier pushed attempt)
```

Then open the PR — base: `apikun/Mango-Studio` `main`, head: your fork's branch:

```bash
# With the GitHub CLI (run inside ~/Mango-Studio-Mikobuntu; it finds the fork on its own):
gh pr create \
  --repo apikun/Mango-Studio \
  --base main --head feat/Linux_Disable_C-States \
  --title "audio: the Audio settings' CPU deep sleep check (Linux)" \
  --body "Prevent CPU deep sleep (C-states) on the Audio page's CPU group: holds /dev/cpu_dma_latency at 0 while the check is on (PM QoS, fd-held, RAII-released). Kills the C-state share of Xruns at 64–256 frames. Linux-only; Windows/macOS/Android compile out. See README-CPU-DMA-LATENCY.md."
```

…or in a browser: open your fork on GitHub, switch to the branch, press **Contribute** ▸ *Open pull request*. If your fork kept the upstream's repo name, the direct link is <https://github.com/apikun/Mango-Studio/compare/main...mikobuntu:feat/Linux_Disable_C-States>; if it is named `Mango-Studio-Mikobuntu`, use <https://github.com/apikun/Mango-Studio/compare/main...mikobuntu:Mango-Studio-Mikobuntu:feat/Linux_Disable_C-States>. CI (Windows + Linux + macOS) runs on the PR; the change is compiled out on the first two and the Android lib, so the matrix should stay green.

**Housekeeping notes**

- The patch's commit is authored as `mikobuntu <335217+mikobuntu@users.noreply.github.com>`. To re-own or reword it: `git commit --amend --reset-author --no-edit` (after your own `git config user.name` / `user.email`), or `--amend` alone to reword.
- If upstream moved since your fork: `git fetch upstream`, then `git rebase upstream/main` on the branch. The change touches four files and adds two, so a clean re-apply is the expected outcome; on any surprise, `git rebase --abort` puts things back.
- To update the PR later: commit on the same branch, `git push` again.
- To back the change out of a tree: `git revert <commit>` (or simply leave the check off — the default — which costs nothing: the module never opens the device unless the check asks).

---

## 11. Design notes for reviewers

- **Why the setting persists even when the device refuses:** the LED is the user's wish; the line under the check is the machine's answer. This matches how the page already treats audio settings (a device that is gone falls back; the page says so). After a permission fix, a restart (or an off/on toggle) makes the wish true without the user re-configuring anything.
- **Why `to_ne_bytes` and not a formatted string:** the kernel's write handler takes a raw `s32` when the write is 4 bytes long (and parses shorter writes as hex). Four exact bytes are also what the kernel copies with `copy_from_user` — no parsing, no locale, no path where the write means something else.
- **Why the manager holds the file rather than each stream:** the constraint is process-wide and cheap to hold; one descriptor for the run means no per-stream bookkeeping, no re-assertion on device switches, and Auto close (the output letting go) never touches it.
- **Why the toggle is Linux-only in the UI:** `/dev/cpu_dma_latency` is a Linux PM QoS interface. Gating the declaration, the paint and the call sites on `cfg(target_os = "linux")` keeps the Windows/macOS binaries byte-for-byte free of it — and the Android lib (which also compiles `prefs.rs` and `views/settings.rs`) untouched, since Android's target OS is `android`, not `linux`.
- **Testing without the device:** the module's tests branch on whether the device allowed the hold, so they pass on root-less CI (refusal path) and on a machine with a udev rule (hold path) without any test-only hooks.

---

## 12. License

The change carries the project's own headers: `Copyright (C) 2026 ItsAlphaNeon`, `SPDX-License-Identifier: GPL-3.0-or-later`, part of Lemon Studio / Mango Studio. No new third-party dependencies; `libc` was already a Linux dependency of `os-app`.
