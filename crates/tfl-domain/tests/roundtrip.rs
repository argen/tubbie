//! Round-trip deserialization tests for plan-mandated types.
//!
//! These tests cover `Line`, `LineStatus`, `StatusEntry`, and `Board` — types
//! mandated by the M1 deliverables spec but not yet consumed upstream. The
//! tests confirm the serde impls are exercised and will catch any future
//! accidental breakage of the wire format.

use chrono::{DateTime, Utc};
use tfl_domain::{
    types::{Board, Line, LineStatus, Platform, SeverityBucket, StatusEntry, ValidityPeriod},
    Direction,
};

// ---------------------------------------------------------------------------
// Line
// ---------------------------------------------------------------------------

#[test]
fn line_deserializes_from_domain_json() {
    // Line uses no rename_all — field names are the domain names (id/name/mode).
    let json = r#"{"id":"northern","name":"Northern","mode":"tube"}"#;
    let line: Line = serde_json::from_str(json).expect("Line must deserialize");
    assert_eq!(line.id, "northern");
    assert_eq!(line.name, "Northern");
    assert_eq!(line.mode, "tube");

    // Round-trip: serialize back and compare values.
    let value = serde_json::to_value(&line).expect("Line must serialize");
    let back: Line = serde_json::from_value(value).expect("Line must re-deserialize");
    assert_eq!(line, back);
}

// ---------------------------------------------------------------------------
// LineStatus
// ---------------------------------------------------------------------------

#[test]
fn line_status_deserializes() {
    // Minimal domain-format JSON (LineStatus is domain-interpreted, not TfL wire).
    // Note: pre-existing payloads omit the post-2026-05 `bucket` and
    // `validity_periods` fields; serde defaults must keep them parseable.
    let json = r#"{
        "line_id": "northern",
        "status": [
            {"severity": 6, "description": "Severe Delays"},
            {"severity": 3, "description": "Part Suspended"}
        ],
        "disruption_text": "Severe delays due to strike action."
    }"#;
    let ls: LineStatus = serde_json::from_str(json).expect("LineStatus must deserialize");
    assert_eq!(ls.line_id, "northern");
    assert_eq!(ls.status.len(), 2);
    assert_eq!(ls.status[0].severity, 6);
    assert_eq!(ls.status[0].description, "Severe Delays");
    assert_eq!(
        ls.disruption_text.as_deref(),
        Some("Severe delays due to strike action.")
    );
    // Backwards compatibility: payload without `validity_periods` defaults to empty.
    assert!(ls.validity_periods.is_empty());

    // Round-trip.
    let v = serde_json::to_value(&ls).expect("LineStatus must serialize");
    let back: LineStatus = serde_json::from_value(v).expect("LineStatus must re-deserialize");
    assert_eq!(ls, back);
}

#[test]
fn line_status_with_validity_periods_round_trips() {
    let from: DateTime<Utc> = "2026-05-04T22:00:00Z".parse().expect("valid timestamp");
    let to: DateTime<Utc> = "2026-05-05T04:30:00Z".parse().expect("valid timestamp");

    let ls = LineStatus {
        line_id: "liberty".to_string(),
        status: vec![StatusEntry {
            severity: 4,
            description: "Planned Closure".to_string(),
            bucket: SeverityBucket::PartClosure,
            affected_segments: Vec::new(),
        }],
        disruption_text: Some("Engineering work — entire line closed.".to_string()),
        validity_periods: vec![
            ValidityPeriod {
                from,
                to,
                is_now: true,
            },
            ValidityPeriod {
                from: to,
                to: "2026-05-05T05:00:00Z".parse().expect("valid timestamp"),
                is_now: false,
            },
        ],
    };

    let v = serde_json::to_value(&ls).expect("LineStatus with validity must serialize");
    let back: LineStatus =
        serde_json::from_value(v).expect("LineStatus with validity must re-deserialize");
    assert_eq!(ls, back);
    assert_eq!(back.validity_periods.len(), 2);
    assert!(back.validity_periods[0].is_now);
    assert_eq!(back.validity_periods[0].from, from);
    assert_eq!(back.validity_periods[0].to, to);
}

#[test]
fn validity_period_deserializes_from_camel_snake_round_trip() {
    let from: DateTime<Utc> = "2026-05-04T22:00:00Z".parse().expect("valid timestamp");
    let to: DateTime<Utc> = "2026-05-05T04:30:00Z".parse().expect("valid timestamp");

    let vp = ValidityPeriod {
        from,
        to,
        is_now: true,
    };
    let v = serde_json::to_value(&vp).expect("ValidityPeriod must serialize");
    // Snake-case field on the IPC boundary (matches the rest of LineStatus).
    assert_eq!(v["is_now"], serde_json::json!(true));
    let back: ValidityPeriod =
        serde_json::from_value(v).expect("ValidityPeriod must re-deserialize");
    assert_eq!(vp, back);
}

// ---------------------------------------------------------------------------
// StatusEntry
// ---------------------------------------------------------------------------

#[test]
fn status_entry_deserializes() {
    let json = r#"{"severity":10,"description":"Good Service"}"#;
    let entry: StatusEntry = serde_json::from_str(json).expect("StatusEntry must deserialize");
    assert_eq!(entry.severity, 10);
    assert_eq!(entry.description, "Good Service");
    // Backwards compatibility: legacy payloads without `bucket` get the
    // serde default (SeverityBucket::Other). Producers (the client layer)
    // populate the field explicitly so this only triggers for stored snapshots.
    assert_eq!(entry.bucket, SeverityBucket::Other);

    // Round-trip — re-serializing now emits the bucket field.
    let v = serde_json::to_value(&entry).expect("StatusEntry must serialize");
    let back: StatusEntry = serde_json::from_value(v).expect("StatusEntry must re-deserialize");
    assert_eq!(entry, back);
}

#[test]
fn status_entry_with_bucket_round_trips() {
    let entry = StatusEntry {
        severity: 6,
        description: "Severe Delays".to_string(),
        bucket: SeverityBucket::SevereDelays,
        affected_segments: Vec::new(),
    };
    let v = serde_json::to_value(&entry).expect("StatusEntry must serialize");
    assert_eq!(v["bucket"], serde_json::json!("SevereDelays"));
    let back: StatusEntry = serde_json::from_value(v).expect("StatusEntry must re-deserialize");
    assert_eq!(entry, back);
}

// ---------------------------------------------------------------------------
// Board
// ---------------------------------------------------------------------------

#[test]
fn board_round_trips() {
    let generated_at: DateTime<Utc> = "2026-04-23T16:31:48Z".parse().expect("valid timestamp");

    let board = Board {
        station_id: "940GZZLUBZP".to_string(),
        platforms: vec![Platform {
            name: "Northbound - Platform 1".to_string(),
            arrivals: vec![],
        }],
        generated_at,
        stale_since: None,
    };

    let json = serde_json::to_string(&board).expect("Board must serialize");
    let back: Board = serde_json::from_str(&json).expect("Board must deserialize");
    assert_eq!(board, back);

    // Spot-check a field to ensure it round-tripped correctly.
    assert_eq!(back.station_id, "940GZZLUBZP");
    assert_eq!(back.platforms.len(), 1);
    assert_eq!(back.platforms[0].name, "Northbound - Platform 1");
    assert!(back.stale_since.is_none());
}

// Board with stale_since set also round-trips.
#[test]
fn board_with_stale_since_round_trips() {
    let generated_at: DateTime<Utc> = "2026-04-23T16:31:48Z".parse().expect("valid timestamp");
    let stale_at: DateTime<Utc> = "2026-04-23T16:35:00Z".parse().expect("valid timestamp");

    let board = Board {
        station_id: "940GZZLUBZP".to_string(),
        platforms: vec![],
        generated_at,
        stale_since: Some(stale_at),
    };

    let json = serde_json::to_string(&board).expect("Board must serialize");
    let back: Board = serde_json::from_str(&json).expect("Board must deserialize");
    assert_eq!(board, back);
    assert_eq!(back.stale_since, Some(stale_at));
}

// ---------------------------------------------------------------------------
// Direction serde round-trip (used by Arrival inside Board)
// ---------------------------------------------------------------------------

#[test]
fn direction_unknown_round_trips() {
    let d = Direction::Unknown;
    let v = serde_json::to_value(d).expect("Direction::Unknown must serialize");
    let back: Direction =
        serde_json::from_value(v).expect("Direction::Unknown must re-deserialize");
    assert_eq!(d, back);
}
