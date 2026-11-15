#!/bin/sh
# Copyright (C) 2026 ItsAlphaNeon
# SPDX-License-Identifier: GPL-3.0-or-later
# This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

# Build the Mango Studio Android APKs, one per ABI: Mango-Studio-android-arm64.apk (arm64-v8a)
# and Mango-Studio-android-arm32.apk (armeabi-v7a). The gradle project's shape and this flow
# follow daijro's honeydew (GPL-3.0-or-later), https://github.com/daijro/honeydew.
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
mkdir -p "$build/jniLibs"

# Build the app's .so for one ABI into its jniLibs dir. The NDK's clang wrappers carry the API
# level (26 = minSdk). The rust target name and the linker wrapper's differ for 32-bit arm
# (rust: armv7-linux-androideabi, NDK: armv7a-linux-androideabi26-clang), hence the case.
build_abi() { # <abi> <rust triple>
    abi=$1 triple=$2
    case $triple in
        aarch64-linux-android)
            export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$TC/bin/aarch64-linux-android26-clang
            export CC_aarch64_linux_android=$TC/bin/aarch64-linux-android26-clang
            export CXX_aarch64_linux_android=$TC/bin/aarch64-linux-android26-clang++
            export AR_aarch64_linux_android=$TC/bin/llvm-ar
            ;;
        armv7-linux-androideabi)
            export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER=$TC/bin/armv7a-linux-androideabi26-clang
            export CC_armv7_linux_androideabi=$TC/bin/armv7a-linux-androideabi26-clang
            export CXX_armv7_linux_androideabi=$TC/bin/armv7a-linux-androideabi26-clang++
            export AR_armv7_linux_androideabi=$TC/bin/llvm-ar
            ;;
        *) echo "unknown abi $abi / triple $triple" >&2; exit 1 ;;
    esac
    mkdir -p "$build/jniLibs/$abi"
    cargo build --release --locked -p os-android --target "$triple"
    cp "target/$triple/release/libos_android.so" "$build/jniLibs/$abi/"
}

# Every non-system library the app's .so needs must ship in the APK, or the device's linker fails
# to `dlopen` it and the app closes the instant it opens (v0.4.0 shipped without libc++_shared.so,
# which the NativeActivity glue's C++ needs: GitHub issue about the launch crash). The NDK keeps
# the ones it provides in its sysroot; cargo-ndk copies libc++_shared.so the same way.
pack_needed() { # <abi> <rust triple (the sysroot's lib dir name)>
    dir=$build/jniLibs/$1
    so=$dir/libos_android.so
    $TC/bin/llvm-readelf -d "$so" | sed -n 's/.*(NEEDED).*Shared library: \[\([^]]*\)\].*/\1/p' |
    while read -r lib; do
        case $lib in
            libandroid.so|libaaudio.so|libc.so|libdl.so|liblog.so|libm.so|libEGL.so|libGLESv1_CM.so|libGLESv2.so|libvulkan.so|libnativewindow.so|libsync.so|libutils.so|libcutils.so|libz.so) ;;
            *)  if [ ! -f "$dir/$lib" ]; then
                    from=$TC/sysroot/usr/lib/$2/$lib
                    if [ -f "$from" ]; then
                        cp "$from" "$dir/"
                        echo "jniLibs: packed $lib for $1 (the app's .so needs it)"
                    else
                        echo "$so needs $lib, and the NDK sysroot has no copy to pack" >&2
                        exit 1
                    fi
                fi ;;
        esac
    done
}

build_abi arm64-v8a aarch64-linux-android
build_abi armeabi-v7a armv7-linux-androideabi
# Both ABIs are in place before gradle runs: its splits (see android/app/build.gradle) pack one
# APK per ABI from its jniLibs dir alone, and verifyNativeInputs requires both. The sysroot's
# lib dir names differ from the rust triples for 32-bit arm (arm-linux-androideabi, no `v7`).
pack_needed arm64-v8a aarch64-linux-android
pack_needed armeabi-v7a arm-linux-androideabi

android/gradlew -p android --no-daemon "$task"

case $task in assembleDebug) kind=debug ;; *) kind=release ;; esac
apk64=$build/outputs/apk/$kind/app-arm64-v8a-$kind.apk
apk32=$build/outputs/apk/$kind/app-armeabi-v7a-$kind.apk
sdk_tools=$ANDROID_HOME/build-tools/36.0.0

# The APK must be signed, aligned, and carry the same libraries its .so needs (the linker fails
# on a missing one).
verify_apk() { # <apk> <abi>
    "$sdk_tools/apksigner" verify "$1"
    "$sdk_tools/zipalign" -c -P 16 4 "$1"
    needed=$($TC/bin/llvm-readelf -d "$build/jniLibs/$2/libos_android.so" |
        sed -n 's/.*(NEEDED).*Shared library: \[\([^]]*\)\].*/\1/p' |
        grep -vE '^lib(android|aaudio|c|dl|log|m|EGL|GLESv1_CM|GLESv2|vulkan|nativewindow|sync|utils|cutils|z)\.so$')
    packed=$("$sdk_tools/aapt" list "$1" | grep "^lib/$2/")
    for lib in $needed; do
        printf '%s\n' "$packed" | grep -qF "lib/$2/$lib" ||
            { echo "$1 is missing $lib (the app's .so needs it): repack" >&2; exit 1; }
    done
}
verify_apk "$apk64" arm64-v8a
verify_apk "$apk32" armeabi-v7a

cp "$apk64" Mango-Studio-android-arm64.apk
cp "$apk32" Mango-Studio-android-arm32.apk
echo "wrote Mango-Studio-android-arm64.apk and Mango-Studio-android-arm32.apk"
