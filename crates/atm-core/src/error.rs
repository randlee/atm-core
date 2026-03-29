use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("home directory is unavailable")]
    HomeDirectoryUnavailable,
    #[error("address parse failed: {0}")]
    AddressParse(String),
    #[error("identity is not configured")]
    IdentityUnavailable,
    #[error("team is not configured")]
    TeamUnavailable,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),
}
