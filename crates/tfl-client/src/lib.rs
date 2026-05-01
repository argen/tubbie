#![deny(unsafe_code)]

pub mod client;
pub mod clock;
pub mod error;
pub mod fixture;
pub mod http;
pub mod nearest;

#[cfg(test)]
mod client_tests;

pub use client::TflClient;
