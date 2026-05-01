use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod linux;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnRequest {
    pub project_dir: String,
    pub model: String,
    pub prompt: Option<String>,
    /// "auto" or an explicit terminal program name/path.
    pub terminal_program: String,
}

#[derive(Debug, Clone)]
pub struct SpawnResult {
    pub claude_pid: i64,
    pub terminal_pid: i64,
    pub terminal_window_handle: Option<String>,
}

pub trait Spawner: Send + Sync {
    fn spawn(&self, req: &SpawnRequest) -> AppResult<SpawnResult>;
}

pub fn default_spawner() -> Box<dyn Spawner> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsSpawner)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacSpawner)
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxSpawner)
    }
}

/// Stub spawner used when no real implementation is available.
#[allow(dead_code)]
pub struct StubSpawner;

impl Spawner for StubSpawner {
    fn spawn(&self, _req: &SpawnRequest) -> AppResult<SpawnResult> {
        Err(AppError::Spawn(
            "spawner not implemented for this platform yet".into(),
        ))
    }
}
