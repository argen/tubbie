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

**Password handling at build time.** The key was generated with a
password (the `-p` flag on `tauri signer generate`). `cargo tauri
build` reads it from the `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` env
var, falling back to an interactive prompt on stdin. In a TTY-less
context (background run, watcher-driven release), the prompt fails
with `os error 6 / Device not configured` and the build aborts at
the updater-artifact signing step.

To unblock unattended builds, the password is stashed in the login
Keychain (item `tubbie-updater`, account `$USER`) and read at
`.envrc`-source time:

```sh
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(security find-generic-password -s tubbie-updater -a "$USER" -w 2>/dev/null)"
```

Bootstrap with `just updater-pwd-store` once per machine — it wraps
`security add-generic-password -U` and prompts for the value
without echoing it. The 1Password copy remains the canonical backup.

**Trade-off this opens.** Any process the dev runs can read the
keychain item without further prompting (`security find-generic-password -w` returns it). If a malicious process executes under the
dev's account, it can sign builds against the real updater key.
Compensating controls:

- The `.key` file itself is `chmod 600` (only readable by the dev's
  user), so the password alone is insufficient — both are needed.
- The Mac is the dev's release machine; no untrusted tenants.
- Alternative (run only in a TTY, type the password every time)
  was tried first and rejected as soon as the background-watcher
  release pattern emerged; typing on every build is fine for a
  manual rare-release rhythm and not fine for a watcher-driven one.

If your threat model doesn't accept the keychain stash, comment out
the `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` line in `.envrc` and run
`just release-local` only in an interactive shell.

### D5. Notarytool authenticates via App Store Connect API key, not Keychain

All release-time `xcrun notarytool` calls authenticate with the App
Store Connect API key via three env vars (`NOTARY_KEY_PATH`,
`NOTARY_KEY_ID`, `NOTARY_ISSUER`) sourced from `.envrc`:

```sh
xcrun notarytool submit /tmp/Tubbie-notarize.zip \
  --key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" \
  --issuer "$NOTARY_ISSUER" --wait
```

The `.p8` private key lives at `~/.appstoreconnect/private_keys/`
with `chmod 600`. Backed up in 1Password — Apple does not let you
re-download.

**Why over the previous Keychain-profile approach** (and over
inline `--apple-id ... --password ...` flags):

- The Keychain approach failed in practice. The first time we needed
  to query a stuck submission the morning after, the
  `tubbie-notary` profile had vanished from the login keychain —
  cause unclear (possibly an unattended GUI prompt during sleep,
  possibly a Keychain Services daemon hiccup). Headless retrieval
  was impossible because the keychain requires GUI confirmation to
  write a new entry, and we couldn't query the old submission to
  find out what happened to it.
- The App-Specific-Password-in-argv concern (the original rationale
  for picking Keychain) doesn't apply to API keys: the `.p8` path in
  argv isn't sensitive, and the actual secret is the file contents,
  never on the command line.
- API keys are Apple's recommended auth method for notarytool since
  2022 and work identically interactively or headless.

**Bootstrap on a fresh Mac:**

1. App Store Connect → Users and Access → Integrations → "+" →
   create an API key with **Developer** access. Download the `.p8`
   (one-time only; Apple never shows it again).
2. Copy `Issuer ID` and `Key ID` from the same page.
3. `mkdir -p ~/.appstoreconnect/private_keys/` and move the `.p8`
   there; `chmod 600`.
4. Populate `NOTARY_KEY_PATH`, `NOTARY_KEY_ID`, `NOTARY_ISSUER` in
   `.envrc` (see `.envrc.example` for the template).
5. `just notary-history` to confirm Apple accepts the key.

**Break-glass:** if the `.p8` leaks, revoke the key in App Store
Connect Integrations and create a new one. Notarytool starts
rejecting submissions signed by the revoked key within minutes —
much faster recovery than updater-key rotation (D4), because the
key isn't baked into shipped binaries.

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

### D7. `macOSPrivateApi: true` retained — confirmed via dry-run

The original `distribution-roadmap.md` warned that `macOSPrivateApi:
true` (used for the undecorated chrome) periodically breaks
notarization when Apple updates its scanner. The Phase 3 dry-run
(submissions `2762ef28-...` on 2026-05-20 and `3fd6c34f-...` on
2026-05-21) **both Accepted** — the first with `macOSPrivateApi:
true`, the second with `false` after we briefly flipped it while
chasing what turned out to be a misdiagnosis (see D9). Apple's
scanner has no problem with the flag in our current binary.

**False alarm story** (worth recording so we don't chase the wrong
gremlin again): for ~24 hours we thought Apple was stalling our
submissions for 87+ minutes / 16+ hours. The submissions were in
fact processing normally — Apple accepted them all within minutes.
Our local `xcrun notarytool submit --wait` poller was dying on
transient Wi-Fi drops (`NSURLErrorDomain -1009`), and we kept
re-querying with a fresh `--wait` that also died, never noticing
the underlying submissions had already resolved. D9 documents the
fix: poll-and-recover wrapper that survives network blinks.

**If a future Apple scanner update does reject `macOSPrivateApi:
true`:** the fallback is mechanical — flip to `false` in
`tauri.conf.json`, remove the `macos-private-api` feature from
`src-tauri/Cargo.toml`'s tauri dependency, drop the
`.transparent(false)` call in `commands.rs::open_settings_window_impl`
(it's a no-op with the feature off), and accept a stock title bar
for that release. The board content is unaffected. Restoring the
undecorated chrome is a v0.X.Y+1 polish item once Tauri / Apple
work it out upstream.

### D8. Auto-update default ON

`UpdatePrefs::default()` returns `auto_check: true`. Opt-OUT, not
opt-in. A live-data app shipping with stale WKWebView CVEs is the
wrong default; the Settings toggle gives users the escape hatch.

### D9. Notarize via poll-and-recover, not `notarytool --wait`

`xcrun notarytool submit --wait` blocks the local process until
Apple returns a verdict. If the local network blinks mid-wait,
`--wait` aborts with `NSURLErrorDomain -1009` and propagates a
non-zero exit. The submission survives on Apple's side, but the
calling pipeline is now broken — and worse, a naïve re-run starts
a brand-new submission instead of re-attaching to the original.

`just notarize-staple` delegates the submit-and-wait step to
`scripts/notarize-submit-and-wait.sh`. The script:

1. Submits without `--wait` (returns immediately with submission id).
2. Polls `notarytool info` every `NOTARY_POLL_INTERVAL_SECS` (default
   30 s). Network errors on the poll fall through as "no status this
   tick" and the loop continues — they do not propagate as exit codes.
3. Exits 0 on `Accepted`, 1 on `Invalid`/`Rejected` (printing the
   notary log), 3 on timeout past `NOTARY_MAX_WAIT_SECS` (default
   30 min).
4. Supports a `query <id>` mode to re-attach to an existing
   submission, e.g. after a laptop sleep blew past the timeout —
   `just notarize-query <id>` is the user-facing entry point.

**Why over a more elaborate retry library** (curl-with-retry,
exponential backoff): the failure mode is binary (network up or
down) and the cost of one extra 30 s tick is trivial. A simple
loop with terminal-status detection is auditable in ~100 lines
of bash, no extra dependencies on the dev's Mac.

**Why over Apple's own `notarytool` retry flags:** as of Xcode
26.1's `notarytool`, there is no `--retry-on-disconnect` option;
`--wait` is a single network session. The wrapper script is the
only place to inject resilience.

This was discovered the hard way during the dry-run — see D7's
"false alarm story" for the misdiagnosis trail.

### D10. `app.restart()` is the caller's responsibility on macOS

`tauri-plugin-updater` 2.10.1 papers over a platform divergence in
its documentation. On Windows (NSIS/MSI) `Update::download_and_install`
hands the installer process the responsibility for killing the parent
PID and launching the new exe; on Linux AppImage the plugin re-execs
the bundle from inside the future itself. **On macOS, neither
happens** — `download_and_install` is download + signature-verify +
atomic-replace of `/Applications/<App>.app`, and then it returns
`Ok(())`. The running process keeps running. The caller has to invoke
`AppHandle::restart()` to `exec` into the new bundle.

We discovered this in the v0.1.0 → v0.1.1 keystone test:
`/Applications/Tubbie.app/Contents/Info.plist` reported
`CFBundleShortVersionString = 0.1.1` (the swap had landed), but the
running v0.1.0 process was still alive, and the Settings UI was
frozen on "Installing — please don't close Tubbie" because
`handleInstall`'s `await installUpdate()` had resolved with the
component pinned to `'installing'`.

`install_update` now does, in order:
1. `update.download_and_install(...)` — staging.
2. `app.emit("updater://restart-imminent", ())` — best-effort
   signal to the renderer so it can paint a transient `'restarting'`
   state before the process dies. Emit failures are ignored.
3. `app.restart()` — returns `-> !`, no code follows.

The renderer arms a 5 s safety timer on install start. If neither
the event nor a process death arrives within the window, it flips
to a new `'restart-failed'` state with copy:

> Update installed, but Tubbie couldn't restart automatically.
> Quit Tubbie and open it again to finish.

This is the recovery path for any future regression in
`app.restart()` (sandbox tightening, NSWorkspace permission churn,
etc.) — the bundle is already in place on disk, so a manual quit-
and-relaunch always completes the upgrade.

**Why not call `app.restart()` from the renderer instead** (via a
separate IPC command): the IPC reply would race the `exec` syscall.
Either the reply lands (renderer transitions normally) or it
doesn't (renderer is left guessing whether the restart was even
attempted). Calling `restart()` inside `install_update` makes the
restart unconditional on staging success and removes the round-trip.

**Why a 5 s window** for the safety timer: long enough for
`download_and_install`'s tail latency + `exec`'s teardown +
Cocoa main-loop drain on the dying process; short enough that a
stuck UI surfaces recovery copy before the user assumes Tubbie
crashed. Tuned in `web/src/lib/components/UpdatesSection.svelte`
as `RESTART_TIMEOUT_MS`; matched in the DOM regression test.

**Why "quit and reopen" rather than auto-retry**: the staged bundle
is byte-identical to what `download_and_install` just wrote.
Re-invoking the install does nothing useful and could re-trigger
the same restart failure. A manual relaunch is one keystroke and
always works.

The v0.1.1 zombie users (whoever did the keystone test before this
ADR landed) are stuck on the broken code path; v0.1.1 → v0.1.2
will require a manual quit-and-reopen after install. v0.1.2 → any
later version restarts cleanly. Release notes call this out.

## Operational details

### Files of record

| Path | Purpose |
|---|---|
| `.envrc` (gitignored, `chmod 600`) | Sources `APPLE_TEAM_ID`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `TAURI_SIGNING_PRIVATE_KEY_PATH`, and the three `NOTARY_*` vars. See `.envrc.example` for the template. |
| `.envrc.example` (committed) | Sanitized template; what a fresh-Mac bootstrap fills in. |
| `~/.tauri/tubbie-updater.key` (`chmod 600`) | Ed25519 private key (updater signature). Backed up in 1Password. |
| Login Keychain item `tubbie-updater` | Password for the updater key above. Read by `.envrc` at source time so background builds don't hit the interactive prompt. Bootstrap via `just updater-pwd-store`. Backed up in 1Password. |
| `~/.appstoreconnect/private_keys/AuthKey_*.p8` (`chmod 600`) | App Store Connect API key for notarytool. Backed up in 1Password (Apple does not re-issue). |
| Login Keychain | Developer ID Application certificate (CN `Developer ID Application: BRUNO BELCASTRO PINTO (5FD9DWK258)`). Backed up as `.p12` in 1Password. |
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

## Rollout status

As of 2026-05-22 the M8 pipeline is **public**. v0.1.0, v0.1.1, and
v0.1.2 shipped; v1.0.0 is the first stable release.

| Step | Status |
|---|---|
| PR-A — `tauri-plugin-updater` registered | merged to main |
| PR-B — signing identity, entitlements, real pubkey in `tauri.conf.json` | merged to main |
| PR-C — local Justfile release pipeline + pre-push hook + scripts | merged to main |
| PR-D — updater IPC commands + frontend wrappers | merged to main |
| PR-E — Settings "Updates" section (nine UI states after D10) | merged to main |
| PR-F — this ADR + README + CLAUDE.md note + PR-template checkbox | merged to main |
| Phase 3 — notarization dry-run | **✓ Accepted + stapled.** Submission `3fd6c34f-2727-485c-98f7-a84dece1ec8b` on 2026-05-21. `spctl --assess` returns `accepted source=Notarized Developer ID`. |
| Phase 7 — cut v0.1.0 + fresh-account install smoke + v0.1.1 no-op auto-update smoke | **✓ v0.1.0 and v0.1.1 published.** Keystone test surfaced the D10 restart bug — bundle staged on disk but process never restarted. |
| Phase 8 — flip repo public + apply branch protection | **✓ argen/tubbie public.** Branch protection: PR-review required, no force-push, no deletions, linear history. |
| D10 hotfix — `app.restart()` after `download_and_install` + 5 s safety-timeout | **✓ v0.1.2** (this PR). v0.1.1 → v0.1.2 still needs a manual quit-and-reopen; later updates restart cleanly. |
| v1.0.0 — first stable release (zero-config onboarding, network-wide status, station search from board, dot-matrix tray icon) | **✓ shipped.** |

**Known carry-over:** anyone who installed v0.1.0 or v0.1.1 and ran
the in-app updater on the broken code path needs to quit Tubbie
(`Cmd+Q` / Activity Monitor) and relaunch — `/Applications/Tubbie.app`
will already be on the newer version. Release notes for v0.1.2 lead
with this.

If a notarization submission ever appears stuck again, do **not**
start a new build. Instead `just notarize-query <id>` against the
original submission id — it almost certainly resolved on Apple's
side while the local `--wait` was dying on a network blink (see D9).
