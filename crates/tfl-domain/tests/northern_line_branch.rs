//! Northern-line branch inference tests.
//!
//! Topology source: <https://en.wikipedia.org/wiki/Northern_line>
//! and TfL Unified API fixture data (fixtures/arrivals/*.json).
//!
//! TfL encodes the branch in the `towards` field:
//! - "via CX"  / "via Charing Cross" → CharingCross branch
//! - "via Bank"                       → Bank branch
//! - Absent/other                     → None (ambiguous)

use tfl_domain::{direction::infer_direction, Direction, NorthernBranch};

/// Helper: assert a specific inferred (Direction, branch) tuple.
fn infer(platform: &str, direction: &str, towards: &str) -> (Direction, Option<NorthernBranch>) {
    infer_direction(platform, direction, "northern", towards, "")
}

#[test]
fn northbound_via_cx_edgware() {
    // Observed in fixtures/arrivals/940GZZLUBZP.json (Belsize Park)
    assert_eq!(
        infer("Northbound - Platform 1", "outbound", "Edgware via CX"),
        (Direction::Northbound, Some(NorthernBranch::CharingCross))
    );
}

#[test]
fn southbound_via_cx_battersea() {
    // Observed in fixtures/arrivals/940GZZLUBZP.json (Belsize Park)
    assert_eq!(
        infer("Southbound - Platform 2", "inbound", "Battersea via CX"),
        (Direction::Southbound, Some(NorthernBranch::CharingCross))
    );
}

#[test]
fn northbound_via_bank_high_barnet() {
    // Observed in fixtures/arrivals/940GZZLUKSX.json (King's Cross)
    assert_eq!(
        infer(
            "Northbound - Platform 7",
            "outbound",
            "High Barnet via Bank",
        ),
        (Direction::Northbound, Some(NorthernBranch::Bank))
    );
}

#[test]
fn southbound_via_bank_morden() {
    // Observed in fixtures/arrivals/940GZZLUKSX.json (King's Cross)
    assert_eq!(
        infer("Southbound - Platform 8", "inbound", "Morden via Bank"),
        (Direction::Southbound, Some(NorthernBranch::Bank))
    );
}

#[test]
fn northbound_high_barnet_no_branch_in_towards() {
    // If TfL omits the "via X" suffix, branch is None (ambiguous).
    // This can happen for engineering trains or short workings.
    assert_eq!(
        infer("Northbound - Platform 4", "outbound", "High Barnet"),
        (Direction::Northbound, None)
    );
}

#[test]
fn southbound_no_towards() {
    // Empty `towards` → branch cannot be determined.
    assert_eq!(
        infer("Southbound - Platform 3", "inbound", ""),
        (Direction::Southbound, None)
    );
}

#[test]
fn platform_prefix_overrides_direction_field() {
    // Even if the raw `direction` field says "inbound", the platform name
    // "Northbound - Platform 4" takes precedence.
    assert_eq!(
        infer("Northbound - Platform 4", "inbound", "High Barnet via Bank"),
        (Direction::Northbound, Some(NorthernBranch::Bank))
    );
}

#[test]
fn via_charing_cross_full_string() {
    // Some towards labels spell out "via Charing Cross" in full.
    assert_eq!(
        infer(
            "Northbound - Platform 1",
            "outbound",
            "Edgware via Charing Cross",
        ),
        (Direction::Northbound, Some(NorthernBranch::CharingCross))
    );
}
