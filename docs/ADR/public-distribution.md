# Public distribution — Developer ID, local-only pipeline

**Status:** Accepted (2026-05-20)

**Supersedes the M8 section of:** [`distribution-roadmap.md`](./distribution-roadmap.md)

## Context

Tubbie now ships publicly to macOS via signed, notarized `.app`/`.dmg`
artifacts on GitHub Releases, plus a `tauri-plugin-updater` channel for
in-app updates. This ADR records the choices made when M8 was executed
and the operational details a future maintainer (or future-self) will
need to keep the pipeline running.

The proposal in `distribution-roadmap.md` covered the architectural
shape: Developer ID + notarization + auto-update. This ADR records what
actually shipped, where it differs from that plan, and why.

## Decisions

### D1. Developer ID Application, not Mac App Store

Direct download from GitHub Releases. The Mac App Store path would
require sandbox + extra entitlements + an App Store Connect listing
for ~2× the work and longer review cycles. MAS can layer on later
once the Developer ID pipeline is proven; the two share the signing
infrastructure but diverge at the entitlements file.

### D2. arm64 only for v0.1.0

Intel Mac share is rounding error for a niche TfL utility. The x86_64
leg would double signing surface and failure modes for negligible
reach. The Tauri config + signing scripts are identical for x86_64; if
a user asks, we add a second `--target x86_64-apple-darwin` build to
`just release-local` and a `darwin-x86_64` entry to `latest.json`.

### D3. Local-only release pipeline, no GitHub Actions

The release pipeline (build → sign → notarize → staple → manifest →
tag → push → draft Release) runs from a Justfile recipe on the dev's
Mac. GitHub Actions are NOT used for releases.

**Why:** The repository is on a GitHub Free plan with a constrained
Actions minute budget. A signed Tauri build on `macos-14` consumes
~15–25 minutes per release; iterating to a clean pipeline would burn
days of budget. The dev's Mac already has the signing cert in its
login Keychain and the `notarytool` profile stored — replicating that
in CI requires base64-encoding the `.p12`, storing it as a secret,
unlocking an ephemeral keychain on every run, and cleanup-on-failure.
Local is simpler and matches the one-machine, one-developer reality.

**Trade-offs:**

- A teammate can't cut a release without setting up the same
  Keychain + notary profile + `.envrc`. Acceptable while solo.
- No audit trail in CI logs. The git history (`v*` tags) is the audit
  trail; `gh release view` shows what was actually published.
- The dev's Mac becomes a single point of failure for releases. The
  Tauri updater key is backed up in 1Password; the Developer ID
  cert is exportable as `.p12` for backup. Recovery is one Xcode
  re-import + one `just notary-store-creds` re-run.

**If the budget situation changes:** lifting `just release` into a
`release.yml` workflow is a tractable follow-up. The Justfile recipes
become the local dev path; CI calls them.

### D4. Updater key custody — offline, 1Password-backed

The Ed25519 keypair was generated via `cargo tauri signer generate
-w ~/.tauri/tubbie-updater.key` on the dev's local Mac. The private
key file is `chmod 600` at that path. **Both the `.key` and its
password are backed up in 1Password.** Neither has ever touched a
cloud secret store.

**Why:** Tauri's updater embeds the public key into every shipped
binary at build time. If the private key leaks, every shipped binary
trusts only that key — there is no in-band key rotation. Recovery
requires every user to download a new, manual-install release signed
with the new key. Keeping the private key off any cloud surface
(GitHub Secrets, etc.) reduces the leak surface to: 1Password
account compromise, or the dev's Mac being stolen with an unlocked
disk. Both are out-of-scope threats for this product.

**Break-glass rotation procedure** (if the private key is ever
suspected leaked):

1. Generate a new keypair offline: `cargo tauri signer generate -w
   ~/.tauri/tubbie-updater-vN.key`. Back up to 1Password
   immediately; `chmod 600`.
2. Update `tauri.conf.json:plugins.updater.pubkey` to the new pubkey.
3. Cut a **non-updater-eligible release** (`createUpdaterArtifacts:
   false` for that one build). Tag e.g. `v0.X.Y-keyroll`. Note in
   the release body that users on existing binaries must manually
   download because their installed binary still trusts the old
   pubkey.
4. Future releases sign with the new key and ship via the normal
   updater channel; users who manually re-installed the keyroll
   build pick those up.
5. Compromise the old key (delete from disk, mark in 1Password as
   revoked, never reuse).

There is no faster recovery. The pubkey-baked-into-binary contract is
the design's strength against MITM and the design's weakness against
key compromise.

### D5. Notarytool credentials in the login Keychain, not env

`xcrun notarytool store-credentials tubbie-notary --apple-id ... --team-id ... --password ...` stores the
App-Specific Password as a keychain item under profile `tubbie-notary`.
All release-time notarytool calls use `--keychain-profile tubbie-notary`.

**Why over the alternative** (`--apple-id ... --password ...` flags):
notarytool's flag form echoes the App-Specific Password into any
error stack trace and into `ps`-visible argv. Keychain-profile form
keeps it sealed.

The App-Specific Password is also backed up in 1Password. If Apple
expires it on next-password-change, regeneration is one trip to
appleid.apple.com → Sign-In & Security → App-Specific Passwords, then
re-run `just notary-store-creds`.

### D6. Two-key entitlements file (JIT only)

`src-tauri/entitlements.plist` declares only:

```xml
<key>com.apple.security.cs.allow-jit</key><true/>
<key>com.apple.security.cs.allow-unsigned-executable-memory</key><true/>
```

Both are required by WKWebView's JavaScriptCore JIT under hardened
runtime — without them the webview crashes on first paint. All other
hardening keys (`disable-library-validation`, `debugger`,
`allow-dyld-environment-variables`, etc.) are Apple's defaults;
explicit `false` entries would restate the defaults and rot when
those defaults shift.

**Keys deliberately omitted:**

- `com.apple.security.network.client` — sandbox-only. Under Developer
  ID, outbound HTTPS is allowed by default; the CSP in
  `tauri.conf.json:app.security.csp` is the real restriction surface.
- `com.apple.security.personal-information.location` — sandbox-only.
  Under Developer ID, CoreLocation works via the existing
  `NSLocationWhenInUseUsageDescription` string in `Info.plist`.
- `com.apple.security.app-sandbox` — Developer ID direct does not
  use App Sandbox. (If we ever add a MAS variant, this entitlement
  file gets a sandboxed sibling.)

### D7. `macOSPrivateApi: true` retained, with a tested fallback

The original `distribution-roadmap.md` warned that `macOSPrivateApi:
true` (used for the undecorated chrome) periodically breaks
notarization when Apple updates its scanner. The Phase 3 local dry-run
(`just release-local v0.1.0-dryrun`) confirmed the current Tauri
2.11.1 + macOS notary combination accepts it.

**If a future Apple scanner update rejects:** flip
`macOSPrivateApi: false` in `tauri.conf.json` and accept a stock
title bar for that release. The undecorated chrome is cosmetic — the
dot-matrix board content is unaffected. A re-cut with this flip
unblocks the release within minutes; restoring the undecorated chrome
is a v0.X.Y+1 polish item once Tauri / Apple work it out upstream.

### D8. Auto-update default ON

`UpdatePrefs::default()` returns `auto_check: true`. Opt-OUT, not
opt-in. A live-data app shipping with stale WKWebView CVEs is the
wrong default; the Settings toggle gives users the escape hatch.

## Operational details

### Files of record

| Path | Purpose |
|---|---|
| `.envrc` (gitignored, `chmod 600`) | Sources `APPLE_TEAM_ID=5FD9DWK258`, `APPLE_SIGNING_IDENTITY="Developer ID Application: BRUNO BELCASTRO PINTO (5FD9DWK258)"`, `APPLE_ID="bbelcastro@gmail.com"`, `TAURI_SIGNING_PRIVATE_KEY_PATH=$HOME/.tauri/tubbie-updater.key` |
| `~/.tauri/tubbie-updater.key` (`chmod 600`) | Ed25519 private key. Backed up in 1Password. |
| Login Keychain | Developer ID Application certificate (CN `Developer ID Application: BRUNO BELCASTRO PINTO (5FD9DWK258)`). Backed up as `.p12` in 1Password. |
| Login Keychain profile `tubbie-notary` | App-Specific Password for `xcrun notarytool`. Re-creatable via `just notary-store-creds`. |
| `src-tauri/entitlements.plist` | Two-key hardened-runtime entitlements (JIT). |
| `src-tauri/capabilities/updater.json` | `updater:default` scoped to Settings window only. |
| `src-tauri/tauri.conf.json` | `bundle.macOS.signingIdentity` / `providerShortName` / `entitlements`; `plugins.updater.pubkey` + endpoint. |

### Cutting a release

```sh
source .envrc                  # if direnv isn't installed
just bump 0.1.1                # edits tauri.conf.json + Cargo.toml in lockstep
# commit + merge the version bump on a normal PR
just release v0.1.1            # preflight + build + sign + notarize + staple + manifest + tag + push + draft
# review the draft on GitHub, then:
gh release edit v0.1.1 --draft=false
```

The `preflight.sh` script (called by `just release`) refuses to start
if: the tree is dirty, current branch isn't `main`, local main
diverges from origin, the tag already exists, signing env vars are
unset, the cert isn't in the Keychain, or the `tubbie-notary` profile
is missing.

### Verification

After `just release` publishes the draft, smoke-test from a fresh
macOS user account (clean keychain, same hardware):

1. Download the `.dmg` from the draft Release page.
2. Mount; drag Tubbie to `/Applications`; launch.
3. **Must not** show a Gatekeeper "damaged or malicious" warning.
4. `spctl --assess --type execute --verbose=4 /Applications/Tubbie.app`
   must return `accepted source=Notarized Developer ID`.
5. Open Settings → Updates → "Check for updates" → "You're up to
   date".

Once a follow-up `vN.N+1` is tagged, repeat from the installed
`vN.N` and confirm the in-app updater installs cleanly without
Gatekeeper interaction.

## Consequences

- Public users get a one-click install + silent in-app updates.
- The README no longer documents `xattr -dr com.apple.quarantine` —
  training users to bypass Gatekeeper trains them to bypass safety.
- The dev does need a local working Mac at release time; this is
  acceptable while solo. CI lift is a future option once the budget
  situation changes.
- Updater key compromise has a manual recovery path (see D4) but no
  automatic one. Compensating control: the key never touches a cloud
  secret store.
