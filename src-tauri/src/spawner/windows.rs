use super::{SpawnRequest, SpawnResult, Spawner};
use crate::error::{AppError, AppResult};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
};

pub struct WindowsSpawner {
    path_lookup: Box<dyn crate::spawner::PathLookup>,
}

impl WindowsSpawner {
    pub fn new() -> Self {
        Self { path_lookup: Box::new(crate::spawner::EnvPathLookup) }
    }

    #[cfg(test)]
    pub fn with_lookup(lookup: Box<dyn crate::spawner::PathLookup>) -> Self {
        Self { path_lookup: lookup }
    }
}

impl Default for WindowsSpawner {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Clone)]
enum TerminalChoice {
    /// Windows Terminal — `wt.exe -w new -d <dir> cmd.exe /K <cmd>`. `-w new`
    /// forces a fresh top-level window so each session has its own HWND we can
    /// close on Kill without affecting the user's other tabs.
    WindowsTerminal(PathBuf),
    /// cmd.exe fallback — `cmd.exe /C start cmd.exe /K <cmd>`, run with cwd set.
    /// Each invocation gets its own console window owned by conhost.exe.
    Cmd,
    /// Explicit user-supplied program — args mirror Windows Terminal style.
    Custom(PathBuf),
}

/// Process names we never want to track as the session leaf.
const BLOCKED_NAMES: &[&str] = &[
    "wt.exe",
    "windowsterminal.exe",
    "openconsole.exe",
    "conhost.exe",
];

/// Process names that own the visible terminal window we want to find.
const HOST_NAMES: &[&str] = &[
    "windowsterminal.exe",
    "openconsole.exe",
    "conhost.exe",
];

/// Build the argv (after the executable) passed to Windows Terminal for a
/// given spawn request. Pure function so we can unit-test argv shape.
///
/// Argv order matters for wt: global flags (`-w`) come first, then per-tab
/// flags (`-d`, `--title`), then the command. Putting `--title` ahead of
/// `-w new` makes wt drop the rest and the spawned cmd never runs.
pub(crate) fn build_wt_argv(req: &SpawnRequest) -> Vec<String> {
    let project_name = std::path::Path::new(&req.project_dir)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("session");
    let claude_cmd = build_claude_command(&req.model, req.prompt.as_deref());
    let claude_args = shlex::split(&claude_cmd).unwrap_or_default();
    let mut argv: Vec<String> = vec![
        "-w".into(),
        "new".into(),
        "-d".into(),
        req.project_dir.clone(),
        "--title".into(),
        format!("FastClaude: {project_name}"),
        "cmd.exe".into(),
        "/K".into(),
    ];
    argv.extend(claude_args);
    argv
}

impl Spawner for WindowsSpawner {
    fn spawn(&self, req: &SpawnRequest) -> AppResult<SpawnResult> {
        if self.path_lookup.find("claude").is_none() {
            return Err(AppError::ClaudeNotOnPath);
        }
        let choice = resolve_terminal(&req.terminal_program)?;

        let mut cmd = match &choice {
            TerminalChoice::WindowsTerminal(path) => {
                // -w new forces a new top-level window so each session is its
                // own HWND and Kill can close just this window.
                let mut c = Command::new(path);
                c.args(build_wt_argv(req));
                c
            }
            TerminalChoice::Cmd => {
                let claude_cmd = build_claude_command(&req.model, req.prompt.as_deref());
                let inner = format!("start \"FastClaude\" cmd.exe /K {claude_cmd}");
                let mut c = Command::new("cmd.exe");
                c.args(["/C", &inner]);
                c.current_dir(&req.project_dir);
                c
            }
            TerminalChoice::Custom(path) => {
                let mut c = Command::new(path);
                c.args(build_wt_argv(req));
                c
            }
        };
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Snapshot existing host-window HWNDs *before* spawn so we can identify
        // the one our launch creates.
        let pre_hwnds = enumerate_host_windows();

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

        // Wait briefly for the new window to appear and grab its HWND.
        let new_hwnd = wait_for_new_host_window(&pre_hwnds, Duration::from_secs(3));

        Ok(SpawnResult {
            claude_pid: claude_pid as i64,
            terminal_pid: terminal_pid as i64,
            terminal_window_handle: new_hwnd.map(|h| h.0.to_string()),
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
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let alias = PathBuf::from(local)
            .join("Microsoft")
            .join("WindowsApps")
            .join("wt.exe");
        if alias.exists() {
            return Some(alias);
        }
    }
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

fn wait_for_claude(project_dir: &str, spawn_time: u64, deadline: Duration) -> AppResult<u32> {
    let start = Instant::now();
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    while start.elapsed() < deadline {
        sys.refresh_processes();
        let mut best: Option<(Pid, u64)> = None;
        for (pid, proc) in sys.processes() {
            if proc.start_time() + 1 < spawn_time {
                continue;
            }
            let name = proc.name().to_lowercase();
            if BLOCKED_NAMES.iter().any(|b| name == *b) {
                continue;
            }
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
         The terminal window should still be open — check it for an error \
         message from claude (e.g. authentication, unknown model name).",
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

/// Enumerate all visible top-level windows whose owning process is one of
/// the known terminal-host names.
fn enumerate_host_windows() -> HashSet<isize> {
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    let host_pids: HashSet<u32> = sys
        .processes()
        .iter()
        .filter(|(_, p)| HOST_NAMES.iter().any(|h| p.name().to_lowercase() == *h))
        .map(|(pid, _)| pid.as_u32())
        .collect();

    struct State {
        host_pids: HashSet<u32>,
        hwnds: HashSet<isize>,
    }
    extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let st = &mut *(lparam.0 as *mut State);
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut wpid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut wpid));
            if st.host_pids.contains(&wpid) {
                st.hwnds.insert(hwnd.0 as isize);
            }
            BOOL(1)
        }
    }
    let mut state = State {
        host_pids,
        hwnds: HashSet::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut state as *mut _ as isize));
    }
    state.hwnds
}

fn wait_for_new_host_window(pre: &HashSet<isize>, deadline: Duration) -> Option<HWND> {
    let start = Instant::now();
    while start.elapsed() < deadline {
        let now = enumerate_host_windows();
        let new: Vec<isize> = now.difference(pre).copied().collect();
        if !new.is_empty() {
            // If multiple new windows appeared, pick any — they're all ours.
            return Some(HWND(new[0]));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(project_dir: &str) -> SpawnRequest {
        SpawnRequest {
            project_dir: project_dir.into(),
            model: "claude-opus-4-7".into(),
            prompt: None,
            terminal_program: "auto".into(),
        }
    }

    #[test]
    fn build_wt_argv_preserves_existing_shape() {
        let argv = build_wt_argv(&req("C:\\proj"));
        // Global flags first, then per-tab flags, then the command.
        assert_eq!(&argv[0..4], &["-w", "new", "-d", "C:\\proj"]);
        assert_eq!(&argv[4..6], &["--title", "FastClaude: proj"]);
        assert_eq!(&argv[6..8], &["cmd.exe", "/K"]);
        assert!(argv.iter().any(|a| a.contains("claude")));
        assert!(argv.iter().any(|a| a.contains("--model")));
        assert!(argv.iter().any(|a| a == "claude-opus-4-7"));
    }

    #[test]
    fn build_wt_argv_includes_prompt_when_set() {
        let mut r = req("C:\\proj");
        r.prompt = Some("hello world".into());
        let argv = build_wt_argv(&r);
        // shlex passes "hello world" as a single quoted token after splitting
        let blob = argv.join(" ");
        assert!(blob.contains("hello"), "prompt text in argv");
    }

    #[test]
    fn build_wt_argv_includes_title_with_project_basename() {
        let argv = build_wt_argv(&req("C:\\GitProjects\\FastClaude"));
        let title_idx = argv.iter().position(|a| a == "--title").expect("--title present");
        assert_eq!(argv[title_idx + 1], "FastClaude: FastClaude");
    }

    #[test]
    fn build_wt_argv_title_uses_basename_for_unix_style_paths() {
        let argv = build_wt_argv(&req("/home/u/cool-project"));
        let title_idx = argv.iter().position(|a| a == "--title").unwrap();
        assert_eq!(argv[title_idx + 1], "FastClaude: cool-project");
    }

    #[test]
    fn build_wt_argv_title_falls_back_when_basename_empty() {
        // Trailing slash / drive root — basename returns None
        let argv = build_wt_argv(&req("C:\\"));
        let title_idx = argv.iter().position(|a| a == "--title").unwrap();
        assert_eq!(argv[title_idx + 1], "FastClaude: session");
    }

    #[test]
    fn spawn_returns_claude_not_on_path_when_missing() {
        use crate::spawner::PathLookup;
        use std::path::PathBuf;

        struct Missing;
        impl PathLookup for Missing {
            fn find(&self, _exe: &str) -> Option<PathBuf> { None }
        }

        let spawner = WindowsSpawner::with_lookup(Box::new(Missing));
        let req = SpawnRequest {
            project_dir: "C:\\proj".into(),
            model: "claude-opus-4-7".into(),
            prompt: None,
            terminal_program: "wt".into(),
        };
        let err = spawner.spawn(&req).unwrap_err();
        assert!(matches!(err, AppError::ClaudeNotOnPath), "expected ClaudeNotOnPath, got {err:?}");
    }
}
