# Tubbie v0.1.0

The first public release. Tubbie is a desktop dot-matrix arrivals board for the **London Underground, DLR, London Overground**, and **Elizabeth line**, powered by [TfL's Unified API](https://api.tfl.gov.uk).

## Install

Download `Tubbie_0.1.0_aarch64.dmg` below, mount it, drag Tubbie to `/Applications`, and launch. The build is **signed with a Developer ID Application certificate and notarized by Apple** — Gatekeeper accepts it on first run, no right-click-Open dance.

**Requirements:** macOS 11 (Big Sur) or later, Apple Silicon (arm64).

## What's in it

- Real-time arrivals for any London Underground, DLR, Overground, or Elizabeth-line station
- Arrivals grouped by line and direction; per-line and per-direction filters
- Four visual themes — classic-amber, classic-orange, modern-white, high-contrast
- Dot-matrix typography with animated row entry, character-reveal, marquee ticker, and "Due" flash
- Settings persisted across restarts (station, filters, theme, display mode)
- Anonymous TfL access out of the box (50 req/min); optional personal app key in Settings (500 req/min)
- Stale-data fallback: last-known arrivals shown when offline, with a visible badge
- **Auto-update on by default** — Settings → Updates → toggle off if you'd rather manage upgrades manually

## Notes

- arm64 only this round. Intel Mac support is a possible follow-up if there's demand — please [open an issue](../../issues).
- This is a first release; expect rough edges. Bug reports and feature requests welcome.
