// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Android: the entry glue behind `crates/os-android`'s `android_main`. The private data
//! folder becomes the app's user folder (`os-paths` reads `OPENSTUDIO_USER_DIR` on every
//! platform), and the winit event loop is built on the NativeActivity's app. Everything
//! after that is the desktop run (`main_body::run_from`): one stream, one window, the same
//! project model. Touch as mouse is `crate::touch`.

use std::sync::OnceLock;

use android_activity::AndroidApp;
use jni::JavaVM;
use winit::event_loop::EventLoop;

/// The app's JVM, for native threads that ask Android for a priority (`raise_priority`) or a folder
/// (`public_dir`), set at entry.
static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();
/// The NativeActivity as a JNI global reference (`AndroidApp::activity_as_ptr`), for `finish_activity`.
static ACTIVITY: OnceLock<usize> = OnceLock::new();

/// The JVM `raise_priority` uses; `None` before `run` (and a desktop build never asks).
pub(crate) fn audio_vm() -> Option<&'static JavaVM> {
    JAVA_VM.get()
}

/// The NativeActivity's app thread runs this: the app lives as long as the activity does.
pub fn run(app: AndroidApp) {
    // The app's files live in the install's private folder, so no storage permission. Set before
    // anything reads a path (the first read is `App::new`'s settings, in `run_from`); the public
    // folder below takes the user one over when its grant is on.
    let internal_user = app.internal_data_path().filter(|p| p.is_absolute()).map(|data| {
        let dir = data.join("user");
        let _ = std::fs::create_dir_all(&dir);
        // Edition 2024: setting the environment is unsafe (threads may be reading it; none started yet).
        unsafe { std::env::set_var("OPENSTUDIO_USER_DIR", &dir) };
        // Cursor themes and other HOME readers look in the app's folder; whatever they miss is skipped.
        unsafe { std::env::set_var("HOME", &data) };
        dir
    });
    // A crash's log is also written here (`crash_log` reads it), because the private folder is reachable only over
    // adb; a file manager opens the external one, so the launch crash is readable on the device itself. When the
    // public folder works, the logs move into it (`Logs` there) and this stays the fallback.
    if let Some(ext) = app.external_data_path().filter(|p| p.is_absolute()) {
        let _ = std::fs::create_dir_all(&ext);
        unsafe { std::env::set_var("OPENSTUDIO_LOG_DIR", &ext) };
    }
    // The JVM before anything asks Android a question (the user folder does).
    // SAFETY: the pointer is the live JVM of this process, still owned by android-activity.
    if let Ok(vm) = (unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }) {
        let _ = JAVA_VM.set(vm);
    }
    // The user folder wants to be in the public Documents (`Documents/Mango Studio`): a file
    // manager reaches it on every device, where the install's private /data folder many devices
    // hide does not — the project saves, the samples and the logs all land somewhere the user can
    // open. It needs the All-files-access grant (Android 11+; `MainActivity` asks for it once per
    // install), and until that is on the app keeps the private folder, which works but stays hidden.
    if let Some(public) = public_dir() {
        if let Some(internal) = &internal_user {
            migrate(internal, &public);
        }
        unsafe { std::env::set_var("OPENSTUDIO_USER_DIR", &public) };
        let logs = public.join("Logs");
        if std::fs::create_dir_all(&logs).is_ok() {
            unsafe { std::env::set_var("OPENSTUDIO_LOG_DIR", &logs) };
        }
    }
    // The starter kit's samples (the APK's assets) land in the user folder's Sounds, where a new
    // project's channels play them from: a phone has no other samples to load, and Play with no
    // notes or samples is silent (the metronome would be the only sound).
    if let Some(user) = os_paths::user_dir() {
        extract_kit(&app, &user.join("Sounds").join("Mango Kit"));
    }
    crate::log_panics();
    // The back button's last step (`App::key`'s Escape arm) asks the activity to finish: keep its
    // global reference. SAFETY: the pointer is the live activity object, owned by android-activity.
    unsafe { let _ = ACTIVITY.set(app.activity_as_ptr() as usize); }
    let mut builder = EventLoop::<()>::with_user_event();
    use winit::platform::android::EventLoopBuilderExtAndroid;
    builder.with_android_app(app);
    crate::run_from(builder, None, true);
}

/// The system back with nothing left to close: `Activity.finish()`, what the button does on any
/// other app. The JNI call comes back false-quietly when the VM or the activity is not there (the
/// app then simply stays), and a pending exception is cleared, as `raise_priority` does.
pub fn finish_activity() {
    let Some(vm) = JAVA_VM.get() else { return };
    let Some(&activity) = ACTIVITY.get() else { return };
    let Ok(mut env) = vm.attach_current_thread() else { return };
    // SAFETY: the reference is android-activity's live global one (it says never to delete it;
    // `from_raw` borrows it without owning, and the `JObject` frees no reference when dropped).
    let activity = unsafe { jni::objects::JObject::from_raw(activity as jni::sys::jobject) };
    let finish = env.call_method(activity, "finish", "()V", &[]);
    if finish.is_err() {
        let _ = env.exception_clear();
    }
}

/// Copies the starter kit's WAVs out of the APK's assets into `dir` (the user folder's
/// `Sounds/Mango Kit`), keeping the ones already there: a reinstall's fresh user folder gets the
/// kit, an update's does not (its files, if the user changed them, stay). The names the starter
/// project plays are `crate::app::starter_kit`'s.
#[cfg(target_os = "android")]
fn extract_kit(app: &AndroidApp, dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir);
    for name in crate::app::starter_kit() {
        let to = dir.join(name);
        if to.is_file() {
            continue;
        }
        let Ok(name) = std::ffi::CString::new(format!("kit/{name}")) else { continue };
        let Some(mut asset) = app.asset_manager().open(name.as_c_str()) else {
            continue;
        };
        use std::io::Read;
        let mut bytes = Vec::new();
        if asset.read_to_end(&mut bytes).is_ok() {
            let _ = std::fs::write(&to, bytes);
        }
    }
}

/// The public folder the app saves into: `Documents/Mango Studio` on the shared storage — a file
/// manager reaches it on every device, where the install's private `/data` folder many devices
/// hide does not. `None` (keep the private folder) until the All-files-access grant is on: it is
/// Android 11's runtime switch for direct writes to the shared storage, `MainActivity` opens its
/// settings page once per install, and a device that never grants it keeps the app working on the
/// private folder it always had.
#[cfg(target_os = "android")]
fn public_dir() -> Option<std::path::PathBuf> {
    use jni::objects::JString;
    let vm = JAVA_VM.get()?;
    let Ok(mut env) = vm.attach_current_thread() else { return None };
    // The grant is `Environment.isExternalStorageManager`; the method does not exist before
    // Android 11, and its exception (like any other) says stay private.
    let granted = env
        .call_static_method("android/os/Environment", "isExternalStorageManager", "()Z", &[])
        .ok()
        .and_then(|v| v.z().ok());
    if env.exception_check().unwrap_or(true) {
        let _ = env.exception_clear();
        return None;
    }
    if granted != Some(true) {
        return None;
    }
    // The shared-storage root: `Environment.getExternalStorageDirectory` (deprecated for app
    // data, still the honest path for the public folders).
    let Ok(root) = env
        .call_static_method("android/os/Environment", "getExternalStorageDirectory", "()Ljava/io/File;", &[])
        .and_then(|v| v.l())
    else {
        let _ = env.exception_clear();
        return None;
    };
    let Ok(path) = env.call_method(&root, "getPath", "()Ljava/lang/String;", &[]).and_then(|v| v.l()) else {
        let _ = env.exception_clear();
        return None;
    };
    let jstring = JString::from(path);
    let Ok(text) = env.get_string(&jstring) else {
        let _ = env.exception_clear();
        return None;
    };
    let text = text.to_string_lossy();
    if text.is_empty() {
        return None;
    }
    let dir = std::path::PathBuf::from(text.into_owned()).join("Documents").join("Mango Studio");
    // The grant is on, but a probe settles it: a device (or its mount) can still refuse the write,
    // and then the app stays on the private folder instead of saving into a folder it cannot read.
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let probe = dir.join(".write-probe");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok.then_some(dir)
}

/// The one-time move of the user's files to the public folder: everything the private folder holds
/// that `Documents/Mango Studio` does not yet is copied (projects, settings, the kit's samples), a
/// marker keeps it from ever running twice — deletions in Documents afterwards stay deleted — and
/// the private copy stays as the fallback the app runs on when the grant is off again.
#[cfg(target_os = "android")]
fn migrate(internal: &std::path::Path, public: &std::path::Path) {
    let marker = internal.join(".migrated-to-documents");
    if marker.exists() {
        return;
    }
    copy_missing(internal, public);
    let _ = std::fs::write(&marker, b"");
}

/// Copies every file of `from` that `to` lacks, recursively (dirs first, then the files they hold;
/// copies that fail are skipped — the next launch with the grant on picks them up).
#[cfg(target_os = "android")]
fn copy_missing(from: &std::path::Path, to: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(from) else { return };
    for entry in entries.flatten() {
        let dest = to.join(entry.file_name());
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            if std::fs::create_dir_all(&dest).is_ok() {
                copy_missing(&entry.path(), &dest);
            }
        } else if !dest.exists() {
            let _ = std::fs::copy(entry.path(), &dest);
        }
    }
}
