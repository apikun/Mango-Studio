#!/bin/sh
# Copyright (C) 2026 ItsAlphaNeon
# SPDX-License-Identifier: GPL-3.0-or-later
# This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

# Build the Mango Studio Android APK (arm64). The gradle project's shape and this flow follow
# daijro's honeydew (GPL-3.0-or-later), https://github.com/daijro/honeydew.
# usage: tools/android-dist.sh [--debug]
set -eu
cd "$(dirname "$0")/.."

: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK directory}"
export ANDROID_SDK_ROOT=$ANDROID_HOME

# The NDK: the first one installed (CI images ship one; the setup puts r28b in place).
NDK_HOME=
for d in "$ANDROID_HOME"/ndk/*/ "$ANDROID_HOME"/android-ndk-*/; do
    [ -d "$d" ] && NDK_HOME="${d%/}" && break
done
[ -n "$NDK_HOME" ] || { echo "no NDK under $ANDROID_HOME (sdkmanager 'ndk;28.1.13356709')" >&2; exit 1; }
export ANDROID_NDK_HOME=$NDK_HOME
TC=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$TC/bin/aarch64-linux-android26-clang
export CC_aarch64_linux_android=$TC/bin/aarch64-linux-android26-clang
export CXX_aarch64_linux_android=$TC/bin/aarch64-linux-android26-clang++
export AR_aarch64_linux_android=$TC/bin/llvm-ar
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}
# The desktop's include paths poison the NDK clang (host glibc headers against Android's libc).
unset CPATH BINDGEN_EXTRA_CLANG_ARGS C_INCLUDE_PATH CPLUS_INCLUDE_PATH 2>/dev/null || true

task=assembleRelease
case "${1:-}" in
    '') ;;
    --debug) task=assembleDebug ;;
    *) echo "usage: $0 [--debug]" >&2; exit 2 ;;
esac

build=${MANGO_ANDROID_BUILD_DIR:-target/android}
mkdir -p "$build/jniLibs/arm64-v8a"
cargo build --release --locked -p os-android --target aarch64-linux-android
cp target/aarch64-linux-android/release/libos_android.so "$build/jniLibs/arm64-v8a/"

# Every non-system library the app's .so needs must ship in the APK, or the device's linker fails
# to `dlopen` it and the app closes the instant it opens (v0.4.0 shipped without libc++_shared.so,
# which the NativeActivity glue's C++ needs: GitHub issue about the launch crash). The NDK keeps
# the ones it provides in its sysroot; cargo-ndk copies libc++_shared.so the same way.
so=$build/jniLibs/arm64-v8a/libos_android.so
$TC/bin/llvm-readelf -d "$so" | sed -n 's/.*(NEEDED).*Shared library: \[\([^]]*\)\].*/\1/p' |
while read -r lib; do
    case $lib in
        libandroid.so|libaaudio.so|libc.so|libdl.so|liblog.so|libm.so|libEGL.so|libGLESv1_CM.so|libGLESv2.so|libvulkan.so|libnativewindow.so|libsync.so|libutils.so|libcutils.so|libz.so) ;;
        *)  if [ ! -f "$build/jniLibs/arm64-v8a/$lib" ]; then
                from=$TC/sysroot/usr/lib/aarch64-linux-android/$lib
                if [ -f "$from" ]; then
                    cp "$from" "$build/jniLibs/arm64-v8a/"
                    echo "jniLibs: packed $lib (the app's .so needs it)"
                else
                    echo "$so needs $lib, and the NDK sysroot has no copy to pack" >&2
                    exit 1
                fi
            fi ;;
    esac
done

android/gradlew -p android --no-daemon "$task"

apk=$(ls "$build"/outputs/apk/*/app-*.apk | head -1)
sdk_tools=$ANDROID_HOME/build-tools/36.0.0
"$sdk_tools/apksigner" verify "$apk"
"$sdk_tools/zipalign" -c -P 16 4 "$apk"
# The APK must carry the same libraries the .so needs (the linker fails on a missing one).
needed=$($TC/bin/llvm-readelf -d "$so" |
    sed -n 's/.*(NEEDED).*Shared library: \[\([^]]*\)\].*/\1/p' |
    grep -vE '^lib(android|aaudio|c|dl|log|m|EGL|GLESv1_CM|GLESv2|vulkan|nativewindow|sync|utils|cutils|z)\.so$')
packed=$("$sdk_tools/aapt" list "$apk" | grep '^lib/arm64-v8a/')
for lib in $needed; do
    printf '%s\n' "$packed" | grep -qF "lib/arm64-v8a/$lib" ||
        { echo "the APK is missing $lib (the app's .so needs it): repack" >&2; exit 1; }
done
cp "$apk" Mango-Studio-android-arm64.apk
echo "wrote Mango-Studio-android-arm64.apk"
