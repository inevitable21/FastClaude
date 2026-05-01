use super::{SpawnRequest, SpawnResult, Spawner};
use crate::error::{AppError, AppResult};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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

/// Build the argv (after the executable) passed to Windows Terminal.
///
/// `command` is what cmd.exe runs after `/K` — typically the path to a
/// per-launch wrapper .bat file (see `write_launcher_bat`) which redirects
/// claude's stderr so we can surface real error messages instead of a
/// generic timeout when claude exits early (bad model, auth failure, etc.).
///
/// Argv order matters for wt: global flags (`-w`) come first, then per-tab
/// flags (`-d`, `--title`), then the command. Putting `--title` ahead of
/// `-w new` makes wt drop the rest and the spawned cmd never runs.
pub(crate) fn build_wt_argv(req: &SpawnRequest, command: &str) -> Vec<String> {
    let project_name = std::path::Path::new(&req.project_dir)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("session");
    vec![
        "-w".into(),
        "new".into(),
        "-d".into(),
        req.project_dir.clone(),
        "--title".into(),
        format!("FastClaude: {project_name}"),
        // claude CLI emits its own ANSI title escape ("Claude Code") on
        // startup that overwrites --title; this flag tells wt to ignore it.
        "--suppressApplicationTitle".into(),
        "cmd.exe".into(),
        "/K".into(),
        command.into(),
    ]
}

/// Write a per-launch wrapper batch file that runs claude with stderr
/// redirected to `err_path`. Lets `wait_for_claude` surface claude's actual
/// failure message in the toast when the process exits early.
fn write_launcher_bat(bat_path: &Path, err_path: &Path, req: &SpawnRequest) -> AppResult<()> {
    let claude_cmd = build_claude_command(&req.model, req.prompt.as_deref(), req.resume.as_deref());
    let content = format!(
        "@echo off\r\n{} 2> \"{}\"\r\n",
        claude_cmd,
        err_path.display()
    );
    std::fs::write(bat_path, content)?;
    Ok(())
}

impl Spawner for WindowsSpawner {
    fn spawn(&self, req: &SpawnRequest) -> AppResult<SpawnResult> {
        if self.path_lookup.find("claude").is_none() {
            return Err(AppError::ClaudeNotOnPath);
        }
        let choice = resolve_terminal(&req.terminal_program)?;

        // Per-launch wrapper bat + err file under %TEMP%. The bat invokes
        // claude with `2> "<err>"` so wait_for_claude can surface claude's
        // own error message if it exits early.
        let launch_id = uuid::Uuid::new_v4();
        let temp = std::env::temp_dir();
        let bat_path = temp.join(format!("fastclaude-{launch_id}.bat"));
        let err_path = temp.join(format!("fastclaude-{launch_id}.err"));
        write_launcher_bat(&bat_path, &err_path, req)?;
        let bat_str = bat_path.to_string_lossy().to_string();

        let mut cmd = match &choice {
            TerminalChoice::WindowsTerminal(path) => {
                // -w new forces a new top-level window so each session is its
                // own HWND and Kill can close just this window.
                let mut c = Command::new(path);
                c.args(build_wt_argv(req, &bat_str));
                c
            }
            TerminalChoice::Cmd => {
                let inner = format!("start \"FastClaude\" cmd.exe /K \"{bat_str}\"");
                let mut c = Command::new("cmd.exe");
                c.args(["/C", &inner]);
                c.current_dir(&req.project_dir);
                c
            }
            TerminalChoice::Custom(path) => {
                let mut c = Command::new(path);
                c.args(build_wt_argv(req, &bat_str));
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
            let _ = std::fs::remove_file(&bat_path);
            AppError::Spawn(format!(
                "failed to launch terminal ({choice:?}): {e}. \
                 Tip: install Windows Terminal from the Microsoft Store, or set \
                 a custom terminal program in Settings."
            ))
        })?;
        drop(_child);

        let claude_pid = match wait_for_claude(
            &req.project_dir,
            spawn_time,
            Duration::from_secs(10),
            &err_path,
        ) {
            Ok(pid) => pid,
            Err(e) => {
                let _ = std::fs::remove_file(&bat_path);
                let _ = std::fs::remove_file(&err_path);
                return Err(e);
            }
        };
        let terminal_pid = parent_pid_of(claude_pid).unwrap_or(claude_pid);

        // Wait briefly for the new window to appear and grab its HWND.
        let new_hwnd = wait_for_new_host_window(&pre_hwnds, Duration::from_secs(3));

        // Bat already executed (cmd /K kept the shell open after the bat
        // returned). Safe to delete now. The err file stays — claude's
        // session may still write to it.
        let _ = std::fs::remove_file(&bat_path);

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

fn build_claude_command(model: &str, prompt: Option<&str>, resume: Option<&str>) -> String {
    let mut cmd = format!("claude --model {model}");
    if let Some(id) = resume.filter(|s| !s.is_empty()) {
        cmd.push_str(&format!(" --resume {}", shell_escape::escape(id.into())));
    }
    if let Some(p) = prompt.filter(|s| !s.is_empty()) {
        cmd.push_str(&format!(" {}", shell_escape::escape(p.into())));
    }
    cmd
}

fn wait_for_claude(
    project_dir: &str,
    spawn_time: u64,
    deadline: Duration,
    err_path: &Path,
) -> AppResult<u32> {
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
        // Did claude exit before we could see it? If anything's in the
        // stderr capture file, treat that as the real failure cause —
        // bad model, auth error, missing dep, etc. — and surface it
        // immediately instead of waiting out the full timeout.
        if let Some(msg) = read_error_capture(err_path) {
            return Err(AppError::Spawn(format!("claude exited: {msg}")));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let suffix = read_error_capture(err_path)
        .map(|s| format!(" claude wrote: {s}"))
        .unwrap_or_default();
    Err(AppError::Spawn(format!(
        "did not see claude process for {project_dir} within {}s.{suffix}",
        deadline.as_secs()
    )))
}

fn read_error_capture(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
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
            resume: None,
        }
    }

    const CMD: &str = "C:\\Temp\\fastclaude-test.bat";

    #[test]
    fn build_wt_argv_preserves_existing_shape() {
        let argv = build_wt_argv(&req("C:\\proj"), CMD);
        // Global flags first, then per-tab flags, then the command.
        assert_eq!(&argv[0..4], &["-w", "new", "-d", "C:\\proj"]);
        assert_eq!(&argv[4..6], &["--title", "FastClaude: proj"]);
        assert_eq!(argv[6], "--suppressApplicationTitle");
        assert_eq!(&argv[7..9], &["cmd.exe", "/K"]);
        assert_eq!(argv[9], CMD, "wrapper bat path is the /K command");
    }

    #[test]
    fn build_wt_argv_passes_command_through_unchanged() {
        let argv = build_wt_argv(&req("C:\\proj"), "C:\\Other Path\\with spaces.bat");
        assert_eq!(argv.last().unwrap(), "C:\\Other Path\\with spaces.bat");
    }

    #[test]
    fn build_wt_argv_includes_title_with_project_basename() {
        let argv = build_wt_argv(&req("C:\\GitProjects\\FastClaude"), CMD);
        let title_idx = argv.iter().position(|a| a == "--title").expect("--title present");
        assert_eq!(argv[title_idx + 1], "FastClaude: FastClaude");
    }

    #[test]
    fn build_wt_argv_title_uses_basename_for_unix_style_paths() {
        let argv = build_wt_argv(&req("/home/u/cool-project"), CMD);
        let title_idx = argv.iter().position(|a| a == "--title").unwrap();
        assert_eq!(argv[title_idx + 1], "FastClaude: cool-project");
    }

    #[test]
    fn build_wt_argv_title_falls_back_when_basename_empty() {
        // Trailing slash / drive root — basename returns None
        let argv = build_wt_argv(&req("C:\\"), CMD);
        let title_idx = argv.iter().position(|a| a == "--title").unwrap();
        assert_eq!(argv[title_idx + 1], "FastClaude: session");
    }

    #[test]
    fn write_launcher_bat_redirects_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let bat = dir.path().join("test.bat");
        let err = dir.path().join("test.err");
        write_launcher_bat(&bat, &err, &req("C:\\proj")).unwrap();
        let content = std::fs::read_to_string(&bat).unwrap();
        assert!(content.contains("@echo off"), "starts with @echo off");
        assert!(content.contains("claude --model claude-opus-4-7"), "runs claude");
        assert!(
            content.contains(&format!("2> \"{}\"", err.display())),
            "redirects stderr to err file: {content}"
        );
    }

    #[test]
    fn read_error_capture_returns_some_when_file_has_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("e.err");
        std::fs::write(&path, "  error: bad model\n\n").unwrap();
        assert_eq!(read_error_capture(&path).as_deref(), Some("error: bad model"));
    }

    #[test]
    fn read_error_capture_returns_none_for_missing_or_empty() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.err");
        assert!(read_error_capture(&missing).is_none());
        let empty = dir.path().join("empty.err");
        std::fs::write(&empty, "   \n  \n").unwrap();
        assert!(read_error_capture(&empty).is_none());
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
            resume: None,
        };
        let err = spawner.spawn(&req).unwrap_err();
        assert!(matches!(err, AppError::ClaudeNotOnPath), "expected ClaudeNotOnPath, got {err:?}");
    }
}
