# ADR: Fixture Replay Testing

**Status:** Accepted

## Context

TfL's API returns live train data that changes every 30 seconds. Tests that hit
the live API are flaky, rate-limited (50 rpm anonymous), and can't run offline
or in CI without network access.

Snapshot-style integration tests require a stable, reproducible input.

## Decision

Use a fixture replay harness:

1. **Fixture recorder** (`just record-fixtures`) hits the live TfL API for a
   curated set of stations and writes verbatim JSON to `fixtures/{endpoint}/{id}.json`.
   The recorder strips `app_key` from recorded URLs before saving.

2. **`FixtureTflHttp`** reads fixtures by `{endpoint}/{id}` key — same interface
   as `ReqwestTflHttp` but reads from disk.

3. **`FakeClock`** is set to the wall-time stored in `{id}.meta.json` alongside
   each fixture, making `time_to_station` formatting deterministic.

4. All default `cargo test` runs use fixtures only — zero network.
   Live tests are gated behind `--features live` and excluded from CI.

5. `insta` snapshots capture the rendered board output for regression detection.

## Consequences

- Tests are fast, deterministic, and CI-safe.
- Fixtures can drift from the live API schema over time — mitigated by contract
  tests that `serde_json::from_str::<Arrival>` each fixture and assert `Ok`.
- Refreshing fixtures requires running `just record-fixtures` periodically
  (recommended before each milestone that touches domain types).

## Status

Accepted — harness lands in M0.
