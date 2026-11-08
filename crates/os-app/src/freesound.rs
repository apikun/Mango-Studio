// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Freesound in the browser (https://freesound.org/docs/api/). The Freesound section searches with the browser's search
//! text (a click on its "Search Freesound" row). The search and each preview download run on their own threads; the
//! app collects their results in `poll` on its tick. A clicked sound's preview is downloaded once into a cache folder,
//! decoded for its waveform, and played through the browser's preview.
//!
//! The API key is read from `~/.mango-studio/freesound_token.txt` (one line) when that file exists, so a user can use their own
//! key; otherwise the program's shared key (`KEY`) is used.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use os_engine::Sample;

/// Tree key of the Freesound root in the browser's All tab.
pub const PREFIX: &str = "0:Freesound";

/// One search result: the fields the browser shows and plays.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    pub id: u64,
    pub name: String,
    pub duration: f32,
    /// The high-quality MP3 preview URL (the low-quality preview is the fallback).
    pub preview: String,
    /// Freesound's small waveform image; results can display it before the audio preview is downloaded.
    pub waveform: String,
}

/// The Freesound section's state (`State::freesound`).
#[derive(Debug, Clone, Default)]
pub struct Fs {
    /// What the section says under its search row.
    pub status: String,
    pub results: Vec<Sound>,
    /// Waveform peaks of each downloaded preview, 0..1 per bin.
    pub peaks: HashMap<u64, Vec<f32>>,
    /// Cached preview files by sound id.
    pub files: HashMap<u64, PathBuf>,
    /// Short MP3 excerpts used to start an audition quickly; playlist drops use the full preview in `files`.
    pub quick_files: HashMap<u64, PathBuf>,
    pub quick_lengths: HashMap<u64, f32>,
    quick_queue: VecDeque<(u64, String)>,
    pub quick_loading: HashSet<u64>,
    quick_fetching: HashSet<u64>,
    /// Peak amplitude of each decoded preview, used to make quiet previews audible.
    pub levels: HashMap<u64, f32>,
    /// A clicked sound whose preview is still downloading: it plays when the download is done.
    pub waiting: Option<u64>,
    /// Sounds whose preview is downloading (all of a new search's results, one after the other).
    pub loading: HashSet<u64>,
    /// Active network jobs (loading also includes queued previews).
    fetching: HashSet<u64>,
    preview_queue: VecDeque<(u64, String)>,
    pub failed: HashSet<u64>,
    waveform_queue: VecDeque<(u64, String)>,
    pub waveform_loading: HashSet<u64>,
    waveform_fetching: HashSet<u64>,
    pub waveform_failed: HashSet<u64>,
    waveform_total: usize,
    waveform_done: usize,
    pub busy: bool,
    /// The sound being heard now (its card shows a stop mark and a moving line over the waveform).
    pub playing: Option<Playing>,
    /// A card's playhead being dragged: the sound and its share of the waveform (0..1). The preview moves there when
    /// the button is let go.
    pub scrub: Option<(u64, f32)>,
    /// Where a waveform was clicked while its preview was still downloading (share of the sound, 0..1): `poll` starts
    /// the preview there once the file is in.
    pub waiting_from: Option<f32>,
    /// Freesound's first page, shown automatically when its browser tab opens.
    pub recommended: bool,
    /// Playlist drops waiting for a preview file to finish downloading.
    pub pending_drops: HashMap<u64, Vec<PendingDrop>>,
}

#[derive(Debug, Clone)]
pub struct PendingDrop {
    pub track: u16,
    pub tick: u32,
    pub placeholder: Option<(u16, u32)>,
}

/// A Freesound preview that finished downloading after it was dropped on the playlist.
#[derive(Debug, Clone)]
pub struct DropReady {
    pub id: u64,
    pub path: PathBuf,
    pub track: u16,
    pub tick: u32,
    pub placeholder: Option<(u16, u32)>,
    pub seconds: f32,
}

/// A preview that sounds: which sound, since when, and for how long (the preview is cut off after a few seconds unless
/// "Full sample preview" is on), and how far into the sound this play began.
#[derive(Debug, Clone, Copy)]
pub struct Playing {
    pub id: u64,
    pub since: Instant,
    pub secs: f32,
    pub duration: f32,
    /// Seconds into the sound where this play started (0 for a normal preview, the playhead's place after a seek).
    pub from: f32,
}

/// Sound `id` starts sounding now, cut off after `limit` seconds when there is a limit.
pub fn start(s: &mut Fs, id: u64, limit: Option<f32>) {
    let duration = s.results.iter().find(|x| x.id == id).map_or(0.0, |x| x.duration);
    let secs = limit.map_or(duration, |l| duration.min(l));
    s.playing = Some(Playing { id, since: Instant::now(), secs, duration, from: 0.0 });
}

/// Sound `id` starts sounding at `from` seconds into it (the card's playhead was moved there).
pub fn start_at(s: &mut Fs, id: u64, from: f32) {
    let duration = s.results.iter().find(|x| x.id == id).map_or(0.0, |x| x.duration);
    let from = from.clamp(0.0, (duration - 0.05).max(0.0));
    s.playing = Some(Playing { id, since: Instant::now(), secs: duration, duration, from });
}

/// Where a click at `frac` (0..1 across the waveform) starts the preview in seconds.
pub fn offset(s: &Fs, id: u64, frac: f32) -> f32 {
    let duration = s.results.iter().find(|x| x.id == id).map_or(0.0, |x| x.duration);
    let from = frac.clamp(0.0, 1.0) * duration;
    from.clamp(0.0, (duration - 0.05).max(0.0))
}

/// How far into sound `id` the preview is (0..1 of the whole sound), while it sounds.
pub fn progress(s: &Fs, id: u64) -> Option<f32> {
    let p = s.playing.filter(|p| p.id == id)?;
    let t = p.from + p.since.elapsed().as_secs_f32();
    (t < p.secs).then(|| (t / p.duration.max(0.01)).min(1.0))
}

/// Called on every tick: ends a finished preview. Returns whether the cards need repainting (a preview sounds).
pub fn tick(s: &mut Fs) -> bool {
    match s.playing {
        Some(p) if p.from + p.since.elapsed().as_secs_f32() >= p.secs => {
            s.playing = None;
            true
        }
        Some(_) => true,
        None => false,
    }
}

enum Msg {
    Results(Result<Vec<Sound>, String>),
    Ready(u64, PathBuf, Vec<f32>, f32, f32),
    QuickReady(u64, PathBuf, Vec<f32>, f32, f32),
    Failed(u64, String),
    Waveform(u64, Result<Vec<f32>, String>),
}

/// Results of the background threads, waiting for `poll`.
fn inbox() -> &'static Mutex<Vec<Msg>> {
    static INBOX: OnceLock<Mutex<Vec<Msg>>> = OnceLock::new();
    INBOX.get_or_init(|| Mutex::new(Vec::new()))
}

fn post(m: Msg) {
    if let Ok(mut q) = inbox().lock() {
        q.push(m);
    }
}

/// The program's own Freesound API key (https://freesound.org/docs/api/), shared by its users; a key in the user's own file
/// takes its place.
const KEY: &str = "WRFyFXPHbWZs1SPPn14l5VjmJs9Lfdm0GJwpsI4Y";

/// The API key: the one in the user's own file when there is one, else the program's.
pub fn token() -> Result<String, String> {
    let own = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".mango-studio").join("freesound_token.txt"))
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    Ok(own.unwrap_or_else(|| KEY.to_string()))
}

/// Starts a search for `query` in the background.
pub fn search(s: &mut Fs, query: &str) {
    let q = query.trim().to_string();
    let recommended = q.is_empty();
    let tok = match token() {
        Ok(t) => t,
        Err(e) => {
            s.status = e;
            return;
        }
    };
    s.busy = true;
    for (id, _) in s.preview_queue.drain(..) {
        s.loading.remove(&id);
    }
    // Discard queued audition prefetches from the previous search. In-flight requests
    // are allowed to finish and will remain cached if they do.
    for (id, _) in s.quick_queue.drain(..) {
        s.quick_loading.remove(&id);
    }
    for (id, _) in s.waveform_queue.drain(..) {
        s.waveform_loading.remove(&id);
    }
    s.waveform_total = 0;
    s.waveform_done = 0;
    s.waveform_failed.clear();
    s.recommended = recommended;
    s.scrub = None;
    s.waiting_from = None;
    s.status = if recommended { "Loading Freesound's highest-rated samples...".into() } else { format!("Searching Freesound for \"{q}\"...") };
    std::thread::spawn(move || post(Msg::Results(fetch_search(&q, &tok, recommended))));
}

/// `curl` without a console window: the app has none of its own, and Windows opens (and closes) one for every child
/// process started without this flag, which flickered for each search and preview download.
fn curl() -> Command {
    let mut cmd = Command::new("curl");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn fetch_search(query: &str, token: &str, recommended: bool) -> Result<Vec<Sound>, String> {
    let mut cmd = curl();
    cmd
        .args(["-fsSL", "-G", "--max-time", "20"])
        .arg("--data-urlencode")
        .arg(format!("query={query}"))
        .arg("--data-urlencode")
        .arg("fields=id,name,duration,previews,images")
        .arg("--data-urlencode")
        .arg(if recommended { "page_size=12" } else { "page_size=30" })
        .arg("--data-urlencode")
        .arg(if recommended { "sort=rating_desc" } else { "sort=score" })
        .arg("--data-urlencode")
        .arg(format!("token={token}"))
        .arg("https://freesound.org/apiv2/search/");
    let out = cmd.output()
        .map_err(|e| format!("curl did not start: {e}"))?;
    if !out.status.success() {
        return Err("Freesound did not answer: check the API key and the connection".into());
    }
    parse(&out.stdout)
}

/// The results of a search response (`results`: id, name, duration, previews).
pub fn parse(json: &[u8]) -> Result<Vec<Sound>, String> {
    let v: serde_json::Value = serde_json::from_slice(json).map_err(|e| format!("Freesound: {e}"))?;
    let list = v.get("results").and_then(|r| r.as_array()).ok_or_else(|| "Freesound: no results in the answer".to_string())?;
    Ok(list
        .iter()
        .filter_map(|r| {
            Some(Sound {
                id: r.get("id")?.as_u64()?,
                name: r.get("name")?.as_str()?.to_string(),
                duration: r.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0) as f32,
                preview: r.get("previews")?.get("preview-hq-mp3").or_else(|| r.get("previews")?.get("preview-lq-mp3"))?.as_str()?.to_string(),
                waveform: r.get("images").and_then(|i| i.get("waveform_m").or_else(|| i.get("waveform_l"))).and_then(|u| u.as_str()).unwrap_or_default().to_string(),
            })
        })
        .collect())
}

/// A sound row was clicked. Returns its preview file when it is already cached; otherwise starts the download and
/// returns `None` (the file plays when `poll` sees it).
pub fn click(s: &mut Fs, id: u64) -> Option<PathBuf> {
    click_mode(s, id, true)
}

/// A seek needs the full preview so the chosen offset exists in the decoded sample.
pub fn click_for_seek(s: &mut Fs, id: u64) -> Option<PathBuf> {
    click_mode(s, id, false)
}

fn click_mode(s: &mut Fs, id: u64, quick: bool) -> Option<PathBuf> {
    s.waiting_from = None;
    s.scrub = None;
    if let Some(p) = s.files.get(&id) {
        let name = s.results.iter().find(|x| x.id == id).map_or("", |x| x.name.as_str());
        s.status = format!("Playing: {name}");
        return Some(p.clone());
    }
    if quick {
        if let Some(p) = s.quick_files.get(&id) {
            let name = s.results.iter().find(|x| x.id == id).map_or("sample", |x| x.name.as_str());
            s.status = format!("Playing: {name}");
            return Some(p.clone());
        }
    }
    let url = s.results.iter().find(|x| x.id == id)?.preview.clone();
    s.waiting = Some(id);
    let name = s.results.iter().find(|x| x.id == id).map_or("sample", |x| x.name.as_str());
    s.status = format!("Downloading preview: {name}...");
    if quick {
        queue_quick(s, id, url, true);
    } else {
        queue_preview(s, id, url, true);
    }
    None
}

/// A Freesound result dropped on the playlist. Returns its cached preview immediately, or queues the clip until the
/// preview download finishes. Freesound requires OAuth2 for original downloads; the API-key-only flow imports its
/// high-quality MP3 preview instead.
pub fn drop_sample(s: &mut Fs, id: u64, track: u16, tick: u32, placeholder: Option<(u16, u32)>) -> Option<PathBuf> {
    if let Some(path) = s.files.get(&id) {
        return Some(path.clone());
    }
    let Some(url) = s.results.iter().find(|x| x.id == id).map(|x| x.preview.clone()) else {
        s.status = "That Freesound sample is no longer in the results".into();
        return None;
    };
    s.pending_drops.entry(id).or_default().push(PendingDrop { track, tick, placeholder });
    s.status = "Downloading Freesound preview for the playlist...".into();
    queue_preview(s, id, url, false);
    None
}

const PREVIEW_CONCURRENCY: usize = 3;
const QUICK_CONCURRENCY: usize = 3;

fn queue_quick(s: &mut Fs, id: u64, url: String, priority: bool) {
    if s.quick_files.contains_key(&id) { return; }
    if s.quick_loading.insert(id) {
        if priority { s.quick_queue.push_front((id, url)); }
        else { s.quick_queue.push_back((id, url)); }
    } else if priority && !s.quick_fetching.contains(&id) {
        if let Some(pos) = s.quick_queue.iter().position(|(queued, _)| *queued == id) {
            s.quick_queue.remove(pos);
            s.quick_queue.push_front((id, url));
        }
    }
}

fn start_quick(s: &mut Fs) {
    while s.quick_fetching.len() < QUICK_CONCURRENCY {
        let Some((id, url)) = s.quick_queue.pop_front() else { break };
        s.quick_fetching.insert(id);
        std::thread::spawn(move || post(fetch_quick_preview(id, &url)));
    }
}

fn queue_preview(s: &mut Fs, id: u64, url: String, priority: bool) {
    s.failed.remove(&id);
    if s.files.contains_key(&id) {
        return;
    }
    if s.loading.insert(id) {
        if priority { s.preview_queue.push_front((id, url)); }
        else { s.preview_queue.push_back((id, url)); }
    } else if priority && !s.fetching.contains(&id) {
        if let Some(pos) = s.preview_queue.iter().position(|(queued, _)| *queued == id) {
            s.preview_queue.remove(pos);
            s.preview_queue.push_front((id, url));
        }
    }
}

fn start_queued(s: &mut Fs) {
    while s.fetching.len() < PREVIEW_CONCURRENCY {
        let Some((id, url)) = s.preview_queue.pop_front() else { break };
        s.fetching.insert(id);
        std::thread::spawn(move || post(fetch_preview(id, &url)));
    }
}

const WAVEFORM_CONCURRENCY: usize = 4;

fn waveform_status(s: &Fs) -> String {
    if s.waveform_done < s.waveform_total {
        format!("Loading waveforms... {} of {}", s.waveform_done, s.waveform_total)
    } else {
        format!("Waveforms ready for {} samples · click to play, drag to playlist", s.waveform_total.saturating_sub(s.waveform_failed.len()))
    }
}

fn start_waveforms(s: &mut Fs) {
    while s.waveform_fetching.len() < WAVEFORM_CONCURRENCY {
        let Some((id, url)) = s.waveform_queue.pop_front() else { break };
        s.waveform_fetching.insert(id);
        std::thread::spawn(move || post(fetch_waveform(id, &url)));
    }
}

fn fetch_waveform(id: u64, url: &str) -> Msg {
    let output = curl().args(["-fsSL", "--retry", "2", "--retry-all-errors", "--retry-delay", "1", "--connect-timeout", "8", "--max-time", "15", "--max-filesize", "2097152"]).arg(url).output();
    let bytes = match output {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Msg::Waveform(id, Err("Freesound waveform did not download".into())),
    };
    let Some((w, h, px)) = os_ui::Canvas::decode_png(&bytes) else {
        return Msg::Waveform(id, Err("Freesound returned an invalid waveform image".into()));
    };
    let bins = 96usize;
    let mut out = vec![0.0f32; bins];
    if w == 0 || h == 0 { return Msg::Waveform(id, Err("Freesound returned an empty waveform".into())); }
    // Freesound's waveform PNGs have a flat background and a contrasting waveform trace. Measure each
    // column's contrast from the image's most common pixel, then reduce it to the browser's peak bars.
    let mut colors = HashMap::<u32, usize>::new();
    for &c in &px { *colors.entry(c).or_default() += 1; }
    let background = colors.into_iter().max_by_key(|(_, n)| *n).map_or(0, |(c, _)| c);
    for x in 0..w {
        let mut top = h / 2;
        let mut bottom = h / 2;
        for y in 0..h {
            let c = px[y * w + x];
            let dr = ((c >> 16) & 255) as i32 - ((background >> 16) & 255) as i32;
            let dg = ((c >> 8) & 255) as i32 - ((background >> 8) & 255) as i32;
            let db = (c & 255) as i32 - (background & 255) as i32;
            if dr.abs() + dg.abs() + db.abs() > 70 {
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
        let height = (h / 2).max(1) as f32;
        let peak = ((h / 2 - top).max(bottom.saturating_sub(h / 2)) as f32 / height).clamp(0.0, 1.0);
        let b = (x * bins / w).min(bins - 1);
        out[b] = out[b].max(peak);
    }
    let max = out.iter().copied().fold(0.0f32, f32::max);
    if max > 0.0 { for v in &mut out { *v /= max; } }
    Msg::Waveform(id, Ok(out))
}

/// Whether `path` is a file of the Freesound cache (a downloaded preview).
pub fn is_cached(path: &str) -> bool {
    cache_dir().is_ok_and(|d| std::path::Path::new(path).starts_with(d))
}

fn cache_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).ok_or_else(|| "Freesound: no home folder".to_string())?;
    let dir = home.join(".orange-studio").join("Freesound");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Freesound cache: {e}"))?;
    Ok(dir)
}

/// A tiny valid audio file so a dropped Freesound sample can appear in the playlist immediately while its full audio downloads.
pub fn placeholder_audio() -> Result<PathBuf, String> {
    let path = cache_dir()?.join("drop-placeholder.wav");
    if path.exists() { return Ok(path); }
    let (rate, frames) = (44100u32, 2205u32);
    let data_len = frames * 4;
    let mut wav = Vec::with_capacity((44 + data_len) as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.resize((44 + data_len) as usize, 0);
    std::fs::write(&path, wav).map_err(|e| format!("Could not prepare the playlist clip: {e}"))?;
    Ok(path)
}

fn fetch_preview(id: u64, url: &str) -> Msg {
    let dir = match cache_dir() { Ok(dir) => dir, Err(e) => return Msg::Failed(id, e) };
    let path = dir.join(format!("{id}.mp3"));
    if !path.exists() {
        // Download to a partial name first, so a cut download is never taken for a finished one.
        let part = dir.join(format!("{id}.part"));
        // HQ previews can be several megabytes and this connection is sometimes slow; a short request timeout
        // repeatedly truncated otherwise-valid files, leaving the card stuck in its loading state.
        let ok = curl().args(["-fsSL", "--retry", "1", "--retry-all-errors", "--retry-delay", "1", "--connect-timeout", "12", "--max-time", "180", "--speed-time", "30", "--speed-limit", "1024", "-o"]).arg(&part).arg(url).status();
        if !matches!(ok, Ok(st) if st.success()) {
            let _ = std::fs::remove_file(&part);
            return Msg::Failed(id, "The preview did not download".into());
        }
        if std::fs::rename(&part, &path).is_err() {
            return Msg::Failed(id, "The preview could not be saved".into());
        }
    }
    match os_engine::sample::load(&path) {
        Ok(sample) => {
            let level = sample.left.iter().chain(&sample.right).fold(0.0f32, |m, x| m.max(x.abs()));
            let seconds = sample.frames() as f32 / sample.rate.max(1) as f32;
            Msg::Ready(id, path, peaks(&sample, WAVE_BINS), level, seconds)
        }
        Err(e) => Msg::Failed(id, format!("The preview did not decode: {e}")),
    }
}

fn fetch_quick_preview(id: u64, url: &str) -> Msg {
    let dir = match cache_dir() { Ok(dir) => dir, Err(e) => return Msg::Failed(id, e) };
    let path = dir.join(format!("{id}-quick.mp3"));
    if !path.exists() {
        let part = dir.join(format!("{id}-quick.part"));
        // A short byte-range is enough for a responsive audition; the full preview is fetched only for a playlist drop.
        let ok = curl().args(["-fsSL", "--retry", "1", "--retry-all-errors", "--retry-delay", "1", "--connect-timeout", "8", "--max-time", "30", "--range", "0-262143", "-o"]).arg(&part).arg(url).status();
        if !matches!(ok, Ok(st) if st.success()) {
            let _ = std::fs::remove_file(&part);
            return Msg::Failed(id, "The sample preview did not download".into());
        }
        if std::fs::rename(&part, &path).is_err() {
            return Msg::Failed(id, "The sample preview could not be saved".into());
        }
    }
    match os_engine::sample::load(&path) {
        Ok(sample) => {
            let level = sample.left.iter().chain(&sample.right).fold(0.0f32, |m, x| m.max(x.abs()));
            let seconds = sample.frames() as f32 / sample.rate.max(1) as f32;
            Msg::QuickReady(id, path, peaks(&sample, WAVE_BINS), level, seconds)
        }
        Err(e) => Msg::Failed(id, format!("The short preview did not decode: {e}")),
    }
}

/// Waveform bins kept per sound.
pub const WAVE_BINS: usize = 96;

/// The loudest sample of each of `bins` slices of the sound, scaled so the loudest bin is 1.
pub fn peaks(s: &Sample, bins: usize) -> Vec<f32> {
    let n = s.left.len().max(s.right.len());
    if n == 0 || bins == 0 {
        return vec![0.0; bins];
    }
    let mut out: Vec<f32> = (0..bins)
        .map(|i| {
            let a = i * n / bins;
            let b = ((i + 1) * n / bins).max(a + 1).min(n);
            (a..b).fold(0f32, |m, k| m.max(s.left.get(k).copied().unwrap_or(0.0).abs()).max(s.right.get(k).copied().unwrap_or(0.0).abs()))
        })
        .collect();
    let top = out.iter().copied().fold(0f32, f32::max).max(1e-6);
    for v in &mut out {
        *v /= top;
    }
    out
}

/// Takes the finished background work into `s`. Returns whether anything changed and the file to play, when a click
/// was waiting for its download: the sound, its file and the seconds into it to start at.
pub fn poll(s: &mut Fs) -> (bool, Option<(u64, PathBuf, f32, f32, Option<f32>)>, Vec<DropReady>) {
    let msgs = match inbox().lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => return (false, None, Vec::new()),
    };
    let changed = !msgs.is_empty();
    let mut play = None;
    let mut drops = Vec::new();
    for m in msgs {
        match m {
            Msg::Results(Ok(list)) => {
                s.busy = false;
                s.status = if list.is_empty() { "No sounds found".into() } else if s.recommended { format!("Highest-rated samples · {} sounds · click to preview, drag to playlist", list.len()) } else if list.len() == 1 { "1 sound".to_string() } else { format!("{} sounds", list.len()) };
                s.failed.clear();
                s.results = list;
                if s.results.is_empty() { s.status = "No sounds found".into(); }
                // Warm small audition excerpts for the first visible results in the
                // background, so the first Play click does not wait on the network.
                let auditions: Vec<_> = s.results.iter().take(6).map(|x| (x.id, x.preview.clone())).collect();
                for (id, url) in auditions {
                    if !url.is_empty() { queue_quick(s, id, url, false); }
                }
                let initial_waveforms: Vec<_> = s.results.iter().take(12).map(|x| (x.id, x.waveform.clone())).collect();
                s.waveform_total = initial_waveforms.len();
                s.waveform_done = 0;
                for (id, url) in initial_waveforms {
                    if url.is_empty() {
                        s.waveform_failed.insert(id);
                        s.waveform_done += 1;
                    } else {
                        s.waveform_loading.insert(id);
                        s.waveform_queue.push_back((id, url));
                    }
                }
                if s.waveform_total > 0 { s.status = waveform_status(s); }
            }
            Msg::Results(Err(e)) => {
                s.busy = false;
                s.status = e;
            }
            Msg::Ready(id, path, bins, level, seconds) => {
                s.loading.remove(&id);
                s.fetching.remove(&id);
                s.files.insert(id, path.clone());
                s.peaks.insert(id, bins);
                s.levels.insert(id, level);
                if let Some(positions) = s.pending_drops.remove(&id) {
                    drops.extend(positions.into_iter().map(|d| DropReady { id, path: path.clone(), track: d.track, tick: d.tick, placeholder: d.placeholder, seconds }));
                }
                if s.waiting == Some(id) {
                    s.waiting = None;
                    let name = s.results.iter().find(|x| x.id == id).map_or("", |x| x.name.as_str());
                    s.status = format!("Playing: {name}");
                    let from = s.waiting_from.take().map_or(0.0, |f| offset(s, id, f));
                    play = Some((id, path, from, level, None));
                }
            }
            Msg::QuickReady(id, path, bins, level, seconds) => {
                s.quick_loading.remove(&id);
                s.quick_fetching.remove(&id);
                s.quick_files.insert(id, path.clone());
                s.quick_lengths.insert(id, seconds);
                s.levels.insert(id, level);
                s.peaks.entry(id).or_insert(bins);
                if s.waiting == Some(id) {
                    s.waiting = None;
                    let name = s.results.iter().find(|x| x.id == id).map_or("sample", |x| x.name.as_str());
                    s.status = format!("Playing: {name}");
                    let from = s.waiting_from.take().unwrap_or(0.0);
                    play = Some((id, path, from, level, Some(seconds)));
                }
            }
            Msg::Failed(id, e) => {
                s.loading.remove(&id);
                s.fetching.remove(&id);
                s.quick_loading.remove(&id);
                s.quick_fetching.remove(&id);
                s.failed.insert(id);
                if s.pending_drops.remove(&id).is_some() {
                    s.status = format!("Could not add Freesound sample: {e}");
                }
                // Only a sound the user asked for reports its failure; the background ones just stay without a waveform.
                if s.waiting == Some(id) {
                    s.waiting = None;
                    s.status = e;
                }
            }
            Msg::Waveform(id, result) => {
                s.waveform_fetching.remove(&id);
                s.waveform_loading.remove(&id);
                s.waveform_done += 1;
                match result {
                    Ok(bins) => { s.peaks.entry(id).or_insert(bins); }
                    Err(_) => { s.waveform_failed.insert(id); }
                }
                if s.waiting.is_none() { s.status = waveform_status(s); }
            }
        }
    }
    start_queued(s);
    start_quick(s);
    start_waveforms(s);
    (changed, play, drops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_search_answer() {
        let json = br#"{"count":2,"results":[{"id":5,"name":"Kick 1","duration":0.5,"previews":{"preview-lq-mp3":"https://x/5.mp3","preview-hq-mp3":"https://x/5h.mp3"},"images":{"waveform_m":"https://x/5.png"}},{"id":7,"name":"lq only","duration":1.0,"previews":{"preview-lq-mp3":"https://x/7.mp3"}},{"id":6,"name":"no preview"}]}"#;
        let got = parse(json).unwrap();
        // The higher-quality preview is the one played; the lower one stands in when a sound has no other.
        assert_eq!(
            got,
            vec![
                Sound { id: 5, name: "Kick 1".into(), duration: 0.5, preview: "https://x/5h.mp3".into(), waveform: "https://x/5.png".into() },
                Sound { id: 7, name: "lq only".into(), duration: 1.0, preview: "https://x/7.mp3".into(), waveform: String::new() },
            ]
        );
    }

    #[test]
    fn the_playing_position_follows_the_clock_and_ends() {
        let mut s = Fs::default();
        s.results.push(Sound { id: 1, name: "a".into(), duration: 10.0, preview: String::new(), waveform: String::new() });
        start(&mut s, 1, Some(5.0));
        assert!(progress(&s, 1).is_some_and(|p| p < 0.05));
        assert!(progress(&s, 2).is_none(), "another sound is not playing");
        assert!(tick(&mut s), "a sounding preview keeps the cards repainting");
        // Past the cut-off the preview is over.
        let back = std::time::Duration::from_secs(6);
        s.playing.as_mut().unwrap().since = Instant::now().checked_sub(back).unwrap_or_else(Instant::now);
        if s.playing.unwrap().since.elapsed() >= back {
            assert!(progress(&s, 1).is_none());
            assert!(tick(&mut s));
            assert!(s.playing.is_none());
            assert!(!tick(&mut s));
        }
    }

    #[test]
    fn a_seek_starts_the_position_at_the_playhead() {
        let mut s = Fs::default();
        s.results.push(Sound { id: 1, name: "a".into(), duration: 10.0, preview: String::new(), waveform: String::new() });
        let from = offset(&s, 1, 0.4);
        assert!((from - 4.0).abs() < 1e-6);
        start_at(&mut s, 1, from);
        let p = s.playing.unwrap();
        assert!((p.from - 4.0).abs() < 1e-6 && p.secs == 10.0);
        assert!(progress(&s, 1).is_some_and(|x| (0.4..0.41).contains(&x)), "the line starts at the playhead");
    }

    #[test]
    fn peaks_scale_to_the_loudest_bin() {
        let s = Sample { rate: 44100, left: vec![0.1, 0.5, 0.0, 0.25], right: vec![0.0; 4] };
        let p = peaks(&s, 2);
        assert_eq!(p.len(), 2);
        assert!((p[0] - 1.0).abs() < 1e-6);
        assert!((p[1] - 0.5).abs() < 1e-6);
    }
}
