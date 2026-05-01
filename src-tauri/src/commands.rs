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
    pub config: Arc<Mutex<Config>>,
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
    // First try to close the terminal window politely via WM_CLOSE on the HWND
    // we captured at spawn time. This makes wt and conhost shut down their
    // window without us having to TerminateProcess things.
    if let Some(handle_str) = s.terminal_window_handle.as_deref() {
        if let Ok(hwnd_isize) = handle_str.parse::<isize>() {
            close_window_handle(hwnd_isize);
        }
    }
    // Backstop: kill the process chain too in case WM_CLOSE was ignored.
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    sys.refresh_processes();
    kill_session_chain(&sys, s.claude_pid as u32);
    state
        .registry
        .mark_ended(&id, chrono::Utc::now().timestamp())?;
    let _ = app.emit("session-changed", &id);
    Ok(())
}

#[cfg(target_os = "windows")]
fn close_window_handle(hwnd_isize: isize) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    unsafe {
        let _ = PostMessageW(hwnd_isize as HWND, WM_CLOSE, WPARAM::default(), LPARAM::default());
    }
}

#[cfg(not(target_os = "windows"))]
fn close_window_handle(_hwnd_isize: isize) {}

/// Walk up the parent chain killing every process until we hit a host or
/// system-critical process. This closes our specific terminal tab/window
/// without touching multi-tab hosts (WindowsTerminal) or system services.
///
/// Typical chains we kill through:
///   wt:   node.exe → cmd.exe → OpenConsole.exe   (stops at WindowsTerminal.exe)
///   cmd:  node.exe → cmd.exe → conhost.exe       (stops at csrss.exe / orphan)
fn kill_session_chain(sys: &System, claude_pid: u32) {
    /// Walk stops here — never killed.
    const STOP_AT: &[&str] = &[
        "windowsterminal.exe",
        "explorer.exe",
        "csrss.exe",
        "services.exe",
        "wininit.exe",
        "smss.exe",
        "winlogon.exe",
        "system",
        "system idle process",
    ];

    let mut to_kill: Vec<Pid> = Vec::new();
    let mut current = Some(Pid::from_u32(claude_pid));
    for _ in 0..6 {
        let Some(pid) = current else { break };
        let Some(proc) = sys.process(pid) else { break };
        let name = proc.name().to_lowercase();
        if STOP_AT.iter().any(|h| name == *h) {
            break;
        }
        to_kill.push(pid);
        current = proc.parent();
    }

    // Kill outermost first so the conpty/console host disconnects before
    // the inner shell exits — closes the window cleanly.
    for pid in to_kill.iter().rev() {
        if let Some(p) = sys.process(*pid) {
            p.kill();
        }
    }
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
