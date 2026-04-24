//! Lock the Station wire shape between Rust and the Svelte frontend.
//!
//! The TypeScript `Station` interface at `web/src/lib/ipc/types.ts` consumes
//! snake_case field names (`common_name`, `line_id`, etc.). The previous
//! `#[serde(rename_all = "camelCase")]` on Station's `Serialize` silently
//! flipped them to `commonName` on the IPC wire — the frontend then read
//! `station.common_name === undefined` and rendered "No tube stations match…"
//! despite the Rust backend returning 5 valid results. This test pins the
//! correct shape so that regression cannot recur.

use tfl_domain::{LineRef, Station};

#[test]
fn station_serializes_with_snake_case_fields_for_ipc() {
    let s = Station {
        id: "940GZZLUCFM".to_string(),
        common_name: "Chalk Farm Underground Station".to_string(),
        modes: vec!["tube".to_string()],
        lat: 51.547_2,
        lon: -0.153_5,
        lines: vec![LineRef {
            id: "northern".to_string(),
            name: "Northern".to_string(),
        }],
    };

    let json = serde_json::to_value(&s).expect("Station must serialize");
    let obj = json.as_object().expect("Station must serialize as object");

    // The TS side keys off `common_name`. If this assertion fails, the search
    // dropdown will render "No tube stations match …" even when the backend
    // returns matches, because every station.common_name will be undefined on
    // the JS side.
    assert!(
        obj.contains_key("common_name"),
        "Station JSON must use snake_case `common_name`, got keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );
    assert!(
        !obj.contains_key("commonName"),
        "Station JSON must NOT use camelCase `commonName`"
    );

    // LineRef inside lines uses short names that are safe under either rename
    // rule, but pin them too so a future edit doesn't accidentally break them.
    let line = obj
        .get("lines")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_object())
        .expect("Station.lines must serialize as array of objects");
    assert!(line.contains_key("id"));
    assert!(line.contains_key("name"));
}
