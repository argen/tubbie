//! Pins the wire-format contract between `scripts/build-latest-json.sh`
//! and `tauri-plugin-updater`'s manifest parser.
//!
//! Tauri v2's updater has had migration churn between point releases —
//! historically including `pub_date` format changes and `platforms`
//! map vs. dynamic-shape switches. A bump that silently breaks the
//! manifest schema would leave every shipped binary failing to
//! discover updates, with no in-app warning to the user.
//!
//! This test loads a fixture that matches what
//! `scripts/build-latest-json.sh` emits at release time, deserialises
//! it into `tauri_plugin_updater::RemoteRelease`, and asserts the
//! fields the plugin will read. When `cargo update` brings in a
//! newer `tauri-plugin-updater` whose Deserialize impl no longer
//! accepts our shape, this test goes RED.

use tauri_plugin_updater::{RemoteRelease, RemoteReleaseInner};

const MANIFEST: &str = include_str!("fixtures/latest.json");

#[test]
fn fixture_latest_json_deserialises_into_remote_release() {
    let release: RemoteRelease = serde_json::from_str(MANIFEST).unwrap_or_else(|e| {
        panic!(
            "fixtures/latest.json must deserialise into tauri_plugin_updater::RemoteRelease — \
             a schema change in the plugin will trip this test on `cargo update`. Error: {e}"
        )
    });

    // Version field is parsed via the plugin's `parse_version` — pins
    // that the plugin still accepts plain semver strings.
    assert_eq!(release.version.to_string(), "0.1.1");

    // pub_date must round-trip RFC 3339 — pins the date contract.
    assert!(release.pub_date.is_some(), "pub_date must parse");

    // Static-shape (`platforms` map) is what scripts/build-latest-json.sh
    // emits. The plugin's `Dynamic` alternative is the older shape we
    // don't use; pinning Static defends against a silent switch to
    // Dynamic-only parsing in a future Tauri.
    match release.data {
        RemoteReleaseInner::Static { platforms } => {
            let darwin = platforms.get("darwin-aarch64").expect(
                "manifest MUST carry the darwin-aarch64 entry — that's the platform key the \
                 desktop bundle reports to the updater",
            );
            assert!(
                darwin
                    .url
                    .as_str()
                    .starts_with("https://github.com/argen/tubbie/releases/download/"),
                "url must point at a GitHub Release asset, got {}",
                darwin.url
            );
            assert!(
                darwin.signature.contains("untrusted comment"),
                "signature must be a minisign block (starts with `untrusted comment:`)"
            );
        }
        RemoteReleaseInner::Dynamic(_) => {
            panic!("Dynamic-shape manifest received; scripts/build-latest-json.sh emits Static");
        }
    }
}
