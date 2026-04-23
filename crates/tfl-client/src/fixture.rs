use crate::error::TflError;
use crate::http::TflHttp;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// A `TflHttp` implementation that reads fixtures from disk.
///
/// Fixture layout:
/// ```text
/// fixtures/
///   arrivals/940GZZLUBZP.json
///   line-status/tube.json
///   stop-points/tube.json
/// ```
///
/// `fetch("arrivals", "940GZZLUBZP")` reads `{fixtures_dir}/arrivals/940GZZLUBZP.json`.
#[derive(Debug, Clone)]
pub struct FixtureTflHttp {
    fixtures_dir: PathBuf,
}

impl FixtureTflHttp {
    /// Create a `FixtureTflHttp` that reads from `fixtures_dir`.
    pub fn new(fixtures_dir: impl AsRef<Path>) -> Self {
        Self {
            fixtures_dir: fixtures_dir.as_ref().to_path_buf(),
        }
    }

    fn fixture_path(&self, endpoint: &str, id: &str) -> PathBuf {
        self.fixtures_dir.join(endpoint).join(format!("{id}.json"))
    }
}

impl TflHttp for FixtureTflHttp {
    async fn fetch(&self, endpoint: &str, id: &str) -> Result<Value, TflError> {
        let path = self.fixture_path(endpoint, id);
        let contents = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                TflError::NotFound(format!("fixture not found: {}", path.display()))
            } else {
                TflError::Io(e)
            }
        })?;
        let value: Value = serde_json::from_str(&contents)?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Returns a path to the workspace fixtures directory, relative to the
    /// crate's manifest location so it works from any working directory.
    fn workspace_fixtures_dir() -> PathBuf {
        // CARGO_MANIFEST_DIR points to crates/tfl-client/
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest.join("../../fixtures")
    }

    // ---------------------------------------------------------------------------
    // FixtureTflHttp — happy paths against real workspace fixtures
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn fixture_http_loads_belsize_park_arrivals() {
        let client = FixtureTflHttp::new(workspace_fixtures_dir());
        let value = client
            .fetch("arrivals", "940GZZLUBZP")
            .await
            .expect("fixture should exist");
        assert!(
            value.is_array(),
            "arrivals fixture should be a JSON array, got: {value:?}"
        );
    }

    #[tokio::test]
    async fn fixture_http_loads_kings_cross_arrivals() {
        let client = FixtureTflHttp::new(workspace_fixtures_dir());
        let value = client
            .fetch("arrivals", "940GZZLUKSX")
            .await
            .expect("fixture should exist");
        assert!(value.is_array());
    }

    #[tokio::test]
    async fn fixture_http_loads_bank_arrivals() {
        let client = FixtureTflHttp::new(workspace_fixtures_dir());
        let value = client
            .fetch("arrivals", "940GZZLUBNK")
            .await
            .expect("fixture should exist");
        assert!(value.is_array());
    }

    #[tokio::test]
    async fn fixture_http_loads_oxford_circus_arrivals() {
        let client = FixtureTflHttp::new(workspace_fixtures_dir());
        let value = client
            .fetch("arrivals", "940GZZLUOXC")
            .await
            .expect("fixture should exist");
        assert!(value.is_array());
    }

    #[tokio::test]
    async fn fixture_http_missing_returns_not_found() {
        let client = FixtureTflHttp::new(workspace_fixtures_dir());
        let err = client
            .fetch("arrivals", "DOESNOTEXIST")
            .await
            .expect_err("should fail with NotFound");
        assert!(
            matches!(err, TflError::NotFound(_)),
            "expected NotFound, got: {err:?}"
        );
    }

    // ---------------------------------------------------------------------------
    // FixtureTflHttp — hand-written test fixture (no real TfL data needed)
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn fixture_http_returns_verbatim_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let endpoint_dir = dir.path().join("arrivals");
        fs::create_dir_all(&endpoint_dir).unwrap();

        let payload = serde_json::json!([{"id": "abc", "timeToStation": 120}]);
        fs::write(
            endpoint_dir.join("TEST001.json"),
            serde_json::to_string(&payload).unwrap(),
        )
        .unwrap();

        let client = FixtureTflHttp::new(dir.path());
        let value = client
            .fetch("arrivals", "TEST001")
            .await
            .expect("should load hand-written fixture");
        assert_eq!(value, payload);
    }
}
