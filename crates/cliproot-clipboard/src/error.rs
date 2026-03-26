#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[error("clipboard error: {0}")]
    Arboard(#[from] arboard::Error),
}
