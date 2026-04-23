# ADR: Use Tauri v2 for the Mac App Shell

**Status:** Accepted

## Context

The app needs to run as a native Mac desktop application displaying TfL tube
arrivals. Options considered: Electron, Tauri v1, Tauri v2, native Swift/AppKit.

Electron was rejected: ~150MB bundle, ships its own Chromium, slow startup.
Tauri v1 is superseded by v2 which has a stable plugin system and better
security defaults.
Native Swift/AppKit would require rewriting the Rust core in Swift/ObjC.

## Decision

Use Tauri v2 as the app shell with SvelteKit + `adapter-static` as the
frontend. The Tauri webview loads a locally-bundled static site; no server
component exists inside the app.

## Consequences

- Binary is ~10MB vs ~150MB for Electron.
- All Server Components, Server Actions, ISR, and `next/image` are unavailable
  (no Node runtime in the webview) — this is why Next.js was rejected in favour
  of SvelteKit.
- The Tauri plugin ecosystem (`tauri-plugin-store` for persistence,
  `tauri-plugin-shell` if needed) is used over reimplementing platform APIs.
- Packaging and notarisation for macOS distribution is deferred to M7.

## Status

Accepted — Tauri v2 is the chosen shell.
