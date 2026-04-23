# Top-level justfile for tubbie
# Just verify is the single entry point for the full gate

# Run both gates (Rust + web)
verify: verify-rust verify-web

# Rust gate
verify-rust: fmt clippy test

# Web gate
verify-web:
    cd web && npm run verify

# Development server: runs Tauri dev (which starts Vite internally via devUrl).
# Tauri v2 reads devUrl from tauri.conf.json and expects Vite to be running
# at http://localhost:5173. This recipe starts both concurrently.
#
# Requirements: `npm install` inside web/ and `cargo install tauri-cli@2`
# (or use `cargo tauri` from the tauri-cli crate).
dev:
    cd web && npm run dev & cargo tauri dev --config src-tauri/tauri.conf.json

# Tauri dev only (when Vite is already running separately)
tauri-dev:
    cargo tauri dev --config src-tauri/tauri.conf.json

# Format all code
fmt:
    cargo fmt --all

# Run clippy with -D warnings
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests
test:
    cargo test --workspace

# Check formatting only (used in CI)
fmt-check:
    cargo fmt --all -- --check

# Run live integration tests against api.tfl.gov.uk (developer-only; not in CI)
# Set TFL_APP_KEY in your environment to avoid anonymous rate-limits.
verify-live:
    cargo test --workspace --features tfl-client/live

# Record TfL API fixtures to fixtures/ (hits live API — run once per milestone)
record-fixtures:
    cargo run -p fixture-recorder --release
