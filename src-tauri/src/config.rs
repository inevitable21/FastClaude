use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    Window,
    Minimized,
    Hidden,
}

impl Default for LaunchMode {
    fn default() -> Self {
        LaunchMode::Window
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub terminal_program: String,
    pub default_model: String,
    pub hotkey: String,
    pub idle_threshold_seconds: u64,
    /// Default --effort flag value for new launches; empty string = don't pass.
    /// Valid: "low" | "medium" | "high" | "xhigh" | "max".
    #[serde(default)]
    pub default_effort: String,
    /// Default --permission-mode flag value; empty = don't pass.
    /// Valid: "acceptEdits" | "auto" | "bypassPermissions" | "default" | "dontAsk" | "plan".
    #[serde(default)]
    pub default_permission_mode: String,
    /// Free-form extra args appended verbatim to every launch (unless
    /// overridden in the LaunchDialog).
    #[serde(default)]
    pub default_extra_args: String,
    /// Default prompt sent to claude on launch (empty = don't pass).
    /// LaunchDialog pre-fills its prompt textarea from this value.
    #[serde(default)]
    pub default_prompt: String,
    /// If true, register FastClaude to launch at Windows login.
    #[serde(default)]
    pub launch_on_login: bool,
    /// How the window should appear when launched by autostart.
    /// (Read on every boot from the loaded config.)
    #[serde(default)]
    pub launch_mode: LaunchMode,
    /// Default state for the LaunchDialog's auto-continue checkbox.
    #[serde(default)]
    pub default_auto_continue: bool,
    /// Default prompt sent to claude when an auto-resume fires.
    /// Per-session `resume_prompt` overrides this at fire time.
    #[serde(default = "default_resume_prompt_value")]
    pub default_resume_prompt: String,
    /// Max auto-resumes per session chain. Frozen onto new sessions at
    /// launch time so a later change does not retroactively re-arm.
    #[serde(default = "default_resume_cap_value")]
    pub default_resume_cap: i64,
}

fn default_resume_prompt_value() -> String { "continue".into() }
fn default_resume_cap_value() -> i64 { 3 }

impl Default for Config {
    fn default() -> Self {
        Self {
            terminal_program: "auto".into(),
            default_model: "claude-opus-4-7".into(),
            hotkey: "Ctrl+Shift+C".into(),
            idle_threshold_seconds: 300,
            default_effort: String::new(),
            default_permission_mode: String::new(),
            default_extra_args: String::new(),
            default_prompt: String::new(),
            launch_on_login: false,
            launch_mode: LaunchMode::Window,
            default_auto_continue: false,
            default_resume_prompt: default_resume_prompt_value(),
            default_resume_cap: default_resume_cap_value(),
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

    #[test]
    fn load_defaults_prompt_to_empty_when_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(
            &path,
            br#"{"terminal_program":"auto","default_model":"claude-opus-4-7",
                "hotkey":"Ctrl+Shift+C","idle_threshold_seconds":300}"#,
        )
        .unwrap();
        let (cfg, _) = load(&path).unwrap();
        assert_eq!(cfg.default_prompt, "");
    }

    #[test]
    fn launch_mode_serde_round_trip() {
        for m in [LaunchMode::Window, LaunchMode::Minimized, LaunchMode::Hidden] {
            let s = serde_json::to_value(&m).unwrap();
            let back: LaunchMode = serde_json::from_value(s.clone()).unwrap();
            assert_eq!(m, back, "round-trip failed for {:?} (serialized as {})", m, s);
        }
    }

    #[test]
    fn launch_mode_serializes_as_snake_case() {
        assert_eq!(serde_json::to_string(&LaunchMode::Window).unwrap(), "\"window\"");
        assert_eq!(serde_json::to_string(&LaunchMode::Minimized).unwrap(), "\"minimized\"");
        assert_eq!(serde_json::to_string(&LaunchMode::Hidden).unwrap(), "\"hidden\"");
    }

    #[test]
    fn launch_mode_default_is_window() {
        assert_eq!(LaunchMode::default(), LaunchMode::Window);
    }

    #[test]
    fn load_defaults_autostart_fields_when_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(
            &path,
            br#"{"terminal_program":"auto","default_model":"claude-opus-4-7",
                "hotkey":"Ctrl+Shift+C","idle_threshold_seconds":300}"#,
        )
        .unwrap();
        let (cfg, _) = load(&path).unwrap();
        assert!(!cfg.launch_on_login, "launch_on_login must default to false");
        assert_eq!(cfg.launch_mode, LaunchMode::Window);
    }

    #[test]
    fn config_default_has_autostart_off() {
        let cfg = Config::default();
        assert!(!cfg.launch_on_login);
        assert_eq!(cfg.launch_mode, LaunchMode::Window);
    }

    #[test]
    fn config_default_has_auto_continue_off_and_cap_three() {
        let cfg = Config::default();
        assert!(!cfg.default_auto_continue);
        assert_eq!(cfg.default_resume_prompt, "continue");
        assert_eq!(cfg.default_resume_cap, 3);
    }

    #[test]
    fn load_defaults_auto_continue_fields_when_missing() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(
            &path,
            br#"{"terminal_program":"auto","default_model":"claude-opus-4-7",
                "hotkey":"Ctrl+Shift+C","idle_threshold_seconds":300}"#,
        ).unwrap();
        let (cfg, _) = load(&path).unwrap();
        assert!(!cfg.default_auto_continue);
        assert_eq!(cfg.default_resume_prompt, "continue");
        assert_eq!(cfg.default_resume_cap, 3);
    }
}
