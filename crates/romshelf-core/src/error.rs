use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("data error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("{0}")]
    External(String),
    /// The network could not be reached at all.
    #[error("Couldn't reach Google: {0}")]
    Network(String),
    /// Signing in is needed, or the sign-in failed. The text is a full sentence for the user.
    #[error("{0}")]
    Auth(String),
    /// Data written by a newer Romshelf. The text names what.
    #[error("{0} was written by a newer Romshelf. Please update Romshelf.")]
    NeedsUpdate(String),
    #[error("No browser could be opened.")]
    NoBrowser,
    #[error("Cancelled.")]
    Cancelled,
    #[error("{0}")]
    Timeout(String),
}

pub type Result<T> = std::result::Result<T, Error>;
