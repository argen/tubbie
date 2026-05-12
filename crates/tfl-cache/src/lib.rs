#![deny(unsafe_code)]

pub mod cache;

pub use cache::TflClient;
pub use cache::CANONICAL_MULTI_MODE_HUBS;
pub use cache::SUPPORTED_MODES;

#[cfg(test)]
mod client_tests;

#[cfg(test)]
mod multi_mode_hub_completeness_tests;
