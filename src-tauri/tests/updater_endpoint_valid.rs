//! Pins the updater endpoint in `tauri.conf.json` to an HTTPS URL on
//! `github.com`. Catches accidental drift to http://, a forked-repo URL, or
//! a missing `plugins.updater.endpoints` block.
//!
//! Goes RED on a regression that would silently disable signed updates or
//! point them at the wrong release feed.

use serde_json::Value;

const CONF: &str = include_str!("../tauri.conf.json");

#[test]
fn updater_endpoint_is_https_github_com() {
    let conf: Value = serde_json::from_str(CONF).expect("tauri.conf.json must parse");

    let endpoints = conf
        .get("plugins")
        .and_then(|p| p.get("updater"))
        .and_then(|u| u.get("endpoints"))
        .and_then(|e| e.as_array())
        .expect("tauri.conf.json must declare plugins.updater.endpoints");

    assert!(
        !endpoints.is_empty(),
        "plugins.updater.endpoints must declare at least one URL"
    );

    for ep in endpoints {
        let url = ep.as_str().expect("endpoint must be a string");
        let parsed = url::Url::parse(url).unwrap_or_else(|e| panic!("endpoint {url:?} parse: {e}"));
        assert_eq!(
            parsed.scheme(),
            "https",
            "updater endpoint MUST be https (no plaintext update channel)"
        );
        assert_eq!(
            parsed.host_str(),
            Some("github.com"),
            "updater endpoint MUST be on github.com — drift to a third-party CDN is a supply-chain risk"
        );
    }
}
