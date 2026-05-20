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

/// Build the argv (after the executable) passed to Windows Terminal when the
/// command-to-run is a single cmd.exe-parsed string (the legacy `.bat` path,
/// kept for the no-prompt case).
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
    let mut argv = build_wt_prefix(req);
    argv.push("cmd.exe".into());
    argv.push("/K".into());
    argv.push(command.into());
    argv
}

/// Build the argv to launch claude.exe DIRECTLY under wt — no cmd.exe shell,
/// no .bat wrapper. Used whenever a prompt is present so the prompt text
/// survives intact (cmd.exe's parsing of quoted args mangles characters like
/// `"`, and that ate the user's planner subtasks). Rust's `Command::args` and
/// wt's argv handling do CommandLineToArgvW-compatible quoting end-to-end, so
/// special characters in the prompt arrive at claude.exe unchanged.
pub(crate) fn build_wt_direct_argv(req: &SpawnRequest) -> Vec<String> {
    let mut argv = build_wt_prefix(req);
    argv.push("claude".into());
    argv.extend(build_claude_argv(req));
    argv
}

fn build_wt_prefix(req: &SpawnRequest) -> Vec<String> {
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
    ]
}

/// Build the argv for claude.exe itself (without the executable name). The
/// caller appends this after `claude` or `claude.exe` in their argv list.
///
/// `extra_args` is split on whitespace into separate argv tokens. This loses
/// some fidelity for users who want to pass a flag value containing spaces via
/// `extra_args`, but accepts the trade-off for clean prompt passing — the
/// alternative is shell parsing, which is what this whole code path exists to
/// avoid. Multi-word flag values can still be added through the regular
/// `--effort` / `--permission-mode` fields, which are passed atomically.
pub(crate) fn build_claude_argv(req: &SpawnRequest) -> Vec<String> {
    let mut a: Vec<String> = vec!["--model".into(), req.model.clone()];
    if !req.effort.is_empty() {
        a.push("--effort".into());
        a.push(req.effort.clone());
    }
    if !req.permission_mode.is_empty() {
        a.push("--permission-mode".into());
        a.push(req.permission_mode.clone());
    }
    if let Some(id) = req.resume.as_deref().filter(|s| !s.is_empty()) {
        a.push("--resume".into());
        a.push(id.to_string());
    }
    let extra = req.extra_args.trim();
    if !extra.is_empty() {
        a.extend(extra.split_whitespace().map(String::from));
    }
    if let Some(p) = req.prompt.as_deref().filter(|s| !s.is_empty()) {
        a.push(p.to_string());
    }
    a
}

/// Normalize a prompt string so it survives going through a `.bat` file:
///
/// - Collapse every line break to a space. A literal newline inside a quoted
///   prompt in a `.bat` terminates the cmd.exe statement, so a multi-line
///   subtask from the planner would be cut off at the first `\n`.
/// - Double every `%`. Even inside double quotes, cmd.exe expands `%FOO%`
///   variables in a `.bat`; doubling makes the literal percent survive.
///
/// `shell_escape::windows::escape` already handles the quote-wrapping and
/// inner-quote escaping; this just covers the two characters it doesn't.
fn sanitize_prompt_for_bat(prompt: &str) -> String {
    prompt
        .replace("\r\n", " ")
        .replace('\n', " ")
        .replace('\r', " ")
        .replace('%', "%%")
}

/// Write a per-launch wrapper batch file that runs claude with stderr
/// redirected to `err_path`. Lets `wait_for_claude` surface claude's actual
/// failure message in the toast when the process exits early.
fn write_launcher_bat(bat_path: &Path, err_path: &Path, req: &SpawnRequest) -> AppResult<()> {
    let sanitized = req.prompt.as_deref().map(sanitize_prompt_for_bat);
    let claude_cmd = crate::spawner::build_claude_command(
        &req.model,
        sanitized.as_deref(),
        req.resume.as_deref(),
        &req.effort,
        &req.permission_mode,
        &req.extra_args,
    );
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

        // When a prompt is present we invoke claude.exe directly under wt with
        // a proper argv — no cmd.exe, no .bat, no shell escaping at all.
        // cmd.exe's quoted-argument parsing mangles `"`, `&`, etc. inside the
        // prompt text, which truncated planner-generated subtasks. The price
        // is losing the per-launch stderr-capture .err file; claude's stderr
        // shows up in the wt window instead, which is visible to the user.
        //
        // No-prompt launches keep the .bat path so the stderr capture remains
        // available for diagnosing startup failures (bad model name, auth,
        // missing dependencies, etc.).
        let prompt_present = req
            .prompt
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false);

        let launch_id = uuid::Uuid::new_v4();
        let temp = std::env::temp_dir();
        let bat_path = temp.join(format!("fastclaude-{launch_id}.bat"));
        let err_path = temp.join(format!("fastclaude-{launch_id}.err"));
        if !prompt_present {
            write_launcher_bat(&bat_path, &err_path, req)?;
        }
        let bat_str = bat_path.to_string_lossy().to_string();

        let mut cmd = match (&choice, prompt_present) {
            (TerminalChoice::WindowsTerminal(path), true) => {
                let mut c = Command::new(path);
                c.args(build_wt_direct_argv(req));
                c
            }
            (TerminalChoice::WindowsTerminal(path), false) => {
                let mut c = Command::new(path);
                c.args(build_wt_argv(req, &bat_str));
                c
            }
            (TerminalChoice::Custom(path), true) => {
                let mut c = Command::new(path);
                c.args(build_wt_direct_argv(req));
                c
            }
            (TerminalChoice::Custom(path), false) => {
                let mut c = Command::new(path);
                c.args(build_wt_argv(req, &bat_str));
                c
            }
            (TerminalChoice::Cmd, _) => {
                // cmd.exe fallback — no wt available. Still uses the .bat
                // because there's no wt to take a direct argv. The prompt
                // sanitizer (newline+%) covers the common breakage; complex
                // prompts may still mangle here. The recommended fix for
                // users on this path is to install Windows Terminal.
                if prompt_present {
                    // We skipped write_launcher_bat above; write it now since
                    // this fallback path still needs the .bat.
                    write_launcher_bat(&bat_path, &err_path, req)?;
                }
                let inner = format!("start \"FastClaude\" cmd.exe /K \"{bat_str}\"");
                let mut c = Command::new("cmd.exe");
                c.args(["/C", &inner]);
                c.current_dir(&req.project_dir);
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
            effort: String::new(),
            permission_mode: String::new(),
            extra_args: String::new(),
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
    fn build_claude_argv_minimal_request() {
        let argv = build_claude_argv(&req("C:\\proj"));
        assert_eq!(argv, vec!["--model", "claude-opus-4-7"]);
    }

    #[test]
    fn build_claude_argv_passes_prompt_as_last_atomic_token() {
        let mut r = req("C:\\proj");
        r.prompt = Some(r#"Build "auth" module with & without OAuth"#.into());
        let argv = build_claude_argv(&r);
        // The prompt MUST appear as a single argv element — that's the whole
        // point of bypassing cmd.exe. If splitting ever sneaks in, this fails.
        assert_eq!(argv.last().unwrap(), r#"Build "auth" module with & without OAuth"#);
        assert!(argv.iter().any(|a| a == "--model"), "model flag still present");
    }

    #[test]
    fn build_claude_argv_preserves_newlines_and_percents_in_prompt() {
        // Direct argv path doesn't need the .bat sanitizer — newlines and
        // percents are just bytes in the prompt argument.
        let mut r = req("C:\\proj");
        r.prompt = Some("line one\nline two\n%PATH% should stay literal".into());
        let argv = build_claude_argv(&r);
        assert_eq!(
            argv.last().unwrap(),
            "line one\nline two\n%PATH% should stay literal"
        );
    }

    #[test]
    fn build_claude_argv_splits_extra_args_on_whitespace() {
        let mut r = req("C:\\proj");
        r.extra_args = "--verbose --debug".into();
        let argv = build_claude_argv(&r);
        assert!(argv.iter().any(|a| a == "--verbose"));
        assert!(argv.iter().any(|a| a == "--debug"));
    }

    #[test]
    fn build_claude_argv_includes_effort_permission_resume_atomically() {
        let mut r = req("C:\\proj");
        r.effort = "high".into();
        r.permission_mode = "accept-edits".into();
        r.resume = Some("session-123".into());
        let argv = build_claude_argv(&r);
        let find = |flag: &str| argv.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone());
        assert_eq!(find("--effort"), Some("high".into()));
        assert_eq!(find("--permission-mode"), Some("accept-edits".into()));
        assert_eq!(find("--resume"), Some("session-123".into()));
    }

    #[test]
    fn build_wt_direct_argv_runs_claude_under_wt_without_cmd_shell() {
        let mut r = req("C:\\proj");
        r.prompt = Some(r#"prompt with "quotes" and & ampersand"#.into());
        let argv = build_wt_direct_argv(&r);
        // No cmd.exe in the argv — that's the whole point.
        assert!(!argv.iter().any(|a| a.eq_ignore_ascii_case("cmd.exe") || a == "/K"));
        // claude appears as its own argv element followed by --model.
        let claude_idx = argv.iter().position(|a| a == "claude").expect("claude in argv");
        assert_eq!(argv[claude_idx + 1], "--model");
        // The prompt is the last argv element, intact.
        assert_eq!(argv.last().unwrap(), r#"prompt with "quotes" and & ampersand"#);
    }

    #[test]
    fn sanitize_prompt_collapses_line_breaks_to_spaces() {
        assert_eq!(
            sanitize_prompt_for_bat("first line\r\nsecond line\nthird\r"),
            "first line second line third "
        );
    }

    #[test]
    fn sanitize_prompt_doubles_percent_signs() {
        assert_eq!(
            sanitize_prompt_for_bat("set USER=%USERNAME% and 50%"),
            "set USER=%%USERNAME%% and 50%%"
        );
    }

    #[test]
    fn sanitize_prompt_passes_through_safe_text() {
        let raw = "Refactor the auth module to use OAuth 2.0 with PKCE.";
        assert_eq!(sanitize_prompt_for_bat(raw), raw);
    }

    #[test]
    fn write_launcher_bat_sanitizes_multiline_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let bat = dir.path().join("ml.bat");
        let err = dir.path().join("ml.err");
        let mut r = req("C:\\proj");
        r.prompt = Some("first line\nsecond line".into());
        write_launcher_bat(&bat, &err, &r).unwrap();
        let content = std::fs::read_to_string(&bat).unwrap();
        // The .bat body line that runs claude must be a single physical line
        // (apart from the leading @echo off line). Splitting on \r\n and
        // counting non-empty lines confirms the prompt's newline was collapsed.
        let lines: Vec<&str> = content.split("\r\n").filter(|l| !l.is_empty()).collect();
        assert_eq!(
            lines.len(),
            2,
            "expected @echo off + one claude line, got {} lines: {content:?}",
            lines.len()
        );
        assert!(
            lines[1].contains("first line second line"),
            "prompt should be joined with a space: {content:?}"
        );
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
            effort: String::new(),
            permission_mode: String::new(),
            extra_args: String::new(),
        };
        let err = spawner.spawn(&req).unwrap_err();
        assert!(matches!(err, AppError::ClaudeNotOnPath), "expected ClaudeNotOnPath, got {err:?}");
    }
}
