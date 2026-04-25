//! Pin the supported-modes whitelist in Station's `lineModeGroups` projection.
//!
//! User-reported regression: Victoria's hub record lists
//! `["52", "390", "38", "district", "circle", "gatwick-express", "southern",
//!   "3", "southeastern", "victoria", "thameslink", "N38"]`
//! under `lineIdentifier`. Without a whitelist those non-tube ids flow into
//! `Station.lines` and the Settings chip UI ends up rendering bus route
//! numbers next to the station.
//!
//! Whitelist coverage: tube + DLR + London Overground (legacy + the six
//! named lines introduced Nov 2024) + Elizabeth.

use tfl_domain::{is_supported_line_id, Station};

#[test]
fn is_supported_line_id_accepts_every_surfaced_line() {
    for id in [
        // Tube
        "bakerloo",
        "central",
        "circle",
        "district",
        "hammersmith-city",
        "jubilee",
        "metropolitan",
        "northern",
        "piccadilly",
        "victoria",
        "waterloo-city",
        // Elizabeth
        "elizabeth",
        "elizabeth-line",
        // DLR
        "dlr",
        // Overground
        "london-overground",
        "liberty",
        "lioness",
        "mildmay",
        "suffragette",
        "weaver",
        "windrush",
    ] {
        assert!(
            is_supported_line_id(id),
            "expected {id:?} to be classified as a supported line"
        );
    }
}

#[test]
fn is_supported_line_id_rejects_bus_rail_and_hub_ids() {
    for id in [
        "52",
        "390",
        "38",
        "N38",
        "3",
        "gatwick-express",
        "southern",
        "southeastern",
        "thameslink",
        "c2c",
    ] {
        assert!(
            !is_supported_line_id(id),
            "non-supported id {id:?} must NOT pass the whitelist"
        );
    }
}

#[test]
fn station_deserialize_strips_non_tube_ids_from_line_mode_groups() {
    // Shape of TfL's hub records: a single lineModeGroups entry with no
    // modeName but a mixed lineIdentifier list containing tube + bus + rail.
    let json = serde_json::json!({
        "id": "HUBVIC",
        "commonName": "Victoria",
        "modes": ["tube", "bus", "national-rail"],
        "lat": 51.495,
        "lon": -0.144,
        "lineModeGroups": [{
            "lineIdentifier": [
                "52", "390", "38",
                "district", "circle", "victoria",
                "gatwick-express", "southern",
                "3", "southeastern", "thameslink", "N38"
            ]
        }]
    });

    let s: Station = serde_json::from_value(json).expect("Station must parse");
    let ids: Vec<&str> = s.lines.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["district", "circle", "victoria"],
        "only tube line ids must survive the whitelist"
    );
}

#[test]
fn station_deserialize_filters_explicit_lines_array_too() {
    // Backward-compat with older fixtures / hypothetical future API: if
    // the response already has a processed `lines` field, the same filter
    // applies so callers never see a bus route sneaking through.
    let json = serde_json::json!({
        "id": "940GZZLUVIC",
        "commonName": "Victoria Underground Station",
        "modes": ["tube"],
        "lat": 51.495,
        "lon": -0.144,
        "lines": [
            { "id": "victoria", "name": "Victoria" },
            { "id": "52",       "name": "52" },
            { "id": "district", "name": "District" }
        ]
    });

    let s: Station = serde_json::from_value(json).expect("Station must parse");
    let ids: Vec<&str> = s.lines.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, vec!["victoria", "district"]);
}
