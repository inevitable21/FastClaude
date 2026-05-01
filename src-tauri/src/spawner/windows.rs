use super::{SpawnRequest, SpawnResult, Spawner};
use crate::error::{AppError, AppResult};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

pub struct WindowsSpawner;

#[derive(Debug, Clone)]
enum TerminalChoice {
    /// Windows Terminal — `wt.exe -d <dir> <cmd...>`
    WindowsTerminal(PathBuf),
    /// cmd.exe fallback — `cmd.exe /C start cmd.exe /K <cmd>`, run with cwd set.
    Cmd,
    /// Explicit user-supplied program — args are `<dir> <cmd...>` style.
    Custom(PathBuf),
}

impl Spawner for WindowsSpawner {
    fn spawn(&self, req: &SpawnRequest) -> AppResult<SpawnResult> {
        let claude_cmd = build_claude_command(&req.model, req.prompt.as_deref());
        let claude_args: Vec<String> = shlex::split(&claude_cmd).unwrap_or_default();
        let choice = resolve_terminal(&req.terminal_program)?;

        let mut cmd = match &choice {
            TerminalChoice::WindowsTerminal(path) => {
                let mut c = Command::new(path);
                c.args(["-d", &req.project_dir]);
                for tok in &claude_args {
                    c.arg(tok);
                }
                c
            }
            TerminalChoice::Cmd => {
                // `start` opens a new window; `/K` keeps it open after claude exits
                // so the user can read any errors. cwd is set on Command itself.
                let inner = format!("start \"FastClaude\" cmd.exe /K {claude_cmd}");
                let mut c = Command::new("cmd.exe");
                c.args(["/C", &inner]);
                c.current_dir(&req.project_dir);
                c
            }
            TerminalChoice::Custom(path) => {
                let mut c = Command::new(path);
                c.args(["-d", &req.project_dir]);
                for tok in &claude_args {
                    c.arg(tok);
                }
                c
            }
        };
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let spawn_time = chrono::Utc::now().timestamp() as u64;
        let _child = cmd.spawn().map_err(|e| {
            AppError::Spawn(format!(
                "failed to launch terminal ({choice:?}): {e}. \
                 Tip: install Windows Terminal from the Microsoft Store, or set \
                 a custom terminal program in Settings."
            ))
        })?;
        drop(_child);

        let claude_pid = wait_for_claude(&req.project_dir, spawn_time, Duration::from_secs(10))?;
        let terminal_pid = parent_pid_of(claude_pid).unwrap_or(claude_pid);

        Ok(SpawnResult {
            claude_pid: claude_pid as i64,
            terminal_pid: terminal_pid as i64,
            terminal_window_handle: None,
        })
    }
}

fn resolve_terminal(setting: &str) -> AppResult<TerminalChoice> {
    if setting != "auto" {
        return Ok(TerminalChoice::Custom(PathBuf::from(setting)));
    }
    if let Some(path) = find_wt_exe() {
        return Ok(TerminalChoice::WindowsTerminal(path));
    }
    Ok(TerminalChoice::Cmd)
}

fn find_wt_exe() -> Option<PathBuf> {
    // App Execution Alias path — present on Windows 11 if Windows Terminal is installed.
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let alias = PathBuf::from(local).join("Microsoft").join("WindowsApps").join("wt.exe");
        if alias.exists() {
            return Some(alias);
        }
    }
    // PATH lookup as a last resort.
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("wt.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

fn build_claude_command(model: &str, prompt: Option<&str>) -> String {
    match prompt {
        Some(p) if !p.is_empty() => {
            format!("claude --model {model} {}", shell_escape::escape(p.into()))
        }
        _ => format!("claude --model {model}"),
    }
}

/// Find the claude process spawned by our terminal launch.
///
/// sysinfo on Windows often can't read another process's `cwd` or full `cmd`
/// for processes outside our session. So we use a more permissive strategy:
/// look for any process started at or after `spawn_time` whose name, exe path,
/// or cmd args mention "claude". If multiple match (the .cmd wrapper plus the
/// underlying node.exe), prefer the leaf — but accept any match within window.
///
/// `project_dir` is currently used only for the error message; we no longer
/// require cwd to match because that filter was too strict.
fn wait_for_claude(project_dir: &str, spawn_time: u64, deadline: Duration) -> AppResult<u32> {
    let start = Instant::now();
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    while start.elapsed() < deadline {
        sys.refresh_processes();
        let mut best: Option<(Pid, u64)> = None;
        for (pid, proc) in sys.processes() {
            // Allow 1s slack on the lower bound — process start_time is in seconds
            // and ours can be a hair late.
            if proc.start_time() + 1 < spawn_time {
                continue;
            }
            let name = proc.name().to_lowercase();
            let exe = proc
                .exe()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let cmd_blob = proc.cmd().join(" ").to_lowercase();
            let mentions_claude = name.contains("claude")
                || exe.contains("claude")
                || cmd_blob.contains("claude");
            if !mentions_claude {
                continue;
            }
            let started = proc.start_time();
            if best.map_or(true, |(_, t)| started >= t) {
                best = Some((*pid, started));
            }
        }
        if let Some((pid, _)) = best {
            return Ok(pid.as_u32());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(AppError::Spawn(format!(
        "did not see claude process for {project_dir} within {}s. \
         Open the terminal that just launched and confirm 'claude' actually started \
         (it may have failed silently — check for an error message in that terminal).",
        deadline.as_secs()
    )))
}

fn parent_pid_of(pid: u32) -> Option<u32> {
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    sys.process(Pid::from_u32(pid))
        .and_then(|p| p.parent())
        .map(|pp| pp.as_u32())
}
