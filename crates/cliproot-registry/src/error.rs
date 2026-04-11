use std::fmt;

#[derive(Debug)]
pub enum RegistryError {
    Http(reqwest::Error),
    Api { code: String, message: String },
    Store(cliproot_store::StoreError),
    Json(serde_json::Error),
    Io(std::io::Error),
    InvalidRegistry(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP error: {e}"),
            Self::Api { code, message } => write!(f, "registry error ({code}): {message}"),
            Self::Store(e) => write!(f, "store error: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::InvalidRegistry(msg) => write!(f, "invalid registry: {msg}"),
        }
    }
}

impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(e) => Some(e),
            Self::Store(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for RegistryError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

impl From<cliproot_store::StoreError> for RegistryError {
    fn from(e: cliproot_store::StoreError) -> Self {
        Self::Store(e)
    }
}

impl From<serde_json::Error> for RegistryError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl From<std::io::Error> for RegistryError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
