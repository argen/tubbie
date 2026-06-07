# ADR: Open-sourcing Checklist

**Status:** Implemented (argen/tubbie flipped public 2026-05-22; see [public-distribution.md](./public-distribution.md) Phase 8)

## Context

The repository starts as private (`argen/tubbie`). At some point the project
may be made public. Flipping visibility before meeting certain criteria risks
exposing secrets, incomplete attribution, or a poor first impression.

## Decision

Before flipping the repository to public, all of the following must be satisfied:

### Security
- [ ] Confirm no secrets were ever committed (check full git history with `git log -S`)
- [ ] `TFL_APP_KEY` and any other credentials confirmed absent from all commits
- [ ] Fixture files confirmed to not contain `app_key` query parameters

### Licensing and attribution
- [ ] `LICENSE` file in repo root (MIT — already committed)
- [ ] TfL Open Data attribution in app UI and README (`Powered by TfL Open Data`)
- [ ] All bundled fonts (VT323, DSEG14-Classic) confirmed SIL OFL — already verified

### Documentation
- [ ] README covers: prerequisites, `just verify`, `just dev`, `just record-fixtures`, `just build`
- [ ] README includes screenshots of at least one theme
- [ ] `CONTRIBUTING.md` added (code style, PR process, ADR process)

### Code quality
- [ ] `just verify` green on a clean checkout
- [ ] No TODO/FIXME comments referencing internal details
- [ ] No debug logging that reveals internal URLs or user data

### GitHub hygiene
- [ ] Repository description, topics, and homepage set
- [ ] Issues and Discussions enabled appropriately

## Consequences

- This ADR acts as a gate checklist — the flip to public is a deliberate,
  reviewed action, not an accident.
- Items are checked off progressively as milestones land.

## Status

Implemented — argen/tubbie flipped public 2026-05-22 as part of the v0.1.0 release. See [`public-distribution.md`](./public-distribution.md) Phase 8 for the as-built record.
