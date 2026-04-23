use tfl_domain::Direction;

/// Configuration for a single arrivals board.
#[derive(Debug, Clone)]
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
}

impl BoardConfig {
    /// Create a config that shows all lines and directions for `station_id`,
    /// polling every 20 seconds.
    pub fn new(station_id: impl Into<String>) -> Self {
        Self {
            station_id: station_id.into(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 20,
        }
    }
}

impl Default for BoardConfig {
    fn default() -> Self {
        Self::new("")
    }
}
