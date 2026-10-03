use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LensError {
    #[error("not supported on this platform: {0}")]
    Unsupported(String),
    #[error("cancelled")]
    Cancelled,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("i/o error: {0}")]
    Io(String),
    #[error("blocked by safety policy: {0}")]
    Blocked(String),
    #[error("{0}")]
    Failed(String),
}

impl From<std::io::Error> for LensError {
    fn from(e: std::io::Error) -> Self {
        LensError::Io(e.to_string())
    }
}

pub type Result<T, E = LensError> = std::result::Result<T, E>;
