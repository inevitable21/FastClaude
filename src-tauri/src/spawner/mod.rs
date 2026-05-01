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
        Box::new(windows::WindowsSpawner::new())
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

use std::path::PathBuf;

/// Resolves an executable name on PATH. Behind a trait so the Windows
/// spawner's PATH preflight (Task 6) can be unit-tested with a fake.
pub trait PathLookup: Send + Sync {
    fn find(&self, exe: &str) -> Option<PathBuf>;
}

pub struct EnvPathLookup;

impl PathLookup for EnvPathLookup {
    fn find(&self, exe: &str) -> Option<PathBuf> {
        which::which(exe).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeLookup(Option<PathBuf>);
    impl PathLookup for FakeLookup {
        fn find(&self, _exe: &str) -> Option<PathBuf> {
            self.0.clone()
        }
    }

    #[test]
    fn fake_lookup_returns_some_when_present() {
        let l = FakeLookup(Some(PathBuf::from("C:\\bin\\claude.exe")));
        assert!(l.find("claude").is_some());
    }

    #[test]
    fn fake_lookup_returns_none_when_absent() {
        let l = FakeLookup(None);
        assert!(l.find("claude").is_none());
    }
}
