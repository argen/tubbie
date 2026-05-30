//! `line_family_key` collapses the whole London Overground family — the
//! legacy `london-overground` id and the six named lines (Liberty, Lioness,
//! Mildmay, Suffragette, Weaver, Windrush) — to a single key, while leaving
//! every other line id untouched.
//!
//! Why this exists: TfL is inconsistent about which Overground id form
//! appears on which endpoint. A station's hub-detail `lineModeGroups` might
//! advertise `mildmay` while the live arrivals feed tags a train `windrush`
//! (or a station advertises the legacy `london-overground` while arrivals
//! carry a named line). The defensive `drop_arrivals_for_lines_not_serving`
//! filter compares an arrival's line against the station's served set; a raw
//! string comparison would drop a legitimate Windrush train at a station
//! whose metadata only listed Mildmay. Folding the family to one key makes
//! that mismatch impossible while still distinguishing Overground from tube /
//! DLR / Elizabeth.

use tfl_domain::line_family_key;

#[test]
fn line_family_key_groups_named_overground_with_legacy() {
    let family = "london-overground";
    for id in [
        "london-overground",
        "liberty",
        "lioness",
        "mildmay",
        "suffragette",
        "weaver",
        "windrush",
    ] {
        assert_eq!(
            line_family_key(id),
            family,
            "{id} must map to the shared Overground family key"
        );
    }
}

#[test]
fn line_family_key_leaves_non_overground_ids_unchanged() {
    for id in [
        "northern",
        "victoria",
        "central",
        "bakerloo",
        "dlr",
        "elizabeth",
        "jubilee",
    ] {
        assert_eq!(
            line_family_key(id),
            id,
            "{id} is not Overground and must be returned unchanged for an exact comparison"
        );
    }
}
