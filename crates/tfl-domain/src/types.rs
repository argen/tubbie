//! Core domain types for TfL tube arrivals.
//!
//! All types implement `Serialize` / `Deserialize`.
//!
//! ## Arrival deserialization note
//!
//! TfL's `Prediction` JSON has a `direction` field that is a raw string
//! (`"inbound"` or `"outbound"`). Our `Direction` enum is richer (it
//! captures compass heading + Northern-line branch).
//!
//! We solve this with a private raw-wire struct (`RawArrival`) that maps
//! directly to TfL's camelCase JSON, and a hand-written `Deserialize` impl
//! for `Arrival` that converts `RawArrival → Arrival` via `infer_direction`.

use crate::direction::{infer_direction, Direction};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};

// ---------------------------------------------------------------------------
// Arrival
// ---------------------------------------------------------------------------

/// A single train arrival prediction from TfL.
///
/// Deserializes from TfL's `Prediction` JSON (camelCase). The `$type` field
/// is accepted but not stored. The `direction` field is enriched via
/// [`infer_direction`] during deserialization.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Arrival {
    /// Unique prediction ID (can be negative in TfL's system).
    pub id: String,

    /// Human-readable station name, e.g. `"Belsize Park Underground Station"`.
    pub station_name: String,

    /// Platform label, e.g. `"Northbound - Platform 1"`.
    pub platform_name: String,

    /// Line identifier, e.g. `"northern"`.
    pub line_id: String,

    /// Human-readable line name, e.g. `"Northern"`.
    pub line_name: String,

    /// Compass direction, enriched from platform name + towards during deserialization.
    pub direction: Direction,

    /// Destination station name, e.g. `"Edgware Underground Station"`.
    pub destination_name: String,

    /// `towards` label from TfL, e.g. `"Edgware via CX"`.
    pub towards: String,

    /// Brief current-location description.
    pub current_location: String,

    /// Seconds until the train reaches this station.
    pub time_to_station: i64,

    /// Absolute expected arrival time (UTC).
    pub expected_arrival: DateTime<Utc>,

    /// NaPTAN stop-point ID of this station.
    pub naptan_id: String,
}

/// Private raw wire struct matching TfL's camelCase JSON.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawArrival {
    id: String,
    #[serde(rename = "$type", default)]
    _type: String,
    station_name: String,
    platform_name: String,
    line_id: String,
    line_name: String,
    /// Raw TfL direction string: "inbound" | "outbound" | "".
    #[serde(default)]
    direction: String,
    #[serde(default)]
    destination_name: String,
    #[serde(default)]
    towards: String,
    #[serde(default)]
    current_location: String,
    time_to_station: i64,
    expected_arrival: DateTime<Utc>,
    #[serde(rename = "naptanId", default)]
    naptan_id: String,
}

impl<'de> Deserialize<'de> for Arrival {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawArrival::deserialize(deserializer)?;
        let direction = infer_direction(
            &raw.platform_name,
            &raw.direction,
            &raw.line_id,
            &raw.towards,
        );
        Ok(Arrival {
            id: raw.id,
            station_name: raw.station_name,
            platform_name: raw.platform_name,
            line_id: raw.line_id,
            line_name: raw.line_name,
            direction,
            destination_name: raw.destination_name,
            towards: raw.towards,
            current_location: raw.current_location,
            time_to_station: raw.time_to_station,
            expected_arrival: raw.expected_arrival,
            naptan_id: raw.naptan_id,
        })
    }
}

// ---------------------------------------------------------------------------
// Station
// ---------------------------------------------------------------------------

/// A tube station from TfL's StopPoint API.
///
/// The trimmed `tube.json` stop-points fixture omits the `lines` field;
/// `#[serde(default)]` handles that gracefully.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Station {
    pub id: String,
    pub common_name: String,
    pub modes: Vec<String>,
    pub lat: f64,
    pub lon: f64,
    /// Lines served by this station (absent in trimmed fixtures → empty vec).
    #[serde(default)]
    pub lines: Vec<LineRef>,
}

/// Thin reference to a line (id + name) used inside `Station`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRef {
    pub id: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Line
// ---------------------------------------------------------------------------

/// A TfL line (tube only for now).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub id: String,
    pub name: String,
    pub mode: String,
}

// ---------------------------------------------------------------------------
// LineStatus / StatusEntry
// ---------------------------------------------------------------------------

/// Status summary for a single line (from `/Line/Mode/{mode}/Status`).
///
/// This is an *interpreted* type — not a direct TfL wire format. The client
/// layer (M2) maps TfL's `lineStatuses[].statusSeverityDescription` and
/// `disruption.description` into this struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineStatus {
    pub line_id: String,
    pub status: Vec<StatusEntry>,
    pub disruption_text: Option<String>,
}

/// An individual severity entry within a `LineStatus`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusEntry {
    /// Numeric severity code from TfL (10 = Good Service).
    pub severity: i32,
    /// Human-readable severity description, e.g. `"Good Service"`.
    pub description: String,
}

// ---------------------------------------------------------------------------
// TfL wire format for LineStatus (for contract-test deserialization)
// ---------------------------------------------------------------------------

/// TfL wire format for a single line from `/Line/Mode/{mode}/Status`.
/// This is separate from the domain `LineStatus` — the client layer converts.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflLine {
    pub id: String,
    pub name: String,
    #[serde(rename = "$type", default)]
    pub _type: String,
    #[serde(default)]
    pub line_statuses: Vec<TflLineStatus>,
}

/// TfL wire format for one entry inside `lineStatuses`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflLineStatus {
    pub status_severity: i32,
    pub status_severity_description: String,
    #[serde(default)]
    pub disruption: Option<TflDisruption>,
}

/// TfL wire format for disruption info nested inside a line status.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflDisruption {
    #[serde(default)]
    pub description: String,
}

// ---------------------------------------------------------------------------
// Board / Platform
// ---------------------------------------------------------------------------

/// A grouped arrivals board for a station, ready for rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub station_id: String,
    pub platforms: Vec<Platform>,
    pub generated_at: DateTime<Utc>,
    /// Set when the last API call failed and we are showing stale data.
    pub stale_since: Option<DateTime<Utc>>,
}

/// All arrivals for one named platform, sorted by `time_to_station` ascending.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Platform {
    pub name: String,
    pub arrivals: Vec<Arrival>,
}

// ---------------------------------------------------------------------------
// Theme (from M6 plan — lives in tfl-domain)
// ---------------------------------------------------------------------------
// (Defined in theme.rs; re-exported from lib.rs.)
