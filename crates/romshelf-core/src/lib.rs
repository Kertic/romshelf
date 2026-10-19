//! Romshelf core: all logic, no Tauri dependency.

pub mod auth;
pub mod clock;
pub mod connection;
pub mod controls;
pub mod db;
pub mod drive;
pub mod emulators;
pub mod error;
pub mod http;
pub mod launch;
pub mod library;
pub mod model;
pub mod paths;
pub mod process;
pub mod saves;
pub mod settings;
pub mod steam;
pub mod sync;
pub mod tokens;
pub mod transfer;

pub use error::{Error, Result};
