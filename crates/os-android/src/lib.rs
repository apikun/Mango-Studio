// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Mango Studio Mobile: the Android entry. A cdylib with `android_main`, loaded by the
//! NativeActivity in `android/` (`android:app.lib_name`). Everything else is `os-app`, the
//! application as a library. The activity's Java side and the gradle project follow daijro's
//! honeydew (GPL-3.0-or-later), https://github.com/daijro/honeydew — credited in
//! THIRD-PARTY-NOTICES.txt.

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    os_app::mobile::run(app);
}
