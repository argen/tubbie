#![deny(unsafe_code)]

pub mod direction;
pub mod format;
pub mod theme;
pub mod types;

// Re-export the most commonly used items at crate root for ergonomics.
pub use direction::{Direction, NorthernBranch};
pub use theme::{Theme, ThemeError};
pub use types::{
    Arrival, Board, Line, LineRef, LineStatus, Platform, Station, StatusEntry, TflDisruption,
    TflLine, TflLineStatus,
};
