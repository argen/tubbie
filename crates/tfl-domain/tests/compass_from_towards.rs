//! Compass-direction inference from `towards` for lines whose platforms
//! TfL labels as bare `"Platform N"` (Elizabeth + the six named
//! Overground lines).
//!
//! The user-visible bug these tests pin: at multi-line stations like
//! Liverpool Street, Farringdon, and Tottenham Court Road, Elizabeth-line
//! arrivals showed up as `Inbound` / `Outbound` instead of the geographically
//! correct `Eastbound` / `Westbound`. Same shape on the Overground six.
//!
//! Tube lines are unaffected — their platform names already carry a
//! compass prefix, so the prefix branch in `infer_direction` fires before
//! we reach the new `infer_compass_from_towards` step.

use tfl_domain::{direction::infer_direction, Direction};

fn dir(platform: &str, raw_direction: &str, line_id: &str, towards: &str) -> Direction {
    infer_direction(platform, raw_direction, line_id, towards).0
}

// ---------------------------------------------------------------------------
// Elizabeth — east-west across central London
// ---------------------------------------------------------------------------

#[test]
fn elizabeth_eastbound_to_abbey_wood() {
    // Liverpool Street platform 5 (typical eastbound bay).
    assert_eq!(
        dir("Platform 5", "outbound", "elizabeth", "Abbey Wood"),
        Direction::Eastbound
    );
}

#[test]
fn elizabeth_eastbound_to_shenfield() {
    assert_eq!(
        dir("Platform 5", "outbound", "elizabeth", "Shenfield"),
        Direction::Eastbound
    );
}

#[test]
fn elizabeth_eastbound_to_stratford() {
    // Short-working terminating at Stratford.
    assert_eq!(
        dir("Platform 5", "outbound", "elizabeth", "Stratford"),
        Direction::Eastbound
    );
}

#[test]
fn elizabeth_westbound_to_paddington() {
    assert_eq!(
        dir("Platform 4", "inbound", "elizabeth", "Paddington"),
        Direction::Westbound
    );
}

#[test]
fn elizabeth_westbound_to_heathrow_terminal_5() {
    assert_eq!(
        dir("Platform 4", "inbound", "elizabeth", "Heathrow Terminal 5"),
        Direction::Westbound
    );
}

#[test]
fn elizabeth_westbound_to_reading() {
    assert_eq!(
        dir("Platform 4", "inbound", "elizabeth", "Reading"),
        Direction::Westbound
    );
}

#[test]
fn elizabeth_westbound_to_hayes_and_harlington() {
    assert_eq!(
        dir("Platform 4", "inbound", "elizabeth", "Hayes & Harlington"),
        Direction::Westbound
    );
}

#[test]
fn elizabeth_unknown_terminus_falls_back_to_inbound() {
    // Some peculiar destination we haven't enumerated → keep the raw
    // direction rather than guess wrong.
    assert_eq!(
        dir("Platform 3", "inbound", "elizabeth", "Network Rail Sidings"),
        Direction::Inbound
    );
}

// ---------------------------------------------------------------------------
// Mildmay (NLL) — Stratford east, Richmond / Clapham Junction west
// ---------------------------------------------------------------------------

#[test]
fn mildmay_eastbound_to_stratford() {
    assert_eq!(
        dir("Platform 1", "outbound", "mildmay", "Stratford"),
        Direction::Eastbound
    );
}

#[test]
fn mildmay_westbound_to_richmond() {
    assert_eq!(
        dir("Platform 2", "inbound", "mildmay", "Richmond"),
        Direction::Westbound
    );
}

#[test]
fn mildmay_westbound_to_clapham_junction() {
    assert_eq!(
        dir("Platform 2", "inbound", "mildmay", "Clapham Junction"),
        Direction::Westbound
    );
}

// ---------------------------------------------------------------------------
// Lioness (Watford DC) — Watford Junction north, Euston south
// ---------------------------------------------------------------------------

#[test]
fn lioness_northbound_to_watford_junction() {
    assert_eq!(
        dir("Platform 9", "outbound", "lioness", "Watford Junction"),
        Direction::Northbound
    );
}

#[test]
fn lioness_southbound_to_euston() {
    assert_eq!(
        dir("Platform 8", "inbound", "lioness", "Euston"),
        Direction::Southbound
    );
}

// ---------------------------------------------------------------------------
// Suffragette (GOBLIN) — Barking east, Gospel Oak west
// ---------------------------------------------------------------------------

#[test]
fn suffragette_eastbound_to_barking_riverside() {
    assert_eq!(
        dir("Platform 1", "outbound", "suffragette", "Barking Riverside"),
        Direction::Eastbound
    );
}

#[test]
fn suffragette_westbound_to_gospel_oak() {
    assert_eq!(
        dir("Platform 2", "inbound", "suffragette", "Gospel Oak"),
        Direction::Westbound
    );
}

// ---------------------------------------------------------------------------
// Weaver — Cheshunt / Enfield Town / Chingford north, Liverpool Street south
// ---------------------------------------------------------------------------

#[test]
fn weaver_northbound_to_chingford() {
    assert_eq!(
        dir("Platform 1", "outbound", "weaver", "Chingford"),
        Direction::Northbound
    );
}

#[test]
fn weaver_northbound_to_enfield_town() {
    assert_eq!(
        dir("Platform 1", "outbound", "weaver", "Enfield Town"),
        Direction::Northbound
    );
}

#[test]
fn weaver_southbound_to_liverpool_street() {
    assert_eq!(
        dir("Platform 2", "inbound", "weaver", "Liverpool Street"),
        Direction::Southbound
    );
}

// ---------------------------------------------------------------------------
// Windrush (East London Line) — Highbury / Dalston north, New Cross / Crystal
// Palace / West Croydon / Clapham Junction south
// ---------------------------------------------------------------------------

#[test]
fn windrush_northbound_to_highbury_and_islington() {
    assert_eq!(
        dir("Platform 1", "outbound", "windrush", "Highbury & Islington"),
        Direction::Northbound
    );
}

#[test]
fn windrush_northbound_to_dalston_junction() {
    assert_eq!(
        dir("Platform 1", "outbound", "windrush", "Dalston Junction"),
        Direction::Northbound
    );
}

#[test]
fn windrush_southbound_to_new_cross() {
    assert_eq!(
        dir("Platform 2", "inbound", "windrush", "New Cross"),
        Direction::Southbound
    );
}

#[test]
fn windrush_southbound_to_crystal_palace() {
    assert_eq!(
        dir("Platform 2", "inbound", "windrush", "Crystal Palace"),
        Direction::Southbound
    );
}

#[test]
fn windrush_southbound_to_clapham_junction() {
    // Same terminus name as Mildmay but the per-line `match` keeps it
    // disambiguated — Windrush's branch to Clapham Junction is southbound.
    assert_eq!(
        dir("Platform 2", "inbound", "windrush", "Clapham Junction"),
        Direction::Southbound
    );
}

// ---------------------------------------------------------------------------
// Liberty — Romford ↔ Upminster shuttle (E-W)
// ---------------------------------------------------------------------------

#[test]
fn liberty_eastbound_to_upminster() {
    assert_eq!(
        dir("Platform 1", "outbound", "liberty", "Upminster"),
        Direction::Eastbound
    );
}

#[test]
fn liberty_westbound_to_romford() {
    assert_eq!(
        dir("Platform 2", "inbound", "liberty", "Romford"),
        Direction::Westbound
    );
}

// ---------------------------------------------------------------------------
// Tube lines are unchanged: the platform-name prefix wins, and the
// `towards`-based mapping never gets a chance to corrupt them.
// ---------------------------------------------------------------------------

#[test]
fn tube_central_eastbound_unchanged() {
    // Platform name has the prefix; raw direction is "outbound" (TfL's
    // wire format). We must still return Eastbound, not be tempted by
    // the new `towards` step.
    assert_eq!(
        dir("Eastbound - Platform 6", "outbound", "central", "Stratford"),
        Direction::Eastbound
    );
}

#[test]
fn dlr_unmapped_falls_back_to_inbound_outbound() {
    // DLR is intentionally not in the per-line table; the raw direction
    // field still drives the result. (If DLR users complain later, this
    // test will flip to a compass mapping.)
    assert_eq!(
        dir("Platform 3", "inbound", "dlr", "Bank"),
        Direction::Inbound
    );
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn empty_towards_falls_back_to_raw_direction_field() {
    // No `towards` data → can't map; keep the raw inbound/outbound
    // rather than collapse to Unknown.
    assert_eq!(
        dir("Platform 3", "inbound", "elizabeth", ""),
        Direction::Inbound
    );
}

#[test]
fn case_insensitive_match() {
    // TfL has been observed to lowercase or title-case the same value
    // across endpoints; the matcher must be case-insensitive.
    assert_eq!(
        dir("Platform 5", "outbound", "elizabeth", "ABBEY WOOD"),
        Direction::Eastbound
    );
    assert_eq!(
        dir("Platform 5", "outbound", "elizabeth", "abbey wood"),
        Direction::Eastbound
    );
}
