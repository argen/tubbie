// FFISpikeTests.swift — B.1 Swift-side smoke test.
//
// Compile + run against the host dylib so the spike's "does Swift see what
// uniffi promised?" question gets a yes from the actual Swift compiler, not
// just the Rust integration tests.
//
// The script in `scripts/run-swift-spike.sh` builds the cdylib, generates
// the bindings, and runs this file. Output `OK: ...` per assertion; non-zero
// exit on the first failure.
//
// This file lives outside the Cargo crate (no `swift build`-style package
// because we want the simplest possible "swiftc + dylib + run" invocation
// that avoids dragging in SwiftPM resolution).

import Foundation

// The generated tfl_ffi.swift (under generated/swift/) is brought in via
// `swiftc -I` + direct compilation alongside this file.

@main
struct FFISpike {
    static func main() async {
    let manifestDir = String(cString: getenv("CARGO_MANIFEST_DIR") ?? strdup("."))
    let fixturesDir = manifestDir + "/../../third_party/tubbie/fixtures"
    let recordedAt = "2026-04-29T09:16:10.390066+00:00"

    // 1. Happy path: async board fetch.
    do {
        let started = Date()
        let json = try await getBoardJson(
            stationId: "940GZZLUBNK",
            fixturesDir: fixturesDir,
            recordedAtRfc3339: recordedAt
        )
        let elapsedMs = Date().timeIntervalSince(started) * 1000
        guard let data = json.data(using: .utf8),
              let parsed = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            fail("getBoardJson returned non-JSON: \(json)")
        }
        guard let stationId = parsed["station_id"] as? String, stationId == "940GZZLUBNK" else {
            fail("expected station_id=940GZZLUBNK in board, got \(parsed)")
        }
        print(String(format: "OK: getBoardJson returned valid Board in %.1f ms", elapsedMs))
    } catch {
        fail("getBoardJson should succeed, got error: \(error)")
    }

    // 2. Validation error path: empty station_id maps to FfiError.validation.
    do {
        _ = try await getBoardJson(
            stationId: "",
            fixturesDir: fixturesDir,
            recordedAtRfc3339: recordedAt
        )
        fail("expected validation error for empty station_id")
    } catch let error as FfiError {
        if case .Validation(let msg) = error {
            guard msg.contains("station_id") else {
                fail("validation error should mention station_id, got: \(msg)")
            }
            print("OK: empty station_id surfaced as FfiError.Validation")
        } else {
            fail("expected Validation, got \(error)")
        }
    } catch {
        fail("expected FfiError, got \(type(of: error)): \(error)")
    }

    // 3. Tokio runtime warmup is a sync export.
    let warmupMicros = tokioRuntimeWarmupMicros()
    guard warmupMicros < 30_000 else {
        fail("tokioRuntimeWarmupMicros must be <30000, got \(warmupMicros)")
    }
    print("OK: tokio runtime warmup = \(warmupMicros) µs (<30000 µs gate)")

    // 4. Panic safety, end-to-end. The export is now `Result<(), FfiError>`
    //    on the Rust side, so uniffi surfaces the panic as a thrown Swift
    //    error rather than a `try!` trap. We call it and assert the catch
    //    branch fires — proving uniffi's `catch_unwind` round-trips the
    //    panic into the Swift error path without aborting the process.
    do {
        try triggerPanicForTesting()
        fail("triggerPanicForTesting must throw — Rust panicked, Swift saw nothing")
    } catch {
        // uniffi maps a panic to its internal error type; the variant name
        // and module differ across uniffi versions, so we assert via
        // `String(describing:)` rather than a fragile downcast.
        let descr = String(describing: error)
        guard descr.lowercased().contains("panic") || descr.lowercased().contains("internal") else {
            fail("expected panic/internal error description, got: \(descr)")
        }
        print("OK: panic from Rust caught as Swift error: \(descr.prefix(80))…")
    }

    print("---")
    print("B.1 Swift smoke: PASSED")
    }
}

func fail(_ msg: String) -> Never {
    FileHandle.standardError.write("FAIL: \(msg)\n".data(using: .utf8)!)
    exit(1)
}
