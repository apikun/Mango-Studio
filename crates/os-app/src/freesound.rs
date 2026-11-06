// Copyright (C) 2026 apikun
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Freesound: the browser's gateway to freesound.org's sample library (the Discord suggestion, as Ardour had it
//! built in). The Freesound root's search row types a query in the browser's search field; Enter asks freesound.org's
//! developer API with the program's own key, and the sounds it found become rows of their own. A click fetches the
//! sound's free preview (an MP3; the full-quality original would need the user's own OAuth login) into
//! `Sounds/Freesound` of the user folder and plays it — from then on the row is a sample like a file's, dragged on
//! the playlist or added to a channel, and a sound fetched before plays again at once. Searches and downloads run on
//! threads of their own (as the zip jobs do), so a slow network never stops the sound.
//!
// spec: ui/browser.md §2, "Online content"

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use crate::app::App;

/// The program's own Freesound API key (https://freesound.org/docs/api/): one key the program's users share, as the
/// FL-style programs before this one shipped theirs. Given out by Orange Studio's developer.
const KEY: &str = "WRFyFXPHbWZs1SPPn14l5VjmJs9Lfdm0GJwpsI4Y";
/// The API's root.
const API: &str = "https://freesound.org/apiv2";
/// Sounds one search asks for.
const PAGE_SIZE: u32 = 30;

/// One sound freesound.org found.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    pub id: u32,
    pub name: String,
    pub user: String,
    /// How long it plays, in seconds.
    pub secs: f32,
    /// The free preview's MP3 URL.
    pub preview: String,
}

/// The browser's Freesound side: what is typed, what the last search found, what is on its way.
#[derive(Default)]
pub struct Browser {
    pub query: String,
    /// The browser's search field edits the query (the search row was picked and not left yet).
    pub typing: bool,
    pub results: Vec<Sound>,
    /// The search in flight.
    pub searching: Option<Receiver<Result<Vec<Sound>, String>>>,
    /// The downloads in flight.
    downloads: Vec<Download>,
    /// Why the last search failed, while its rows say so.
    pub error: Option<String>,
}

/// A preview fetch on its way.
struct Download {
    rx: Receiver<Result<PathBuf, String>>,
    id: u32,
    name: String,
}

/// The search URL for `query`: the API's text search, with the fields the rows read.
pub fn search_url(query: &str, page: u32) -> String {
    format!("{API}/search/text/?query={}&fields=id,name,username,duration,previews&page_size={PAGE_SIZE}&page={page}", percent(query))
}

/// A query as a URL part: everything but an unreserved character percent-encoded, as spaces are (`%20`).
fn percent(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The API reply's sounds (a reply without its results, or one not JSON at all, is the reason it could not be read).
pub fn parse_results(body: &str) -> Result<Vec<Sound>, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("the reply is not JSON ({e})"))?;
    let rs = v.get("results").and_then(|r| r.as_array()).ok_or("the reply has no results")?;
    Ok(rs
        .iter()
        .filter_map(|r| {
            Some(Sound {
                id: r.get("id")?.as_u64()? as u32,
                name: r.get("name")?.as_str()?.to_string(),
                user: r.get("username").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                secs: r.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0) as f32,
                preview: r.get("previews")?.get("preview-hq-mp3")?.as_str()?.to_string(),
            })
        })
        .collect())
}

/// GET `url` with the program's key: the body, or the reason it did not come.
fn get(url: &str) -> Result<Vec<u8>, String> {
    let mut res = ureq::get(url)
        .set("Authorization", &format!("Token {KEY}"))
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|e| e.to_string())?;
    let mut body = Vec::new();
    res.into_reader().read_to_end(&mut body).map_err(|e| e.to_string())?;
    Ok(body)
}

/// The search itself, on the thread (`start_search` made).
fn search(query: &str) -> Result<Vec<Sound>, String> {
    let body = get(&search_url(query, 1))?;
    parse_results(std::str::from_utf8(&body).map_err(|_| "the reply is not text".to_string())?)
}

/// The search row's pick (or Enter while typing): the query goes to freesound.org on a thread of its own.
pub fn start_search(app: &mut App) {
    let query = app.state.freesound.query.trim().to_string();
    if query.is_empty() {
        return;
    }
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new().name("freesound-search".into()).spawn(move || {
        let _ = tx.send(search(&query));
    });
    let fs = &mut app.state.freesound;
    if spawned.is_err() {
        fs.error = Some("the search could not start".into());
        return;
    }
    fs.typing = false;
    fs.searching = Some(rx);
    fs.error = None;
    app.invalidate();
}

/// A result row's click: a sound fetched before plays at once; the others are fetched first.
pub fn preview(app: &mut App, i: usize) {
    let Some(sound) = app.state.freesound.results.get(i).cloned() else { return };
    let file = file_of(&sound);
    if file.exists() {
        play(app, &file);
        return;
    }
    if app.state.freesound.downloads.iter().any(|d| d.id == sound.id) {
        return; // already on its way
    }
    let (tx, rx) = channel();
    let id = sound.id;
    let name = sound.name.clone();
    let spawned = std::thread::Builder::new().name("freesound-download".into()).spawn(move || {
        let _ = tx.send(fetch_preview(&sound, &file));
    });
    if spawned.is_err() {
        app.state.message = Some("Freesound: the download could not start".into());
        return;
    }
    app.state.freesound.downloads.push(Download { rx, id, name: name.clone() });
    app.state.message = Some(format!("Freesound: downloading {name}…"));
    app.invalidate();
}

/// Fetches a sound's preview and keeps it in the cache folder.
fn fetch_preview(sound: &Sound, file: &Path) -> Result<PathBuf, String> {
    let bytes = get(&sound.preview)?;
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(file, &bytes).map_err(|e| e.to_string())?;
    Ok(file.to_path_buf())
}

/// Pumps the searches and downloads in flight (each frame, as the zip jobs are).
pub fn tick(app: &mut App) {
    if app.state.freesound.searching.is_some() {
        let mut done: Option<Result<Vec<Sound>, String>> = None;
        if let Some(rx) = &app.state.freesound.searching {
            done = match rx.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err("stopped".into())),
            };
        }
        if let Some(r) = done {
            let fs = &mut app.state.freesound;
            fs.searching = None;
            match r {
                Ok(results) => {
                    fs.results = results;
                    fs.error = None;
                }
                Err(e) => {
                    fs.error = Some(e.clone());
                    app.state.message = Some(format!("Freesound: {e}"));
                }
            }
            app.invalidate();
        }
    }
    let mut played: Vec<String> = Vec::new();
    let mut said: Option<String> = None;
    {
        let fs = &mut app.state.freesound;
        let mut left = Vec::new();
        for d in fs.downloads.drain(..) {
            match d.rx.try_recv() {
                Ok(Ok(path)) => played.push(path.to_string_lossy().into_owned()),
                Ok(Err(e)) => said = Some(format!("Freesound: {}: {e}", d.name)),
                Err(TryRecvError::Empty) => left.push(d),
                Err(TryRecvError::Disconnected) => {}
            }
        }
        fs.downloads = left;
    }
    if let Some(m) = said {
        app.state.message = Some(m);
    }
    for p in played {
        let path = PathBuf::from(&p);
        app.state.message = Some(format!("Freesound: saved {}", file_label(&path)));
        play(app, &path);
    }
}

/// Plays a sound file, as the browser plays one of its own.
fn play(app: &mut App, file: &Path) {
    let path = file.to_string_lossy().into_owned();
    app.preview_sample(&path);
}

/// The folder the sounds are kept in: `Sounds/Freesound` of the user folder, where the Mango Pack's folder is too.
pub fn cache_dir() -> Option<PathBuf> {
    os_paths::user_dir().map(|d| d.join("Sounds").join("Freesound"))
}

/// The file a sound is kept as: its id and its name, made file-safe.
pub fn file_of(sound: &Sound) -> PathBuf {
    let name: String = sound.name.chars().map(|c| match c { '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ', _ => c }).collect();
    cache_dir().unwrap_or_else(std::env::temp_dir).join(format!("{} {name}.mp3", sound.id))
}

/// A result's label: the sound's name without its extension, as the browser names a file's row.
pub fn sound_label(sound: &Sound) -> String {
    match sound.name.rsplit_once('.') {
        Some((stem, ext)) if ext.len() <= 4 && ext.chars().all(|c| c.is_ascii_alphanumeric()) => stem.to_string(),
        _ => sound.name.clone(),
    }
}

/// A file's label, as messages name it.
fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    /// The search URL carries the query percent-encoded (a space `%20`), the fields the rows read, and the page.
    #[test]
    fn the_search_url_carries_the_query() {
        assert_eq!(
            search_url("kick drum", 1),
            format!("{API}/search/text/?query=kick%20drum&fields=id,name,username,duration,previews&page_size={PAGE_SIZE}&page=1")
        );
        assert!(search_url("a&b", 1).contains("query=a%26b"));
    }

    /// The API reply's sounds: id, name, user, seconds, the MP3 preview; a reply that is not JSON says so.
    #[test]
    fn the_reply_becomes_sounds() {
        let body = r#"{"count":2,"next":null,"previous":null,"results":[
            {"id": 91662, "name": "Kick 01.wav", "username": "Suicidity", "duration": 2.07,
             "previews": {"preview-hq-mp3": "https://cdn.freesound.org/previews/91/91662_12810-hq.mp3",
                          "preview-lq-mp3": "https://cdn.freesound.org/previews/91/91662_12810-lq.mp3"}},
            {"id": 7, "name": "Snare.ogg", "username": "someone", "duration": 0.5, "previews": {}}
        ]}"#;
        let sounds = parse_results(body).unwrap();
        assert_eq!(sounds.len(), 1, "a sound without an MP3 preview is left out");
        assert_eq!(sounds[0], Sound {
            id: 91662,
            name: "Kick 01.wav".into(),
            user: "Suicidity".into(),
            secs: 2.07,
            preview: "https://cdn.freesound.org/previews/91/91662_12810-hq.mp3".into(),
        });
        assert!(parse_results("not json").unwrap_err().contains("not JSON"));
        assert!(parse_results(r#"{"count": 0}"#).unwrap_err().contains("no results"));
    }

    /// A sound is kept under its own name with its id in front, made file-safe; a row names it without the extension.
    #[test]
    fn a_sound_has_its_own_file() {
        let s = Sound { id: 91662, name: r#"Tony's "Kick"/Drum: 01?.wav"#.into(), user: "u".into(), secs: 2.0, preview: "p".into() };
        let f = file_of(&s);
        assert_eq!(f.file_name().unwrap().to_string_lossy(), "91662 Tony's  Kick  Drum  01 .wav.mp3");
        assert_eq!(sound_label(&s), r#"Tony's "Kick"/Drum: 01?"#);
        assert_eq!(sound_label(&Sound { id: 1, name: "no extension".into(), user: String::new(), secs: 0.0, preview: String::new() }), "no extension");
    }

    /// A finished search fills the rows; a failed one says why.
    #[test]
    fn a_finished_search_becomes_rows() {
        let mut app = App::new(None);
        let (tx, rx) = channel();
        tx.send(Ok(vec![Sound { id: 7, name: "Snare.ogg".into(), user: "s".into(), secs: 0.5, preview: "p".into() }])).unwrap();
        app.state.freesound.searching = Some(rx);
        tick(&mut app);
        assert!(app.state.freesound.searching.is_none());
        assert_eq!(app.state.freesound.results.len(), 1);
        assert!(app.state.freesound.error.is_none());
        let (tx, rx) = channel();
        tx.send(Err("no network".into())).unwrap();
        app.state.freesound.searching = Some(rx);
        tick(&mut app);
        assert_eq!(app.state.freesound.error.as_deref(), Some("no network"));
        assert!(app.state.message.as_deref().is_some_and(|m| m.contains("no network")));
    }

    /// A result's click fetches its preview on a thread; a fetch already on its way takes the click, and a file no
    /// server has comes back as a message, nothing kept downloading.
    #[test]
    fn a_click_fetches_a_sound() {
        let mut app = App::new(None);
        app.state.freesound.results = vec![Sound { id: 1, name: "Gone.mp3".into(), user: String::new(), secs: 1.0, preview: "not a url".into() }];
        // A fetch already on its way takes the click: no second download.
        let (_hold, rx) = channel();
        app.state.freesound.downloads.push(Download { rx, id: 1, name: "Hold.mp3".into() });
        preview(&mut app, 0);
        assert_eq!(app.state.freesound.downloads.len(), 1, "a second click waits for the first");
        app.state.freesound.downloads.clear();
        preview(&mut app, 0);
        assert_eq!(app.state.freesound.downloads.len(), 1, "the download started");
        assert!(app.state.message.as_deref().is_some_and(|m| m.contains("downloading")), "{:?}", app.state.message);
        // The fetch of "not a url" fails at once; the pump (tick) clears it and says so.
        for _ in 0..500 {
            if app.state.freesound.downloads.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            tick(&mut app);
        }
        assert!(app.state.freesound.downloads.is_empty(), "the failed download is gone");
        assert!(app.state.message.as_deref().is_some_and(|m| m.contains("Gone.mp3")), "{:?}", app.state.message);
    }
}
