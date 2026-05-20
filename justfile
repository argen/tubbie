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

# Nuke all regenerable build output. Run when disk is tight — next build is cold.
# Reclaims: target/ (Rust), web/node_modules/.vite + web/build + web/.svelte-kit (web).
# Does NOT delete node_modules itself (that's a slower reinstall) or .git.
clean-deep:
    cargo clean
    rm -rf web/build web/.svelte-kit web/node_modules/.vite web/node_modules/.cache

# Build the release .app bundle (macOS).
# Prerequisites: Node 24, Rust stable, cargo-tauri v2 (cargo install tauri-cli@^2),
#                Xcode Command Line Tools (xcode-select --install).
# Produces an UNSIGNED bundle at target/release/bundle/macos/Tubbie.app.
# Sign + notarize steps are wired below — see `just release-local` /
# `just release` and the M8 ADR.
build:
    . "$HOME/.cargo/env" && cargo tauri build

# ---------------------------------------------------------------------------
# Public-distribution / signed-release pipeline (M8). Runs entirely on the
# local Mac — no GitHub Actions involvement, to respect the Free-plan
# minute budget. See `docs/ADR/distribution-roadmap.md` and the M8 ADR.
# ---------------------------------------------------------------------------

# Install the pre-push git hook. Symlink so updates to
# `scripts/git-hooks/pre-push` apply without reinstalling.
install-hooks:
    ln -sfn ../../scripts/git-hooks/pre-push .git/hooks/pre-push
    @echo "[just] pre-push hook installed."

# Bump version in tauri.conf.json + src-tauri/Cargo.toml in lockstep.
bump version:
    scripts/bump-version.sh {{version}}

# One-off Apple Developer setup: store notarytool credentials in the
# login keychain under the `tubbie-notary` profile. Requires APPLE_ID,
# APPLE_TEAM_ID, APPLE_APP_SPECIFIC_PASSWORD in env (don't echo).
notary-store-creds:
    xcrun notarytool store-credentials tubbie-notary \
      --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
      --password "$APPLE_APP_SPECIFIC_PASSWORD"

# Codesign + notarize + staple the just-built `.app`. Idempotent;
# reads APPLE_SIGNING_IDENTITY from env, notarytool creds from the
# `tubbie-notary` keychain profile.
notarize-staple:
    codesign --deep --force --options runtime --timestamp \
      --sign "$APPLE_SIGNING_IDENTITY" \
      --entitlements src-tauri/entitlements.plist \
      target/aarch64-apple-darwin/release/bundle/macos/Tubbie.app
    ditto -c -k --keepParent \
      target/aarch64-apple-darwin/release/bundle/macos/Tubbie.app \
      /tmp/Tubbie-notarize.zip
    xcrun notarytool submit /tmp/Tubbie-notarize.zip \
      --keychain-profile tubbie-notary --wait
    rm -f /tmp/Tubbie-notarize.zip
    xcrun stapler staple \
      target/aarch64-apple-darwin/release/bundle/macos/Tubbie.app
    xcrun stapler validate \
      target/aarch64-apple-darwin/release/bundle/macos/Tubbie.app

# Verify the signed bundle: deep codesign verify + Gatekeeper assess.
sign-verify:
    codesign --verify --deep --strict --verbose=2 \
      target/aarch64-apple-darwin/release/bundle/macos/Tubbie.app
    spctl --assess --type execute --verbose=4 \
      target/aarch64-apple-darwin/release/bundle/macos/Tubbie.app

# Write the Tauri v2 updater manifest from the just-built artifacts.
gen-update-manifest tag:
    scripts/build-latest-json.sh {{tag}} \
      > target/aarch64-apple-darwin/release/bundle/macos/latest.json
    @echo "[just] wrote target/aarch64-apple-darwin/release/bundle/macos/latest.json"

# Local signed dress-rehearsal: build, sign, notarize, staple, verify,
# manifest. No tagging, no GitHub upload. Use this before `just release`.
release-local tag:
    cargo tauri build --target aarch64-apple-darwin
    just notarize-staple
    just sign-verify
    just gen-update-manifest {{tag}}

# Full signed release: preflight, build, sign, notarize, staple,
# manifest, tag, push, draft GitHub Release. Run from main, clean tree.
# Promote draft -> published with `gh release edit {{tag}} --draft=false`
# after manual smoke from a fresh macOS user account.
release tag:
    scripts/preflight.sh {{tag}}
    just release-local {{tag}}
    git tag -a {{tag}} -m "{{tag}} signed release"
    git push origin {{tag}}
    gh release create {{tag}} \
      --title "Tubbie {{tag}}" \
      --notes-file CHANGELOG-{{tag}}.md \
      --draft \
      target/aarch64-apple-darwin/release/bundle/macos/*.dmg \
      target/aarch64-apple-darwin/release/bundle/macos/*.app.tar.gz \
      target/aarch64-apple-darwin/release/bundle/macos/*.app.tar.gz.sig \
      target/aarch64-apple-darwin/release/bundle/macos/latest.json
    @echo ""
    @echo "[just] draft release at https://github.com/argen/tubbie/releases/tag/{{tag}}"
    @echo "[just] review the draft, then:"
    @echo "       gh release edit {{tag}} --draft=false"
