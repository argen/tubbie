//! Pure ranking helpers for the "find nearest station" feature.
//!
//! Lives at the client layer (not domain) because it operates on the same
//! `Vec<Station>` shape that the stop-points cache holds. Nothing here
//! touches the network, the cache, or the filesystem — `rank_nearest`
//! takes an iterator of stations and returns a ranked subset, period.
//!
//! ## Why haversine and a `MAX_RADIUS_M`
//!
//! The TfL stop-points cache is the entire London network. Without a
//! radius cap, a query from Paris would happily return Heathrow as the
//! "nearest" — useless to the user, and the listbox becomes a permanent
//! 8-row dead end. 25 km comfortably covers the network's outer reach
//! (Amersham, Chesham, Upminster, Chesham, Epping); 50 km lands in
//! Reading and would happily surface stations the user can't actually
//! get to.
//!
//! ## What this module deliberately does NOT do
//!
//! - It does NOT apply the NaPTAN-prefix whitelist or the hub dedupe.
//!   That is `whitelist_and_dedupe` in `client.rs`, run *before*
//!   `rank_nearest` so the user never sees `940GZZLUBNK` and
//!   `940GZZDLBNK` as two separate "Bank" rows ranked 1st and 2nd.
//! - It does NOT apply the 1.3× walking-distance fudge factor. The
//!   wire type carries the raw geodesic distance; the renderer scales
//!   when formatting. Keeping the fudge out of the rank lets future
//!   consumers (analytics, a debug overlay) see the unmodified value.

use tfl_domain::{NearbyStation, Station};

/// Hard cap on how far a station can be from the query point and still
/// show up in the results. Picked so that an out-of-network query
/// (Paris, Manchester, …) returns an empty list rather than an
/// arbitrarily-distant Heathrow row that the user can't act on. London's
/// surfaced network reaches ~24 km from the centre at its furthest
/// (Amersham, Upminster); 25 km is the smallest round number that
/// covers it without inviting Reading.
pub const MAX_RADIUS_M: f64 = 25_000.0;

/// Mean Earth radius in metres. Standard value used for haversine
/// calculations at the scale we care about (city, not satellite).
const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Great-circle distance between two `(lat, lon)` pairs in metres.
///
/// Uses the haversine formula. Plenty of precision for ranking stations
/// at city scale — sub-metre error accrues only at antipodal distances
/// we never query against. Inputs in degrees.
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let phi1 = lat1.to_radians();
    let phi2 = lat2.to_radians();
    let dphi = (lat2 - lat1).to_radians();
    let dlambda = (lon2 - lon1).to_radians();
    let a = (dphi / 2.0).sin().powi(2)
        + phi1.cos() * phi2.cos() * (dlambda / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    EARTH_RADIUS_M * c
}

/// Rank an iterator of stations by haversine distance from `(lat, lon)`,
/// drop anything farther than [`MAX_RADIUS_M`], and return the closest
/// `limit` results in ascending distance order.
///
/// Stations with `lat == 0.0 && lon == 0.0` are treated as missing
/// coordinates and skipped — TfL occasionally serves stop-points with a
/// zeroed location, and "Null Island" being closer to Paris than London
/// is the kind of bug we don't want to debug at 1 a.m.
pub fn rank_nearest<I>(stations: I, lat: f64, lon: f64, limit: usize) -> Vec<NearbyStation>
where
    I: IntoIterator<Item = Station>,
{
    let mut scored: Vec<NearbyStation> = stations
        .into_iter()
        .filter(|s| !(s.lat == 0.0 && s.lon == 0.0))
        .map(|s| {
            let distance_m = haversine_m(lat, lon, s.lat, s.lon);
            NearbyStation {
                station: s,
                distance_m,
            }
        })
        .filter(|n| n.distance_m <= MAX_RADIUS_M)
        .collect();

    // total_cmp is stable for f64 with NaN; haversine never produces NaN
    // for finite inputs, but defensiveness is free here.
    scored.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
    scored.truncate(limit);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use tfl_domain::Station;

    fn station(id: &str, name: &str, lat: f64, lon: f64) -> Station {
        Station {
            id: id.to_string(),
            common_name: name.to_string(),
            modes: vec!["tube".to_string()],
            lat,
            lon,
            lines: vec![],
            hub_naptan_code: None,
        }
    }

    #[test]
    fn haversine_zero_distance_when_points_match() {
        let d = haversine_m(51.5074, -0.1278, 51.5074, -0.1278);
        assert!(d < 1.0, "expected ~0 m, got {d} m");
    }

    #[test]
    fn haversine_matches_known_baker_to_oxford_circus() {
        // Baker Street ≈ (51.5226, -0.1571), Oxford Circus ≈ (51.5152, -0.1419).
        // Crow-flies distance is ~1.3 km; tolerate ±100 m.
        let d = haversine_m(51.5226, -0.1571, 51.5152, -0.1419);
        assert!(
            (1_200.0..=1_400.0).contains(&d),
            "Baker → Oxford Circus expected ~1.3 km, got {d} m"
        );
    }

    #[test]
    fn rank_nearest_orders_by_distance_ascending() {
        // Query from Bank (~51.5133, -0.0886). Order should be
        // Bank < Monument < St Paul's < Oxford Circus.
        let stations = vec![
            station("OXC", "Oxford Circus", 51.5152, -0.1419),
            station("BNK", "Bank", 51.5133, -0.0886),
            station("MNT", "Monument", 51.5108, -0.0863),
            station("STP", "St Paul's", 51.5146, -0.0973),
        ];
        let ranked = rank_nearest(stations, 51.5133, -0.0886, 4);
        let names: Vec<&str> = ranked.iter().map(|n| n.station.common_name.as_str()).collect();
        assert_eq!(names, vec!["Bank", "Monument", "St Paul's", "Oxford Circus"]);
    }

    #[test]
    fn rank_nearest_drops_stations_outside_radius() {
        // Query from central London. Reading is ~60 km; must not appear.
        let stations = vec![
            station("BNK", "Bank", 51.5133, -0.0886),
            station("RDG", "Reading", 51.4585, -0.9710),
        ];
        let ranked = rank_nearest(stations, 51.5133, -0.0886, 8);
        let names: Vec<&str> = ranked.iter().map(|n| n.station.common_name.as_str()).collect();
        assert_eq!(names, vec!["Bank"], "Reading must be dropped (outside 25 km)");
    }

    #[test]
    fn rank_nearest_truncates_to_limit() {
        let stations = vec![
            station("A", "A", 51.5101, -0.1000),
            station("B", "B", 51.5102, -0.1000),
            station("C", "C", 51.5103, -0.1000),
            station("D", "D", 51.5104, -0.1000),
        ];
        let ranked = rank_nearest(stations, 51.5100, -0.1000, 2);
        assert_eq!(ranked.len(), 2);
    }

    #[test]
    fn rank_nearest_skips_zero_zero_coords() {
        // A station with (0, 0) is closer to the equator than to anything
        // on the Tube map. Ranking must skip it rather than place it
        // ahead of legitimate London stations on a Paris query.
        let stations = vec![
            station("BAD", "Null Island", 0.0, 0.0),
            station("BNK", "Bank", 51.5133, -0.0886),
        ];
        let ranked = rank_nearest(stations, 48.8566, 2.3522, 8);
        assert!(
            ranked.is_empty(),
            "Paris query must yield zero results; got {ranked:?}"
        );
    }

    #[test]
    fn rank_nearest_returns_empty_when_query_far_from_all() {
        // Manchester query — no London station is within 25 km.
        let stations = vec![
            station("BNK", "Bank", 51.5133, -0.0886),
            station("OXC", "Oxford Circus", 51.5152, -0.1419),
        ];
        let ranked = rank_nearest(stations, 53.4808, -2.2426, 8);
        assert!(ranked.is_empty());
    }

    #[test]
    fn rank_nearest_includes_amersham_within_25km_of_baker() {
        // Amersham is the Tube's far-northwest reach (~30 km from central).
        // From Baker Street (~51.5226, -0.1571) it should be over 25 km
        // away and excluded; from Watford (~51.6565, -0.3963) it's
        // ~10 km and should be included. This pins down the radius we picked.
        let amersham = station("AMR", "Amersham", 51.6740, -0.6075);
        let from_baker = rank_nearest(vec![amersham.clone()], 51.5226, -0.1571, 8);
        assert!(
            from_baker.is_empty(),
            "Amersham is ~37 km from Baker Street; must be outside 25 km cap"
        );
        let from_watford = rank_nearest(vec![amersham], 51.6565, -0.3963, 8);
        assert_eq!(
            from_watford.len(),
            1,
            "Amersham is ~14 km from Watford; must be inside 25 km cap"
        );
    }
}
