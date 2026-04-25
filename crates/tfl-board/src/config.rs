use serde::{Deserialize, Serialize};
use tfl_domain::Direction;

/// The set of valid theme IDs. Validated at the command layer.
pub const VALID_THEME_IDS: &[&str] = &[
    "classic-amber",
    "classic-orange",
    "modern-white",
    "high-contrast",
];

/// Default theme ID used when no theme is persisted.
pub const DEFAULT_THEME: &str = "classic-amber";

/// Configuration for a single arrivals board.
///
/// Derives `Serialize + Deserialize` so Tauri IPC and `tauri-plugin-store`
/// can round-trip the config through JSON without a conversion layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardConfig {
    /// NaPTAN stop-point ID of the station to display, e.g. `"940GZZLUBZP"`.
    pub station_id: String,

    /// Restrict board to these line IDs (case-insensitive, e.g. `["northern"]`).
    /// Empty = no filter (show all lines).
    pub line_ids: Vec<String>,

    /// Restrict board to these directions.
    /// Empty = no filter (show all directions).
    pub directions: Vec<Direction>,

    /// How often to poll the TfL API, in seconds. Default: 20.
    pub poll_seconds: u32,

    /// UI theme ID. Must be one of `VALID_THEME_IDS`. Default: `"classic-amber"`.
    /// Persisted alongside the board config so the user's theme preference
    /// survives app restarts. Frontend applies the theme by setting
    /// `document.documentElement.className = "theme-{id}"`.
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_theme() -> String {
    DEFAULT_THEME.to_string()
}

impl BoardConfig {
    /// Create a config that shows all lines and directions for `station_id`,
    /// polling every 30 seconds, with the default classic-amber theme.
    ///
    /// 30 s matches TfL's ~30 s arrivals-data refresh cadence; polling faster
    /// returns identical payloads and wastes ~33 % of the API quota.
    pub fn new(station_id: impl Into<String>) -> Self {
        Self {
            station_id: station_id.into(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 30,
            theme: DEFAULT_THEME.to_string(),
        }
    }
}

impl Default for BoardConfig {
    fn default() -> Self {
        Self::new("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_poll_seconds_is_30() {
        assert_eq!(
            BoardConfig::new("940GZZLUBZP").poll_seconds,
            30,
            "default poll_seconds should be 30 (TfL refreshes ~every 30s)"
        );
    }
}
