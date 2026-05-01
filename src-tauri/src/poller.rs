use crate::error::AppResult;
use crate::session_registry::Registry;
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

#[derive(Debug, PartialEq)]
pub struct TickReport {
    pub ended_ids: Vec<String>,
}

pub fn tick(registry: &Registry, probe: &mut dyn LivenessProbe, now: i64) -> AppResult<TickReport> {
    let active = registry.list_active()?;
    let mut ended_ids = Vec::new();
    for s in active {
        if !probe.alive(s.claude_pid as u32) {
            registry.mark_ended(&s.id, now)?;
            ended_ids.push(s.id);
        }
    }
    Ok(TickReport { ended_ids })
}

pub async fn run_loop(
    registry: Arc<Registry>,
    interval: std::time::Duration,
    on_tick: impl Fn(TickReport) + Send + 'static,
) {
    let mut probe = SysInfoProbe::new();
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        let now = chrono::Utc::now().timestamp();
        match tick(&registry, &mut probe, now) {
            Ok(report) => on_tick(report),
            Err(e) => eprintln!("poller error: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let report = tick(&r, &mut probe, 12345).unwrap();
        assert_eq!(report.ended_ids, vec![dead.id.clone()]);

        let active = r.list_active().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, alive.id);
    }
}
