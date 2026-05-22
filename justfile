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

# Resolve the bundle output dir, honouring CARGO_TARGET_DIR if set
# (some setups put target/ on a faster disk or a shared cache like
# `~/.cache/cargo-target`). Falls back to the in-repo `target/`.
target_dir := env_var_or_default("CARGO_TARGET_DIR", "target")
macos_bundle := target_dir / "aarch64-apple-darwin/release/bundle/macos"
dmg_bundle := target_dir / "aarch64-apple-darwin/release/bundle/dmg"

# Install the pre-push git hook. Symlink so updates to
# `scripts/git-hooks/pre-push` apply without reinstalling.
install-hooks:
    ln -sfn ../../scripts/git-hooks/pre-push .git/hooks/pre-push
    @echo "[just] pre-push hook installed."

# Bump version in tauri.conf.json + src-tauri/Cargo.toml in lockstep.
bump version:
    scripts/bump-version.sh {{version}}

# Query notarization submission history. Confirms the API-key env
# vars are wired up and that Apple accepts them. Quick smoke before
# `release-local` / `release`.
notary-history:
    xcrun notarytool history \
      --key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" \
      --issuer "$NOTARY_ISSUER"

# One-off: stash the Tauri updater-key password in the login Keychain
# so `cargo tauri build` can sign updater artifacts unattended. Prompts
# interactively for the password — the value is never echoed. Idempotent
# via -U (update if exists). See ADR D4 for the trade-off.
updater-pwd-store:
    @echo "Enter the Tauri updater-key password (will not echo)."
    @echo "This is the password set when you ran \`cargo tauri signer generate -p ...\`."
    @security add-generic-password -U -s "tubbie-updater" -a "$USER" -w
    @echo "[just] stored. source .envrc to pick it up."

# Codesign + notarize + staple the just-built `.app`. Idempotent;
# reads APPLE_SIGNING_IDENTITY for codesign, and the three NOTARY_*
# env vars (App Store Connect API key) for notarytool. See ADR
# `docs/ADR/public-distribution.md` D5 for the auth choice.
#
# Notarization uses scripts/notarize-submit-and-wait.sh which polls
# resiliently — a local network blink mid-submission no longer aborts
# the pipeline (the submission survives on Apple's side and the
# script re-queries on next tick). See D9 for the rationale.
notarize-staple:
    codesign --deep --force --options runtime --timestamp \
      --sign "$APPLE_SIGNING_IDENTITY" \
      --entitlements src-tauri/entitlements.plist \
      "{{macos_bundle}}/Tubbie.app"
    ditto -c -k --keepParent \
      "{{macos_bundle}}/Tubbie.app" \
      /tmp/Tubbie-notarize.zip
    scripts/notarize-submit-and-wait.sh /tmp/Tubbie-notarize.zip
    rm -f /tmp/Tubbie-notarize.zip
    xcrun stapler staple "{{macos_bundle}}/Tubbie.app"
    xcrun stapler validate "{{macos_bundle}}/Tubbie.app"

# Re-attach to a notarization submission that timed out locally (e.g.
# laptop slept past NOTARY_MAX_WAIT_SECS). Use the submission id printed
# by `notarize-staple` / `notarize-submit-and-wait.sh`. On Accepted the
# script exits 0; you can then `xcrun stapler staple <app>` manually.
notarize-query id:
    scripts/notarize-submit-and-wait.sh query {{id}}

# Verify the signed bundle: deep codesign verify + Gatekeeper assess.
sign-verify:
    codesign --verify --deep --strict --verbose=2 "{{macos_bundle}}/Tubbie.app"
    spctl --assess --type execute --verbose=4 "{{macos_bundle}}/Tubbie.app"

# Write the Tauri v2 updater manifest from the just-built artifacts.
gen-update-manifest tag:
    scripts/build-latest-json.sh {{tag}} > "{{macos_bundle}}/latest.json"
    @echo "[just] wrote {{macos_bundle}}/latest.json"

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
      "{{dmg_bundle}}"/*.dmg \
      "{{macos_bundle}}"/*.app.tar.gz \
      "{{macos_bundle}}"/*.app.tar.gz.sig \
      "{{macos_bundle}}/latest.json"
    @echo ""
    @echo "[just] draft release at https://github.com/argen/tubbie/releases/tag/{{tag}}"
    @echo "[just] review the draft, then:"
    @echo "       gh release edit {{tag}} --draft=false"
