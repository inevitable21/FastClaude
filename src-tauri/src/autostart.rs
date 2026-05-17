//! Reconciles the OS autostart registry entry with the user's saved config.
//!
//! Called from `set_config` on every settings save. Returns the underlying
//! plugin error (boxed as `anyhow`) so it can surface as a toast in the UI.

use crate::config::Config;
use crate::error::AppResult;
use tauri::{AppHandle, Runtime};

#[cfg(not(debug_assertions))]
use crate::error::AppError;
#[cfg(not(debug_assertions))]
use tauri_plugin_autostart::ManagerExt;

/// Production behavior: enable or disable the registry entry based on `cfg`.
#[cfg(not(debug_assertions))]
pub fn reconcile_autostart<R: Runtime>(app: &AppHandle<R>, cfg: &Config) -> AppResult<()> {
    let manager = app.autolaunch();
    if cfg.launch_on_login {
        manager
            .enable()
            .map_err(|e| AppError::Other(format!("enable autostart: {e}")))?;
    } else {
        // Only disable if currently enabled. The underlying auto-launch crate
        // raises ERROR_FILE_NOT_FOUND when deleting a non-existent registry
        // value, which would otherwise fail every new user's first Settings
        // save (default state: launch_on_login = false).
        let enabled = manager
            .is_enabled()
            .map_err(|e| AppError::Other(format!("check autostart state: {e}")))?;
        if enabled {
            manager
                .disable()
                .map_err(|e| AppError::Other(format!("disable autostart: {e}")))?;
        }
    }
    Ok(())
}

/// Dev-mode no-op: developer machines should never see registry writes from
/// `cargo run` / `tauri dev`. The Settings toggle still persists to config;
/// only the OS-level effect is skipped.
#[cfg(debug_assertions)]
pub fn reconcile_autostart<R: Runtime>(_app: &AppHandle<R>, _cfg: &Config) -> AppResult<()> {
    Ok(())
}
