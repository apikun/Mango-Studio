// Copyright (C) 2026 Artig3nce
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Windows plugins on Linux through yabridge (https://github.com/robbert-vdh/yabridge): `yabridgectl` keeps a list of
//! Windows plugin folders and makes a Linux plugin for each Windows one (VST 3 and CLAP under `~/.vst3/yabridge` and
//! `~/.clap/yabridge`, VST 2 as a `.so` beside its `.dll`), which runs the Windows plugin in Wine. Before each scan
//! the usual Windows plugin folders of the Wine prefix are added to that list and the bridges made again
//! (`prepare`), so a Windows plugin installed with Wine shows up like a Linux one. Elsewhere than Linux nothing here
//! does anything.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What the plugin manager says about Windows plugins (`info`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    /// `yabridgectl`, when installed.
    pub ctl: Option<PathBuf>,
    /// Its version ("5.1.1").
    pub version: String,
    /// Wine's version ("wine-10.0"), when Wine is installed.
    pub wine: Option<String>,
    /// The Windows plugin folders yabridge bridges.
    pub dirs: Vec<PathBuf>,
}

impl Info {
    /// The plugin manager's status line.
    pub fn status(&self) -> String {
        match (&self.ctl, &self.wine) {
            (Some(_), Some(w)) => format!("yabridge {} with {w}: Windows plugins installed with Wine are found with Linux ones", self.version),
            (None, Some(_)) => "yabridge is not installed: Install yabridge to use Windows plugins".into(),
            (_, None) => "Wine is not installed: install it from your package manager (sudo apt install wine)".into(),
        }
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Where Install yabridge puts it, as yabridge's own instructions say: `~/.local/share/yabridge`.
fn install_dir() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).or_else(|| home().map(|h| h.join(".local/share")));
    data.map(|d| d.join("yabridge"))
}

/// `yabridgectl`: on the `PATH`, else where Install yabridge puts it.
pub fn ctl() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let on_path = std::env::var_os("PATH").into_iter().flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).map(|d| d.join("yabridgectl"));
    on_path.chain(install_dir().map(|d| d.join("yabridgectl"))).find(|p| p.is_file())
}

/// The first line a program prints for `args`, when it runs.
fn first_line(program: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok().filter(|o| o.status.success())?;
    String::from_utf8_lossy(&out.stdout).lines().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty())
}

/// What `info` reads: yabridge, Wine and the bridged folders. Runs two small programs: not on every frame.
pub fn info() -> Info {
    // Tests never look at the machine's own programs.
    if !cfg!(target_os = "linux") || cfg!(test) {
        return Info::default();
    }
    let ctl = ctl();
    let version = ctl.as_deref().and_then(|c| first_line(c, &["--version"])).map(|v| v.trim_start_matches("yabridgectl").trim().to_string()).unwrap_or_default();
    let wine = first_line(Path::new("wine"), &["--version"]);
    Info { ctl, version, wine, dirs: dirs() }
}

/// The Windows plugin folders in yabridgectl's settings (`~/.config/yabridgectl/config.toml`, `plugin_dirs`).
pub fn dirs() -> Vec<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).or_else(|| home().map(|h| h.join(".config")));
    config.and_then(|c| std::fs::read_to_string(c.join("yabridgectl/config.toml")).ok()).map_or_else(Vec::new, |t| plugin_dirs(&t))
}

/// The `plugin_dirs` array of yabridgectl's settings text: its strings.
fn plugin_dirs(toml: &str) -> Vec<PathBuf> {
    let Some(start) = toml.lines().position(|l| l.trim_start().starts_with("plugin_dirs")) else { return Vec::new() };
    let mut out = Vec::new();
    let mut s = String::new();
    let (mut quoted, mut escaped) = (false, false);
    for (i, line) in toml.lines().skip(start).enumerate() {
        let line = if i == 0 { line.split_once('=').map_or(line, |(_, v)| v) } else { line };
        for c in line.chars() {
            match c {
                _ if escaped => {
                    s.push(c);
                    escaped = false;
                }
                '\\' if quoted => escaped = true,
                '"' if quoted => {
                    out.push(PathBuf::from(std::mem::take(&mut s)));
                    quoted = false;
                }
                '"' => quoted = true,
                ']' if !quoted => return out,
                _ if quoted => s.push(c),
                _ => {}
            }
        }
    }
    out
}

/// The Wine prefix Windows plugins are installed in: `WINEPREFIX`, else `~/.wine`.
pub fn prefix() -> Option<PathBuf> {
    std::env::var_os("WINEPREFIX").filter(|p| !p.is_empty()).map(PathBuf::from).or_else(|| home().map(|h| h.join(".wine")))
}

/// The folders Windows plugin installers use, in Wine prefix `prefix`: those that are there.
pub fn wine_dirs(prefix: &Path) -> Vec<PathBuf> {
    let c = prefix.join("drive_c");
    [
        "Program Files/Common Files/VST3",
        "Program Files/Common Files/CLAP",
        "Program Files/Common Files/VST2",
        "Program Files/VstPlugins",
        "Program Files/Steinberg/VstPlugins",
    ]
    .into_iter()
    .map(|d| c.join(d))
    .filter(|d| d.is_dir())
    .collect()
}

/// Whether the plugin at `path` is a Windows plugin bridged by yabridge: in a `yabridge` folder (its VST 3 and CLAP
/// bridges), or a `.so` beside a `.dll` of the same name (its VST 2 bridges).
pub fn is_bridged(path: &Path) -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    if path.components().any(|c| c.as_os_str() == "yabridge") {
        return true;
    }
    let so = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("so"));
    so && ["dll", "DLL", "Dll"].iter().any(|e| path.with_extension(e).is_file())
}

/// Before a scan: adds the usual Windows plugin folders of the Wine prefix and `extra` to yabridge's list, then makes
/// the bridges again. Returns the bridged folders (searched for VST 2 bridges) and, when something went wrong, why.
pub fn prepare(extra: &[PathBuf]) -> (Vec<PathBuf>, Option<String>) {
    let Some(ctl) = ctl() else { return (Vec::new(), (!extra.is_empty()).then(|| "yabridge is not installed".to_string())) };
    let known = dirs();
    let wanted = prefix().map(|p| wine_dirs(&p)).unwrap_or_default().into_iter().chain(extra.iter().cloned());
    let mut error = None;
    for d in wanted.filter(|d| !known.iter().any(|k| crate::plugin_db::same_path(k, d))) {
        if let Err(e) = run(&ctl, &[std::ffi::OsStr::new("add"), d.as_os_str()]) {
            error = Some(e);
        }
    }
    if let Err(e) = run(&ctl, &["sync".as_ref()]) {
        error = Some(e);
    }
    (dirs(), error)
}

/// Runs yabridgectl with `args`; its last line when it fails.
fn run(ctl: &Path, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let out = Command::new(ctl).args(args).output().map_err(|e| format!("yabridgectl did not start: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
    Err(text.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("yabridgectl failed").to_string())
}

/// Whether Wine `version` ("wine-10.0", "wine-9.21 (Staging)") needs yabridge's development build: the 5.1.1
/// release does not work with Wine 9.22 and later (yabridge's README, "Troubleshooting").
fn needs_dev_build(version: &str) -> bool {
    let num = version.trim_start_matches("wine-").split(|c: char| !(c.is_ascii_digit() || c == '.')).next().unwrap_or("");
    let mut parts = num.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    (major, minor) >= (9, 22)
}

/// Downloads `url` into file `to` with `curl`.
fn download(url: &str, to: &Path) -> Result<(), String> {
    let ok = Command::new("curl").args(["-fsSL", "-o"]).arg(to).arg(url).status().map_err(|e| format!("curl did not start: {e}"))?;
    if ok.success() { Ok(()) } else { Err(format!("the download of {url} failed")) }
}

/// Unpacks `.tar.gz` file `file` into folder `into` with `tar`.
fn untar(file: &Path, into: &Path) -> Result<(), String> {
    let ok = Command::new("tar").arg("-xzf").arg(file).arg("-C").arg(into).status().map_err(|e| e.to_string())?;
    if ok.success() { Ok(()) } else { Err("the download could not be unpacked".into()) }
}

/// The `.tar.gz` inside zip file `zip` (GitHub's build downloads wrap the tarball in a zip), written into `into`.
fn tarball_from_zip(zip: &Path, into: &Path) -> Result<PathBuf, String> {
    let file = std::fs::File::open(zip).map_err(|e| e.to_string())?;
    let mut a = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    for i in 0..a.len() {
        let mut f = a.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = f.enclosed_name().and_then(|n| n.file_name().map(|n| n.to_owned())) else { continue };
        if name.to_string_lossy().ends_with(".tar.gz") {
            let out = into.join(name);
            let mut w = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, &mut w).map_err(|e| e.to_string())?;
            return Ok(out);
        }
    }
    Err("the download held no yabridge archive".into())
}

/// The release's files, unpacked into `tmp/yabridge`: the latest release from yabridge's GitHub page. Its version.
fn fetch_release(tmp: &Path) -> Result<String, String> {
    let api = "https://api.github.com/repos/robbert-vdh/yabridge/releases/latest";
    let out = Command::new("curl").args(["-fsSL", "-H", "Accept: application/vnd.github+json", api]).output().map_err(|e| format!("curl did not start: {e}"))?;
    if !out.status.success() {
        return Err("could not reach yabridge's GitHub page".into());
    }
    let release: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| "yabridge's GitHub page gave an unreadable answer")?;
    // The plain build (`yabridge-5.1.1.tar.gz`), not the one for older systems (`-ubuntu-…`).
    let url = release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| Some((a["name"].as_str()?, a["browser_download_url"].as_str()?)))
        .find(|(n, _)| n.starts_with("yabridge-") && n.ends_with(".tar.gz") && !n.contains("ubuntu"))
        .map(|(_, u)| u.to_string())
        .ok_or("no Linux download in yabridge's latest release")?;
    let file = tmp.join("yabridge.tar.gz");
    download(&url, &file)?;
    untar(&file, tmp)?;
    Ok(release["tag_name"].as_str().unwrap_or("").to_string())
}

/// yabridge's newest development build (master, built by GitHub, through nightly.link: no GitHub login), unpacked into
/// `tmp/yabridge` with yabridgectl beside its files, as yabridge's README says. Its version ("5.1.1-57-gb580a9f7").
fn fetch_dev_build(tmp: &Path) -> Result<String, String> {
    let page = "https://nightly.link/robbert-vdh/yabridge/workflows/build/master";
    let out = Command::new("curl").args(["-fsSL", page]).output().map_err(|e| format!("curl did not start: {e}"))?;
    if !out.status.success() {
        return Err("could not reach yabridge's development builds".into());
    }
    let html = String::from_utf8_lossy(&out.stdout);
    let link = |name: &str| {
        html.split('"').find(|s| s.starts_with(page) && s.ends_with(".tar.gz.zip") && s.rsplit('/').next().is_some_and(|f| f.starts_with(name))).map(str::to_string)
    };
    let (Some(lib), Some(ctl)) = (link("yabridge-"), link("yabridgectl-")) else { return Err("no development build of yabridge was listed".into()) };
    for (url, zip) in [(&lib, "yabridge.zip"), (&ctl, "yabridgectl.zip")] {
        let zip = tmp.join(zip);
        download(url, &zip)?;
        let tarball = tarball_from_zip(&zip, tmp)?;
        untar(&tarball, tmp)?;
    }
    std::fs::rename(tmp.join("yabridgectl/yabridgectl"), tmp.join("yabridge/yabridgectl")).map_err(|e| e.to_string())?;
    let file = lib.rsplit('/').next().unwrap_or("");
    Ok(file.trim_start_matches("yabridge-").trim_end_matches(".tar.gz.zip").to_string())
}

/// Install yabridge: downloads it from yabridge's GitHub page (the development build when Wine is 9.22 or later, which
/// the release does not work with) and puts it in `~/.local/share/yabridge` (with `curl` and `tar`). Returns the
/// version installed.
pub fn install() -> Result<String, String> {
    let dir = install_dir().ok_or("no home folder")?;
    let tmp = std::env::temp_dir().join(format!("mango-yabridge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let dev = first_line(Path::new("wine"), &["--version"]).is_some_and(|w| needs_dev_build(&w));
    let got = if dev { fetch_dev_build(&tmp) } else { fetch_release(&tmp) };
    let new = tmp.join("yabridge");
    let version = match got {
        Ok(v) if new.join("yabridgectl").is_file() => v,
        Ok(_) => Err("the download was not a yabridge build".to_string())?,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    // In the old one's place.
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(&dir);
    let moved = std::fs::rename(&new, &dir).is_ok() || Command::new("cp").arg("-r").arg(&new).arg(&dir).status().is_ok_and(|s| s.success());
    let _ = std::fs::remove_dir_all(&tmp);
    if moved { Ok(version) } else { Err(format!("yabridge could not be put in {}", dir.display())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_bridged_folders() {
        let toml = "method = \"copy\"\nplugin_dirs = [\n    \"/home/me/.wine/drive_c/Program Files/Common Files/VST3\",\n    \"/a \\\"b\\\"\",\n]\nvst2_location = \"centralized\"\n";
        assert_eq!(plugin_dirs(toml), [PathBuf::from("/home/me/.wine/drive_c/Program Files/Common Files/VST3"), PathBuf::from("/a \"b\"")]);
        assert_eq!(plugin_dirs("plugin_dirs = [\"/x\", \"/y\"]"), [PathBuf::from("/x"), PathBuf::from("/y")]);
        assert!(plugin_dirs("method = \"copy\"").is_empty());
    }

    /// The release does not work with Wine 9.22 and later: those get the development build.
    #[test]
    fn newer_wine_needs_the_development_build() {
        assert!(needs_dev_build("wine-10.0"));
        assert!(needs_dev_build("wine-9.22 (Staging)"));
        assert!(needs_dev_build("wine-11.3"));
        assert!(!needs_dev_build("wine-9.21 (Staging)"));
        assert!(!needs_dev_build("wine-8.0"));
    }

    /// GitHub's build downloads: a zip holding the tarball.
    #[test]
    fn takes_the_tarball_out_of_the_zip() {
        use std::io::Write;
        let d = std::env::temp_dir().join(format!("mango-yabridge-zip-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let zip = d.join("a.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&zip).unwrap());
        w.start_file("yabridgectl-5.1.1.tar.gz", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(b"tarball").unwrap();
        w.finish().unwrap();
        let got = tarball_from_zip(&zip, &d).unwrap();
        assert_eq!((got.file_name().unwrap().to_str().unwrap(), std::fs::read(&got).unwrap()), ("yabridgectl-5.1.1.tar.gz", b"tarball".to_vec()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn knows_bridged_plugins() {
        let d = std::env::temp_dir().join(format!("mango-yabridge-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Synth.dll"), b"").unwrap();
        std::fs::write(d.join("Synth.so"), b"").unwrap();
        std::fs::write(d.join("Linux.so"), b"").unwrap();
        assert!(is_bridged(&d.join("Synth.so")));
        assert!(!is_bridged(&d.join("Linux.so")));
        assert!(is_bridged(Path::new("/home/me/.vst3/yabridge/Serum.vst3")));
        assert!(!is_bridged(Path::new("/usr/lib/vst3/Surge XT.vst3")));
        let _ = std::fs::remove_dir_all(&d);
        // The usual install folders, those that are there.
        let p = std::env::temp_dir().join(format!("mango-wine-test-{}", std::process::id()));
        std::fs::create_dir_all(p.join("drive_c/Program Files/Common Files/VST3")).unwrap();
        assert_eq!(wine_dirs(&p), [p.join("drive_c/Program Files/Common Files/VST3")]);
        let _ = std::fs::remove_dir_all(&p);
    }
}
