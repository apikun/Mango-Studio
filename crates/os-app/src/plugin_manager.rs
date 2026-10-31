// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The plugin manager (Settings > Plugins): search folders, scan options, the format preference and the plugin table.
// spec: ui/plugin-manager.md §2, §3

use winit::keyboard::{Key, NamedKey};

use crate::app::App;
use crate::plugin_db::{self as db, DirKind, Mode};
use crate::views::settings::{self, Plug};

impl App {
    /// The plugin manager's checks (`settings::plugin_check`).
    pub(crate) fn plugin_check(&mut self, key: &str) {
        let p = &mut self.state.plugdb.prefs;
        match key {
            settings::STARTUP_CHECK => p.startup_scan = !p.startup_scan,
            settings::ERRORS_CHECK => p.rescan_errors = !p.rescan_errors,
            settings::COMBINE_CHECK => p.combine = !p.combine,
            _ => return,
        }
        self.prefs_changed();
    }

    /// The plugin manager's other controls.
    pub(crate) fn plugin_act(&mut self, a: Plug) {
        if a != Plug::Find {
            self.state.plugdb.finding = false;
        }
        let dir = |app: &App, i: usize| db::search_dirs(&app.state.plugdb.prefs).into_iter().nth(i);
        match a {
            // Any folder change starts a scan (§2).
            Plug::Folder(i) => {
                let Some(d) = dir(self, i) else { return };
                self.state.plugdb.prefs.toggle_folder(&d.path);
                self.prefs_changed();
                self.start_scan(Mode::Changed);
            }
            Plug::Remove(i) => {
                let Some(d) = dir(self, i).filter(|d| d.kind == DirKind::Added) else { return };
                self.state.plugdb.prefs.remove_folder(&d.path);
                self.prefs_changed();
                self.start_scan(Mode::Changed);
                self.state.message = Some(format!("Removed search folder {}", d.path.display()));
            }
            Plug::Prefer(p) => {
                self.state.plugdb.prefs.prefer = p;
                self.prefs_changed();
            }
            Plug::Sort(c) => {
                let d = &mut self.state.plugdb;
                d.sort = if d.sort.0 == c { (c, !d.sort.1) } else { (c, false) };
                d.refilter();
            }
            Plug::Table | Plug::Row(_) => {}
            Plug::Star(i) | Plug::Show(i) => {
                let d = &mut self.state.plugdb;
                let Some(uid) = d.rows.get(i).filter(|r| r.toggles).map(|r| r.uid.clone()) else { return };
                if matches!(a, Plug::Star(_)) {
                    d.prefs.toggle_favorite(&uid);
                } else {
                    d.prefs.toggle_hidden(&uid);
                }
                self.prefs_changed();
            }
            Plug::Find => self.state.plugdb.finding = true,
        }
        self.invalidate();
    }

    /// Add folder... (§2): a folder dialog; the folder and its sub-folders are searched for every format.
    pub(crate) fn add_plugin_folder(&mut self) {
        use crate::filedlg::{FileDlg, Mode};
        self.file_dialog(FileDlg::new(Mode::Folder, "Add a plugin folder", "plugins", crate::dialog::Then::PluginFolder));
    }

    /// Install yabridge (Windows plugins, Linux): downloads it on a background thread, then looks for plugins.
    pub(crate) fn install_yabridge(&mut self) {
        self.windows_job("Installing yabridge from github.com/robbert-vdh/yabridge...", || crate::yabridge::install().map(|v| format!("Installed yabridge {v}")));
    }

    /// Run Windows installer...: a Windows plugin's installer (`.exe`, `.msi`), run in Wine.
    pub(crate) fn pick_windows_installer(&mut self) {
        use crate::filedlg::{FileDlg, Mode};
        let dlg = FileDlg::new(Mode::Open, "Run a Windows plugin installer", "windows installer", crate::dialog::Then::WindowsInstaller).filter("Windows installers", &["exe", "msi"]);
        let downloads = std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Downloads"));
        self.file_dialog(dlg.start(downloads.as_deref()));
    }

    /// The installer chosen: it runs in Wine (its own windows); when it ends, Mango looks for plugins.
    pub(crate) fn run_windows_installer(&mut self, path: std::path::PathBuf) {
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let msi = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("msi"));
        self.windows_job(&format!("Running {name} in Wine: finish the installer, then Mango Studio looks for the new plugins"), move || {
            let mut cmd = std::process::Command::new("wine");
            if msi {
                cmd.args(["msiexec", "/i"]);
            }
            match cmd.arg(&path).status() {
                Ok(_) => Ok(format!("{name} is done")),
                Err(e) => Err(format!("Wine did not start: {e}")),
            }
        });
    }

    /// Add Windows folder...: a folder of Windows plugins (`.dll`, `.vst3`, `.clap`) for yabridge to bridge.
    pub(crate) fn add_windows_folder(&mut self) {
        use crate::filedlg::{FileDlg, Mode};
        let start = crate::yabridge::prefix().map(|p| p.join("drive_c"));
        self.file_dialog(FileDlg::new(Mode::Folder, "Add a folder of Windows plugins", "windows plugins", crate::dialog::Then::WindowsFolder).start(start.as_deref()));
    }

    /// The Windows folder chosen: the scan has yabridge bridge it, then finds its plugins.
    pub(crate) fn windows_folder_chosen(&mut self, path: std::path::PathBuf) {
        self.state.message = Some(format!("Added Windows plugin folder {}: looking for plugins", path.display()));
        self.state.plugdb.win_add.push(path);
        self.start_scan(Mode::Changed);
    }

    /// Runs `job` on a background thread, saying `doing` meanwhile; its outcome comes back in `windows_tick`.
    fn windows_job(&mut self, doing: &str, job: impl FnOnce() -> Result<String, String> + Send + 'static) {
        if self.win_job.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(job());
            os_host::wake();
        });
        self.win_job = Some(rx);
        self.state.plugdb.win_busy = Some(doing.to_string());
        self.state.message = Some(doing.to_string());
        self.invalidate();
    }

    /// UI tick: Install yabridge or a Windows installer ended: what happened, then a look for new plugins.
    pub(crate) fn windows_tick(&mut self) {
        let Some(rx) = &self.win_job else { return };
        let Ok(done) = rx.try_recv() else { return };
        self.win_job = None;
        self.state.plugdb.win_busy = None;
        self.state.plugdb.windows = crate::yabridge::info();
        match done {
            Ok(said) => {
                self.start_scan(Mode::Changed);
                self.state.message = Some(format!("{said}: looking for plugins"));
            }
            Err(e) => self.state.message = Some(e),
        }
        self.invalidate();
    }

    /// The folder chosen in Add folder...
    pub(crate) fn plugin_folder_chosen(&mut self, path: std::path::PathBuf) {
        self.state.message = Some(if self.state.plugdb.prefs.add_folder(&path) {
            self.prefs_changed();
            self.start_scan(Mode::Changed);
            format!("Added search folder {}: looking for plugins", path.display())
        } else {
            format!("{} is searched already", path.display())
        });
    }

    /// Typing into the plugin table's Find box (§2). True when the key was taken.
    pub(crate) fn plugin_find_key(&mut self, key: &Key, ctrl: bool) -> bool {
        let s = &mut self.state;
        if !(s.settings.open && s.settings.tab == settings::PLUGINS && s.plugdb.finding) {
            s.plugdb.finding = false;
            return false;
        }
        let d = &mut s.plugdb;
        match key {
            Key::Named(NamedKey::Enter) => d.finding = false,
            Key::Named(NamedKey::Escape) => {
                d.finding = false;
                d.find.clear();
            }
            Key::Named(NamedKey::Backspace) => {
                d.find.pop();
            }
            Key::Named(NamedKey::Space) => d.find.push(' '),
            Key::Character(c) if !ctrl => d.find.push_str(c),
            _ => return true,
        }
        d.top = 0;
        d.refilter();
        self.invalidate();
        true
    }
}
