use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Integration(String),
    #[error("{0}")]
    Validation(String),
    #[error("{0} was not found. Refresh the workspace and try again.")]
    NotFound(&'static str),
    #[error("The local database operation failed. Your change was not saved.")]
    Database(#[from] sqlx::Error),
    #[error("The local database could not be initialized.")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("The local data folder could not be accessed.")]
    Io(#[from] std::io::Error),
    #[error("Cached email data could not be read.")]
    Serialization(#[from] serde_json::Error),
}

// IPC errors deliberately omit SQL, bound values, file contents and credentials.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl AppError {
    pub fn log_safe(&self, operation: &'static str) {
        let kind = match self {
            Self::Integration(_) => "integration",
            Self::Validation(_) => "validation",
            Self::NotFound(_) => "not_found",
            Self::Database(_) => "database",
            Self::Migration(_) => "migration",
            Self::Io(_) => "filesystem",
            Self::Serialization(_) => "serialization",
        };
        tracing::warn!(operation, error_kind = kind, "JobView operation failed");
    }
}

pub type AppResult<T> = Result<T, AppError>;
