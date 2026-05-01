use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pricing {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub terminal_program: String,
    pub default_model: String,
    pub hotkey: String,
    pub idle_threshold_seconds: u64,
    pub pricing: HashMap<String, Pricing>,
}

impl Default for Config {
    fn default() -> Self {
        let mut pricing = HashMap::new();
        pricing.insert(
            "claude-opus-4-7".into(),
            Pricing { input: 15.0, output: 75.0, cache_read: 1.5, cache_write: 18.75 },
        );
        pricing.insert(
            "claude-sonnet-4-6".into(),
            Pricing { input: 3.0, output: 15.0, cache_read: 0.3, cache_write: 3.75 },
        );
        pricing.insert(
            "claude-haiku-4-5".into(),
            Pricing { input: 1.0, output: 5.0, cache_read: 0.1, cache_write: 1.25 },
        );
        Self {
            terminal_program: "auto".into(),
            default_model: "claude-opus-4-7".into(),
            hotkey: "Ctrl+Shift+C".into(),
            idle_threshold_seconds: 300,
            pricing,
        }
    }
}

pub fn load(path: &PathBuf) -> AppResult<Config> {
    if !path.exists() {
        let cfg = Config::default();
        save(path, &cfg)?;
        return Ok(cfg);
    }
    let bytes = std::fs::read(path)?;
    let cfg: Config = serde_json::from_slice(&bytes)?;
    Ok(cfg)
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
    fn load_creates_default_when_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.default_model, "claude-opus-4-7");
        assert!(path.exists(), "default config must be persisted");
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("c.json");
        let mut cfg = Config::default();
        cfg.default_model = "claude-sonnet-4-6".into();
        save(&path, &cfg).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.default_model, "claude-sonnet-4-6");
    }

    #[test]
    fn load_corrupt_json_returns_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(&path, b"not json").unwrap();
        assert!(matches!(load(&path), Err(AppError::Json(_))));
    }
}
