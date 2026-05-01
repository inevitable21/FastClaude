use super::{SpawnRequest, SpawnResult, Spawner};
use crate::error::{AppError, AppResult};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

pub struct WindowsSpawner;

impl Spawner for WindowsSpawner {
    fn spawn(&self, req: &SpawnRequest) -> AppResult<SpawnResult> {
        let program = if req.terminal_program == "auto" {
            "wt.exe"
        } else {
            req.terminal_program.as_str()
        };

        let claude_cmd = build_claude_command(&req.model, req.prompt.as_deref());

        let mut cmd = Command::new(program);
        cmd.args(["-d", &req.project_dir]);
        for tok in shlex::split(&claude_cmd).unwrap_or_default() {
            cmd.arg(tok);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let _child = cmd
            .spawn()
            .map_err(|e| AppError::Spawn(format!("failed to launch {program}: {e}")))?;
        // wt.exe exits immediately on Windows Terminal — _child.id() is unreliable.
        drop(_child);

        let claude_pid = wait_for_claude(&req.project_dir, Duration::from_secs(3))?;
        let terminal_pid = parent_pid_of(claude_pid).unwrap_or(claude_pid);

        Ok(SpawnResult {
            claude_pid: claude_pid as i64,
            terminal_pid: terminal_pid as i64,
            terminal_window_handle: None,
        })
    }
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
