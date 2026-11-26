#!/usr/bin/env bash
# Cross-compile the openshield binary for aarch64 Android (Termux or adb shell).
# Needs: rustup target aarch64-linux-android, Android NDK (NDK_HOME or
# $ANDROID_HOME/ndk/<version>). Builds the phone feature set: web-api only.
# telegram is left out because teloxide's default features pull in
# native-tls/openssl-sys, which has no Android sysroot here.
# Usage: scripts/build-android-core.sh [extra cargo args]
set -euo pipefail
cd "$(dirname "$0")/.."
API=${ANDROID_API:-26}
ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
NDK=${NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}
TC=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin
export CC_aarch64_linux_android=$TC/aarch64-linux-android${API}-clang
export CXX_aarch64_linux_android=$TC/aarch64-linux-android${API}-clang++
export AR_aarch64_linux_android=$TC/llvm-ar
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$TC/aarch64-linux-android${API}-clang
cargo build --release --locked --target aarch64-linux-android \
  --no-default-features --features web-api --bin openshield "$@"
out=${CARGO_TARGET_DIR:-target}/aarch64-linux-android/release/openshield
"$TC/llvm-strip" -o "$out.stripped" "$out"
echo "Built $out.stripped"
