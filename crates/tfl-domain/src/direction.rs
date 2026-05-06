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
/// 1. `platform_name` prefix (most reliable — TfL sets it explicitly on
///    every tube line: `"Northbound - Platform 4"`).
/// 2. Per-line `towards`-terminus compass mapping (Elizabeth and the six
///    named Overground lines): TfL labels their platforms as bare
///    `"Platform 3"` so the prefix branch never fires, and the raw
///    `direction` field only carries `inbound|outbound`. Mapping the
///    terminus name to a compass direction recovers the user-natural
///    label (Eastbound/Westbound on Elizabeth at Farringdon, Northbound/
///    Southbound on Lioness, …).
/// 3. Raw `direction` field (`"inbound"` / `"outbound"`) as a fallback.
/// 4. `Direction::Unknown` if nothing matches.
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
    destination_name: &str,
) -> (Direction, Option<NorthernBranch>) {
    let platform_lower = platform_name.to_ascii_lowercase();

    let northern_branch = if line_id == "northern" {
        infer_northern_branch(towards)
    } else {
        None
    };

    let allow_ns = line_allows_north_south(line_id);
    let allow_ew = line_allows_east_west(line_id);

    // TfL's live `/Arrivals` endpoint leaves `towards` empty for many
    // Elizabeth and Overground predictions at hub stations (verified at
    // Liverpool Street's `910GLIVST` on 2026-05-06: every Elizabeth /
    // Weaver entry had `towards: ""`). `destinationName` is the more
    // robust signal for those cases — fall back to it when `towards`
    // alone gives us nothing. Without this, every Elizabeth prediction
    // resolved to `Direction::Inbound` / `Outbound` and was then
    // silently dropped by `drop_off_axis_predictions` (Elizabeth is
    // pinned to `EastWest` in `line_compass_axis`), producing the user
    // symptom "no Elizabeth at Liverpool Street".
    let towards_compass = infer_compass_from_towards(line_id, towards)
        .or_else(|| infer_compass_from_towards(line_id, destination_name));

    let dir = if platform_lower.starts_with("northbound") && allow_ns {
        Direction::Northbound
    } else if platform_lower.starts_with("southbound") && allow_ns {
        Direction::Southbound
    } else if platform_lower.starts_with("eastbound") && allow_ew {
        Direction::Eastbound
    } else if platform_lower.starts_with("westbound") && allow_ew {
        Direction::Westbound
    } else if let Some(compass) = towards_compass {
        compass
    } else {
        match direction {
            "inbound" => Direction::Inbound,
            "outbound" => Direction::Outbound,
            _ => Direction::Unknown,
        }
    };

    (dir, northern_branch)
}

/// Compass axis a TfL line is constrained to, when the topology is
/// uniform across the network. `None` means the line legitimately
/// labels its platforms differently at different stations (Met line
/// at Baker Street is N/S but at Watford is E/W; Jubilee at Stratford
/// is E, Stanmore is NW; Piccadilly, District, DLR — all multi-axis).
///
/// Used at refresh time to drop predictions that resolved to an
/// off-axis bucket (Inbound/Outbound on an E/W-only line, etc.).
/// Those are typically unsigned "Check Front of Train" starter trains
/// physically sitting on a different line's platform; without this
/// filter they pollute the line group with a third direction bucket
/// that the user can't act on. See user-reported regression at Baker
/// Street, 2026-05-01: H&C surfaced EB + WB + a phantom Inbound /
/// Outbound bucket from such trains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompassAxis {
    EastWest,
    NorthSouth,
}

/// Strict compass axis for a line, or `None` when the line spans
/// multiple axes on the network. Source of truth shared by the
/// platform-prefix gate in [`infer_direction`] and the off-axis
/// arrival filter in `tfl-board::service`.
pub fn line_compass_axis(line_id: &str) -> Option<CompassAxis> {
    match line_id {
        "hammersmith-city" | "circle" | "waterloo-city" | "central" | "elizabeth"
        | "elizabeth-line" => Some(CompassAxis::EastWest),
        "bakerloo" | "victoria" => Some(CompassAxis::NorthSouth),
        _ => None,
    }
}

/// Whether a TfL line legitimately runs along the north-south axis
/// somewhere on the network (and therefore TfL may emit a `"Northbound"` /
/// `"Southbound"` platform_name prefix that we should trust).
///
/// Lines listed here as `false` are east-west everywhere TfL labels them;
/// an N/S platform_name on a prediction for one of these lines is a TfL
/// data quirk (typically a starter / unsigned train physically sitting
/// on a different line's platform), so we ignore the prefix and let
/// `infer_direction` fall through to the towards-based or raw-direction
/// branches. See user-reported regression at Baker Street, 2026-05-01:
/// H&C surfaced a phantom Northbound bucket alongside its real EB/WB.
fn line_allows_north_south(line_id: &str) -> bool {
    !matches!(
        line_id,
        "hammersmith-city"
            | "circle"
            | "waterloo-city"
            | "central"
            | "elizabeth"
            | "elizabeth-line"
    )
}

/// Mirror of [`line_allows_north_south`] for the east-west axis. Lines
/// listed as `false` are north-south only, and an E/W platform prefix on
/// such a prediction would be a misclassification.
fn line_allows_east_west(line_id: &str) -> bool {
    !matches!(line_id, "bakerloo" | "victoria")
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

/// Infer a compass `Direction` from the `(line_id, towards)` pair when the
/// `platform_name` prefix didn't supply one.
///
/// TfL labels Elizabeth and Overground platforms as bare `"Platform 3"`,
/// so the prefix path in `infer_direction` falls through and the only
/// remaining signal is the raw `direction` field — which TfL hands back
/// as `"inbound"` / `"outbound"`. That's correct on the wire but visually
/// jarring at a station where the line clearly runs east-west (Liverpool
/// Street, Farringdon, Tottenham Court Road on Elizabeth). Mapping the
/// terminus name to a compass direction recovers the label users expect.
///
/// Per-line, the mapping covers TfL's published termini and common
/// short-working / branch terminations. Ambiguous cases (e.g. mid-route
/// terminations like `"Liverpool Street"` on the Elizabeth line, where
/// the same string could be either heading) intentionally return `None`
/// and fall back to inbound/outbound — better to keep TfL's raw label
/// than guess wrong.
///
/// DLR is intentionally not mapped: its multi-branch topology
/// (Bank/Tower Gateway westwards, Stratford north, Lewisham south,
/// Beckton/Woolwich Arsenal east) does not fit a single per-terminus
/// compass mapping and the user hasn't reported it as a problem.
pub(crate) fn infer_compass_from_towards(line_id: &str, towards: &str) -> Option<Direction> {
    let lower = towards.to_ascii_lowercase();
    if lower.trim().is_empty() {
        return None;
    }
    let any = |needles: &[&str]| -> bool { needles.iter().any(|n| lower.contains(n)) };

    match line_id {
        // East: Stratford / Shenfield / Abbey Wood / Gidea Park / Romford
        // West: Paddington / Heathrow / Reading / Maidenhead /
        //       Hayes & Harlington / West Drayton / Ealing
        //
        // The TfL API surfaces Elizabeth-line arrivals with `line_id`
        // `"elizabeth-line"` (the canonical mode form), but
        // `is_supported_line_id` also accepts the bare `"elizabeth"` form
        // — match both so a config or upstream variant doesn't silently
        // bypass the compass mapping. Verified live: in production, real
        // arrivals at Liverpool Street / Farringdon / Tottenham Court
        // Road carry `line_id == "elizabeth-line"`.
        "elizabeth" | "elizabeth-line" => {
            if any(&[
                "abbey wood",
                "shenfield",
                "stratford",
                "gidea park",
                "romford",
            ]) {
                Some(Direction::Eastbound)
            } else if any(&[
                "paddington",
                "heathrow",
                "reading",
                "maidenhead",
                "hayes",
                "west drayton",
                "west ealing",
                "ealing broadway",
            ]) {
                Some(Direction::Westbound)
            } else {
                None
            }
        }

        // Mildmay (NLL): Stratford ↔ Richmond / Clapham Junction. E-W.
        "mildmay" => {
            if lower.contains("stratford") {
                Some(Direction::Eastbound)
            } else if any(&["richmond", "clapham junction"]) {
                Some(Direction::Westbound)
            } else {
                None
            }
        }

        // Lioness (Watford DC): Watford Junction ↔ Euston. N-S.
        "lioness" => {
            if lower.contains("watford") {
                Some(Direction::Northbound)
            } else if lower.contains("euston") {
                Some(Direction::Southbound)
            } else {
                None
            }
        }

        // Suffragette (GOBLIN): Gospel Oak ↔ Barking / Barking Riverside. E-W.
        "suffragette" => {
            if lower.contains("barking") {
                Some(Direction::Eastbound)
            } else if lower.contains("gospel oak") {
                Some(Direction::Westbound)
            } else {
                None
            }
        }

        // Weaver: Liverpool Street ↔ Cheshunt / Enfield Town / Chingford. N-S.
        "weaver" => {
            if any(&["cheshunt", "enfield town", "chingford"]) {
                Some(Direction::Northbound)
            } else if lower.contains("liverpool street") {
                Some(Direction::Southbound)
            } else {
                None
            }
        }

        // Windrush (East London Line): Highbury & Islington / Dalston Junction
        // ↔ New Cross / New Cross Gate / Crystal Palace / West Croydon /
        // Clapham Junction. N-S.
        "windrush" => {
            if any(&["highbury", "dalston"]) {
                Some(Direction::Northbound)
            } else if any(&[
                "new cross",
                "crystal palace",
                "west croydon",
                "clapham junction",
            ]) {
                Some(Direction::Southbound)
            } else {
                None
            }
        }

        // Liberty: Romford ↔ Upminster shuttle. E-W.
        "liberty" => {
            if lower.contains("upminster") {
                Some(Direction::Eastbound)
            } else if lower.contains("romford") {
                Some(Direction::Westbound)
            } else {
                None
            }
        }

        // Hammersmith & City: Hammersmith ↔ Barking. East-west everywhere.
        // Common short workings: Whitechapel, Plaistow, East Ham,
        // Edgware Road (as a westbound terminus).
        "hammersmith-city" => {
            if any(&["barking", "plaistow", "east ham", "whitechapel"]) {
                Some(Direction::Eastbound)
            } else if any(&["hammersmith", "edgware road"]) {
                Some(Direction::Westbound)
            } else {
                None
            }
        }

        // Circle: looped E-W line via Edgware Road. Termini in TfL's API
        // are mostly Edgware Road and Hammersmith (when running clockwise
        // / anticlockwise via Aldgate). We map the common short-working
        // termini, leaving ambiguous cases (e.g. unsigned trains) to fall
        // through to inbound/outbound.
        "circle" => {
            if any(&["aldgate", "tower hill", "liverpool street"]) {
                Some(Direction::Eastbound)
            } else if any(&["hammersmith", "edgware road", "paddington"]) {
                Some(Direction::Westbound)
            } else {
                None
            }
        }

        // Waterloo & City: Waterloo (S/W) ↔ Bank (E). Only two stops.
        "waterloo-city" => {
            if lower.contains("bank") {
                Some(Direction::Eastbound)
            } else if lower.contains("waterloo") {
                Some(Direction::Westbound)
            } else {
                None
            }
        }

        // Bakerloo: Harrow & Wealdstone (N) ↔ Elephant & Castle (S).
        // Common short workings: Stonebridge Park, Queen's Park, Willesden
        // Junction (all northbound termini).
        "bakerloo" => {
            if any(&[
                "harrow & wealdstone",
                "harrow and wealdstone",
                "stonebridge park",
                "queen's park",
                "queens park",
                "willesden junction",
            ]) {
                Some(Direction::Northbound)
            } else if lower.contains("elephant") {
                Some(Direction::Southbound)
            } else {
                None
            }
        }

        _ => None,
    }
}
