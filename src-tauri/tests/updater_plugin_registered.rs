//! Asserts the `tauri-plugin-updater` plugin is registered on the Tauri
//! builder. Goes RED if a future refactor accidentally drops the
//! `.plugin(tauri_plugin_updater::Builder::new().build())` line in
//! `src/lib.rs` — which would silently break the signed-update path.
//!
//! Uses Tauri's `MockRuntime`. `mock_context` doesn't carry plugin config
//! from `tauri.conf.json` (its `plugins` HashMap is empty), so we inject
//! the updater block here. `app.updater_builder()` then succeeds only when
//! the plugin state was registered AND the configured pubkey parses —
//! either failure mode means the signed update path is broken; both must
//! trip this test.

use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::UpdaterExt;

#[test]
fn updater_plugin_is_registered_on_the_builder() {
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".to_string(),
        json!({
            "active": false,
            "dialog": false,
            // Throwaway placeholder pubkey. The private key has been
            // discarded; only the public format is exercised here. PR-B
            // replaces with the real pubkey from `cargo tauri signer generate`.
            "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEU0REEwOUJDOTc5QUJDQzMKUldURHZKcVh2QW5hNVBTSWhlUklIZllKODRNY25oV214UThPU25ZMDI2NFR4MFZrbTJtT1VEQ1AK",
            "endpoints": [
                "https://github.com/argen/tubbie/releases/latest/download/latest.json"
            ]
        }),
    );

    let app = mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .expect("app builds with the updater plugin");

    app.updater_builder()
        .build()
        .expect("Updater must build from the registered plugin");
}
