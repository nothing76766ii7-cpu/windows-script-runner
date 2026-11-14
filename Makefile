# Makefile for Windows Script Runner (Rust VFS + Android APK Edition v3.0)

PREFIX ?= $(HOME)/.antigem_tools
JNI_DIR = android/app/src/main/jniLibs

.PHONY: all rust-bins test android-jni apk clean

all: rust-bins

rust-bins:
	mkdir -p $(PREFIX)/bin
	rustc tools/bat2sh.rs -O -o $(PREFIX)/bin/bat2sh
	rustc tools/reg_parser.rs -O -o $(PREFIX)/bin/reg_parser
	cargo build --release --bins
	cp -f target/release/wsr $(PREFIX)/bin/wsr
	chmod +x $(PREFIX)/bin/*

test:
	cargo test --release

android-jni:
	cargo ndk -t arm64-v8a -t x86_64 -o $(JNI_DIR) build --release --lib
	cp -f ui/index.html android/app/src/main/assets/index.html

apk: android-jni
	gradle assembleDebug assembleRelease

clean:
	cargo clean
	rm -rf android/app/build $(JNI_DIR)
