//! One-shot diagnostic: hit live TfL and verify hub-merge produces
//! Elizabeth + Overground lines for Liverpool Street and Tottenham
//! Court Road. Not part of CI; not shipped to iOS.
//!
//! Usage: `cargo run --bin diag-hub -- <station_id> [app_key]`.
//! If app_key is omitted, runs anonymous (slower, may rate-limit).
//!
//! Prints, for the queried station:
//! - merged `Station.lines` ids (post-warm)
//! - `hub_naptan_code`
//! - `allowed_line_ids_for` result
//! - the live arrivals' distinct `line_id`s (before any board filtering)
//! - the resolved arrival ids (i.e., what `resolve_arrival_ids` returned)
//!
//! This isolates "is the code path correct?" from "is the user's
//! device hitting a rate-limit / network issue?".

use std::env;
use std::sync::Arc;

use tfl_cache::TflClient;
use tfl_client::http::ReqwestTflHttp;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: diag-hub <station_id> [app_key]");
        std::process::exit(2);
    }
    let station_id = &args[1];
    let app_key = args.get(2).cloned();

    let http = match app_key {
        Some(k) => ReqwestTflHttp::with_app_key(k),
        None => ReqwestTflHttp::new(),
    };
    let client = Arc::new(TflClient::new(http));

    println!("warming stop-points cache (this is what subscribe_board_live awaits)…");
    let count = client
        .warm_stop_points_cache()
        .await
        .expect("warm should succeed");
    println!("  warmed; {count} stations cached");

    println!("\nallowed_line_ids_for({station_id}):");
    let allowed = client
        .allowed_line_ids_for(station_id)
        .await
        .expect("allowed_line_ids_for should succeed");
    let mut sorted: Vec<&String> = allowed.iter().collect();
    sorted.sort();
    println!("  {sorted:?}");
    let has_elizabeth = sorted.iter().any(|s| s.as_str() == "elizabeth");
    let has_og = sorted.iter().any(|s| {
        matches!(
            s.as_str(),
            "weaver"
                | "liberty"
                | "lioness"
                | "mildmay"
                | "suffragette"
                | "windrush"
                | "london-overground"
        )
    });
    println!("  contains 'elizabeth': {has_elizabeth}");
    println!("  contains any OG line: {has_og}");

    println!("\nlive arrivals at {station_id} (post hub-merge fan-out):");
    match client.get_arrivals(station_id).await {
        Ok(arrivals) => {
            let mut by_line = std::collections::BTreeMap::<String, usize>::new();
            for a in &arrivals {
                *by_line.entry(a.line_id.clone()).or_default() += 1;
            }
            println!("  total: {}", arrivals.len());
            for (line, n) in &by_line {
                println!("    {line}: {n}");
            }
        }
        Err(e) => println!("  ERROR: {e}"),
    }
}
