use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("repository not found (no .cliproot/ in any ancestor)")]
    NotFound,

    #[error("repository already exists at {0}")]
    AlreadyExists(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("core error: {0}")]
    Core(#[from] cliproot_core::CoreError),

    #[error("{0}")]
    Other(String),
}
