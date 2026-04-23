//! Insta snapshot test — fixture index.
//!
//! Captures the list of `(endpoint, id)` tuples present in the fixtures directory.
//! This snapshot changes only when fixtures are added, removed, or renamed —
//! making it a lightweight sentinel against accidental fixture changes.

use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../../fixtures")
}

/// Collect all `(endpoint, id)` pairs from the fixtures directory tree,
/// sorted for determinism.
fn collect_fixture_index() -> Vec<(String, String)> {
    let root = fixtures_dir();
    let mut entries = Vec::new();

    let Ok(read_root) = std::fs::read_dir(&root) else {
        return entries;
    };

    for entry in read_root.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let endpoint = path.file_name().unwrap().to_string_lossy().into_owned();

        let Ok(read_endpoint) = std::fs::read_dir(&path) else {
            continue;
        };

        for file_entry in read_endpoint.flatten() {
            let file_path = file_entry.path();
            let file_name = file_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();

            // Include only `.json` files; skip `.meta.json` sidecars.
            if file_name.ends_with(".meta.json") {
                continue;
            }
            if !file_name.ends_with(".json") {
                continue;
            }

            let id = file_name.trim_end_matches(".json").to_string();
            entries.push((endpoint.clone(), id));
        }
    }

    entries.sort();
    entries
}

#[test]
fn snapshot_fixture_index() {
    let index = collect_fixture_index();
    // Render as a simple text table so the snapshot is human-readable.
    let rendered = index
        .iter()
        .map(|(ep, id)| format!("{ep}/{id}"))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("fixture_index", rendered);
}
