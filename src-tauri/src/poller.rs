use crate::config::Config;
use crate::error::AppResult;
use crate::recent_projects;
use crate::session_registry::{Registry, Session, Status};
use crate::usage_reader;
use std::path::PathBuf;
use std::sync::Arc;
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

#[derive(Debug, Default)]
pub struct FireReport {
    pub fired_ids: Vec<String>,
    pub failed_ids: Vec<(String, String)>, // (session_id, error_msg)
    pub gave_up_ids: Vec<String>,
}

const RESUME_BACKOFF_SECS: i64 = 5 * 60;
const RESUME_MAX_FAILURES: i64 = 3;

/// Fires all due auto-resumes: rows whose `next_resume_at` has passed
/// and that are still below their `resume_cap`.
///
/// For each row: derives the claude session UUID from the JSONL filename
/// stem, spawns `claude --resume <uuid>` with the per-session or global
/// resume prompt, inserts a successor row inheriting the cap (and
/// `resume_count + 1`), and links predecessor → successor via
/// `record_resume_success`.
///
/// On spawn failure, schedules a 5-minute retry. After
/// `RESUME_MAX_FAILURES` consecutive failures the row is marked as
/// permanently given up — the user can re-toggle auto-continue to reset
/// the failure count.
///
/// Called every poller tick after the liveness check; tick errors are
/// surfaced via the eprintln in `run_loop` and do not abort the loop.
pub fn fire_due_resumes(
    registry: &Registry,
    spawner: &dyn crate::spawner::Spawner,
    cfg: &Config,
    now: i64,
) -> AppResult<FireReport> {
    let mut report = FireReport::default();
    for s in registry.list_due_resumes(now)? {
        let Some(jsonl) = s.jsonl_path.as_deref() else {
            // No jsonl path → we can't form a --resume id yet. Defer one tick.
            continue;
        };
        let Some(uuid) = jsonl_session_id(jsonl) else { continue };

        let prompt = s
            .resume_prompt
            .clone()
            .unwrap_or_else(|| cfg.default_resume_prompt.clone());

        let req = crate::spawner::SpawnRequest {
            project_dir: s.project_dir.clone(),
            model: s.model.clone(),
            prompt: Some(prompt),
            terminal_program: cfg.terminal_program.clone(),
            resume: Some(uuid),
            effort: cfg.default_effort.clone(),
            permission_mode: cfg.default_permission_mode.clone(),
            extra_args: cfg.default_extra_args.clone(),
        };

        match spawner.spawn(&req) {
            Ok(result) => {
                let new_row = registry.insert(crate::session_registry::NewSession {
                    project_dir: s.project_dir.clone(),
                    model: s.model.clone(),
                    claude_pid: result.claude_pid,
                    terminal_pid: result.terminal_pid,
                    terminal_window_handle: result.terminal_window_handle,
                    auto_continue: true,
                    resume_prompt: s.resume_prompt.clone(),
                    resume_cap: s.resume_cap,
                    resume_count: s.resume_count + 1,
                    jsonl_path: s.jsonl_path.clone(),
                    jsonl_offset: s.jsonl_offset,
                    subtask_id: s.subtask_id.clone(),
                })?;
                registry.record_resume_success(&s.id, &new_row.id)?;
                report.fired_ids.push(s.id.clone());
            }
            Err(e) => {
                let msg = format!("{e}");
                let new_failures = s.resume_failures + 1;
                if new_failures >= RESUME_MAX_FAILURES {
                    registry.record_final_failure(&s.id)?;
                    report.gave_up_ids.push(s.id.clone());
                } else {
                    registry.record_resume_failure(&s.id, now + RESUME_BACKOFF_SECS)?;
                    report.failed_ids.push((s.id.clone(), msg));
                }
            }
        }
    }
    Ok(report)
}

fn jsonl_session_id(jsonl_path: &str) -> Option<String> {
    std::path::Path::new(jsonl_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

pub trait LivenessProbe: Send + Sync {
    fn alive(&mut self, pid: u32) -> bool;
}

pub struct SysInfoProbe(System);

impl SysInfoProbe {
    pub fn new() -> Self {
        Self(System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
        ))
    }
}

impl Default for SysInfoProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl LivenessProbe for SysInfoProbe {
    fn alive(&mut self, pid: u32) -> bool {
        self.0.refresh_processes();
        self.0.process(Pid::from_u32(pid)).is_some()
    }
}

#[derive(Debug, PartialEq, Default)]
pub struct TickReport {
    pub ended_ids: Vec<String>,
    pub usage_changed: bool,
}

pub fn tick(
    registry: &Registry,
    probe: &mut dyn LivenessProbe,
    cfg: &Config,
    now: i64,
) -> AppResult<TickReport> {
    let mut report = TickReport::default();
    let active = registry.list_active()?;
    for s in active {
        let alive = probe.alive(s.claude_pid as u32);

        // Resolve jsonl_path even for dead-claude rows so we capture any final
        // rate-limit signal claude wrote on its way out.
        let jsonl_path: Option<PathBuf> = match s.jsonl_path.clone() {
            Some(p) => Some(PathBuf::from(p)),
            None => {
                if let Some(p) = find_jsonl_for(&s) {
                    let _ = registry.set_jsonl_path(&s.id, &p.to_string_lossy());
                    Some(p)
                } else {
                    None
                }
            }
        };

        if let Some(jsonl) = jsonl_path {
            let mtime = match std::fs::metadata(&jsonl).and_then(|m| m.modified()) {
                Ok(t) => t
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                Err(_) => 0,
            };
            if mtime > s.last_activity_at {
                let delta = usage_reader::read_delta(&jsonl, s.jsonl_offset as u64)?;
                registry.apply_usage_delta(
                    &s.id,
                    delta.new_offset as i64,
                    delta.tokens_in,
                    delta.tokens_out,
                    delta.tokens_cache_read,
                    delta.tokens_cache_write,
                    mtime,
                )?;
                // Status transitions only make sense for live rows.
                if alive && s.status != Status::Running {
                    registry.set_status(&s.id, Status::Running)?;
                }
                report.usage_changed = true;

                // Arm pending resume if claude reported a rate-limit.
                // set_pending_resume is gated on auto_continue=1, below cap, AND
                // ended_at IS NULL — so we must call it BEFORE mark_ended below.
                if let Some(ev) = delta.limit_event {
                    let reset_at = if ev.reset_at > 0 {
                        ev.reset_at
                    } else {
                        // Sentinel 0 from usage_reader: apply the caller-side
                        // fallback of "last activity + 5h + 60s of slack".
                        mtime + 5 * 3600 + 60
                    };
                    registry.set_pending_resume(&s.id, reset_at)?;
                }
            } else if alive
                && now - s.last_activity_at > cfg.idle_threshold_seconds as i64
                && s.status != Status::Idle
            {
                registry.set_status(&s.id, Status::Idle)?;
            }
        }

        // Finally: mark ended if claude died. By this point, any pending resume
        // has been armed via set_pending_resume; mark_ended no longer cascades
        // a clear of next_resume_at so the fire loop can act on this row.
        if !alive {
            registry.mark_ended(&s.id, now)?;
            report.ended_ids.push(s.id);
        }
    }
    Ok(report)
}

/// Find the JSONL file Claude created for THIS session — not any old file
/// in the same project directory.
///
/// Claude creates a new `<session-uuid>.jsonl` file when each session starts.
/// We identify ours by file CREATION time (not mtime): any file created BEFORE
/// our session began belongs to a previous session. Picking by mtime was
/// unreliable because an unrelated old file can get touched recently and win.
///
/// Allows 2s slack on the lower bound for clock skew between our wall-clock
/// `started_at` and the file system's creation timestamp.
fn find_jsonl_for(s: &Session) -> Option<PathBuf> {
    let root = recent_projects::default_claude_root().ok()?;
    let encoded = encode_project_dir(&s.project_dir);
    let dir = root.join("projects").join(encoded);
    let mut best: Option<(PathBuf, i64)> = None;
    for entry in std::fs::read_dir(&dir).ok()? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let candidate_time = match meta.created() {
            Ok(t) => t
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            // Some filesystems / older OSes don't expose creation time; fall back
            // to mtime but apply the same strict filter — we want a file that
            // came into existence after our session started.
            Err(_) => match meta.modified() {
                Ok(t) => t
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                Err(_) => continue,
            },
        };
        if candidate_time + 2 < s.started_at {
            continue;
        }
        if best.as_ref().map_or(true, |(_, t)| candidate_time > *t) {
            best = Some((path, candidate_time));
        }
    }
    best.map(|(p, _)| p)
}

/// Inverse of recent_projects::decode_name.
fn encode_project_dir(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if let Some((drive, rest)) = normalized.split_once(":/") {
        format!("{drive}--{}", rest.replace('/', "-"))
    } else if let Some(stripped) = normalized.strip_prefix('/') {
        format!("-{}", stripped.replace('/', "-"))
    } else {
        normalized.replace('/', "-")
    }
}

pub async fn run_loop(
    registry: Arc<Registry>,
    spawner: Arc<dyn crate::spawner::Spawner>,
    cfg: Arc<std::sync::Mutex<Config>>,
    interval: std::time::Duration,
    on_tick: impl Fn(TickReport, FireReport) + Send + 'static,
) {
    let mut probe = SysInfoProbe::new();
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        let now = chrono::Utc::now().timestamp();
        let snapshot = cfg.lock().unwrap().clone();
        let tick_report = match tick(&registry, &mut probe, &snapshot, now) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("poller error: {e}");
                continue;
            }
        };
        let fire_report = match fire_due_resumes(&registry, spawner.as_ref(), &snapshot, now) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("fire-resume error: {e}");
                FireReport::default()
            }
        };
        on_tick(tick_report, fire_report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::session_registry::NewSession;
    use std::collections::HashSet;

    use crate::spawner::{SpawnRequest, SpawnResult, Spawner};
    use crate::error::AppResult as ResumeResult;
    use std::sync::Mutex as SpawnerMutex;

    struct FakeSpawner {
        calls: SpawnerMutex<Vec<SpawnRequest>>,
        result: SpawnResult,
    }
    impl FakeSpawner {
        fn new(result: SpawnResult) -> Self {
            Self { calls: SpawnerMutex::new(Vec::new()), result }
        }
        fn calls(&self) -> Vec<SpawnRequest> {
            self.calls.lock().unwrap().clone()
        }
    }
    impl Spawner for FakeSpawner {
        fn spawn(&self, req: &SpawnRequest) -> ResumeResult<SpawnResult> {
            self.calls.lock().unwrap().push(req.clone());
            Ok(self.result.clone())
        }
    }

    struct FailingSpawner;
    impl Spawner for FailingSpawner {
        fn spawn(&self, _req: &SpawnRequest) -> ResumeResult<SpawnResult> {
            Err(crate::error::AppError::Spawn("nope".into()))
        }
    }

    #[test]
    fn fires_due_resume_and_creates_successor_row() {
        let r = Registry::open_in_memory().unwrap();
        let cfg = Config::default();
        let s = r.insert(crate::session_registry::NewSession {
            project_dir: "/p".into(),
            model: "claude-opus-4-7".into(),
            claude_pid: 1,
            terminal_pid: 2,
            terminal_window_handle: None,
            auto_continue: true,
            resume_prompt: Some("keep going".into()),
            resume_cap: 3,
            resume_count: 0,
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
        }).unwrap();
        r.set_jsonl_path(&s.id, "/tmp/abc-1234.jsonl").unwrap();
        r.set_pending_resume(&s.id, 500).unwrap();

        let spawner = FakeSpawner::new(SpawnResult {
            claude_pid: 42,
            terminal_pid: 41,
            terminal_window_handle: Some("hwnd-1".into()),
        });
        let report = fire_due_resumes(&r, &spawner, &cfg, 1000).unwrap();
        assert_eq!(report.fired_ids.len(), 1);

        let calls = spawner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].resume.as_deref(), Some("abc-1234"),
            "uuid is derived from jsonl filename stem");
        assert_eq!(calls[0].prompt.as_deref(), Some("keep going"),
            "per-session prompt takes precedence");
        assert_eq!(calls[0].model, "claude-opus-4-7");

        let pred = r.get(&s.id).unwrap();
        assert_eq!(pred.next_resume_at, None);
        assert!(pred.resumed_into.is_some());

        let new_id = pred.resumed_into.unwrap();
        let succ = r.get(&new_id).unwrap();
        assert_eq!(succ.resume_count, 1);
        assert_eq!(succ.resume_cap, 3);
        assert!(succ.auto_continue);
    }

    #[test]
    fn falls_back_to_global_resume_prompt_when_per_session_unset() {
        let r = Registry::open_in_memory().unwrap();
        let cfg = Config { default_resume_prompt: "global continue".into(), ..Config::default() };
        let s = r.insert(crate::session_registry::NewSession {
            project_dir: "/p".into(),
            model: "claude-opus-4-7".into(),
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
        r.set_jsonl_path(&s.id, "/tmp/abc.jsonl").unwrap();
        r.set_pending_resume(&s.id, 500).unwrap();
        let spawner = FakeSpawner::new(SpawnResult {
            claude_pid: 42, terminal_pid: 41, terminal_window_handle: None,
        });
        fire_due_resumes(&r, &spawner, &cfg, 1000).unwrap();
        assert_eq!(spawner.calls()[0].prompt.as_deref(), Some("global continue"));
    }

    #[test]
    fn cap_reached_blocks_further_fires() {
        let r = Registry::open_in_memory().unwrap();
        let cfg = Config::default();
        let s = r.insert(crate::session_registry::NewSession {
            project_dir: "/p".into(),
            model: "claude-opus-4-7".into(),
            claude_pid: 1,
            terminal_pid: 2,
            terminal_window_handle: None,
            auto_continue: true,
            resume_prompt: None,
            resume_cap: 1,
            resume_count: 1, // already at cap
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
        }).unwrap();
        r.set_jsonl_path(&s.id, "/tmp/abc.jsonl").unwrap();
        let _ = r.set_pending_resume(&s.id, 500); // no-op due to cap check
        let spawner = FakeSpawner::new(SpawnResult {
            claude_pid: 42, terminal_pid: 41, terminal_window_handle: None,
        });
        let report = fire_due_resumes(&r, &spawner, &cfg, 1000).unwrap();
        assert!(report.fired_ids.is_empty());
        assert!(spawner.calls().is_empty());
    }

    #[test]
    fn spawn_failure_bumps_failure_count_and_backs_off() {
        let r = Registry::open_in_memory().unwrap();
        let cfg = Config::default();
        let s = r.insert(crate::session_registry::NewSession {
            project_dir: "/p".into(),
            model: "claude-opus-4-7".into(),
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
        r.set_jsonl_path(&s.id, "/tmp/abc.jsonl").unwrap();
        r.set_pending_resume(&s.id, 500).unwrap();

        let report = fire_due_resumes(&r, &FailingSpawner, &cfg, 1000).unwrap();
        assert!(report.fired_ids.is_empty());
        assert_eq!(report.failed_ids.len(), 1);
        let got = r.get(&s.id).unwrap();
        assert_eq!(got.resume_failures, 1);
        assert_eq!(got.next_resume_at, Some(1000 + 5 * 60), "5-min back-off scheduled");
    }

    #[test]
    fn three_spawn_failures_give_up() {
        let r = Registry::open_in_memory().unwrap();
        let cfg = Config::default();
        let s = r.insert(crate::session_registry::NewSession {
            project_dir: "/p".into(),
            model: "claude-opus-4-7".into(),
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
        r.set_jsonl_path(&s.id, "/tmp/abc.jsonl").unwrap();
        r.set_pending_resume(&s.id, 500).unwrap();

        let mut now = 1000i64;
        for _ in 0..3 {
            let _ = fire_due_resumes(&r, &FailingSpawner, &cfg, now).unwrap();
            let row = r.get(&s.id).unwrap();
            now = row.next_resume_at.unwrap_or(now) + 1;
        }
        let got = r.get(&s.id).unwrap();
        assert_eq!(got.resume_failures, 3);
        assert_eq!(got.next_resume_at, None, "after 3 strikes we give up");
    }

    struct FakeProbe(HashSet<u32>);
    impl LivenessProbe for FakeProbe {
        fn alive(&mut self, pid: u32) -> bool {
            self.0.contains(&pid)
        }
    }

    #[test]
    fn marks_dead_sessions_ended_only() {
        let r = Registry::open_in_memory().unwrap();
        let cfg = Config::default();
        let alive = r
            .insert(NewSession {
                project_dir: "/p/a".into(),
                model: "claude-opus-4-7".into(),
                claude_pid: 100,
                terminal_pid: 99,
                terminal_window_handle: None,
                auto_continue: false,
                resume_prompt: None,
                resume_cap: 3,
                resume_count: 0,
                jsonl_path: None,
                jsonl_offset: 0,
                subtask_id: None,
            })
            .unwrap();
        let dead = r
            .insert(NewSession {
                project_dir: "/p/b".into(),
                model: "claude-opus-4-7".into(),
                claude_pid: 200,
                terminal_pid: 199,
                terminal_window_handle: None,
                auto_continue: false,
                resume_prompt: None,
                resume_cap: 3,
                resume_count: 0,
                jsonl_path: None,
                jsonl_offset: 0,
                subtask_id: None,
            })
            .unwrap();
        let mut probe = FakeProbe([100u32].into_iter().collect());

        let report = tick(&r, &mut probe, &cfg, 12345).unwrap();
        assert_eq!(report.ended_ids, vec![dead.id.clone()]);

        let active = r.list_active().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, alive.id);
    }

    #[test]
    fn tick_processes_jsonl_for_dead_claude_and_arms_resume() {
        use std::io::Write;
        use tempfile::NamedTempFile;
        use crate::session_registry::NewSession;

        let r = Registry::open_in_memory().unwrap();
        let cfg = Config::default();
        let s = r.insert(NewSession {
            project_dir: "/p".into(),
            model: "claude-opus-4-7".into(),
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

        let mut jsonl = NamedTempFile::new().unwrap();
        writeln!(
            jsonl,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"5-hour limit reached, resets at 23:30"}}],"usage":{{"input_tokens":1,"output_tokens":1}}}}}}"#
        ).unwrap();
        jsonl.flush().unwrap();
        r.set_jsonl_path(&s.id, &jsonl.path().to_string_lossy()).unwrap();
        r.backdate_last_activity(&s.id, 1).unwrap();

        // claude is DEAD (not in the alive set) — emulates the rate-limit case
        // where claude exited after writing its final message.
        let mut probe = FakeProbe(std::collections::HashSet::new());
        let report = tick(&r, &mut probe, &cfg, 1000).unwrap();
        assert_eq!(report.ended_ids.len(), 1, "session is marked ended");

        let got = r.get(&s.id).unwrap();
        assert!(got.ended_at.is_some(), "ended_at is set");
        assert!(got.next_resume_at.is_some(),
            "pending resume must be armed even though claude is dead — \
             this is the restart-recovery and rate-limit-exit case");
    }

    #[test]
    fn encode_windows_drive() {
        assert_eq!(
            encode_project_dir("C:/GitProjects/FastClaude"),
            "C--GitProjects-FastClaude"
        );
    }

    #[test]
    fn encode_unix_path() {
        assert_eq!(encode_project_dir("/home/tal/portfolio"), "-home-tal-portfolio");
    }

    #[test]
    fn encode_normalizes_backslashes() {
        assert_eq!(
            encode_project_dir(r"C:\GitProjects\FastClaude"),
            "C--GitProjects-FastClaude"
        );
    }

    #[test]
    fn arms_pending_resume_when_limit_event_seen_on_optin_row() {
        use std::io::Write;
        use tempfile::NamedTempFile;
        use crate::session_registry::NewSession;

        let r = Registry::open_in_memory().unwrap();
        let cfg = Config::default();
        let s = r.insert(NewSession {
            project_dir: "/p".into(),
            model: "claude-opus-4-7".into(),
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

        let mut jsonl = NamedTempFile::new().unwrap();
        writeln!(
            jsonl,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"5-hour limit reached, resets at 23:30"}}],"usage":{{"input_tokens":1,"output_tokens":1}}}}}}"#
        ).unwrap();
        jsonl.flush().unwrap();
        r.set_jsonl_path(&s.id, &jsonl.path().to_string_lossy()).unwrap();
        // Backdate so the file mtime is always > last_activity_at.
        r.backdate_last_activity(&s.id, 0).unwrap();

        let mut probe = FakeProbe([1u32].into_iter().collect());
        let report = tick(&r, &mut probe, &cfg, 1000).unwrap();
        assert!(report.usage_changed);

        let got = r.get(&s.id).unwrap();
        assert!(got.next_resume_at.is_some(), "must arm pending resume on opt-in row");
    }

    #[test]
    fn does_not_arm_pending_resume_when_session_not_optin() {
        use std::io::Write;
        use tempfile::NamedTempFile;
        use crate::session_registry::NewSession;

        let r = Registry::open_in_memory().unwrap();
        let cfg = Config::default();
        let s = r.insert(NewSession {
            project_dir: "/p".into(),
            model: "claude-opus-4-7".into(),
            claude_pid: 1,
            terminal_pid: 2,
            terminal_window_handle: None,
            auto_continue: false,  // not opted in
            resume_prompt: None,
            resume_cap: 3,
            resume_count: 0,
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
        }).unwrap();
        let mut jsonl = NamedTempFile::new().unwrap();
        writeln!(
            jsonl,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"5-hour limit reached, resets at 14:30"}}],"usage":{{"input_tokens":1,"output_tokens":1}}}}}}"#
        ).unwrap();
        jsonl.flush().unwrap();
        r.set_jsonl_path(&s.id, &jsonl.path().to_string_lossy()).unwrap();
        // Backdate so the file mtime is always > last_activity_at.
        r.backdate_last_activity(&s.id, 0).unwrap();

        let mut probe = FakeProbe([1u32].into_iter().collect());
        let _ = tick(&r, &mut probe, &cfg, 1000).unwrap();
        let got = r.get(&s.id).unwrap();
        assert!(got.next_resume_at.is_none());
    }

    #[test]
    fn successor_inherits_predecessor_jsonl() {
        let r = Registry::open_in_memory().unwrap();
        let cfg = Config::default();
        let s = r.insert(crate::session_registry::NewSession {
            project_dir: "/p".into(),
            model: "claude-opus-4-7".into(),
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
        // Predecessor has a JSONL path and an advanced offset (already processed
        // the limit-hit line and prior tokens).
        r.set_jsonl_path(&s.id, "/tmp/abc-1234.jsonl").unwrap();
        r.apply_usage_delta(&s.id, 5000, 0, 0, 0, 0, 100).unwrap();
        r.set_pending_resume(&s.id, 500).unwrap();

        let spawner = FakeSpawner::new(SpawnResult {
            claude_pid: 42, terminal_pid: 41, terminal_window_handle: None,
        });
        fire_due_resumes(&r, &spawner, &cfg, 1000).unwrap();

        let pred = r.get(&s.id).unwrap();
        let new_id = pred.resumed_into.unwrap();
        let succ = r.get(&new_id).unwrap();
        assert_eq!(succ.jsonl_path.as_deref(), Some("/tmp/abc-1234.jsonl"),
            "successor reuses predecessor's JSONL (claude --resume appends to same file)");
        assert_eq!(succ.jsonl_offset, 5000,
            "successor starts from where predecessor left off so we don't re-tally");
    }

    #[test]
    fn falls_back_to_jsonl_mtime_plus_5h_when_reset_unparseable() {
        use std::io::Write;
        use tempfile::NamedTempFile;
        use crate::session_registry::NewSession;

        let r = Registry::open_in_memory().unwrap();
        let cfg = Config::default();
        let s = r.insert(NewSession {
            project_dir: "/p".into(),
            model: "claude-opus-4-7".into(),
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
        let mut jsonl = NamedTempFile::new().unwrap();
        writeln!(
            jsonl,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"5-hour limit reached. Try again later."}}],"usage":{{"input_tokens":1,"output_tokens":1}}}}}}"#
        ).unwrap();
        jsonl.flush().unwrap();
        r.set_jsonl_path(&s.id, &jsonl.path().to_string_lossy()).unwrap();
        // Backdate so the file mtime is always > last_activity_at.
        r.backdate_last_activity(&s.id, 0).unwrap();

        let mut probe = FakeProbe([1u32].into_iter().collect());
        let _ = tick(&r, &mut probe, &cfg, 1000).unwrap();
        let got = r.get(&s.id).unwrap();
        // last_activity_at gets set to the JSONL mtime by apply_usage_delta.
        let expected = got.last_activity_at + 5 * 3600 + 60;
        assert_eq!(got.next_resume_at, Some(expected),
            "fallback is last_activity_at + 5h + 60s when usage_reader returns sentinel 0");
    }
}
