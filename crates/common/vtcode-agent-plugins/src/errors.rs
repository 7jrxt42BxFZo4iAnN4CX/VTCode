use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("invalid plugin manifest: {0}")]
    InvalidManifest(String),

    #[error("invalid plugin name: {0}")]
    InvalidName(String),

    #[error("invalid MCP configuration: {0}")]
    InvalidMcp(String),

    #[error("invalid skill: {0}")]
    InvalidSkill(String),

    #[error("path escapes plugin root: {0}")]
    PathEscape(String),

    #[error("unsupported plugin schema version: {0}")]
    UnsupportedSchema(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
