#!/bin/sh
# Copyright (C) 2026 ItsAlphaNeon
# SPDX-License-Identifier: GPL-3.0-or-later
# This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

# Build the Mango Studio Android APK. The gradle project's shape and this flow follow
# daijro's honeydew (GPL-3.0-or-later), https://github.com/daijro/honeydew.
#
# The APK is universal: it carries one native library per ABI in the list below, so a single
# download installs on both 64-bit (arm64-v8a) and 32-bit (armeabi-v7a) ARM devices. Older
# phones and some embedded Android targets are 32-bit only; without armeabi-v7a they cannot
# install an arm64-only APK at all.
#
# usage: tools/android-dist.sh [--debug]
#   MANGO_ANDROID_ABIS   space-separated ABI list (default "arm64-v8a armeabi-v7a")
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
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}
# The desktop's include paths poison the NDK clang (host glibc headers against Android's libc).
unset CPATH BINDGEN_EXTRA_CLANG_ARGS C_INCLUDE_PATH CPLUS_INCLUDE_PATH 2>/dev/null || true

# The minimum SDK the APK targets (android/app/build.gradle keeps minSdk in sync); the NDK clang
# driver bakes this into its name, e.g. aarch64-linux-android26-clang.
API=26

# The ABIs the universal APK carries. Each maps to a Rust target, the NDK clang driver's triple
# (armv7 uses "armv7a" and the "eabi" suffix), and the cargo linker/CC variable (the Rust target
# upper-cased with '-' as '_'). The sysroot subdir uses the clang triple without the API level.
default_abis='arm64-v8a armeabi-v7a'
ABIS=${MANGO_ANDROID_ABIS:-$default_abis}

rust_target_for() {
    case $1 in
        arm64-v8a)   echo aarch64-linux-android ;;
        armeabi-v7a) echo armv7-linux-androideabi ;;
        x86_64)      echo x86_64-linux-android ;;
        x86)         echo i686-linux-android ;;
        *) echo "unknown ABI: $1" >&2; return 1 ;;
    esac
}
clang_triple_for() {
    case $1 in
        arm64-v8a)   echo aarch64-linux-android ;;
        armeabi-v7a) echo armv7a-linux-androideabi ;;
        x86_64)      echo x86_64-linux-android ;;
        x86)         echo i686-linux-android ;;
        *) echo "unknown ABI: $1" >&2; return 1 ;;
    esac
}
sysroot_dir_for() {
    # The sysroot library directory uses the plain triple (armv7 drops the "a", keeps "eabi").
    case $1 in
        arm64-v8a)   echo aarch64-linux-android ;;
        armeabi-v7a) echo arm-linux-androideabi ;;
        x86_64)      echo x86_64-linux-android ;;
        x86)         echo i686-linux-android ;;
        *) echo "unknown ABI: $1" >&2; return 1 ;;
    esac
}
cargo_env_key_for() {
    # The Rust target, upper-cased with '-' -> '_' (CARGO_TARGET_<TRIPLE>_LINKER and CC_<triple>).
    rust_target_for "$1" | tr 'a-z-' 'A-Z_'
}

task=assembleRelease
case "${1:-}" in
    '') ;;
    --debug) task=assembleDebug ;;
    *) echo "usage: $0 [--debug]" >&2; exit 2 ;;
esac

build=${MANGO_ANDROID_BUILD_DIR:-target/android}

# The non-system libraries the NDK provides (never packed; the device supplies them).
is_system_lib() {
    case $1 in
        libandroid.so|libaaudio.so|libc.so|libdl.so|liblog.so|libm.so|libEGL.so|libGLESv1_CM.so|libGLESv2.so|libvulkan.so|libnativewindow.so|libsync.so|libutils.so|libcutils.so|libz.so) return 0 ;;
        *) return 1 ;;
    esac
}

# Build each ABI's native library and pack it (plus the extra libraries it needs) into the ABI's
# jniLibs directory.
for abi in $ABIS; do
    rust_target=$(rust_target_for "$abi")
    clang_triple=$(clang_triple_for "$abi")
    sysroot_dir=$(sysroot_dir_for "$abi")
    env_key=$(cargo_env_key_for "$abi")

    clang=$TC/bin/${clang_triple}${API}-clang
    [ -x "$clang" ] || { echo "no NDK clang for $abi at $clang" >&2; exit 1; }

    # Per-ABI cargo linker and bindgen compilers (exported for this target's build only).
    export "CARGO_TARGET_${env_key}_LINKER=$clang"
    eval "export CC_${rust_target}=\"$clang\""
    eval "export CXX_${rust_target}=\"${clang}++\""
    eval "export AR_${rust_target}=\"$TC/bin/llvm-ar\""

    dest=$build/jniLibs/$abi
    mkdir -p "$dest"
    cargo build --release --locked -p os-android --target "$rust_target"
    cp "target/$rust_target/release/libos_android.so" "$dest/"

    # Every non-system library the app's .so needs must ship in the APK, or the device's linker
    # fails to `dlopen` it and the app closes the instant it opens (v0.4.0 shipped without
    # libc++_shared.so, which the NativeActivity glue's C++ needs: GitHub issue about the launch
    # crash). The NDK keeps the ones it provides in its sysroot; cargo-ndk copies them the same way.
    so=$dest/libos_android.so
    "$TC/bin/llvm-readelf" -d "$so" | sed -n 's/.*(NEEDED).*Shared library: \[\([^]]*\)\].*/\1/p' |
    while read -r lib; do
        if is_system_lib "$lib"; then continue; fi
        if [ ! -f "$dest/$lib" ]; then
            from=$TC/sysroot/usr/lib/$sysroot_dir/$lib
            if [ -f "$from" ]; then
                cp "$from" "$dest/"
                echo "jniLibs[$abi]: packed $lib (the app's .so needs it)"
            else
                echo "$so needs $lib, and the NDK sysroot has no copy to pack" >&2
                exit 1
            fi
        fi
    done
done

# Tell gradle which ABIs to filter in (so the manifest and APK carry exactly these), then build.
MANGO_ANDROID_ABIS="$ABIS" android/gradlew -p android --no-daemon "$task"

apk=$(ls "$build"/outputs/apk/*/app-*.apk | head -1)
sdk_tools=$ANDROID_HOME/build-tools/36.0.0
"$sdk_tools/apksigner" verify "$apk"
"$sdk_tools/zipalign" -c -P 16 4 "$apk"

# The APK must carry, for every ABI, the same libraries that ABI's .so needs (the linker fails on
# a missing one).
packed=$("$sdk_tools/aapt" list "$apk")
for abi in $ABIS; do
    so=$build/jniLibs/$abi/libos_android.so
    needed=$("$TC/bin/llvm-readelf" -d "$so" |
        sed -n 's/.*(NEEDED).*Shared library: \[\([^]]*\)\].*/\1/p' |
        grep -vE '^lib(android|aaudio|c|dl|log|m|EGL|GLESv1_CM|GLESv2|vulkan|nativewindow|sync|utils|cutils|z)\.so$')
    for lib in $needed; do
        printf '%s\n' "$packed" | grep -qF "lib/$abi/$lib" ||
            { echo "the APK is missing lib/$abi/$lib (the app's .so needs it): repack" >&2; exit 1; }
    done
done

cp "$apk" Mango-Studio-android.apk
echo "wrote Mango-Studio-android.apk (ABIs: $ABIS)"
