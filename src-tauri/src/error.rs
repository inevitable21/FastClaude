use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("spawn failed: {0}")]
    Spawn(String),
    #[error("focus failed: {0}")]
    Focus(String),
    #[error("`claude` CLI not found on PATH. Install Claude Code from https://docs.claude.com/en/docs/claude-code/setup, then restart FastClaude.")]
    ClaudeNotOnPath,
    #[error("FastClaude doesn't yet support {0} — contributions welcome at https://github.com/inevitable21/FastClaude")]
    PlatformUnsupported(&'static str),
    #[error("{0}")]
    Other(String),
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
