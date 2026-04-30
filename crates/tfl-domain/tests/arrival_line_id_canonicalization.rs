//! `Arrival` deserialization MUST canonicalise TfL's mode-form line ids
//! to the line-form used by station metadata and the user's chip filter.
//!
//! TfL ships Elizabeth-line arrivals with `lineId: "elizabeth-line"`
//! (the mode), but `Station.lines[].id == "elizabeth"` (the line
//! identifier from `lineIdentifier`). The user's saved `line_ids` chip
//! list is built from `Station.lines`, so without canonicalisation the
//! frontend filter `lineIds.includes(arrival.line_id)` never matches
//! and the board collapses to "No platforms to display".
//!
//! This test pins the wire-format → canonical mapping so we can't
//! regress.

use tfl_domain::Arrival;

fn arrival_json(line_id: &str) -> String {
    format!(
        r#"{{
            "$type": "Tfl.Api.Presentation.Entities.Prediction, Tfl.Api.Presentation.Entities",
            "id": "1",
            "stationName": "Liverpool Street",
            "platformName": "Platform 5",
            "lineId": "{line_id}",
            "lineName": "Elizabeth",
            "direction": "outbound",
            "destinationName": "Abbey Wood Underground Station",
            "towards": "Abbey Wood",
            "currentLocation": "On schedule",
            "timeToStation": 60,
            "expectedArrival": "2026-04-30T08:00:00Z",
            "naptanId": "910GLIVPLST"
        }}"#
    )
}

#[test]
fn arrival_canonicalises_elizabeth_line_to_elizabeth() {
    // TfL hands the live Elizabeth payload back with `lineId: "elizabeth-line"`
    // (the mode form). After deserialization the `line_id` MUST equal
    // `"elizabeth"` so it matches `Station.lines[].id` and the user's
    // saved chip filter.
    let json = arrival_json("elizabeth-line");
    let arrival: Arrival = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(arrival.line_id, "elizabeth");
}

#[test]
fn arrival_passes_already_canonical_elizabeth_through_unchanged() {
    let json = arrival_json("elizabeth");
    let arrival: Arrival = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(arrival.line_id, "elizabeth");
}

#[test]
fn arrival_does_not_touch_unrelated_line_ids() {
    // Tube and Overground line ids are already canonical at the source.
    for raw in [
        "central",
        "northern",
        "victoria",
        "mildmay",
        "windrush",
        "weaver",
        "lioness",
        "suffragette",
        "liberty",
        "dlr",
    ] {
        let json = arrival_json(raw);
        let arrival: Arrival = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            arrival.line_id, raw,
            "unrelated line id {raw} must pass through unchanged",
        );
    }
}
