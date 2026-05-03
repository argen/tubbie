#![deny(unsafe_code)]

pub mod direction;
pub mod format;
pub mod theme;
pub mod types;

// Re-export the most commonly used items at crate root for ergonomics.
pub use direction::{line_compass_axis, CompassAxis, Direction, NorthernBranch};
pub use theme::{Theme, ThemeError};
pub use types::{
    is_supported_line_id, pretty_line_name, severity_bucket, Arrival, Board, Favorite, Line,
    LineRef, LineStatus, NearbyStation, Platform, SeverityBucket, Station, StatusEntry,
    TflDisruption, TflLine, TflLineStatus, TflValidityPeriod, ValidityPeriod,
};
