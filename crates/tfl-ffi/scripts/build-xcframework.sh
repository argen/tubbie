#!/usr/bin/env bash
# Build TflFFI.xcframework — the iOS-shippable artifact for B.2.
#
# Bundles two static archives (device arm64 + simulator arm64) plus the
# uniffi-generated C header + modulemap into a single .xcframework that
# Xcode can drop into a target's "Frameworks, Libraries, and Embedded
# Content" pane.
#
# Output: src-tauri/gen/apple/Externals/TflFFI.xcframework
#
# Run from anywhere:
#     bash crates/tfl-ffi/scripts/build-xcframework.sh
#
# Idempotent: deletes any prior xcframework before recreating it.
#
# ## Why a release-mode bindgen probe (M1 from B.2 review)
#
# uniffi 0.31 reads its scaffolding metadata from the library at bindgen
# time. If the bindgen probe is built with a different feature set (or
# debug vs release) than the iOS slices, the emitted Swift bindings can
# diverge from the ABI the iOS app actually links against — silently. A
# future feature-gated export (e.g. `panic-probe` from `run-swift-spike.sh`)
# could land in the host probe but not the shipped archives, producing
# "_undefined symbol" at link time or, worse, a mismatched variant tag.
# We anchor on a release-mode HOST probe with NO non-default features so
# the metadata snapshot matches the iOS release archives.
#
# ## Why staticlib only for iOS
#
# src-tauri/Cargo.toml has a long comment about iOS link timing — rustc
# can't resolve `extern "C"` Swift symbols at link time when the bridge
# is the consumer target. tfl-ffi has the inverse problem (Swift→Rust
# direction) but keeps the same constraint: ship .a, let Xcode app-link.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
cd "$REPO"

# --- Preflight (S2): make the toolchain failure mode helpful ---------------

require_target() {
    local t=$1
    if ! rustup target list --installed | grep -qx "$t"; then
        echo "error: rustup target '$t' is not installed." >&2
        echo "       run: rustup target add $t" >&2
        exit 1
    fi
}
require_target aarch64-apple-ios
require_target aarch64-apple-ios-sim

# --- Preflight (M2): submodule must be initialised before the iOS target ---
# fixtures end up bundled. xcodegen will silently embed the absolute path
# into pbxproj, and `xcodebuild` then fails mid-build with a confusing
# "no such file or directory" during the resource-copy phase. Catch it now.

if [[ ! -d third_party/tubbie/fixtures ]]; then
    echo "error: third_party/tubbie/fixtures/ is missing." >&2
    echo "       The new SwiftUI target bundles these as a build resource." >&2
    echo "       Run: git submodule update --init third_party/tubbie" >&2
    exit 1
fi

# --- Build the iOS slices --------------------------------------------------

echo "==> Cross-compiling tfl-ffi for iOS targets (release, default features)..."
# `cargo rustc --crate-type staticlib` cuts the cdylib build from the iOS
# slice. Cargo.toml declares `crate-type = ["cdylib", "staticlib", "lib"]`
# for the host bindgen path, but the iOS cdylib builds pull in
# reqwest/TLS deps (rustls + aws-lc-sys + Security framework symbols)
# that fail to link as a dylib for aarch64-apple-ios — and the iOS app
# doesn't WANT a dylib anyway, it links the staticlib into the app
# binary at xcodebuild time. Keep cdylib out so the build doesn't waste
# 30 s + fail the link step.
#
# `cargo rustc` (vs `cargo build`) is the stable-toolchain way to pass
# `--crate-type` overrides; `cargo build --crate-type` is nightly-only.
#
# Deployment target MUST match the Xcode project's
# `IPHONEOS_DEPLOYMENT_TARGET` (see `src-tauri/gen/apple/project.yml`).
# Without it, cc-rs / CMake tag every object with the active Xcode SDK
# version (currently iOS 26.x), and xcodebuild then emits one
# "built for newer 'iOS' version (X) than being linked (17.0)" warning
# per object at link time — hundreds of lines of harmless noise that
# drown out real warnings. Keep all three values in sync.
#
# Why three vars:
#   - `IPHONEOS_DEPLOYMENT_TARGET` — honoured by rustc + cc-rs (Rust
#     deps that compile C via the `cc` crate).
#   - `CMAKE_OSX_DEPLOYMENT_TARGET` — honoured by aws-lc-sys, which
#     drives its C build via CMake and ignores the cc-rs env var.
#   - `MACOSX_DEPLOYMENT_TARGET` is irrelevant here (iOS targets only)
#     and deliberately omitted.
export IPHONEOS_DEPLOYMENT_TARGET="17.0"
export CMAKE_OSX_DEPLOYMENT_TARGET="17.0"
cargo rustc -p tfl-ffi --target aarch64-apple-ios     --release --lib --crate-type staticlib
cargo rustc -p tfl-ffi --target aarch64-apple-ios-sim --release --lib --crate-type staticlib

# Resolve cargo's actual target directory. Honours `CARGO_TARGET_DIR`
# (a per-developer setting some of us use to keep build artifacts off
# the repo disk — e.g. `~/.cache/cargo-target/`). Without this, the
# script would silently pick up a STALE `target/aarch64-apple-ios/...`
# from before `CARGO_TARGET_DIR` was set, embedding archives in the
# xcframework that were built with the WRONG deployment target.
TARGET_DIR="$(cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"

# --- Bindgen probe ---------------------------------------------------------
# Build the host library in RELEASE with default features (no panic-probe,
# no uniffi/cli on the lib target) so the metadata snapshot matches the
# iOS release archives byte-for-byte.

echo "==> Building host bindgen probe (release, default features)..."
cargo build -p tfl-ffi --release

echo "==> Building uniffi-bindgen (debug ok — bindgen is a build-host tool)..."
cargo build -p tfl-ffi --bin uniffi-bindgen --features uniffi/cli

# uniffi-bindgen's scan reads the cdylib's metadata section. Use the
# release host dylib so the export set matches the iOS slices.
HOST_DYLIB="$TARGET_DIR/release/libtfl_ffi.dylib"
if [[ ! -f "$HOST_DYLIB" ]]; then
    echo "error: $HOST_DYLIB is missing after release build." >&2
    exit 1
fi

echo "==> Regenerating Swift bindings from release host probe..."
mkdir -p crates/tfl-ffi/generated/swift
"$TARGET_DIR/debug/uniffi-bindgen" generate \
    --library "$HOST_DYLIB" \
    --language swift \
    --out-dir crates/tfl-ffi/generated/swift

BINDINGS="crates/tfl-ffi/generated/swift"
HEADERS_DIR="$(mktemp -d)/Headers"
mkdir -p "$HEADERS_DIR"
# `xcodebuild -create-xcframework` expects `module.modulemap`; uniffi
# emits it as `tfl_ffiFFI.modulemap`. Plain rename — no transformation.
cp "$BINDINGS/tfl_ffiFFI.h"        "$HEADERS_DIR/tfl_ffiFFI.h"
cp "$BINDINGS/tfl_ffiFFI.modulemap" "$HEADERS_DIR/module.modulemap"

# --- Assemble the xcframework ----------------------------------------------

OUT="src-tauri/gen/apple/Externals/TflFFI.xcframework"
rm -rf "$OUT"

echo "==> Creating xcframework at $OUT..."
xcodebuild -create-xcframework \
    -library "$TARGET_DIR/aarch64-apple-ios/release/libtfl_ffi.a"       -headers "$HEADERS_DIR" \
    -library "$TARGET_DIR/aarch64-apple-ios-sim/release/libtfl_ffi.a"   -headers "$HEADERS_DIR" \
    -output "$OUT"

# --- Stage Swift binding for the consuming target --------------------------
#
# NOTE: this is a **build-time copy**, not a hand-edit. The canonical Swift
# binding lives at crates/tfl-ffi/generated/swift/tfl_ffi.swift; this script
# emits it twice (once next to the headers for the spike runner, once into
# the iOS target's compile sources). A pre-build CI guard could fail on
# diff between the two; until that lands, regenerate via this script and
# both copies update in lockstep.

mkdir -p "src-tauri/gen/apple/Tubbie/Generated"
cp "$BINDINGS/tfl_ffi.swift" "src-tauri/gen/apple/Tubbie/Generated/tfl_ffi.swift"

echo "==> Done."
echo "    Static archives merged for: ios-arm64, ios-arm64-simulator"
echo "    Swift binding installed at: src-tauri/gen/apple/Tubbie/Generated/tfl_ffi.swift"
echo "    xcframework at:             $OUT"
