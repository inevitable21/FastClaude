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

        let _child = cmd.spawn().map_err(|e| {
            AppError::Spawn(format!(
                "failed to launch terminal ({choice:?}): {e}. \
                 Tip: install Windows Terminal from the Microsoft Store, or set \
                 a custom terminal program in Settings."
            ))
        })?;
        drop(_child);

        let claude_pid = wait_for_claude(&req.project_dir, Duration::from_secs(5))?;
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

fn wait_for_claude(project_dir: &str, deadline: Duration) -> AppResult<u32> {
    let start = Instant::now();
    let target = std::path::PathBuf::from(project_dir);
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    while start.elapsed() < deadline {
        sys.refresh_processes();
        for (pid, proc) in sys.processes() {
            let name = proc.name().to_lowercase();
            if !(name == "claude.exe" || name == "claude") {
                continue;
            }
            if let Some(cwd) = proc.cwd() {
                if cwd == target {
                    return Ok(pid.as_u32());
                }
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Err(AppError::Spawn(format!(
        "did not see claude process for {project_dir} within timeout"
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
