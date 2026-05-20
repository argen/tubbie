## Summary

<!-- Describe what this PR does and why. Link to any relevant issue. -->

## Checklist

- [ ] `just verify` passes locally
- [ ] `crates/tfl-*` untouched (the pre-push hook from `just install-hooks` enforces this; tubbie-ios submodules these crates and breaking changes need the bump-core dance — see [`docs/ADR/crates-as-public-contract.md`](../docs/ADR/crates-as-public-contract.md))
- [ ] `tauri.conf.json` and `src-tauri/Cargo.toml` versions match (the pre-push lockstep check enforces this — use `just bump <semver>` for version bumps)
- [ ] New ADR added or updated if this introduces a non-obvious design decision (see [ADR index](../docs/ADR/README.md))
- [ ] No secrets committed
