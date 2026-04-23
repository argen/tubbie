# Northern Line Branch Inference

**Status:** Accepted

## Context

The Northern line has two distinct branches south of Camden Town:

- **Via Bank (eastern/City branch):** runs through London Bridge, Monument/Bank, Moorgate, Old Street, Angel, King's Cross St. Pancras, Euston (via Bank platforms), Camden Town — and north of Camden Town serves the Edgware branch and the High Barnet branch.
- **Via Charing Cross (western/West End branch):** runs through Waterloo, Embankment, Charing Cross, Leicester Square, Tottenham Court Road, Goodge Street, Warren Street, Euston (via Charing Cross platforms), Camden Town.

South of Stockwell / Kennington the two branches merge. Between Kennington and Camden Town the branches share stations.

For the arrivals board we want to show the branch alongside the compass direction (Northbound/Southbound) so a passenger at, say, King's Cross knows whether the next Southbound train goes via Bank or via Charing Cross.

TfL's arrival prediction JSON provides:
- `platformName` — e.g. `"Northbound - Platform 7"` (always present and reliable).
- `direction` — raw string `"inbound"` or `"outbound"` (less reliable; TfL uses it for circular lines too).
- `towards` — a human-readable destination label set by TfL's prediction engine.
- `lineId` — `"northern"` for Northern line trains.

## Decision

Branch is inferred **from the `towards` field** using simple substring matching:

| `towards` substring | Branch |
|---------------------|--------|
| `"via Bank"` | `NorthernBranch::Bank` |
| `"via CX"` | `NorthernBranch::CharingCross` |
| `"via Charing Cross"` | `NorthernBranch::CharingCross` |
| Anything else | `None` (ambiguous) |

This is implemented in `tfl_domain::direction::infer_northern_branch`.

The compass direction (Northbound/Southbound) is derived from the `platformName` prefix, which is more reliable than the `direction` field.

**Ambiguous cases:** When `towards` lacks a `"via X"` suffix (e.g. engineering trains, short workings, or TfL API bugs), `via` is set to `None`. No guess is made. The branch label is simply omitted from the board display.

**Sources consulted:**
- [Wikipedia — Northern line](https://en.wikipedia.org/wiki/Northern_line) for branch topology.
- TfL Unified API fixture data (`fixtures/arrivals/*.json`) for observed `towards` values:
  - `"Edgware via CX"` — CharingCross branch, Northbound at Belsize Park.
  - `"Battersea via CX"` — CharingCross branch, Southbound at Belsize Park.
  - `"High Barnet via Bank"` — Bank branch, Northbound at King's Cross and Bank.
  - `"Morden via Bank"` — Bank branch, Southbound at King's Cross and Bank.

## Consequences

**Positive:**
- Zero API calls required — inference is purely textual, no lookup tables to maintain.
- Gracefully handles missing/ambiguous `towards` by falling back to `via: None`.
- All eight test cases in `crates/tfl-domain/tests/northern_line_branch.rs` pass against real fixture data.

**Negative / risks:**
- If TfL changes the wording of their `towards` labels (e.g. from `"via CX"` to `"via Charing Cross"` globally) the inference breaks silently. Mitigation: the contract tests in `tfl-client` will start deserializing with `via: None` instead of a branch value, which is incorrect but not a crash. When fixtures are refreshed (`just record-fixtures`) the regression will surface immediately.
- Stations south of the split (e.g. Stockwell, Oval) where both branches share a platform: `towards` determines the branch correctly in the fixture data, so inference works. No special casing needed.
- Stations at the exact split point (Camden Town): both branches serve the same platform; inference still works because `towards` carries the suffix.
