<h1>
  <img src="docs/assets/logo.png" alt="" width="48" align="left" />
  Tubbie
</h1>

> A desktop dot-matrix arrivals board for the London Underground, DLR, London Overground, and Elizabeth line, powered by [TfL's Unified API](https://api.tfl.gov.uk). Works out of the box — no TfL account needed. Built with Tauri v2, SvelteKit, and Rust.

[![CI](https://github.com/argen/tubbie/actions/workflows/ci.yml/badge.svg)](https://github.com/argen/tubbie/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
![Platform: macOS](https://img.shields.io/badge/platform-macOS-lightgrey)

<p align="center">
  <img src="docs/assets/screenshot.png" alt="Tubbie in window mode showing live arrivals at Bank, grouped by line — Northern, Central, Waterloo &amp; City, and DLR — above a network service-status ticker" width="720" />
</p>

## What it is

Tubbie replicates the classic amber LED dot-matrix boards found on Underground platforms. Pick any station on the **Tube, DLR, London Overground (Mildmay / Lioness / Suffragette / Windrush / Weaver / Liberty), or Elizabeth line**, filter by line and direction, and watch real-time arrival predictions scroll in. Four built-in themes — classic amber, classic orange, modern white, and high-contrast — match the visual character of different eras of board hardware.

## Features

- **Zero-config onboarding** — live arrivals on first launch, no TfL account or API key required; personal key in Settings always takes priority
- Real-time arrivals for any London Underground, DLR, Overground, or Elizabeth-line station, grouped by line and direction
- **Network-wide service status** — worst-first disruption panel (marquee ticker) and a full Status view covering every TfL line, with affected route segments ("Watford ↔ Harrow" / "Entire line") and expandable disruption detail
- **Change station from the board** — search bar drops in from the header without opening Settings
- **Menu-bar mode** with a dot-matrix tray icon (original glyph, not the TfL roundel); switches to an alert variant when a watched line is disrupted; two-row header so long station names never truncate
- Line and direction filters across all surfaced modes
- Four visual themes (classic-amber, classic-orange, modern-white, high-contrast)
- Dot-matrix typography with animated row entry, character-reveal, marquee ticker, and "Due" flash
- Settings persisted across restarts; first-run prompt to pick your station on first launch
- Stale-data fallback: last-known arrivals shown when offline, with a visible badge

## Requirements

| Requirement | Version |
|---|---|
| macOS | 11 (Big Sur) or later |
| Node.js | 24 (see `web/.nvmrc`) |
| Rust | stable (see `rust-toolchain.toml`) |
| just | 1.x |
| Xcode Command Line Tools | any recent |

Install Rust via [rustup](https://rustup.rs/). Install `just` via `cargo install just` or `brew install just`.

## Clone (for local dev)

```sh
git clone git@github.com:argen/tubbie.git
cd tubbie
cd web && npm install && cd ..
```

End users don't need to clone — see the **Install** section below for the signed-DMG path.

## Local dev

```sh
just dev
```

Starts the SvelteKit dev server at `http://localhost:5173` and the Tauri shell concurrently. Hot-reload is active for both the frontend and backend.

## Commands

| Command | What it does |
|---|---|
| `just verify` | Full gate: Rust (`fmt`, `clippy`, `test`) + web (`lint`, `format:check`, `typecheck`, `test`) |
| `just build` | Produce a release `.app` bundle at `target/release/bundle/macos/Tubbie.app` |
| `just dev` | Start Tauri dev + Vite dev server concurrently |
| `just verify-live` | Live integration tests against `api.tfl.gov.uk` (not in CI; requires network) |
| `just record-fixtures` | Refresh committed TfL API fixtures from the live API |

## Configuration

User preferences are stored by Tauri in the platform app-data directory:

- **macOS:** `~/Library/Application Support/app.tubbie/`

The `config.json` inside that directory holds the current station, line filters, direction filters, poll interval, and TfL API key. You can inspect it, but editing it manually is not necessary — the Settings screen in the app covers all fields.

### TfL API key (optional)

Tubbie works without any configuration. At startup it reads a pooled key from a background cache — no visible prompt, no account required. If the pool is ever unavailable it falls back to anonymous access (TfL allows 50 requests/minute), so the board is never blocked waiting on a key.

To use your own key: register a free one at [api-portal.tfl.gov.uk](https://api-portal.tfl.gov.uk) and enter it in Settings → TfL API Key. Your personal key always overrides the pool. It is stored in the macOS Keychain and never committed or logged.

For contributors running dev builds, set `TUBBIE_DEV_APP_KEY=<key>` in your shell — dev builds bypass the Keychain and read from that env var instead. (The underlying client also honours a `TFL_APP_KEY` environment variable at launch, if you'd rather not store a key at all.)

## Install

### From a signed release

Download the `.dmg` from [GitHub Releases](../../releases), mount, drag Tubbie to `/Applications`, and launch. Builds are signed with a Developer ID Application certificate and notarized by Apple, so Gatekeeper accepts them on first run — no right-click-Open dance.

In-app auto-update is on by default: Settings → Updates → **Check for updates**. Toggle off there if you'd rather control updates manually.

#### Gatekeeper recovery

If macOS ever says Tubbie is "damaged or malicious", it usually means the download was interrupted. **Re-download from the Releases page** — don't strip the quarantine attribute by hand. The signed binary's notarization ticket is the safety net; bypassing it loses that protection.

#### Location permission

Tubbie can find your nearest station with one tap. Location is requested **only when you tap "find nearest"**, used in that moment for a single fix, and never sent anywhere except `api.tfl.gov.uk`. macOS will show the system prompt the first time; choose "Allow While Using App".

### Build locally

```sh
just build
```

Produces an unsigned `.app` at `target/release/bundle/macos/Tubbie.app` for development. Suitable for local use; not for distribution.

### Cutting a release

Releases run **locally** from the dev's Mac — there is no CI release workflow. Prerequisites are documented in [`docs/ADR/public-distribution.md`](docs/ADR/public-distribution.md) (Developer ID cert in login Keychain, Ed25519 updater key, `tubbie-notary` notarytool profile, `.envrc`).

```sh
source .envrc                  # if you don't use direnv
just bump 1.0.1                # bumps tauri.conf.json + Cargo.toml in lockstep
# commit + merge the version bump on a PR to main
just release v1.0.1            # preflight + build + sign + notarize + staple + manifest + tag + push + draft Release
gh release edit v1.0.1 --draft=false   # publish after smoking from a clean macOS user account
```

`just release` chains the full pipeline. `scripts/preflight.sh` refuses to start if the tree is dirty, the branch isn't `main`, the version doesn't match the tag, or any signing prerequisite is missing.

## TfL attribution

Powered by [TfL Open Data](https://tfl.gov.uk/info-for/open-data-users/). Contains OS data © Crown copyright and database rights 2016. Collated by TfL. Use of TfL data is governed by the [TfL Open Data Licence](https://tfl.gov.uk/corporate/terms-and-conditions/transport-data-service).

## Licence

MIT — see [`LICENSE`](LICENSE).

## Contributing

See [`docs/README.md`](docs/README.md) for conceptual documentation and [`docs/ADR/README.md`](docs/ADR/README.md) for architectural decisions. All contributions welcome via pull request against `main`.

Before opening a PR, confirm `just verify` is green.
