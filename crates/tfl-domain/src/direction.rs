//! Direction type and Northern-line branch inference.
//!
//! ## Northern Line Branch Topology
//!
//! The Northern line splits into two distinct branches north of Camden Town:
//!
//! **Via Bank branch** (eastern route through the City):
//!   Morden → ... → Stockwell → Clapham Common → ... → Oval → Kennington →
//!   Borough → London Bridge → Monument/Bank → Moorgate → Old Street →
//!   Angel → King's Cross → Euston (via Bank platform) → Camden Town →
//!   [Northern: Edgware branch] or [High Barnet branch]
//!
//! **Via Charing Cross branch** (western route through the West End):
//!   Battersea → Nine Elms → Kennington → Elephant & Castle → Waterloo →
//!   Embankment → Charing Cross → Leicester Square → Tottenham Court Road →
//!   Goodge Street → Warren Street → Euston (via CX platform) → Camden Town
//!
//! South of Kennington both branches merge. North of Camden Town:
//! - **Edgware branch**: Camden → Chalk Farm → Belsize Park → Hampstead →
//!   Golders Green → Brent Cross → Hendon Central → Brent Cross → Edgware
//! - **High Barnet branch**: Camden → Archway → Highgate → East Finchley →
//!   Finchley Central → West Finchley → Woodside Park → Totteridge → Whetstone →
//!   High Barnet; also Mill Hill East spur
//!
//! ## Branch Inference from `towards`
//!
//! TfL encodes the branch in the `towards` field using one of these suffixes:
//! - `"via CX"` or `"via Charing Cross"` → `NorthernBranch::CharingCross`
//! - `"via Bank"` → `NorthernBranch::Bank`
//!
//! If neither suffix is present, `via` is `None` (ambiguous — e.g., short
//! workings or engineering trains). See ADR `northern-line-branch-inference.md`.
//!
//! Sources:
//! - <https://en.wikipedia.org/wiki/Northern_line>
//! - TfL fixture data: fixtures/arrivals/*.json
//! - TfL Unified API `towards` field observed values:
//!   "Edgware via CX", "Battersea via CX", "High Barnet via Bank", "Morden via Bank"

use serde::{Deserialize, Serialize};

/// Compass direction of a train, capturing Northern-line branch where relevant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "via")]
pub enum Direction {
    /// Northbound service.
    /// `via` is populated for Northern line trains where the branch is known.
    Northbound {
        via: Option<NorthernBranch>,
    },
    /// Southbound service.
    /// `via` is populated for Northern line trains where the branch is known.
    Southbound {
        via: Option<NorthernBranch>,
    },
    Eastbound,
    Westbound,
    /// Inbound on a circular / terminal line (Circle, H&C, District terminus, etc.)
    Inbound,
    /// Outbound on a circular / terminal line.
    Outbound,
    /// Direction unknown (field absent or unrecognised from TfL).
    Unknown,
}

/// Northern line branch identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NorthernBranch {
    /// Via Bank / City branch.
    Bank,
    /// Via Charing Cross / West End branch.
    CharingCross,
}

impl Direction {
    /// Derive the display label used on real TfL boards.
    pub fn label(&self) -> &str {
        match self {
            Direction::Northbound { .. } => "Northbound",
            Direction::Southbound { .. } => "Southbound",
            Direction::Eastbound => "Eastbound",
            Direction::Westbound => "Westbound",
            Direction::Inbound => "Inbound",
            Direction::Outbound => "Outbound",
            Direction::Unknown => "Unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// Inference helpers
// ---------------------------------------------------------------------------

/// Infer a `Direction` from a TfL arrival's `platform_name`, `direction` field,
/// and `line_id` / `towards` values.
///
/// Priority:
/// 1. `platform_name` prefix (most reliable — TfL sets it explicitly).
/// 2. Northern-line branch from `towards` suffix when `line_id == "northern"`.
/// 3. Raw `direction` field (`"inbound"` / `"outbound"`) as a fallback.
/// 4. `Direction::Unknown` if nothing matches.
pub fn infer_direction(
    platform_name: &str,
    direction: &str,
    line_id: &str,
    towards: &str,
) -> Direction {
    // Platform name prefix is authoritative.
    let platform_lower = platform_name.to_ascii_lowercase();

    // For Northern line, read branch from the `towards` suffix before anything else.
    let northern_branch = if line_id == "northern" {
        infer_northern_branch(towards)
    } else {
        None
    };

    if platform_lower.starts_with("northbound") {
        return Direction::Northbound {
            via: northern_branch,
        };
    }
    if platform_lower.starts_with("southbound") {
        return Direction::Southbound {
            via: northern_branch,
        };
    }
    if platform_lower.starts_with("eastbound") {
        return Direction::Eastbound;
    }
    if platform_lower.starts_with("westbound") {
        return Direction::Westbound;
    }

    // Fall back to TfL's raw `direction` field.
    match direction {
        "inbound" => Direction::Inbound,
        "outbound" => Direction::Outbound,
        _ => Direction::Unknown,
    }
}

/// Infer the Northern line branch from the `towards` label.
///
/// Returns `None` when the branch cannot be determined from the string alone.
///
/// Observed values from TfL fixtures:
/// - `"Edgware via CX"` → `CharingCross`
/// - `"Battersea via CX"` → `CharingCross`
/// - `"High Barnet via Bank"` → `Bank`
/// - `"Morden via Bank"` → `Bank`
fn infer_northern_branch(towards: &str) -> Option<NorthernBranch> {
    let lower = towards.to_ascii_lowercase();
    if lower.contains("via bank") {
        Some(NorthernBranch::Bank)
    } else if lower.contains("via cx") || lower.contains("via charing cross") {
        Some(NorthernBranch::CharingCross)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Serde round-trip support for Arrival deserialization
// ---------------------------------------------------------------------------
//
// When TfL JSON is deserialized into `Arrival`, the `direction` field holds
// a raw string (`"inbound"` / `"outbound"`) that doesn't match `Direction`'s
// enum variants. The client layer (M2) is responsible for calling
// `infer_direction` and replacing the raw value with the rich type.
//
// For M1 we provide a `from_tfl_raw` constructor so the contract tests can
// parse the raw `direction` string and convert it in one step.

impl Direction {
    /// Construct a `Direction` from the raw TfL `direction` string, without
    /// any platform or `towards` context. Useful for simple round-trip tests.
    pub fn from_raw(s: &str) -> Self {
        match s {
            "inbound" => Direction::Inbound,
            "outbound" => Direction::Outbound,
            _ => Direction::Unknown,
        }
    }
}
