// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

package dev.apikun.mangostudio;

import android.app.NativeActivity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.Settings;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import java.io.File;
import java.io.IOException;

/**
 * The one activity, as {@link NativeActivity} with the system bars handled. The manifest's theme
 * hides the status bar but not the navigation bar, and nothing ever hid that one again: on a phone
 * held sideways the bar (back and recents) sits on the right edge and stayed there for good. In
 * sticky immersive mode it only shows while the user reaches for it, over the app, and hides
 * itself again after a few seconds — as in every other fullscreen app.
 */
public class MainActivity extends NativeActivity {
    /** The marker that the All-files-access page was already opened once; per install (per user). */
    private static final String ASKED_ALL_FILES = "asked-all-files";
    /** The marker that the microphone was already asked about once; per install (per user). */
    private static final String ASKED_MICROPHONE = "asked-microphone";
    /** The code the microphone's runtime permission ask answers. */
    private static final int ASK_MICROPHONE = 1;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        askForAllFiles();
        askForMicrophone();
        hideBars();
    }

    /**
     * The mixer's tracks and One-click audio recording take an input device in (Settings > Audio's
     * input device): recording hears the microphone, which needs the runtime {@code RECORD_AUDIO}
     * grant. Once per install this asks for it at the launch; a device that never grants it records
     * no input, and everything else keeps working.
     */
    private void askForMicrophone() {
        if (checkSelfPermission(android.Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            return;
        }
        File asked = new File(getFilesDir(), ASKED_MICROPHONE);
        if (asked.exists()) {
            return;
        }
        requestPermissions(new String[] {android.Manifest.permission.RECORD_AUDIO}, ASK_MICROPHONE);
        try {
            asked.createNewFile();
        } catch (IOException ignored) {
        }
    }

    /**
     * The app saves in the public Documents ({@code Documents/Mango Studio}), which a file manager
     * reaches on every device — the install's private /data folder many devices hide does not.
     * Writing there on Android 11+ needs the All-files-access grant, a switch in the system's
     * settings: once per install this opens that page (one toggle) at the launch, and the native
     * side moves the user folder over when it finds the grant on. A device that never grants it
     * keeps the app on the private folder, which works but stays hidden.
     */
    private void askForAllFiles() {
        if (Build.VERSION.SDK_INT < 30 || Environment.isExternalStorageManager()) {
            return;
        }
        File asked = new File(getFilesDir(), ASKED_ALL_FILES);
        if (asked.exists()) {
            return;
        }
        try {
            Intent i =
                new Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                    Uri.parse("package:" + getPackageName()));
            startActivity(i);
        } catch (Exception e) {
            try {
                startActivity(new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION));
            } catch (Exception ignored) {
            }
        }
        try {
            asked.createNewFile();
        } catch (IOException ignored) {
        }
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        // Bars come back when an overlay (a dialog, the keyboard) takes the focus: hide them again
        // when the app has it.
        if (hasFocus) {
            hideBars();
        }
    }

    private void hideBars() {
        Window w = getWindow();
        if (Build.VERSION.SDK_INT >= 30) {
            WindowInsetsController c = w.getInsetsController();
            if (c != null) {
                // Transient bars: a swipe shows them over the app and they go away on their own.
                c.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                c.hide(WindowInsets.Type.systemBars());
            }
        } else {
            w.getDecorView().setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_FULLSCREEN
                | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
        }
    }
}
