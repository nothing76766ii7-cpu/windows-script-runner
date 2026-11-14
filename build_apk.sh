#!/bin/sh
# One-command local build script for compiling Rust JNI libraries and building the Android APK
set -e

echo "[1/4] Running Rust VFS & Transpiler unit tests..."
cargo test --release

echo "[2/4] Compiling Rust JNI libraries (libwsr_jni.so) for arm64-v8a and x86_64..."
mkdir -p android/app/src/main/jniLibs
cargo ndk -t arm64-v8a -t x86_64 -o android/app/src/main/jniLibs build --release --lib

echo "[3/4] Syncing Windows Explorer SVG UI into Android assets..."
mkdir -p android/app/src/main/assets
cp -f ui/index.html android/app/src/main/assets/index.html

echo "[4/4] Building Android APK via Gradle..."
gradle assembleDebug assembleRelease

echo "Done! APKs generated under android/app/build/outputs/apk/"
