//! Capture a real `Board` JSON snapshot for use as a SwiftUI test
//! fixture. Hits live TfL via the same code path the iOS app uses,
//! refreshes ONE board, prints the resulting JSON to stdout.
//!
//! Usage: `cargo run --bin dump-board-json -- <station_id> [app_key] > fixture.json`
//!
//! The output is the exact byte-for-byte JSON the FFI emits to Swift via
//! `BoardSubscription::nextSnapshot()`. A SwiftUI test that decodes
//! this fixture exercises the same decode → `groupByLine` pipeline as
//! the live app — without any TfL network dependency in CI.

use std::env;
use std::sync::Arc;

use tfl_board::{BoardConfig, BoardService};
use tfl_cache::TflClient;
use tfl_client::clock::SystemClock;
use tfl_client::http::ReqwestTflHttp;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: dump-board-json <station_id> [app_key]");
        std::process::exit(2);
    }
    let station_id = &args[1];
    let app_key = args.get(2).cloned();

    let http = match app_key {
        Some(k) => ReqwestTflHttp::with_app_key(k),
        None => ReqwestTflHttp::new(),
    };
    let client = Arc::new(TflClient::new(http));

    eprintln!("warming stop-points cache…");
    let _ = client
        .warm_stop_points_cache()
        .await
        .expect("warm should succeed");

    eprintln!("refreshing board for {station_id}…");
    let service = BoardService::new(client, SystemClock);
    let cfg = BoardConfig::new(station_id);
    let board = service.refresh(&cfg).await.expect("refresh should succeed");

    let json = serde_json::to_string_pretty(&board).expect("board serialises");
    println!("{json}");

    eprintln!("\n— summary —");
    let mut by_line = std::collections::BTreeMap::<String, usize>::new();
    let mut by_direction = std::collections::BTreeMap::<String, usize>::new();
    for p in &board.platforms {
        for a in &p.arrivals {
            *by_line.entry(a.line_id.clone()).or_default() += 1;
            *by_direction.entry(p.name.clone()).or_default() += 1;
        }
    }
    eprintln!("lines:      {by_line:?}");
    eprintln!("directions: {by_direction:?}");
}
