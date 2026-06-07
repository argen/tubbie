//! Tests for TflDisruption.affected_routes deserialization and RouteSegment domain type.

use tfl_domain::types::{RouteSegment, TflLineStatus};

/// TfL wire JSON with a disruption that has two route entries forming a
/// reverse-duplicate pair. After domain mapping they must collapse to one
/// segment Harrow-on-the-Hill ↔ Watford.
const INLINE_JSON: &str = r#"
{
    "statusSeverity": 6,
    "statusSeverityDescription": "Severe Delays",
    "reason": "Met Line: severe delays",
    "disruption": {
        "description": "Severe delays on the Metropolitan line.",
        "affectedRoutes": [
            {
                "name": "Watford - Aldgate",
                "originationName": "Harrow-on-the-Hill",
                "destinationName": "Watford"
            },
            {
                "name": "Watford - Harrow",
                "originationName": "Watford",
                "destinationName": "Harrow-on-the-Hill"
            }
        ]
    }
}
"#;

/// The raw wire struct deserializes and exposes affected_routes.
#[test]
fn tfl_line_status_deserializes_affected_routes() {
    let status: TflLineStatus =
        serde_json::from_str(INLINE_JSON).expect("TflLineStatus must deserialize");

    let disruption = status.disruption.expect("disruption must be present");
    assert_eq!(disruption.affected_routes.len(), 2);

    let r0 = &disruption.affected_routes[0];
    assert_eq!(r0.origination_name, "Harrow-on-the-Hill");
    assert_eq!(r0.destination_name, "Watford");

    let r1 = &disruption.affected_routes[1];
    assert_eq!(r1.origination_name, "Watford");
    assert_eq!(r1.destination_name, "Harrow-on-the-Hill");
}

/// RouteSegment serializes and round-trips correctly.
#[test]
fn route_segment_round_trips() {
    let seg = RouteSegment {
        from: "Harrow-on-the-Hill".to_string(),
        to: "Watford".to_string(),
    };
    let v = serde_json::to_value(&seg).expect("RouteSegment must serialize");
    assert_eq!(v["from"], "Harrow-on-the-Hill");
    assert_eq!(v["to"], "Watford");
    let back: RouteSegment = serde_json::from_value(v).expect("RouteSegment must re-deserialize");
    assert_eq!(seg, back);
}

/// A TflLineStatus with no disruption field (Good Service) must parse and
/// produce zero affected_routes.
#[test]
fn tfl_line_status_no_disruption_has_no_routes() {
    let json = r#"
    {
        "statusSeverity": 10,
        "statusSeverityDescription": "Good Service"
    }
    "#;
    let status: TflLineStatus = serde_json::from_str(json).expect("TflLineStatus must deserialize");
    assert!(status.disruption.is_none());
}
