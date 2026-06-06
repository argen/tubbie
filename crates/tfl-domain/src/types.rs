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

use crate::direction::{infer_direction, Direction, NorthernBranch};
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

    /// Northern-line branch (via Bank vs via Charing Cross). Populated for
    /// Northern-line arrivals whose `towards` string carries a `"via …"`
    /// suffix; `None` for every other line and for ambiguous Northern
    /// services (e.g. short workings).
    #[serde(default)]
    pub northern_branch: Option<NorthernBranch>,

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
        // TfL ships Elizabeth-line arrivals with `lineId: "elizabeth-line"`
        // (the *mode* form, used by the routing API), but station metadata
        // and the user's chip filter use `"elizabeth"` (the *line*
        // identifier — what TfL puts in `lineIdentifier` on stop-points).
        // Without normalisation here, filtering by the Elizabeth chip
        // hides every arrival because `"elizabeth" != "elizabeth-line"`.
        // Canonicalise the mode form to the line form on ingest so every
        // downstream consumer (display filter, compass mapping, defensive
        // line-allow filter) sees one stable id.
        let line_id = canonicalize_line_id(&raw.line_id);
        let (direction, northern_branch) = infer_direction(
            &raw.platform_name,
            &raw.direction,
            &line_id,
            &raw.towards,
            &raw.destination_name,
        );
        Ok(Arrival {
            id: raw.id,
            station_name: raw.station_name,
            platform_name: raw.platform_name,
            line_id,
            line_name: raw.line_name,
            direction,
            northern_branch,
            destination_name: raw.destination_name,
            towards: raw.towards,
            current_location: raw.current_location,
            time_to_station: raw.time_to_station,
            expected_arrival: raw.expected_arrival,
            naptan_id: raw.naptan_id,
        })
    }
}

/// Map a TfL `lineId` to the canonical line form used by station metadata
/// and the user's chip filter.
///
/// TfL is inconsistent across endpoints:
/// - `/StopPoint/{id}/Arrivals` returns Elizabeth predictions with
///   `lineId: "elizabeth-line"` (the *mode* form).
/// - `/Line/Mode/elizabeth-line/Status` returns lines with
///   `id: "elizabeth-line"` (also the mode form).
/// - `/StopPoint/Mode/elizabeth-line` returns each station's lines with
///   `lineIdentifier: "elizabeth"` (the *line* form).
///
/// Station metadata + the user's chip filter use the line form. To make
/// every downstream consumer (display filter, compass mapping, defensive
/// line-allow filter, line-status ticker lookup) see one stable id, this
/// canonicaliser is applied at every TfL → domain seam: `Arrival` and
/// `TflLine` deserialization.
pub fn canonicalize_line_id(raw: &str) -> String {
    match raw {
        "elizabeth-line" => "elizabeth".to_string(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Station
// ---------------------------------------------------------------------------

/// A tube station from TfL's StopPoint API.
///
/// ## Line info projection
///
/// TfL's stop-points response doesn't emit a ready-made `lines` array. It
/// emits `lineModeGroups` (grouped by transport mode). Our `Deserialize` impl
/// reads both:
///   - if the JSON already contains a processed `lines` array, use it verbatim
///     (backward-compat with inline-JSON tests and any future API that adds it);
///   - otherwise project the tube entry of `lineModeGroups` into `lines`.
///
/// When neither is present (trimmed fixture), `lines` is empty — callers fall
/// back to the global "all tube lines" list.
///
/// ## Serialization is snake_case (deliberate)
///
/// `Serialize` emits `common_name`, matching the TypeScript `Station` interface
/// at `web/src/lib/ipc/types.ts:84`. The Deserialize impl below reads the
/// TfL wire format (camelCase `commonName`, `lineModeGroups`) via an internal
/// `RawStation` — the two directions don't share a rename rule.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Station {
    pub id: String,
    pub common_name: String,
    pub modes: Vec<String>,
    pub lat: f64,
    pub lon: f64,
    pub lines: Vec<LineRef>,
    /// Hub NaPTAN id (e.g. `HUBTCR` for Tottenham Court Road). Present on
    /// tube parents that share a station with non-tube modes (DLR,
    /// Overground, Elizabeth). When set, the arrivals endpoint must be
    /// queried against the hub id rather than the tube id — otherwise TfL
    /// returns only the tube child's arrivals and DLR / Overground /
    /// Elizabeth trains never appear on the board.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hub_naptan_code: Option<String>,
}

impl<'de> Deserialize<'de> for Station {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct LineModeGroup {
            #[serde(default)]
            mode_name: String,
            #[serde(default)]
            line_identifier: Vec<String>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawStation {
            id: String,
            common_name: String,
            #[serde(default)]
            modes: Vec<String>,
            #[serde(default)]
            lat: f64,
            #[serde(default)]
            lon: f64,
            #[serde(default)]
            lines: Vec<LineRef>,
            #[serde(default)]
            line_mode_groups: Vec<LineModeGroup>,
            #[serde(default)]
            hub_naptan_code: Option<String>,
        }

        let raw = RawStation::deserialize(deserializer)?;
        let lines: Vec<LineRef> = if !raw.lines.is_empty() {
            raw.lines
                .into_iter()
                .filter(|l| is_supported_line_id(&l.id))
                .collect()
        } else {
            raw.line_mode_groups
                .into_iter()
                // Accept entries for any of the modes we surface, and accept
                // entries with an absent/empty modeName (our trimmed fixture
                // drops the field to save space). Bus, coach, river-bus,
                // tram, national-rail groups are dropped at this layer so
                // their line ids never reach the per-id whitelist below.
                .filter(|g| {
                    g.mode_name.is_empty()
                        || matches!(
                            g.mode_name.as_str(),
                            "tube" | "dlr" | "overground" | "elizabeth-line"
                        )
                })
                .flat_map(|g| g.line_identifier.into_iter())
                // Hub stop-points mix bus routes, national-rail services, and
                // our supported lines into the same list when modeName is
                // absent; this whitelist removes the rest so the chip UI
                // doesn't render "52, 390, GATWICK-EXPRESS, …" next to a
                // station name.
                .filter(|id| is_supported_line_id(id))
                .map(|id| {
                    let name = pretty_line_name(&id).to_string();
                    LineRef { id, name }
                })
                .collect()
        };
        Ok(Station {
            id: raw.id,
            common_name: raw.common_name,
            modes: raw.modes,
            lat: raw.lat,
            lon: raw.lon,
            lines,
            hub_naptan_code: raw.hub_naptan_code.filter(|s| !s.is_empty()),
        })
    }
}

/// A station ranked by distance from a given coordinate, returned by the
/// `find_nearest_stations` IPC command.
///
/// `distance_m` is the great-circle (haversine) distance in metres from the
/// query point to the station's `lat`/`lon`. The renderer applies a 1.3×
/// fudge factor when formatting to approximate walking distance — keeping
/// the raw geodesic value in the wire type so other consumers (analytics,
/// debug overlay) can read it unmodified.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NearbyStation {
    pub station: Station,
    pub distance_m: f64,
}

/// Thin reference to a line (id + name) used inside `Station`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRef {
    pub id: String,
    pub name: String,
}

/// Map a TfL line id (kebab-case) to a human display name. Unknown ids fall
/// back to the id itself so the UI still renders something useful.
pub fn pretty_line_name(id: &str) -> &str {
    match id {
        "bakerloo" => "Bakerloo",
        "central" => "Central",
        "circle" => "Circle",
        "district" => "District",
        "elizabeth" | "elizabeth-line" => "Elizabeth",
        "hammersmith-city" => "Hammersmith & City",
        "jubilee" => "Jubilee",
        "metropolitan" => "Metropolitan",
        "northern" => "Northern",
        "piccadilly" => "Piccadilly",
        "victoria" => "Victoria",
        "waterloo-city" => "Waterloo & City",
        "dlr" => "DLR",
        "london-overground" => "Overground",
        // Six named Overground lines introduced by TfL in November 2024.
        "liberty" => "Liberty",
        "lioness" => "Lioness",
        "mildmay" => "Mildmay",
        "suffragette" => "Suffragette",
        "weaver" => "Weaver",
        "windrush" => "Windrush",
        other => other,
    }
}

/// `true` iff `id` is a TfL line id we want to surface in the UI — tube,
/// DLR, London Overground (legacy + the six named lines), and Elizabeth.
///
/// Used as a whitelist when projecting hub stop-points, whose
/// `lineModeGroups` mix bus routes, national-rail services, and our
/// supported lines into the same list (e.g. Victoria the hub has
/// `52, 390, 38, district, circle, gatwick-express, …`). Bus route
/// numbers and rail operators are rejected so the chip UI never renders
/// "52, GATWICK-EXPRESS" next to a station name.
pub fn is_supported_line_id(id: &str) -> bool {
    matches!(
        id,
        // Tube
        "bakerloo"
            | "central"
            | "circle"
            | "district"
            | "hammersmith-city"
            | "jubilee"
            | "metropolitan"
            | "northern"
            | "piccadilly"
            | "victoria"
            | "waterloo-city"
            // Elizabeth line
            | "elizabeth"
            | "elizabeth-line"
            // DLR
            | "dlr"
            // London Overground — legacy single id + the six named lines.
            | "london-overground"
            | "liberty"
            | "lioness"
            | "mildmay"
            | "suffragette"
            | "weaver"
            | "windrush"
    )
}

/// Collapse the entire London Overground family — the legacy
/// `london-overground` id and the six named lines TfL introduced in
/// November 2024 (Liberty, Lioness, Mildmay, Suffragette, Weaver, Windrush)
/// — to a single key. Every other line id is returned unchanged.
///
/// TfL is inconsistent about which Overground id form appears on which
/// endpoint: a station's hub-detail `lineModeGroups` may advertise
/// `mildmay` (or the legacy `london-overground`) while the live arrivals
/// feed tags a calling train `windrush`, and the two feeds disagree
/// station-to-station post-rename. The defensive
/// `drop_arrivals_for_lines_not_serving` filter compares an arrival's line
/// against the station's served set; a raw-string comparison drops a
/// legitimate Windrush train at a station whose metadata only listed
/// Mildmay — the user-reported "no predicted trains for the Windrush line
/// at Highbury & Islington" bug. Comparing on the family key makes that
/// mismatch impossible while still distinguishing Overground from tube /
/// DLR / Elizabeth (a `bakerloo` phantom at an Overground station is still
/// a different family and still dropped).
pub fn line_family_key(id: &str) -> &str {
    match id {
        "london-overground" | "liberty" | "lioness" | "mildmay" | "suffragette" | "weaver"
        | "windrush" => "london-overground",
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Line
// ---------------------------------------------------------------------------

/// A TfL line (tube only for now).
///
/// TODO(M2): Line enrichment from /Line endpoint for board line-colour lookup.
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
///
/// TODO(M4): LineStatus drives <LineStatusTicker/> disruption text; see M4 polling-stream spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineStatus {
    pub line_id: String,
    pub status: Vec<StatusEntry>,
    pub disruption_text: Option<String>,
    /// Active disruption time windows from TfL's `validityPeriods[]`,
    /// surfaced on the Status tab as the "Until …" chip on planned
    /// closures. `#[serde(default)]` keeps backwards compatibility with
    /// pre-existing payloads (empty Vec when omitted) so the iOS submodule
    /// bump is non-breaking for any cached serialized state.
    #[serde(default)]
    pub validity_periods: Vec<ValidityPeriod>,
}

/// A single affected route segment within a disrupted line status.
///
/// Derived from TfL's `disruption.affectedRoutes[].originationName` /
/// `destinationName`. The UI renders "from ↔ to" or "Entire line" when
/// `affected_segments` is empty.
///
/// `#[serde(default)]` on the parent `StatusEntry` field keeps iOS
/// submodule pins compiling — additive field, no old data is broken.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteSegment {
    pub from: String,
    pub to: String,
}

/// An individual severity entry within a `LineStatus`.
///
/// TODO(M4): StatusEntry is the severity row inside LineStatus; rendered by M6 ticker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusEntry {
    /// Numeric severity code from TfL (10 = Good Service).
    pub severity: i32,
    /// Human-readable severity description, e.g. `"Good Service"`.
    pub description: String,
    /// Render-tier bucket derived from `severity` via [`severity_bucket`].
    /// Computed at the wire-format seam so UI consumers never re-map raw
    /// codes — see `SeverityBucket` docs for the canonicality contract.
    /// `#[serde(default)]` keeps backwards compatibility with payloads
    /// that pre-date the field; default is computed from `severity`.
    #[serde(default = "default_status_entry_bucket")]
    pub bucket: SeverityBucket,
    /// Affected route segments from TfL's `disruption.affectedRoutes[]`.
    /// Empty when the status has no disruption (Good Service) or when TfL
    /// provides no route data. The UI renders "Entire line" in that case.
    /// `#[serde(default)]` keeps pre-existing serialized payloads (iOS
    /// submodule) compiling — additive, non-breaking.
    #[serde(default)]
    pub affected_segments: Vec<RouteSegment>,
}

fn default_status_entry_bucket() -> SeverityBucket {
    // Used only when deserializing legacy payloads that omit `bucket`.
    // The interpreted-type LineStatus is built by tfl-client which always
    // populates it, so this is a safety net for stored snapshots.
    SeverityBucket::Other
}

/// Render-tier bucket for a TfL severity code.
///
/// Encodes the contract published at https://api.tfl.gov.uk/StatusSeverity
/// (codes 0–20). This mapping is the **single canonical source** consumed
/// by every UI surface — Svelte today, future SwiftUI. UI code MUST NOT
/// redefine it; consume the `bucket` field on `StatusEntry` instead.
///
/// Sort order (via [`SeverityBucket::sort_rank`]) is "worst first":
/// Closed < PartClosure < SevereDelays < ReducedService < MinorDelays
/// < Information < Other < GoodService.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SeverityBucket {
    /// 1 (Closed), 2 (Suspended), 16 (Not Running), 20 (Service Closed).
    /// Renders with strikethrough on the Status tab.
    Closed,
    /// 3 (Part Suspended), 4 (Planned Closure), 5 (Part Closure), 11 (Part Closed).
    PartClosure,
    /// 6 (Severe Delays).
    SevereDelays,
    /// 7 (Reduced Service), 8 (Bus Service replacement), 15 (Diverted).
    ReducedService,
    /// 9 (Minor Delays), 14 (Change of Frequency).
    MinorDelays,
    /// 17 (Issues Reported), 19 (Information).
    Information,
    /// 0 (Special Service), 12 (Exit Only), 13 (No Step Free Access),
    /// plus any unrecognised future code. Treated as low-priority.
    Other,
    /// 10 (Good Service), 18 (No Issues). Sorts last.
    GoodService,
}

impl SeverityBucket {
    /// Numeric ordering key — lower = worse. Drives the worst-first
    /// sort on the Status tab. `GoodService` sorts strictly last so the
    /// "all other lines: Good Service" footer grouping is stable.
    pub fn sort_rank(&self) -> u8 {
        match self {
            SeverityBucket::Closed => 0,
            SeverityBucket::PartClosure => 1,
            SeverityBucket::SevereDelays => 2,
            SeverityBucket::ReducedService => 3,
            SeverityBucket::MinorDelays => 4,
            SeverityBucket::Information => 5,
            SeverityBucket::Other => 6,
            SeverityBucket::GoodService => 7,
        }
    }
}

/// Map a TfL numeric severity code to its render-tier bucket.
///
/// Out-of-range codes (negative or unrecognised) fall through to
/// [`SeverityBucket::Other`] so a future TfL extension never panics
/// or silently mis-categorises into a more severe tier.
pub fn severity_bucket(severity: i32) -> SeverityBucket {
    match severity {
        1 | 2 | 16 | 20 => SeverityBucket::Closed,
        3 | 4 | 5 | 11 => SeverityBucket::PartClosure,
        6 => SeverityBucket::SevereDelays,
        7 | 8 | 15 => SeverityBucket::ReducedService,
        9 | 14 => SeverityBucket::MinorDelays,
        10 | 18 => SeverityBucket::GoodService,
        17 | 19 => SeverityBucket::Information,
        _ => SeverityBucket::Other,
    }
}

/// Time window during which a `TflLineStatus` entry applies. Mirrors
/// TfL's `validityPeriods[]`. Surfaced on the Status tab as an
/// "Until …" chip on planned closures so the user knows when service
/// resumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidityPeriod {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// TfL's `isNow` flag — true when the period is the currently-active
    /// one. The client preserves this so consumers can pick the active
    /// window without re-running clock comparisons.
    pub is_now: bool,
}

// ---------------------------------------------------------------------------
// TfL wire format for LineStatus (for contract-test deserialization)
// ---------------------------------------------------------------------------

/// TfL wire format for a single line from `/Line/Mode/{mode}/Status`.
/// This is separate from the domain `LineStatus` — the client layer converts.
#[derive(Debug, Clone)]
pub struct TflLine {
    pub id: String,
    pub name: String,
    pub _type: String,
    pub line_statuses: Vec<TflLineStatus>,
}

impl<'de> Deserialize<'de> for TflLine {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawTflLine {
            id: String,
            name: String,
            #[serde(rename = "$type", default)]
            _type: String,
            #[serde(default)]
            line_statuses: Vec<TflLineStatus>,
        }
        let raw = RawTflLine::deserialize(deserializer)?;
        // Canonicalise the line id at the wire-format seam so the
        // line-status ticker lookup matches what arrivals carry. Without
        // this, post-Arrival-canonicalisation the iOS marquee would
        // silently drop the Elizabeth disruption line because the lookup
        // key (`"elizabeth"`) wouldn't match the cached TflLine id
        // (`"elizabeth-line"`).
        Ok(TflLine {
            id: canonicalize_line_id(&raw.id),
            name: raw.name,
            _type: raw._type,
            line_statuses: raw.line_statuses,
        })
    }
}

/// TfL wire format for one entry inside `lineStatuses`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflLineStatus {
    pub status_severity: i32,
    pub status_severity_description: String,
    /// Free-text reason string from TfL, often duplicating `disruption.description`.
    /// Present when there is a disruption; absent for Good Service.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub disruption: Option<TflDisruption>,
    /// TfL `validityPeriods[]` for this status entry. Drives the
    /// "Until …" chip on planned closures in the iOS Status tab. Empty
    /// for Good Service entries (TfL omits the array).
    #[serde(default)]
    pub validity_periods: Vec<TflValidityPeriod>,
}

/// TfL wire-format validity period — separate from the domain
/// [`ValidityPeriod`] because the field names differ (`fromDate`/`toDate`
/// on the wire vs. snake-case on the IPC boundary).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflValidityPeriod {
    pub from_date: DateTime<Utc>,
    pub to_date: DateTime<Utc>,
    #[serde(default)]
    pub is_now: bool,
}

/// A single route entry from TfL's `disruption.affectedRoutes[]`.
/// Maps to [`RouteSegment`] after pair-dedup in the cache layer.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflAffectedRoute {
    /// Human-readable route name (e.g. `"Watford - Aldgate"`). Informational only.
    #[serde(default)]
    pub name: String,
    /// Origin station name (e.g. `"Harrow-on-the-Hill"`).
    #[serde(default)]
    pub origination_name: String,
    /// Destination station name (e.g. `"Watford"`).
    #[serde(default)]
    pub destination_name: String,
}

/// TfL wire format for disruption info nested inside a line status.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflDisruption {
    #[serde(default)]
    pub description: String,
    /// Affected route segments from TfL's payload. May be empty when TfL
    /// provides no route data (common for Good Service). `#[serde(default)]`
    /// keeps pre-existing fixture JSON compiling — additive, non-breaking.
    #[serde(default)]
    pub affected_routes: Vec<TflAffectedRoute>,
}

// ---------------------------------------------------------------------------
// Board / Platform
// ---------------------------------------------------------------------------

/// A grouped arrivals board for a station, ready for rendering.
///
/// TODO(M4): Board is the top-level state returned by BoardService::refresh; see M4.
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
// Favorite
// ---------------------------------------------------------------------------

/// A station saved as a favorite by the user.
///
/// `lines` is a snapshot of the lines served at save time so the Favorites
/// list can render line chips even when the stop-points cache is cold.
///
/// Stored under the `"favorites"` key in the Tauri plugin-store (sibling of
/// `"board_config"`). Mutations bypass `cfg_tx` entirely — selecting a
/// favorite goes through the existing `save_config` path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Favorite {
    pub station_id: String,
    pub common_name: String,
    /// Lines served at save time (snapshotted). Used for rendering chips
    /// in the Favorites list without requiring a hot station-cache lookup.
    pub lines: Vec<LineRef>,
}

// ---------------------------------------------------------------------------
// Theme (from M6 plan — lives in tfl-domain)
// ---------------------------------------------------------------------------
// (Defined in theme.rs; re-exported from lib.rs.)
