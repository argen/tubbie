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

/// Compass direction of a train.
///
/// Serialises as a bare string (`"Northbound"`, `"Eastbound"`, …) so the
/// TypeScript `Direction` union in `web/src/lib/ipc/types.ts` is a faithful
/// mirror and `save_config` can accept the same string form it emits.
/// Northern-line branch information lives separately on `Arrival.northern_branch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    Northbound,
    Southbound,
    Eastbound,
    Westbound,
    /// Inbound on a circular / terminal line (Circle, H&C, District terminus, etc.)
    Inbound,
    /// Outbound on a circular / terminal line.
    Outbound,
    /// Graceful-degradation fallback when the TfL direction string is unrecognised. See `infer_direction` priority list.
    Unknown,
}

/// Northern line branch identifier. Lives on `Arrival.northern_branch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NorthernBranch {
    /// Via Bank / City branch.
    Bank,
    /// Via Charing Cross / West End branch.
    CharingCross,
}

// ---------------------------------------------------------------------------
// Inference helpers
// ---------------------------------------------------------------------------

/// Infer a `(Direction, Option<NorthernBranch>)` from a TfL arrival's
/// `platform_name`, `direction` field, and `line_id` / `towards` values.
///
/// Priority for the direction:
/// 1. `platform_name` prefix (most reliable — TfL sets it explicitly).
/// 2. Raw `direction` field (`"inbound"` / `"outbound"`) as a fallback.
/// 3. `Direction::Unknown` if nothing matches.
///
/// The branch is always derived from the `towards` suffix (Northern line only)
/// — returned alongside so callers can persist it on `Arrival.northern_branch`.
///
/// Exposed for integration tests and arrival enrichment; normally invoked via
/// the `Deserialize` impl on `Arrival`.
pub fn infer_direction(
    platform_name: &str,
    direction: &str,
    line_id: &str,
    towards: &str,
) -> (Direction, Option<NorthernBranch>) {
    let platform_lower = platform_name.to_ascii_lowercase();

    let northern_branch = if line_id == "northern" {
        infer_northern_branch(towards)
    } else {
        None
    };

    let dir = if platform_lower.starts_with("northbound") {
        Direction::Northbound
    } else if platform_lower.starts_with("southbound") {
        Direction::Southbound
    } else if platform_lower.starts_with("eastbound") {
        Direction::Eastbound
    } else if platform_lower.starts_with("westbound") {
        Direction::Westbound
    } else {
        match direction {
            "inbound" => Direction::Inbound,
            "outbound" => Direction::Outbound,
            _ => Direction::Unknown,
        }
    };

    (dir, northern_branch)
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
