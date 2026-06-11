#!/usr/bin/env bash
# B.1 Swift smoke runner.
#
# Compiles the host cdylib, generates Swift bindings, and runs the
# `FFISpikeTests.swift` smoke against the live dylib. Exits non-zero on the
# first failed assertion.
#
# Run from anywhere — the script `cd`s to the workspace root.
set -euo pipefail

cd "$(dirname "$0")/../../.."

CRATE_DIR="crates/tfl-ffi"
BINDINGS_DIR="$CRATE_DIR/generated/swift"
SPIKE_DIR="$CRATE_DIR/swift-spike"
DYLIB="target/debug/libtfl_ffi.dylib"

echo "==> Building tfl-ffi cdylib (debug, with panic-probe for the smoke)…"
# `panic-probe` enables the panic-safety probe export. `uniffi/cli` is needed
# only by the bindgen bin and is enabled in the next step.
cargo build -p tfl-ffi --features panic-probe

echo "==> Building uniffi-bindgen (debug)…"
cargo build -p tfl-ffi --bin uniffi-bindgen --features uniffi/cli

# Cargo emits the cdylib at both `target/debug/libtfl_ffi.dylib` (for `-L`)
# and `target/debug/deps/libtfl_ffi.dylib` (the install_name target). When
# the workspace also contains test artifacts, the deps copy can lag behind
# the top-level one (different feature sets). Force them in lock-step so
# dyld resolves the same dylib both link-time and run-time.
cp -f target/debug/libtfl_ffi.dylib target/debug/deps/libtfl_ffi.dylib

echo "==> Regenerating Swift bindings…"
mkdir -p "$BINDINGS_DIR"
target/debug/uniffi-bindgen generate \
    --library "$DYLIB" \
    --language swift \
    --out-dir "$BINDINGS_DIR"

echo "==> Compiling + running FFISpikeTests.swift…"
# `-import-objc-header` lets swiftc pick up the C header without a SwiftPM module map dance.
# `-l tfl_ffi` resolves the dylib via `-L target/debug`.
# `-Xlinker -rpath` makes the resulting binary findable at runtime without
# DYLD_LIBRARY_PATH cooperation.
TMPDIR_RUN="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_RUN"' EXIT

swiftc \
    -parse-as-library \
    -import-objc-header "$BINDINGS_DIR/tfl_ffiFFI.h" \
    -L target/debug \
    -l tfl_ffi \
    -Xlinker -rpath -Xlinker "$(pwd)/target/debug" \
    -o "$TMPDIR_RUN/spike-runner" \
    "$BINDINGS_DIR/tfl_ffi.swift" \
    "$SPIKE_DIR/FFISpikeTests.swift"

CARGO_MANIFEST_DIR="$(pwd)/$CRATE_DIR" "$TMPDIR_RUN/spike-runner"
