# Tubbie v0.1.2

A small but important hotfix over [v0.1.1](../../releases/tag/v0.1.1) for the in-app updater.

## Changes

- **Fixed: auto-update no longer freezes on "Installing — please don't close Tubbie."** The new version was already on disk, but the running process wasn't being restarted, so the UI sat there indefinitely. v0.1.2 now restarts Tubbie automatically once the download is verified and staged.
- **New "Restarting Tubbie…" state.** Brief moment between install completion and the new process taking over — gives you a clear "yes, something is happening" before the window blinks out and back.
- **Safety net for future failures.** If the restart ever fails to fire within 5 seconds, the Settings UI now shows a recovery message ("Quit Tubbie and open it again to finish") instead of staying frozen.

## If you're upgrading from v0.1.1 (or v0.1.0)

You'll hit the original bug one last time on the way to v0.1.2 — the code that fixes the restart only takes effect *after* you're running v0.1.2. So:

1. Settings → Updates → Check for updates → Install and restart.
2. The UI will say "Installing — please don't close Tubbie."
3. Wait ~30 seconds. The download has finished and v0.1.2 is already in `/Applications` — but the v0.1.1 process won't relaunch itself.
4. **Quit Tubbie manually** (`Cmd+Q`, or via Activity Monitor).
5. Open Tubbie again. You're now on v0.1.2.

From here on, future updates will restart cleanly with no manual intervention.

If you're installing fresh: grab `Tubbie_0.1.2_aarch64.dmg` below, mount it, drag Tubbie to `/Applications`. Signed + notarized — Gatekeeper accepts it on first run.
