//! Behavioural tests for `Theme::validate`.

use std::collections::HashMap;
use tfl_domain::{Theme, ThemeError};

fn full_palette() -> HashMap<String, String> {
    let tokens = [
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
    tokens
        .iter()
        .map(|&t| (t.to_string(), "#000".to_string()))
        .collect()
}

#[test]
fn valid_theme_passes() {
    let theme = Theme {
        id: "classic-amber".to_string(),
        name: "Classic Amber".to_string(),
        palette: full_palette(),
    };
    assert!(theme.validate().is_ok());
}

#[test]
fn empty_palette_returns_all_missing() {
    let theme = Theme {
        id: "broken".to_string(),
        name: "Broken".to_string(),
        palette: HashMap::new(),
    };
    let err = theme.validate().unwrap_err();
    match err {
        ThemeError::MissingTokens { missing } => {
            assert_eq!(
                missing.len(),
                9,
                "All 9 tokens should be missing, got: {missing:?}"
            );
        }
    }
}

#[test]
fn single_missing_token_reported() {
    let mut palette = full_palette();
    palette.remove("due-flash");
    let theme = Theme {
        id: "partial".to_string(),
        name: "Partial".to_string(),
        palette,
    };
    let err = theme.validate().unwrap_err();
    match err {
        ThemeError::MissingTokens { missing } => {
            assert_eq!(missing, vec!["due-flash".to_string()]);
        }
    }
}

#[test]
fn multiple_missing_tokens_reported() {
    let mut palette = full_palette();
    palette.remove("bg");
    palette.remove("stale-accent");
    palette.remove("row-divider");
    let theme = Theme {
        id: "partial".to_string(),
        name: "Partial".to_string(),
        palette,
    };
    let err = theme.validate().unwrap_err();
    match err {
        ThemeError::MissingTokens { missing } => {
            // validate() sorts missing tokens alphabetically for deterministic output.
            assert_eq!(
                missing,
                vec![
                    "bg".to_string(),
                    "row-divider".to_string(),
                    "stale-accent".to_string(),
                ],
                "missing tokens must be returned in sorted alphabetical order"
            );
        }
    }
}

#[test]
fn extra_tokens_do_not_affect_validity() {
    let mut palette = full_palette();
    palette.insert("custom-extra".to_string(), "#fff".to_string());
    let theme = Theme {
        id: "extended".to_string(),
        name: "Extended".to_string(),
        palette,
    };
    assert!(theme.validate().is_ok());
}

#[test]
fn error_display_lists_missing_tokens() {
    let mut palette = full_palette();
    palette.remove("bg");
    let theme = Theme {
        id: "x".to_string(),
        name: "X".to_string(),
        palette,
    };
    let display = theme.validate().unwrap_err().to_string();
    assert!(
        display.contains("bg"),
        "Display should mention 'bg': {display}"
    );
}
