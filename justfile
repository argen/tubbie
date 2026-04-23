# Top-level justfile for tubbie
# Just verify is the single entry point for the full gate

# Run both gates (Rust + web)
verify: verify-rust verify-web

# Rust gate
verify-rust: fmt clippy test

# Web gate
verify-web:
    cd web && npm run verify

# Development server (wired in M5/M6)
dev:
    @echo "dev server not yet wired — lands in M5/M6"

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
