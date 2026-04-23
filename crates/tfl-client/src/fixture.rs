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

/// Validate a single path component (endpoint or id) to prevent path traversal.
///
/// Allowed characters: ASCII alphanumeric, `-`, `_`.
/// Rejected: empty strings, `..`, `/`, `\`, null bytes, absolute-path prefixes,
/// and any character outside the allowed set.
///
/// # Errors
/// Returns `TflError::InvalidRequest` if the component is invalid.
fn validate_path_component(component: &str, field: &str) -> Result<(), TflError> {
    if component.is_empty() {
        return Err(TflError::InvalidRequest {
            reason: format!("{field} must not be empty"),
        });
    }

    // Reject any character that is not ASCII alphanumeric, `-`, or `_`.
    // This implicitly rejects `.`, `/`, `\`, null bytes, spaces, and everything else.
    let all_safe = component
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if !all_safe {
        return Err(TflError::InvalidRequest {
            reason: format!(
                "{field} contains disallowed characters (only ASCII alphanumeric, '-', '_' are permitted): {component:?}"
            ),
        });
    }

    Ok(())
}

impl TflHttp for FixtureTflHttp {
    async fn fetch(&self, endpoint: &str, id: &str) -> Result<Value, TflError> {
        // SECURITY: validate both components before touching the filesystem.
        // This prevents path-traversal attacks such as `..`, `../../etc/passwd`, etc.
        validate_path_component(endpoint, "endpoint")?;
        validate_path_component(id, "id")?;

        let path = self.fixture_path(endpoint, id);
        let contents = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                TflError::NotFound(format!("fixture not found: {}", path.display()))
            } else {
                TflError::Io(e)
            }
        })?;
        let value: Value = serde_json::from_str(&contents).map_err(|e| TflError::ParseAt {
            path: path.clone(),
            source: e,
        })?;
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
    // FixtureTflHttp — path-traversal guard (task #11)
    // ---------------------------------------------------------------------------

    /// Helper: creates a FixtureTflHttp pointing at a temp dir (no real fixtures needed
    /// for validation tests — errors occur before any filesystem access).
    fn any_fixture_http() -> FixtureTflHttp {
        FixtureTflHttp::new(PathBuf::from("/tmp"))
    }

    #[tokio::test]
    async fn rejects_dotdot_endpoint() {
        let err = any_fixture_http()
            .fetch("..", "passwd")
            .await
            .expect_err("must reject .. endpoint");
        assert!(
            matches!(err, TflError::InvalidRequest { .. }),
            "expected InvalidRequest, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_path_traversal_in_id() {
        let err = any_fixture_http()
            .fetch("arrivals", "../../etc/passwd")
            .await
            .expect_err("must reject path-traversal id");
        assert!(
            matches!(err, TflError::InvalidRequest { .. }),
            "expected InvalidRequest, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_empty_endpoint() {
        let err = any_fixture_http()
            .fetch("", "940GZZLUBZP")
            .await
            .expect_err("must reject empty endpoint");
        assert!(
            matches!(err, TflError::InvalidRequest { .. }),
            "expected InvalidRequest, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_empty_id() {
        let err = any_fixture_http()
            .fetch("arrivals", "")
            .await
            .expect_err("must reject empty id");
        assert!(
            matches!(err, TflError::InvalidRequest { .. }),
            "expected InvalidRequest, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_slash_in_id() {
        let err = any_fixture_http()
            .fetch("arrivals", "some/path")
            .await
            .expect_err("must reject slash in id");
        assert!(
            matches!(err, TflError::InvalidRequest { .. }),
            "expected InvalidRequest, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_backslash_in_endpoint() {
        let err = any_fixture_http()
            .fetch("arri\\vals", "940GZZLUBZP")
            .await
            .expect_err("must reject backslash in endpoint");
        assert!(
            matches!(err, TflError::InvalidRequest { .. }),
            "expected InvalidRequest, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_null_byte_in_id() {
        let err = any_fixture_http()
            .fetch("arrivals", "940GZZLU\0BZP")
            .await
            .expect_err("must reject null byte in id");
        assert!(
            matches!(err, TflError::InvalidRequest { .. }),
            "expected InvalidRequest, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_dot_in_id() {
        let err = any_fixture_http()
            .fetch("arrivals", "some.thing")
            .await
            .expect_err("must reject dot in id (potential .. bypass)");
        assert!(
            matches!(err, TflError::InvalidRequest { .. }),
            "expected InvalidRequest, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn accepts_valid_endpoint_and_id() {
        // Valid inputs should pass validation (they'll fail as NotFound since /tmp
        // doesn't have a fixtures dir, but they must NOT return InvalidRequest).
        let err = any_fixture_http()
            .fetch("arrivals", "940GZZLUBZP")
            .await
            .expect_err("should fail with NotFound, not InvalidRequest");
        assert!(
            matches!(err, TflError::NotFound(_) | TflError::Io(_)),
            "expected NotFound or Io (not InvalidRequest), got: {err:?}"
        );
    }

    #[tokio::test]
    async fn accepts_hyphen_and_underscore_in_id() {
        let err = any_fixture_http()
            .fetch("stop-points", "some_fixture-id")
            .await
            .expect_err("should fail with NotFound not InvalidRequest");
        assert!(
            matches!(err, TflError::NotFound(_) | TflError::Io(_)),
            "expected NotFound or Io (not InvalidRequest), got: {err:?}"
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
