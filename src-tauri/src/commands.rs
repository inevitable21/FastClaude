use crate::config::Config;
use crate::error::AppResult;
use crate::recent_projects::{self, RecentProject};
use crate::session_registry::{NewSession, Registry, Session};
use crate::spawner::{SpawnRequest, Spawner};
use crate::window_focus::WindowFocus;
use std::sync::{Arc, Mutex};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
use tauri::{Emitter, State};

pub struct AppState {
    pub registry: Arc<Registry>,
    pub spawner: Box<dyn Spawner>,
    pub focus: Box<dyn WindowFocus>,
    pub config: Mutex<Config>,
}

#[derive(serde::Deserialize)]
pub struct LaunchInput {
    pub project_dir: String,
    pub model: Option<String>,
    pub prompt: Option<String>,
}

#[tauri::command]
pub fn list_sessions(state: State<'_, AppState>) -> AppResult<Vec<Session>> {
    state.registry.list_active()
}

#[tauri::command]
pub fn list_all_sessions(state: State<'_, AppState>) -> AppResult<Vec<Session>> {
    state.registry.list_all()
}

#[tauri::command]
pub fn launch_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: LaunchInput,
) -> AppResult<Session> {
    let cfg = state.config.lock().unwrap().clone();
    let model = input.model.unwrap_or(cfg.default_model.clone());
    let req = SpawnRequest {
        project_dir: input.project_dir.clone(),
        model: model.clone(),
        prompt: input.prompt,
        terminal_program: cfg.terminal_program.clone(),
    };
    let result = state.spawner.spawn(&req)?;
    let session = state.registry.insert(NewSession {
        project_dir: input.project_dir,
        model,
        claude_pid: result.claude_pid,
        terminal_pid: result.terminal_pid,
        terminal_window_handle: result.terminal_window_handle,
    })?;
    let _ = app.emit("session-changed", &session);
    Ok(session)
}

#[tauri::command]
pub fn kill_session(app: tauri::AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    let s = state.registry.get(&id)?;
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    sys.refresh_processes();
    // Kill the leaf (claude/node.exe) AND its shell parent so the cmd /K shell
    // doesn't linger after we end the session. Use Process::kill (TerminateProcess
    // on Windows); kill_with(Signal::Term) is a no-op on Windows.
    if let Some(p) = sys.process(Pid::from_u32(s.claude_pid as u32)) {
        if let Some(parent_pid) = p.parent() {
            if let Some(parent) = sys.process(parent_pid) {
                parent.kill();
            }
        }
        p.kill();
    }
    state
        .registry
        .mark_ended(&id, chrono::Utc::now().timestamp())?;
    let _ = app.emit("session-changed", &id);
    Ok(())
}

#[tauri::command]
pub fn focus_session(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let s = state.registry.get(&id)?;
    // Pass claude_pid; the focus impl walks up the parent chain to find a
    // visible-window-owning ancestor (e.g. WindowsTerminal.exe).
    state
        .focus
        .focus(s.claude_pid as u32, s.terminal_window_handle.as_deref())
}

#[tauri::command]
pub fn recent_projects(limit: usize) -> AppResult<Vec<RecentProject>> {
    let root = recent_projects::default_claude_root()?;
    recent_projects::list(&root, limit)
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> AppResult<Config> {
    Ok(state.config.lock().unwrap().clone())
}
