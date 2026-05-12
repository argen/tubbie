# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for suspected security
vulnerabilities. Use one of the channels below; we will acknowledge
within **5 business days** and aim for a fix or mitigation plan within
**30 days** for high-severity reports.

1. **Preferred** — GitHub's [private vulnerability reporting](https://github.com/argen/tubbie/security/advisories/new).
2. **Fallback** — email `iam[at]brunobelcastro.com` with `[tubbie security]`
   in the subject.

If you would like credit in release notes for the fix, please say so
in your initial report and provide the name/handle you'd like used.

## Supported versions

Only the `main` branch and the most recent tagged release receive
security fixes. Older releases are not patched.

## Threat model (informal)

tubbie is a local-first Tauri 2 desktop app showing live TfL arrivals.
There is no server, no auth, no user-controlled HTML or cookies. The
only network egress is to the public TfL Unified API (read-only). The
shipped frontend is a SvelteKit static-adapter build (no SSR).

### Credential storage

The TfL API key is stored in the **macOS Keychain** (system credential
storage) via `security-framework`'s `set_generic_password` /
`get_generic_password`. Service identifier: `app.tubbie`. It is never
written to `config.json` or any other file on disk. The on-disk store
(`~/Library/Application Support/app.tubbie/config.json`) contains only
non-sensitive config: current station, line filters, direction filters,
display mode, and poll interval.

### `TFL_APP_KEY` environment-variable fallback (INFO-1)

`ReqwestTflHttp::new()` reads `TFL_APP_KEY` from the process environment
at construction time if no key is supplied via the Keychain. This is a
developer-convenience path — running `TFL_APP_KEY=xxx cargo test` or
`TFL_APP_KEY=xxx cargo tauri dev` skips the Keychain prompt during local
development. The same variable is used by CI as a repository secret
(`Settings → Secrets and variables → Actions → TFL_APP_KEY`) for the
live-test gate (`.github/workflows/live-tfl-gate.yml`) and the monthly
fixture-freshness workflow (`.github/workflows/fixture-freshness.yml`).

Environment variables share the trust level of other files in the user's
home directory: any process running as the same user can read `/proc/…/environ`
(Linux) or inspect `ps -E` (macOS with SIP disabled). The key is wrapped in
the `AppKey` type (zeroized on drop, `Debug` → `<redacted>`) immediately after
reading, so it does not persist in Rust memory beyond its use. In CI the
variable is injected by GitHub Actions' secret mechanism and never echoed.
No action needed for the current threat model.

The non-trivial attack surfaces we track are:

1. **TfL response parsing** — a hostile or compromised upstream
   response could reach `tfl-client` / `tfl-board` parsers.
2. **Supply chain at build time** — proc-macro / build-script
   compromise on a contributor's or CI host (e.g. `xz`-class incidents).
3. **Forks running `npm run dev`** — once open source, contributors run
   the SvelteKit dev server, which exercises code paths the shipped
   static bundle does not.
4. **API key in TfL request URLs** — The TfL Unified API requires the
   `app_key` credential to be sent as a URL query parameter
   (`?app_key=…`) rather than an HTTP request header; this is a TfL API
   design constraint, not a Tubbie choice. As a result, the key is
   likely to appear in TfL's server-side access logs and almost
   certainly in any intermediate proxy logs
   (e.g. a corporate HTTPS-intercepting proxy or a local debugging tool
   such as Charles or Proxyman). Tubbie already redacts the request URL
   from all error message paths and wraps the key in a zero-on-drop
   `AppKey` type, but these controls do not affect TfL's own logging.
   Treat `app_key` as a rotatable credential rather than a long-lived
   secret: if you believe the key has been exposed through a log breach
   or proxy capture, regenerate it via the TfL developer portal
   (https://api-portal.tfl.gov.uk) and update it in Tubbie's Settings.

## Accepted-risk register

The following advisories are surfaced by Dependabot but **not** patched
in tree, with rationale. Each entry has an explicit re-triage date so
the deferral does not silently expire.

| Advisory | Package | Where | Why deferred | Re-triage |
|----------|---------|-------|--------------|-----------|
| [GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g) | `glib < 0.20` | `gtk 0.18 → tauri/wry` (Linux runtime) | tubbie does not call `VariantStrIter`; macOS/Windows unaffected at runtime. glib 0.20 needs gtk 0.20+, which Tauri 2.10 does not yet pin. | Next Tauri minor, or **2026-07-25**, whichever first. |
| [GHSA-cq8v-f236-94qc](https://github.com/advisories/GHSA-cq8v-f236-94qc) | `rand 0.7.3` | `phf_generator → … → tauri-utils` (build-time only) | This advisory's exploit precondition (a custom `log::Log` impl that hijacks `rand::rng()` during code generation) does not exist in our build env. | **2026-07-25**, or sooner if advisory is upgraded. |

These are also listed under `ignore:` in `.github/dependabot.yml` so
version-update PRs don't churn while we wait.

### `macOSPrivateApi` — transparent window and traffic-light chrome (LOW-3)

`tauri.conf.json` sets `app.security.macOSPrivateApi = true`. This flag
enables Tauri features that depend on undocumented `NSWindow` and
`NSApplication` APIs reached via `objc2::msg_send!`:

- **Transparent window** — the decorations-free floating board uses
  `NSWindowStyleMaskFullSizeContentView` and hides the traffic-light
  buttons to fill the window with the dot-matrix canvas.
- **Menubar / tray mode** — switching between floating-window and menubar
  modes calls `set_activation_policy` and conditionally removes the status
  bar item, both Cocoa main-thread operations.

**Risk surface:** These are unsupported Apple APIs. A macOS update can
silently change their semantics, causing the window to render incorrectly
or the mode switch to crash (`EXC_BREAKPOINT` if called off the main
thread). The thread-dispatch guards in `apply_display_mode_effects` /
`strip_native_chrome` mitigate the main-thread constraint, but cannot
guard against API removal. If Tubbie is ever submitted to the Mac App
Store, the private-API usage will likely trigger rejection. There is no
alternative implementation without rewriting to a fully native macOS
window layer.

**Re-triage condition:** Remove or replace when Tauri exposes a stable
public API for decorations-free transparent windows and activation-policy
control, or when App Store distribution becomes a requirement.

### Escape hatch — `[patch.crates-io]` for `phf_generator`

If GHSA-cq8v-f236-94qc is upgraded or a custom log implementation lands
in our build chain, we can force `phf_generator` (and therefore `rand`)
to a current version via a workspace-level `[patch.crates-io]`:

```toml
# Cargo.toml (workspace root)
[patch.crates-io]
phf_generator = { git = "https://github.com/rust-phf/rust-phf", branch = "master" }
```

This is documented but **not** applied today — applying it pulls a deep
transitive on `master` which has its own risks.

### Notarisation status — unsigned macOS release distribution (LOW-4)

Released `.app` and `.dmg` bundles are **not currently notarised**. Users
who download from GitHub Releases must right-click → Open (or run
`xattr -dr com.apple.quarantine Tubbie.app`) to bypass the Gatekeeper
quarantine warning. This is documented in the README under **Install**.

**Implication:** macOS cannot verify the code-signing chain of the
downloaded binary. A modified binary distributed from an unofficial mirror
or fork is indistinguishable from the legitimate build by the OS or the
user. Users who build from source are unaffected.

**Path to fix:** `tauri-apps/tauri-action` supports codesigning and
notarisation given an Apple Developer ID Application certificate plus an
app-specific password stored as Actions secrets. The concrete M8 steps
(certificate secrets, `xcrun notarytool submit`, stapling) are documented
in [`docs/ADR/distribution-roadmap.md`](docs/ADR/distribution-roadmap.md).

**Recommendation:** Defer until first wider public distribution. Track as
item M8 on the distribution roadmap; revisit when flipping the repo to
public or when the first non-developer user installs the app.

## Repo settings checklist (manual, post-OSS flip)

These are GitHub repo-settings toggles, not file-tracked. The repo
maintainer must enable them when flipping the repo to public (most
require either public visibility or a Pro / GitHub Advanced Security
plan on private repos):

- [ ] **Settings → Code security → Private vulnerability reporting** — on.
- [ ] **Settings → Code security → Secret scanning + push protection** — on.
- [ ] **Settings → Code security → Dependabot security updates** — on.
- [ ] **Settings → Code security → Dependabot version updates** — on
      (config is already in `.github/dependabot.yml`).
- [ ] **Branch protection on `main`** (see below).

### Branch protection on `main`

When ready, apply via:

```bash
gh api -X PUT /repos/argen/tubbie/branches/main/protection \
  -f required_status_checks.strict=true \
  -f 'required_status_checks.contexts[]=web' \
  -f 'required_status_checks.contexts[]=rust' \
  -f 'required_status_checks.contexts[]=cargo-deny' \
  -f 'required_status_checks.contexts[]=osv-scan' \
  -F enforce_admins=false \
  -F required_pull_request_reviews.required_approving_review_count=1 \
  -F required_pull_request_reviews.dismiss_stale_reviews=true \
  -F restrictions= \
  -F allow_force_pushes=false \
  -F allow_deletions=false
```

`enforce_admins=false` keeps an admin bypass while the project is
solo-maintained; flip to `true` once there's a second maintainer.

#### Why `Live TfL gate` and `Fixture freshness` are NOT in the required-checks list

Both Phase 4 workflows (`live-tfl-gate.yml` and `fixture-freshness.yml`)
register status checks but neither belongs in `required_status_checks`:

- **`Live TfL gate / live-tested tag present`** is path-conditional —
  it only runs on PRs touching `crates/tfl-*/**`, `crates/fixture-recorder/**`,
  or `Cargo.lock`. Adding it to required checks would block every
  unrelated PR ("expected status check not found"). The discipline it
  enforces is for reviewers to read the PR description and confirm
  `[live-tested]` is present — equivalent to a manual review gate.

- **`Fixture freshness`** is `schedule: cron` + `workflow_dispatch` only;
  it never runs on `pull_request`, so it can never produce a status
  for a PR. Making it required is a category error.

## CI secrets

| Secret | Used by | Purpose |
|--------|---------|---------|
| `TFL_APP_KEY` | `.github/workflows/fixture-freshness.yml` | Authenticates the monthly `just record-fixtures` run against the live TfL API. Set at: **Settings → Secrets and variables → Actions → New repository secret**. Without it the workflow fails loudly before touching any fixtures. |

## Upstream watchlist

Issues we're watching to remove deferrals:

- `sveltejs/kit` — `cookie` 0.7 bump request: *(URL pasted here when
  the upstream issue is opened)*
- `tauri-apps/tauri` — `gtk 0.20` ecosystem bump: tracking next minor.
