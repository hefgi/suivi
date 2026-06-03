use thiserror::Error;

#[derive(Debug, Error)]
pub enum SuiviError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[allow(dead_code)]
    #[error("config error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("toml error: {0}")]
    Toml(String),
}

impl From<toml::de::Error> for SuiviError {
    fn from(e: toml::de::Error) -> Self {
        SuiviError::Toml(e.to_string())
    }
}

impl From<toml::ser::Error> for SuiviError {
    fn from(e: toml::ser::Error) -> Self {
        SuiviError::Toml(e.to_string())
    }
}
