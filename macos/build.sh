#!/usr/bin/env bash
# Builds the Rust core as a universal static library, then the SwiftUI app and
# wraps it in Simplenotorious.app.  Run on macOS with Xcode + rustup installed.
set -euo pipefail
cd "$(dirname "$0")"
ROOT="$(cd .. && pwd)"

rustup target add aarch64-apple-darwin x86_64-apple-darwin
for t in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release -p simplenotorious-ffi --target "$t" --manifest-path "$ROOT/Cargo.toml"
done
mkdir -p lib
lipo -create \
  "$ROOT/target/aarch64-apple-darwin/release/libsimplenotorious_ffi.a" \
  "$ROOT/target/x86_64-apple-darwin/release/libsimplenotorious_ffi.a" \
  -output lib/libsimplenotorious_ffi.a

swift build -c release --arch arm64 --arch x86_64

APP=Simplenotorious.app
rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$(swift build -c release --arch arm64 --arch x86_64 --show-bin-path)/Simplenotorious" "$APP/Contents/MacOS/"
cp Info.plist "$APP/Contents/Info.plist"
echo "Built $(pwd)/$APP"
