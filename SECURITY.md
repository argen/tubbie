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

The non-trivial attack surfaces we track are:

1. **TfL response parsing** — a hostile or compromised upstream
   response could reach `tfl-client` / `tfl-board` parsers.
2. **Supply chain at build time** — proc-macro / build-script
   compromise on a contributor's or CI host (e.g. `xz`-class incidents).
3. **Forks running `npm run dev`** — once open source, contributors run
   the SvelteKit dev server, which exercises code paths the shipped
   static bundle does not.

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

## Repo settings checklist (manual, post-OSS flip)

These are GitHub repo-settings toggles, not file-tracked. The repo
maintainer must enable them when flipping the repo to public:

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

## Upstream watchlist

Issues we're watching to remove deferrals:

- `sveltejs/kit` — `cookie` 0.7 bump request: *(URL pasted here when
  the upstream issue is opened)*
- `tauri-apps/tauri` — `gtk 0.20` ecosystem bump: tracking next minor.
