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
fn elizabeth_line_canonical_id_eastbound_to_abbey_wood() {
    // Production: TfL hands the Elizabeth line back as
    // `line_id: "elizabeth-line"` (the canonical mode-form), NOT as the
    // bare `"elizabeth"`. Both must hit the compass mapping or real
    // arrivals leak through to the inbound/outbound fallback. This
    // assertion is what unblocks the user's iOS report (Elizabeth at
    // Liverpool Street / Farringdon / TCR was showing INBOUND/OUTBOUND).
    assert_eq!(
        dir("Platform 5", "outbound", "elizabeth-line", "Abbey Wood"),
        Direction::Eastbound
    );
}

#[test]
fn elizabeth_line_canonical_id_westbound_to_heathrow() {
    assert_eq!(
        dir(
            "Platform 4",
            "inbound",
            "elizabeth-line",
            "Heathrow Terminal 4"
        ),
        Direction::Westbound
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

// ---------------------------------------------------------------------------
// Per-line compass-axis whitelist
//
// User-reported regression at Baker Street (2026-05-01): the Hammersmith &
// City line surfaced four direction buckets — Northbound, Eastbound,
// Westbound, AND a phantom set of trains under "Northbound" with
// `towards: "Check Front of Train"`. Those are starter / unsigned trains
// physically sitting on the Met line's NB platform; TfL tags them with
// `line_id: "hammersmith-city"` and `platform_name: "Northbound -
// Platform 4"`, but H&C is an east-west line everywhere on the network
// and should never bucket as N/S.
//
// The fix gates the platform_name-prefix branch on a per-line compass-
// axis whitelist:
//   - East-west only (NB/SB rejected): hammersmith-city, circle,
//     waterloo-city, central
//   - North-south only (EB/WB rejected): bakerloo, victoria
// Lines with mixed topology (jubilee, metropolitan, district, piccadilly,
// dlr) keep accepting all four prefixes because TfL legitimately labels
// them differently at different stations.
//
// When the prefix is rejected, inference falls through to the existing
// per-line `towards`-compass mapping, then the raw `direction` field —
// so a signed H&C "Hammersmith" train still resolves to Westbound.
// ---------------------------------------------------------------------------

#[test]
fn hammersmith_city_rejects_northbound_platform_prefix() {
    // Phantom Baker Street starter: TfL tags an unsigned train sitting on
    // the Met NB platform as H&C. The platform prefix is wrong for this
    // line; we fall through to the raw direction field.
    assert_eq!(
        dir(
            "Northbound - Platform 4",
            "outbound",
            "hammersmith-city",
            "Check Front of Train"
        ),
        Direction::Outbound,
        "H&C is east-west only; the NB prefix must not bucket the train as Northbound"
    );
}

#[test]
fn hammersmith_city_keeps_westbound_platform_prefix() {
    // Real H&C platform at Baker Street: westbound to Hammersmith.
    assert_eq!(
        dir(
            "Westbound - Platform 6",
            "inbound",
            "hammersmith-city",
            "Hammersmith"
        ),
        Direction::Westbound
    );
}

#[test]
fn hammersmith_city_towards_hammersmith_resolves_westbound() {
    // Even when platform_name has no prefix, a signed Hammersmith
    // destination should map to Westbound via the per-line towards table.
    assert_eq!(
        dir("Platform 6", "inbound", "hammersmith-city", "Hammersmith"),
        Direction::Westbound
    );
}

#[test]
fn hammersmith_city_towards_barking_resolves_eastbound() {
    assert_eq!(
        dir("Platform 5", "outbound", "hammersmith-city", "Barking"),
        Direction::Eastbound
    );
}

#[test]
fn circle_rejects_southbound_platform_prefix() {
    // Circle line is east-west everywhere TfL labels it; an SB tag on a
    // Circle prediction is a TfL data quirk we don't trust.
    assert_eq!(
        dir(
            "Southbound - Platform 1",
            "inbound",
            "circle",
            "Check Front of Train"
        ),
        Direction::Inbound
    );
}

#[test]
fn waterloo_city_rejects_northbound_platform_prefix() {
    // W&C is east-west (Waterloo ↔ Bank). The wrong-axis prefix is
    // ignored; the towards-based mapping recovers the correct compass
    // (Bank → Eastbound), which is better than falling all the way
    // through to the raw `outbound` field.
    assert_eq!(
        dir(
            "Northbound - Platform 1",
            "outbound",
            "waterloo-city",
            "Bank"
        ),
        Direction::Eastbound
    );
}

#[test]
fn bakerloo_rejects_eastbound_platform_prefix() {
    // Bakerloo is north-south only (Harrow & Wealdstone N to Elephant &
    // Castle S). An EB prefix is misclassified data; with a valid
    // `towards` we still land on the correct compass direction.
    assert_eq!(
        dir(
            "Eastbound - Platform 1",
            "inbound",
            "bakerloo",
            "Harrow & Wealdstone"
        ),
        Direction::Northbound
    );
}

#[test]
fn bakerloo_keeps_northbound_platform_prefix() {
    // The legitimate case still works.
    assert_eq!(
        dir(
            "Northbound - Platform 4",
            "inbound",
            "bakerloo",
            "Harrow & Wealdstone"
        ),
        Direction::Northbound
    );
}

#[test]
fn metropolitan_keeps_northbound_platform_prefix_at_baker_street() {
    // Met IS north-south at Baker Street (Aldgate S, Amersham/Watford N),
    // and TfL legitimately labels it that way. The whitelist should NOT
    // touch Met — it has multi-axis topology elsewhere on the network.
    assert_eq!(
        dir(
            "Northbound - Platform 4",
            "inbound",
            "metropolitan",
            "Amersham"
        ),
        Direction::Northbound
    );
}

#[test]
fn jubilee_keeps_eastbound_platform_prefix() {
    // Jubilee at Stratford runs east-west; Jubilee at Baker Street is
    // labeled N/S. We allow both.
    assert_eq!(
        dir(
            "Eastbound - Platform 14",
            "inbound",
            "jubilee",
            "Stratford"
        ),
        Direction::Eastbound
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