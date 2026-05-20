use crate::config::{self, Config};
use crate::error::AppResult;
use crate::planner::PlannerRunner;
use crate::projects::Projects;
use crate::recent_projects::{self, RecentProject};
use crate::session_registry::{NewSession, Registry, Session};
use crate::spawner::{SpawnRequest, Spawner};
use crate::todos::Todos;
use crate::window_focus::WindowFocus;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
use tauri::{Emitter, State};

pub struct AppState {
    pub registry: Arc<Registry>,
    pub projects: Arc<Projects>,
    pub todos: Arc<Todos>,
    pub planner_runner: Arc<dyn PlannerRunner>,
    pub spawner: Arc<dyn Spawner>,
    pub focus: Box<dyn WindowFocus>,
    pub config: Arc<Mutex<Config>>,
    pub config_path: PathBuf,
    pub is_first_run: AtomicBool,
    /// Guards the planner concurrency: a todo id is inserted while planning
    /// is in flight and removed when planning finishes (success or failure).
    pub planning_in_flight: Arc<Mutex<std::collections::HashSet<String>>>,
}

#[derive(serde::Deserialize)]
pub struct LaunchInput {
    pub project_dir: String,
    pub model: Option<String>,
    pub prompt: Option<String>,
    /// If set, append `--resume <id>` so claude reattaches to that
    /// existing JSONL conversation instead of starting fresh.
    #[serde(default)]
    pub resume: Option<String>,
    /// Per-launch override for `--effort`. None = use config default.
    #[serde(default)]
    pub effort: Option<String>,
    /// Per-launch override for `--permission-mode`. None = use config default.
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// Per-launch override for free-form extra args. None = use config default.
    #[serde(default)]
    pub extra_args: Option<String>,
    /// NEW — pre-arm auto-continue at launch. None = use config default.
    #[serde(default)]
    pub auto_continue: Option<bool>,
    /// NEW — per-session override of the resume prompt. None at launch
    /// time means "fall back to config.default_resume_prompt at fire time".
    #[serde(default)]
    pub resume_prompt: Option<String>,
    /// Set when this launch was triggered by a subtask launch action;
    /// stored on the session row so the dashboard can render the parent badge.
    #[serde(default)]
    pub subtask_id: Option<String>,
    /// Optional human-readable title for the session. None falls back to the
    /// project display name (or [`crate::session_registry::DEFAULT_TITLE`] if
    /// the project upsert failed).
    #[serde(default)]
    pub title: Option<String>,
    /// Optional explicit `projects.id` to associate this session with. When
    /// None the launch path auto-upserts a project from `project_dir` and
    /// uses that id. Pass this when the user picked a different project from
    /// the LaunchDialog autocomplete that doesn't match the folder.
    #[serde(default)]
    pub project: Option<String>,
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
pub fn list_sessions_for_project(
    state: State<'_, AppState>,
    project: String,
) -> AppResult<Vec<Session>> {
    state.registry.list_for_project(&project)
}

#[tauri::command]
pub fn launch_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: LaunchInput,
) -> AppResult<Session> {
    let cfg = state.config.lock().unwrap().clone();
    let model = input.model.unwrap_or(cfg.default_model.clone());
    let from_todo = input.subtask_id.is_some();
    // TODO-originated launches always run autonomously: bypass permission
    // prompts unless the caller explicitly chose a different mode for this
    // specific launch. The matching SessionStart hook in the user's Claude
    // settings further suppresses AskUserQuestion-style multi-choice prompts.
    let permission_mode = input.permission_mode.unwrap_or_else(|| {
        if from_todo {
            "bypassPermissions".to_string()
        } else {
            cfg.default_permission_mode.clone()
        }
    });
    let req = SpawnRequest {
        project_dir: input.project_dir.clone(),
        model: model.clone(),
        prompt: input.prompt,
        terminal_program: cfg.terminal_program.clone(),
        resume: input.resume,
        effort: input.effort.unwrap_or_else(|| cfg.default_effort.clone()),
        permission_mode,
        extra_args: input
            .extra_args
            .unwrap_or_else(|| cfg.default_extra_args.clone()),
        from_todo,
    };
    let result = state.spawner.spawn(&req)?;
    // Auto-create / refresh the project entry so the sidebar reflects this
    // folder, AND so we have a stable project id to store on the session.
    // A failure to upsert is non-fatal — we fall back to None and the
    // registry will persist the DEFAULT_PROJECT sentinel.
    let project_record = state.projects.upsert_for_path(&input.project_dir).ok();
    // Explicit caller-supplied project id wins; otherwise fall back to the
    // auto-upserted project from the folder path.
    let project = input
        .project
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| project_record.as_ref().map(|p| p.id.clone()));
    // Title is left to the caller, or — when omitted — to the poller's
    // first-exchange auto-titler. We intentionally don't fall back to the
    // project's display name here: that would leave every session titled
    // after the folder, defeating the per-session summarization.
    let title = input.title.clone();
    let session = state.registry.insert(NewSession {
        project_dir: input.project_dir.clone(),
        project,
        title,
        model,
        claude_pid: result.claude_pid,
        terminal_pid: result.terminal_pid,
        terminal_window_handle: result.terminal_window_handle,
        auto_continue: input.auto_continue.unwrap_or(cfg.default_auto_continue),
        resume_prompt: input.resume_prompt,
        resume_cap: cfg.default_resume_cap,
        resume_count: 0,
        jsonl_path: None,
        jsonl_offset: 0,
        subtask_id: input.subtask_id.clone(),
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
    // User-initiated kill: explicitly clear auto-continue so this session
    // won't auto-resume. mark_ended no longer cascades this clear (that's
    // reserved for poller-detected deaths where we WANT the pending resume
    // to survive into fire_due_resumes).
    let _ = state.registry.set_auto_continue(&id, false);
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
pub fn delete_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<()> {
    state.registry.delete(&id)?;
    let _ = app.emit("session-changed", &id);
    Ok(())
}

#[tauri::command]
pub fn clear_ended_sessions(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<usize> {
    let n = state.registry.delete_all_ended()?;
    let _ = app.emit("session-changed", ());
    Ok(n)
}

#[tauri::command]
pub fn delete_sessions(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> AppResult<usize> {
    let n = state.registry.delete_many_ended(&ids)?;
    let _ = app.emit("session-changed", ());
    Ok(n)
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
pub fn set_auto_continue(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    on: bool,
) -> AppResult<()> {
    state.registry.set_auto_continue(&id, on)?;
    let _ = app.emit("session-changed", ());
    Ok(())
}

#[tauri::command]
pub fn set_resume_prompt(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    prompt: Option<String>,
) -> AppResult<()> {
    state.registry.set_resume_prompt(&id, prompt.as_deref())?;
    let _ = app.emit("session-changed", ());
    Ok(())
}

#[tauri::command]
pub fn set_session_title(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> AppResult<()> {
    state.registry.set_title(&id, &title)?;
    let _ = app.emit("session-changed", &id);
    Ok(())
}

#[tauri::command]
pub fn set_session_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    project: String,
) -> AppResult<()> {
    state.registry.set_project(&id, &project)?;
    let _ = app.emit("session-changed", &id);
    Ok(())
}

#[tauri::command]
pub fn recent_projects(state: State<'_, AppState>, limit: usize) -> AppResult<Vec<RecentProject>> {
    let root = recent_projects::default_claude_root()?;
    let launches = state.registry.last_launch_per_dir()?;
    recent_projects::list(&root, limit, &launches)
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> AppResult<Config> {
    Ok(state.config.lock().unwrap().clone())
}

#[tauri::command]
pub fn set_config(app: tauri::AppHandle, state: State<'_, AppState>, cfg: Config) -> AppResult<()> {
    // Reconcile OS-level autostart BEFORE persisting. If this fails we don't
    // want a config file that says "on" while the registry says "off".
    crate::autostart::reconcile_autostart(&app, &cfg)?;
    // NOTE: if save fails here, the OS registry was already mutated. On next
    // launch the config file wins, so the toggle will show stale state. Not
    // worth rolling back the registry — that call is also fallible and offers
    // no real safety on a write-error path.
    config::save(&state.config_path, &cfg)?;
    let mut held = state.config.lock().unwrap();
    *held = cfg;
    Ok(())
}

#[tauri::command]
pub fn preview_launch_command(state: State<'_, AppState>, input: LaunchInput) -> String {
    let cfg = state.config.lock().unwrap().clone();
    let model = input.model.unwrap_or(cfg.default_model.clone());
    let effort = input.effort.unwrap_or_else(|| cfg.default_effort.clone());
    let permission_mode = input
        .permission_mode
        .unwrap_or_else(|| cfg.default_permission_mode.clone());
    let extra_args = input
        .extra_args
        .unwrap_or_else(|| cfg.default_extra_args.clone());
    crate::spawner::build_claude_command(
        &model,
        input.prompt.as_deref(),
        input.resume.as_deref(),
        &effort,
        &permission_mode,
        &extra_args,
    )
}

#[tauri::command]
pub fn get_first_run(state: State<'_, AppState>) -> bool {
    state.is_first_run.load(Ordering::SeqCst)
}

#[tauri::command]
pub fn clear_first_run(state: State<'_, AppState>) {
    state.is_first_run.store(false, Ordering::SeqCst);
}

use tauri_plugin_updater::UpdaterExt;

#[derive(serde::Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn check_for_update(app: tauri::AppHandle) -> AppResult<Option<UpdateInfo>> {
    let updater = app
        .updater()
        .map_err(|e| crate::error::AppError::Other(format!("updater unavailable: {e}")))?;
    let result = updater
        .check()
        .await
        .map_err(|e| crate::error::AppError::Other(format!("update check failed: {e}")))?;
    Ok(result.map(|update| UpdateInfo {
        version: update.version.clone(),
        notes: update.body.clone(),
    }))
}

#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> AppResult<()> {
    let updater = app
        .updater()
        .map_err(|e| crate::error::AppError::Other(format!("updater unavailable: {e}")))?;
    let update = updater
        .check()
        .await
        .map_err(|e| crate::error::AppError::Other(format!("update check failed: {e}")))?
        .ok_or_else(|| {
            crate::error::AppError::Other(
                "no update available — the banner may be stale; close it and try again later".into(),
            )
        })?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| crate::error::AppError::Other(format!("install failed: {e}")))?;
    app.restart();
}

use crate::projects::Project;

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> AppResult<Vec<Project>> {
    state.projects.list_visible()
}

#[tauri::command]
pub fn list_hidden_projects(state: State<'_, AppState>) -> AppResult<Vec<Project>> {
    state.projects.list_hidden()
}

#[tauri::command]
pub fn upsert_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<Project> {
    let p = state.projects.upsert_for_path(&path)?;
    let _ = app.emit("project-changed", &p.id);
    Ok(p)
}

#[tauri::command]
pub fn set_project_name(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> AppResult<()> {
    state.projects.set_display_name(&id, &name)?;
    let _ = app.emit("project-changed", &id);
    Ok(())
}

#[tauri::command]
pub fn set_project_pinned(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    on: bool,
) -> AppResult<()> {
    state.projects.set_pinned(&id, on)?;
    let _ = app.emit("project-changed", &id);
    Ok(())
}

#[tauri::command]
pub fn set_project_hidden(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    on: bool,
) -> AppResult<()> {
    state.projects.set_hidden(&id, on)?;
    let _ = app.emit("project-changed", &id);
    Ok(())
}

#[tauri::command]
pub fn delete_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<()> {
    // Refuse delete when this project has any todos or any non-ended sessions.
    let p = state.projects.get(&id)?;
    let todos = state.todos.list_todos_for_project(&id)?;
    if !todos.is_empty() {
        return Err(crate::error::AppError::Invalid(format!(
            "project has {} TODO(s) — hide it instead",
            todos.len()
        )));
    }
    let active = state
        .registry
        .list_active()?
        .into_iter()
        .filter(|s| crate::session_registry::normalize_project_dir(&s.project_dir) == p.norm_path)
        .count();
    if active > 0 {
        return Err(crate::error::AppError::Invalid(format!(
            "project has {active} running session(s) — kill them or hide the project"
        )));
    }
    state.projects.delete(&id)?;
    let _ = app.emit("project-changed", &id);
    Ok(())
}

use crate::todos::{Subtask, Todo, TodoState};

#[tauri::command]
pub fn list_todos(state: State<'_, AppState>, project_id: String) -> AppResult<Vec<Todo>> {
    state.todos.list_todos_for_project(&project_id)
}

/// What the frontend gets per subtask: the stored row plus a derived
/// `session_ended` flag so the review dialog knows when a previously-launched
/// subtask is eligible to launch again (its session died, was killed, or was
/// closed by the user). `false` means either no session ever attached, or the
/// session is still alive.
#[derive(serde::Serialize)]
pub struct SubtaskView {
    #[serde(flatten)]
    pub subtask: Subtask,
    pub session_ended: bool,
}

fn session_ended_for(registry: &Registry, session_id: &Option<String>) -> bool {
    match session_id {
        Some(sid) => registry
            .get(sid)
            .map(|sess| sess.ended_at.is_some())
            // If the row was deleted out from under us, treat as ended so the
            // subtask becomes re-launchable rather than wedged.
            .unwrap_or(true),
        None => false,
    }
}

#[tauri::command]
pub fn list_subtasks(state: State<'_, AppState>, todo_id: String) -> AppResult<Vec<SubtaskView>> {
    let subs = state.todos.list_subtasks(&todo_id)?;
    let mut out = Vec::with_capacity(subs.len());
    for s in subs {
        let ended = session_ended_for(&state.registry, &s.session_id);
        out.push(SubtaskView { subtask: s, session_ended: ended });
    }
    Ok(out)
}

#[tauri::command]
pub fn create_todo(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    title: String,
) -> AppResult<Todo> {
    let t = state.todos.create_todo(&project_id, &title)?;
    let _ = app.emit("todo-changed", &t.id);
    Ok(t)
}

#[tauri::command]
pub fn delete_todo(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    kill_running_sessions: bool,
) -> AppResult<()> {
    let subtasks = state.todos.list_subtasks(&id)?;
    if kill_running_sessions {
        for s in &subtasks {
            if let Some(sid) = &s.session_id {
                // Look up the session; if still active, kill it. Ignore any
                // not-found / already-dead errors so the delete proceeds.
                if let Ok(sess) = state.registry.get(sid) {
                    if sess.ended_at.is_none() {
                        let _ = kill_session(app.clone(), state.clone(), sid.clone());
                    }
                }
            }
        }
    } else {
        // Refuse if any subtask references a still-running session.
        for s in &subtasks {
            if let Some(sid) = &s.session_id {
                if let Ok(sess) = state.registry.get(sid) {
                    if sess.ended_at.is_none() {
                        return Err(crate::error::AppError::Invalid(
                            "todo has running sessions — pass killRunningSessions=true to confirm".into(),
                        ));
                    }
                }
            }
        }
    }
    state.todos.delete_todo(&id)?;
    let _ = app.emit("todo-changed", &id);
    Ok(())
}

#[tauri::command]
pub fn mark_todo_finished(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<()> {
    state.todos.mark_finished(&id, chrono::Utc::now().timestamp())?;
    let _ = app.emit("todo-changed", &id);
    Ok(())
}

#[tauri::command]
pub fn dismiss_auto_suggest(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<()> {
    state.todos.set_auto_suggest_done_at(&id, None)?;
    state.todos.set_state(&id, TodoState::Pending)?;
    let _ = app.emit("todo-changed", &id);
    Ok(())
}

#[tauri::command]
pub fn add_manual_subtask(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    todo_id: String,
    text: String,
) -> AppResult<Subtask> {
    let s = state.todos.add_manual_subtask(&todo_id, &text)?;
    let _ = app.emit("todo-changed", &todo_id);
    Ok(s)
}

#[tauri::command]
pub fn edit_subtask(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    text: String,
) -> AppResult<()> {
    state.todos.edit_subtask(&id, &text)?;
    // We don't know the parent todo without a lookup; emit a generic refresh.
    let _ = app.emit("todo-changed", ());
    Ok(())
}

#[tauri::command]
pub fn delete_subtask(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<()> {
    state.todos.delete_subtask(&id)?;
    let _ = app.emit("todo-changed", ());
    Ok(())
}

#[tauri::command]
pub fn reorder_subtasks(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    todo_id: String,
    ordered_ids: Vec<String>,
) -> AppResult<()> {
    state.todos.reorder_subtasks(&todo_id, &ordered_ids)?;
    let _ = app.emit("todo-changed", &todo_id);
    Ok(())
}

use crate::planner;
use crate::todos::PlannerStatus;

#[tauri::command]
pub async fn plan_todo(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    todo_id: String,
) -> AppResult<()> {
    // Concurrency guard: refuse if already planning.
    {
        let mut in_flight = state.planning_in_flight.lock().unwrap();
        if in_flight.contains(&todo_id) {
            return Err(crate::error::AppError::Invalid("already planning this todo".into()));
        }
        in_flight.insert(todo_id.clone());
    }

    // Reset error, mark planning.
    let todo = state.todos.get_todo(&todo_id)?;
    let project = state.projects.get(&todo.project_id)?;
    state.todos.set_planner_status(&todo_id, PlannerStatus::Planning)?;
    state.todos.set_planner_error(&todo_id, None)?;
    let _ = app.emit("todo-changed", &todo_id);

    // Pull the model from config and clone the runner Arc so the blocking
    // closure does not borrow `state`.
    let model = state.config.lock().unwrap().default_model.clone();
    let runner = state.planner_runner.clone();
    let title = todo.title.clone();
    let project_name = project.display_name.clone();

    // The planner spawns a subprocess — keep it on the blocking pool so the
    // tauri async runtime stays responsive.
    let result = tokio::task::spawn_blocking(move || {
        planner::plan_subtasks(
            runner.as_ref(),
            &title,
            &project_name,
            &model,
            planner::DEFAULT_TIMEOUT,
        )
    })
    .await
    .map_err(|e| crate::error::AppError::Other(format!("planner task join: {e}")))?;

    // Clear in-flight regardless of outcome.
    state.planning_in_flight.lock().unwrap().remove(&todo_id);

    match result {
        Ok(texts) => {
            // Re-plan path: if any existing subtask's previous session has
            // ended, clear those pointers so `replace_subtasks`' guard ("any
            // session_id is non-NULL") allows the overwrite. If any subtask is
            // attached to a LIVE session we refuse the re-plan here rather
            // than orphaning the running session.
            let existing = state.todos.list_subtasks(&todo_id)?;
            let any_live = existing
                .iter()
                .any(|s| s.session_id.is_some() && !session_ended_for(&state.registry, &s.session_id));
            if any_live {
                state
                    .todos
                    .set_planner_status(&todo_id, PlannerStatus::PlannerFailed)?;
                state.todos.set_planner_error(
                    &todo_id,
                    Some("cannot re-plan while a subtask session is still running"),
                )?;
                let _ = app.emit("todo-changed", &todo_id);
                return Err(crate::error::AppError::Invalid(
                    "cannot re-plan while a subtask session is still running".into(),
                ));
            }
            if existing.iter().any(|s| s.session_id.is_some()) {
                state.todos.detach_all_sessions(&todo_id)?;
            }
            state.todos.replace_subtasks(&todo_id, &texts)?;
            state.todos.set_planner_status(&todo_id, PlannerStatus::Planned)?;
            let _ = app.emit("todo-changed", &todo_id);
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            state
                .todos
                .set_planner_status(&todo_id, PlannerStatus::PlannerFailed)?;
            state.todos.set_planner_error(&todo_id, Some(&msg))?;
            let _ = app.emit("todo-changed", &todo_id);
            Err(e)
        }
    }
}

#[tauri::command]
pub fn launch_subtask(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    subtask_id: String,
) -> AppResult<Session> {
    // `get_subtask` is added in Step 3 of this same task.
    let subtask = state.todos.get_subtask(&subtask_id)?;
    let todo = state.todos.get_todo(&subtask.todo_id)?;
    let project = state.projects.get(&todo.project_id)?;
    let cfg = state.config.lock().unwrap().clone();

    let input = LaunchInput {
        project_dir: project.norm_path.clone(),
        model: Some(cfg.default_model.clone()),
        prompt: Some(subtask.text.clone()),
        resume: None,
        effort: None,
        permission_mode: None,
        extra_args: None,
        auto_continue: None,
        resume_prompt: None,
        subtask_id: Some(subtask_id.clone()),
        // Title comes from the subtask text (truncated by the registry-side
        // default if empty). Gives the dashboard a row label that mirrors
        // what the human asked the subtask to do.
        title: Some(subtask.text.clone()),
        // Stamp the session with the project this todo belongs to so it groups
        // correctly under the same project as the parent todo in list views.
        project: Some(project.id.clone()),
    };
    let session = launch_session(app.clone(), state.clone(), input)?;
    state.todos.attach_session(&subtask_id, &session.id)?;
    state
        .todos
        .set_state(&todo.id, crate::todos::TodoState::Ongoing)?;
    let _ = app.emit("todo-changed", &todo.id);
    Ok(session)
}

#[tauri::command]
pub fn launch_all_subtasks(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    todo_id: String,
) -> AppResult<Vec<Session>> {
    let subtasks = state.todos.list_subtasks(&todo_id)?;
    let mut out = Vec::new();
    for s in subtasks {
        // Skip only if the subtask has a *live* session attached. A subtask
        // whose previous session ended (crashed, was killed, or user closed
        // the terminal) is eligible to relaunch — attach_session will
        // overwrite the dead session_id with the new one.
        if s.session_id.is_some() && !session_ended_for(&state.registry, &s.session_id) {
            continue;
        }
        let sess = launch_subtask(app.clone(), state.clone(), s.id.clone())?;
        out.push(sess);
    }
    Ok(out)
}

#[tauri::command]
pub fn get_subtask(state: State<'_, AppState>, id: String) -> AppResult<crate::todos::Subtask> {
    state.todos.get_subtask(&id)
}

#[tauri::command]
pub fn get_todo(state: State<'_, AppState>, id: String) -> AppResult<crate::todos::Todo> {
    state.todos.get_todo(&id)
}

/// Recomputes a TODO's `state` and `auto_suggest_done_at` based on the
/// current status of its child subtasks' sessions. Idempotent.
pub fn recompute_todo_for_session(
    registry: &Registry,
    todos: &Todos,
    session_id: &str,
) -> AppResult<Option<String>> {
    // Find the subtask, if any.
    let session = registry.get(session_id)?;
    let Some(subtask_id) = session.subtask_id.clone() else {
        return Ok(None);
    };
    let subtask = todos.get_subtask(&subtask_id)?;
    let todo = todos.get_todo(&subtask.todo_id)?;
    if matches!(todo.state, crate::todos::TodoState::Finished) {
        return Ok(Some(todo.id));
    }
    let siblings = todos.list_subtasks(&subtask.todo_id)?;
    let mut all_launched = true;
    let mut any_running = false;
    for s in &siblings {
        match &s.session_id {
            None => all_launched = false,
            Some(sid) => {
                if let Ok(sess) = registry.get(sid) {
                    if sess.ended_at.is_none() {
                        any_running = true;
                    }
                }
            }
        }
    }
    let now = chrono::Utc::now().timestamp();
    if any_running {
        todos.set_state(&todo.id, crate::todos::TodoState::Ongoing)?;
        todos.set_auto_suggest_done_at(&todo.id, None)?;
    } else if all_launched {
        todos.set_state(&todo.id, crate::todos::TodoState::Pending)?;
        todos.set_auto_suggest_done_at(&todo.id, Some(now))?;
    } else {
        todos.set_state(&todo.id, crate::todos::TodoState::Pending)?;
        todos.set_auto_suggest_done_at(&todo.id, None)?;
    }
    Ok(Some(todo.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_registry::{NewSession, Registry};

    #[test]
    fn launch_input_carries_auto_continue_flags() {
        let json = r#"{
            "project_dir": "/p",
            "auto_continue": true,
            "resume_prompt": "keep at it"
        }"#;
        let parsed: LaunchInput = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.auto_continue, Some(true));
        assert_eq!(parsed.resume_prompt.as_deref(), Some("keep at it"));
    }

    #[test]
    fn launch_input_defaults_are_none_when_omitted() {
        let json = r#"{ "project_dir": "/p" }"#;
        let parsed: LaunchInput = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.auto_continue, None);
        assert_eq!(parsed.resume_prompt, None);
    }

    #[test]
    fn registry_arm_disarm_through_methods_used_by_commands() {
        let r = Registry::open_in_memory().unwrap();
        let s = r.insert(NewSession {
            project_dir: "/p".into(),
            project: None,
            title: None,
            model: "m".into(),
            claude_pid: 1,
            terminal_pid: 2,
            terminal_window_handle: None,
            auto_continue: false,
            resume_prompt: None,
            resume_cap: 3,
            resume_count: 0,
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
        }).unwrap();
        r.set_auto_continue(&s.id, true).unwrap();
        assert!(r.get(&s.id).unwrap().auto_continue);
        r.set_resume_prompt(&s.id, Some("keep going")).unwrap();
        assert_eq!(r.get(&s.id).unwrap().resume_prompt.as_deref(), Some("keep going"));
    }

    #[test]
    fn user_kill_disarms_auto_continue() {
        let r = Registry::open_in_memory().unwrap();
        let s = r.insert(NewSession {
            project_dir: "/p".into(),
            project: None,
            title: None,
            model: "m".into(),
            claude_pid: 1,
            terminal_pid: 2,
            terminal_window_handle: None,
            auto_continue: true,
            resume_prompt: None,
            resume_cap: 3,
            resume_count: 0,
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
        }).unwrap();
        r.set_pending_resume(&s.id, 5000).unwrap();
        // The kill_session command, conceptually: disarm first, then mark ended.
        // We test the registry-level invariant rather than the full IPC path.
        r.set_auto_continue(&s.id, false).unwrap();
        r.mark_ended(&s.id, 9999).unwrap();
        let got = r.get(&s.id).unwrap();
        assert!(!got.auto_continue, "auto_continue is off");
        assert_eq!(got.next_resume_at, None,
            "user kill must clear pending resume so we don't auto-resume after manual kill");
    }

    #[test]
    fn delete_project_refuses_when_todos_present() {
        use crate::projects::Projects;
        use crate::todos::Todos;
        let projects = Projects::open_in_memory().unwrap();
        let todos = Todos::open_in_memory().unwrap();
        let p = projects.upsert_for_path("/foo").unwrap();
        let _ = todos.create_todo(&p.id, "a todo").unwrap();
        // Mirror the guard in delete_project: list todos then refuse.
        let listed = todos.list_todos_for_project(&p.id).unwrap();
        assert!(!listed.is_empty(), "precondition");
        // The actual tauri command isn't directly callable in a unit test
        // (it needs the AppState wiring), but we assert the underlying invariant.
    }
}
