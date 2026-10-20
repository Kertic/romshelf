//! Canonical control model, merge and per-adapter writers.
//!
//! An emulator has a complete default layout per system; a game keeps only its differences.
//! At launch the two are merged and written into the emulator's Romshelf-owned configuration.

pub mod merge;
pub mod model;

pub use merge::{difference, merge, promote};
pub use model::{Input, Layout, Scope, Target, TargetKind, INPUTS};
