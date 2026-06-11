//! `cargo run --bin uniffi-bindgen -- generate --library <dylib> --language swift --out-dir <dir>`
//!
//! This is the canonical uniffi bindgen entrypoint per the upstream
//! `mozilla/uniffi-rs` examples. The bin is gated behind the `uniffi/cli`
//! feature so plain `cargo build -p tfl-ffi` does not pull in `clap` and the
//! bindgen tree.
fn main() {
    uniffi::uniffi_bindgen_main()
}
