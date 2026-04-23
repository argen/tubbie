//! Theme type and validation.
//!
//! Themes are CSS-variable bundles persisted alongside the board config.
//! Validation happens on the Rust side before the palette reaches the frontend.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Required palette tokens that every theme must define.
const REQUIRED_TOKENS: &[&str] = &[
    "bg",
    "fg",
    "accent",
    "platform-label",
    "due-flash",
    "ticker-bg",
    "ticker-fg",
    "stale-accent",
    "row-divider",
];

/// A named colour palette for the arrivals board UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    /// Short machine-readable identifier, e.g. `"classic-amber"`.
    pub id: String,
    /// Human-readable display name, e.g. `"Classic Amber"`.
    pub name: String,
    /// CSS colour values keyed by token name (e.g. `"bg"` → `"#000000"`).
    pub palette: HashMap<String, String>,
}

impl Theme {
    /// Validate that the palette contains all required tokens.
    ///
    /// Returns `Ok(())` if every required token is present.
    /// Returns `Err(ThemeError::MissingTokens { missing })` listing every
    /// absent token, sorted for deterministic output.
    pub fn validate(&self) -> Result<(), ThemeError> {
        let mut missing: Vec<String> = REQUIRED_TOKENS
            .iter()
            .filter(|&&token| !self.palette.contains_key(token))
            .map(|&t| t.to_string())
            .collect();
        missing.sort();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(ThemeError::MissingTokens { missing })
        }
    }
}

/// Errors produced by [`Theme::validate`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ThemeError {
    /// One or more required palette tokens are absent.
    #[error("theme is missing required palette tokens: {}", missing.join(", "))]
    MissingTokens { missing: Vec<String> },
}
