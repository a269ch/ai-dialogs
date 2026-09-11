use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Dialogue '{0}' not found")]
    NotFound(String),

    #[error("Path '{0}' does not exist")]
    PathNotFound(PathBuf),

    #[error("{0}")]
    General(String),
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AppError::NotFound("abc-123".to_string());
        assert_eq!(err.to_string(), "Dialogue 'abc-123' not found");

        let err2 = AppError::General("custom failure".to_string());
        assert_eq!(err2.to_string(), "custom failure");

        let err3 = AppError::PathNotFound(PathBuf::from("/non/existent"));
        assert_eq!(err3.to_string(), "Path '/non/existent' does not exist");
    }
}
