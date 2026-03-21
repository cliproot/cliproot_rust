use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid content hash: {0}")]
    InvalidContentHash(String),

    #[error("invalid CRP id: {0}")]
    InvalidCrpId(String),

    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("verification failed: {0}")]
    VerificationFailed(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
