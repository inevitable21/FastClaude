use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub terminal_program: String,
    pub default_model: String,
    pub hotkey: String,
    pub idle_threshold_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            terminal_program: "auto".into(),
            default_model: "claude-opus-4-7".into(),
            hotkey: "Ctrl+Shift+C".into(),
            idle_threshold_seconds: 300,
        }
    }
}

pub fn load(path: &PathBuf) -> AppResult<(Config, bool)> {
    if !path.exists() {
        let cfg = Config::default();
        save(path, &cfg)?;
        return Ok((cfg, true));
    }
    let bytes = std::fs::read(path)?;
    let cfg: Config = serde_json::from_slice(&bytes)?;
    Ok((cfg, false))
}

pub fn save(path: &PathBuf, cfg: &Config) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(cfg)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use tempfile::TempDir;

    #[test]
    fn load_signals_first_run_when_creating_default() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        let (cfg, was_created) = load(&path).unwrap();
        assert_eq!(cfg.default_model, "claude-opus-4-7");
        assert!(was_created);
        assert!(path.exists(), "default config must be persisted");
    }

    #[test]
    fn load_signals_not_first_run_when_file_exists() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        save(&path, &Config::default()).unwrap();
        let (_cfg, was_created) = load(&path).unwrap();
        assert!(!was_created);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("c.json");
        let mut cfg = Config::default();
        cfg.default_model = "claude-sonnet-4-6".into();
        save(&path, &cfg).unwrap();
        let (loaded, _) = load(&path).unwrap();
        assert_eq!(loaded.default_model, "claude-sonnet-4-6");
    }

    #[test]
    fn load_corrupt_json_returns_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(&path, b"not json").unwrap();
        assert!(matches!(load(&path), Err(AppError::Json(_))));
    }

    #[test]
    fn load_ignores_legacy_pricing_field() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(
            &path,
            br#"{"terminal_program":"auto","default_model":"claude-opus-4-7",
                "hotkey":"Ctrl+Shift+C","idle_threshold_seconds":300,
                "pricing":{"claude-opus-4-7":{"input":15,"output":75,"cache_read":1.5,"cache_write":18.75}}}"#,
        )
        .unwrap();
        let (cfg, _) = load(&path).unwrap();
        assert_eq!(cfg.default_model, "claude-opus-4-7");
    }
}
