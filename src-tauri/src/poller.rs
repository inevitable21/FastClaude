use crate::config::Config;
use crate::cost_reader;
use crate::error::AppResult;
use crate::recent_projects;
use crate::session_registry::{Registry, Session, Status};
use std::path::PathBuf;
use std::sync::Arc;
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

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
        if !probe.alive(s.claude_pid as u32) {
            registry.mark_ended(&s.id, now)?;
            report.ended_ids.push(s.id);
            continue;
        }
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
        let Some(jsonl) = jsonl_path else { continue };
        let mtime = match std::fs::metadata(&jsonl).and_then(|m| m.modified()) {
            Ok(t) => t
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            Err(_) => continue,
        };
        if mtime > s.last_activity_at {
            let delta = cost_reader::read_delta(
                &jsonl,
                s.jsonl_offset as u64,
                &s.model,
                &cfg.pricing,
            )?;
            registry.apply_usage_delta(
                &s.id,
                delta.new_offset as i64,
                delta.tokens_in,
                delta.tokens_out,
                delta.tokens_cache_read,
                delta.tokens_cache_write,
                delta.cost_usd,
                mtime,
            )?;
            if s.status != Status::Running {
                registry.set_status(&s.id, Status::Running)?;
            }
            report.usage_changed = true;
        } else if now - s.last_activity_at > cfg.idle_threshold_seconds as i64
            && s.status != Status::Idle
        {
            registry.set_status(&s.id, Status::Idle)?;
        }
    }
    Ok(report)
}

/// Find the newest JSONL file in `~/.claude/projects/<encoded>/` whose mtime
/// is at or after the session's start_time (with a small slack).
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
        let Ok(mod_time) = meta.modified() else { continue };
        let mtime = mod_time
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if mtime + 5 < s.started_at {
            continue;
        }
        if best.as_ref().map_or(true, |(_, t)| mtime > *t) {
            best = Some((path, mtime));
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
    cfg: Arc<std::sync::Mutex<Config>>,
    interval: std::time::Duration,
    on_tick: impl Fn(TickReport) + Send + 'static,
) {
    let mut probe = SysInfoProbe::new();
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        let now = chrono::Utc::now().timestamp();
        let snapshot = cfg.lock().unwrap().clone();
        match tick(&registry, &mut probe, &snapshot, now) {
            Ok(report) => on_tick(report),
            Err(e) => eprintln!("poller error: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::session_registry::NewSession;
    use std::collections::HashSet;

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
            })
            .unwrap();
        let dead = r
            .insert(NewSession {
                project_dir: "/p/b".into(),
                model: "claude-opus-4-7".into(),
                claude_pid: 200,
                terminal_pid: 199,
                terminal_window_handle: None,
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
}
