# FastClaude Plan 2 — Polish

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Layer the v1 polish features on top of the working MVP — cost tracking, idle detection, error toasts, the global hotkey, and a settings page. macOS and Linux spawner/focus implementations are deferred to Plan 3.

**Architecture:** Reuse the existing modules. Add `cost_reader`, extend `poller` to compute cost and idle state, add new IPC commands and events, plus three frontend pieces (toast wiring, usage strip, settings page).

**Tech Stack:** Same as Plan 1. New crates: none for cost; `tauri-plugin-global-shortcut` for the hotkey. Frontend keeps shadcn primitives + `react-router-dom` if we route between dashboard and settings.

**Spec:** `docs/superpowers/specs/2026-05-01-fastclaude-design.md`
**Plan 1 (MVP, complete):** `docs/superpowers/plans/2026-05-01-fastclaude-mvp.md`

---

## File structure (created or modified)

```
src-tauri/src/
├── cost_reader.rs                NEW — parse JSONL, tally usage × pricing
├── poller.rs                     MODIFY — call cost_reader; idle transitions; emit usage_updated
├── commands.rs                   MODIFY — set_config, kill_session emits, idle threshold update
├── main.rs                       MODIFY — register global-shortcut plugin and capability
├── lib.rs                        MODIFY — pub mod cost_reader;
└── tauri.conf.json + capabilities/default.json   MODIFY — add globalShortcut perms

src/
├── components/
│   ├── UsageStrip.tsx            NEW
│   ├── Settings.tsx              NEW
│   ├── App.tsx                   MODIFY — wrap with Toaster + simple route switch
│   └── Dashboard.tsx             MODIFY — listens for hotkey-fired and usage-updated
├── lib/ipc.ts                    MODIFY — setConfig, listen helpers for usage + hotkey
└── types.ts                      MODIFY — UsageSummary type
```

---

## Task 1: `cost_reader` module

**Files:**
- Create: `src-tauri/src/cost_reader.rs`
- Modify: `src-tauri/src/lib.rs`

Reads a session's JSONL file from a stored byte offset, sums token usage from each `assistant` event, multiplies by pricing, returns the deltas to add to the session row.

JSONL line shape (worth pinning in a fixture):

```jsonl
{"type":"user","message":{"role":"user","content":"hi"},"uuid":"..."}
{"type":"assistant","message":{"role":"assistant","content":[...],"usage":{"input_tokens":120,"output_tokens":35,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}},"uuid":"..."}
{"type":"assistant","message":{"role":"assistant","content":[...],"usage":{"input_tokens":210,"output_tokens":60,"cache_creation_input_tokens":1024,"cache_read_input_tokens":256}},"uuid":"..."}
```

Pricing table is per-million tokens (already in `Config::pricing`).

- [ ] **Step 1: Write the module + tests**

```rust
use crate::config::Pricing;
use crate::error::AppResult;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct UsageDelta {
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub tokens_cache_read: i64,
    pub tokens_cache_write: i64,
    pub cost_usd: f64,
    /// New byte offset to persist for next call.
    pub new_offset: u64,
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    typ: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: i64,
    #[serde(default)]
    cache_read_input_tokens: i64,
}

/// Stream-reads `path` from `start_offset`, sums usage, and prices the totals
/// using `pricing[model]`. Returns the delta the caller should add to the
/// session row.
pub fn read_delta(
    path: &Path,
    start_offset: u64,
    model: &str,
    pricing: &HashMap<String, Pricing>,
) -> AppResult<UsageDelta> {
    let mut file = File::open(path)?;
    let total_len = file.metadata()?.len();
    if start_offset >= total_len {
        return Ok(UsageDelta { new_offset: total_len, ..Default::default() });
    }
    file.seek(SeekFrom::Start(start_offset))?;

    let reader = BufReader::new(file);
    let mut delta = UsageDelta { new_offset: start_offset, ..Default::default() };

    for line in reader.lines() {
        let line = line?;
        delta.new_offset += line.len() as u64 + 1; // +1 for newline
        if line.is_empty() {
            continue;
        }
        let parsed: Line = match serde_json::from_str(&line) {
            Ok(p) => p,
            Err(_) => continue, // malformed line — skip, don't fail the whole tally
        };
        if parsed.typ.as_deref() != Some("assistant") {
            continue;
        }
        let Some(Message { usage: Some(u) }) = parsed.message else { continue };
        delta.tokens_in += u.input_tokens;
        delta.tokens_out += u.output_tokens;
        delta.tokens_cache_read += u.cache_read_input_tokens;
        delta.tokens_cache_write += u.cache_creation_input_tokens;
    }

    // Cap new_offset at file length so we never read past EOF on the next call.
    if delta.new_offset > total_len {
        delta.new_offset = total_len;
    }

    if let Some(p) = pricing.get(model) {
        delta.cost_usd = (delta.tokens_in as f64) * p.input / 1_000_000.0
            + (delta.tokens_out as f64) * p.output / 1_000_000.0
            + (delta.tokens_cache_read as f64) * p.cache_read / 1_000_000.0
            + (delta.tokens_cache_write as f64) * p.cache_write / 1_000_000.0;
    }
    Ok(delta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn pricing() -> HashMap<String, Pricing> {
        let mut p = HashMap::new();
        p.insert(
            "claude-opus-4-7".into(),
            Pricing { input: 15.0, output: 75.0, cache_read: 1.5, cache_write: 18.75 },
        );
        p
    }

    fn write_jsonl(lines: &[&str]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        f.flush().unwrap();
        f
    }

    #[test]
    fn tallies_assistant_usage() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":1000,"cache_read_input_tokens":200}}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":50,"output_tokens":25}}}"#,
        ]);
        let d = read_delta(f.path(), 0, "claude-opus-4-7", &pricing()).unwrap();
        assert_eq!(d.tokens_in, 150);
        assert_eq!(d.tokens_out, 75);
        assert_eq!(d.tokens_cache_read, 200);
        assert_eq!(d.tokens_cache_write, 1000);
        // 150*15 + 75*75 + 200*1.5 + 1000*18.75 = all per 1M
        let expect = (150.0 * 15.0 + 75.0 * 75.0 + 200.0 * 1.5 + 1000.0 * 18.75) / 1_000_000.0;
        assert!((d.cost_usd - expect).abs() < 1e-9);
        assert!(d.new_offset > 0);
    }

    #[test]
    fn ignores_non_assistant_lines() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
            r#"{"type":"summary","content":"..."}"#,
        ]);
        let d = read_delta(f.path(), 0, "claude-opus-4-7", &pricing()).unwrap();
        assert_eq!(d.tokens_in, 0);
        assert_eq!(d.cost_usd, 0.0);
    }

    #[test]
    fn skips_malformed_lines() {
        let f = write_jsonl(&[
            r#"not even json"#,
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":10,"output_tokens":5}}}"#,
        ]);
        let d = read_delta(f.path(), 0, "claude-opus-4-7", &pricing()).unwrap();
        assert_eq!(d.tokens_in, 10);
        assert_eq!(d.tokens_out, 5);
    }

    #[test]
    fn resumes_from_offset() {
        let f = write_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":100,"output_tokens":50}}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":1,"output_tokens":2}}}"#,
        ]);
        let first = read_delta(f.path(), 0, "claude-opus-4-7", &pricing()).unwrap();
        // Rewind to where we were after first line and see only the second.
        let line1_len = (
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":100,"output_tokens":50}}}"#
        ).len() as u64 + 1;
        let second = read_delta(f.path(), line1_len, "claude-opus-4-7", &pricing()).unwrap();
        assert_eq!(second.tokens_in, 1);
        assert_eq!(second.tokens_out, 2);
        assert_eq!(first.tokens_in, 101);
    }

    #[test]
    fn missing_model_pricing_zeroes_cost() {
        let f = write_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":100,"output_tokens":50}}}"#,
        ]);
        let d = read_delta(f.path(), 0, "claude-unknown", &pricing()).unwrap();
        assert_eq!(d.tokens_in, 100);
        assert_eq!(d.cost_usd, 0.0);
    }
}
```

- [ ] **Step 2: Add module to `lib.rs`**

In `src-tauri/src/lib.rs`, insert (alphabetical):

```rust
pub mod cost_reader;
```

- [ ] **Step 3: Run tests, expect PASS**

```bash
cd C:/GitProjects/FastClaude/src-tauri && cargo test --lib cost_reader::
```

Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/cost_reader.rs src-tauri/src/lib.rs
git commit -m "feat(cost): tally JSONL usage and price by model"
```

---

## Task 2: Extend `session_registry` with cost and JSONL update helpers

**Files:**
- Modify: `src-tauri/src/session_registry.rs`

We need ways to (a) set `jsonl_path` once discovered, (b) update offsets and totals atomically, (c) update `last_activity_at` and `status` (for idle).

- [ ] **Step 1: Add methods to `Registry`**

In `src-tauri/src/session_registry.rs`, inside `impl Registry { ... }`, add:

```rust
pub fn set_jsonl_path(&self, id: &str, path: &str) -> AppResult<()> {
    let conn = self.conn.lock().unwrap();
    let n = conn.execute(
        "UPDATE sessions SET jsonl_path = ?1 WHERE id = ?2",
        rusqlite::params![path, id],
    )?;
    if n == 0 { return Err(AppError::NotFound(format!("session {id}"))); }
    Ok(())
}

pub fn apply_usage_delta(
    &self,
    id: &str,
    new_offset: i64,
    add_tokens_in: i64,
    add_tokens_out: i64,
    add_tokens_cache_read: i64,
    add_tokens_cache_write: i64,
    add_cost_usd: f64,
    last_activity_at: i64,
) -> AppResult<()> {
    let conn = self.conn.lock().unwrap();
    let n = conn.execute(
        r#"
        UPDATE sessions SET
            jsonl_offset = ?1,
            tokens_in = tokens_in + ?2,
            tokens_out = tokens_out + ?3,
            tokens_cache_read = tokens_cache_read + ?4,
            tokens_cache_write = tokens_cache_write + ?5,
            cost_usd = cost_usd + ?6,
            last_activity_at = ?7
        WHERE id = ?8
        "#,
        rusqlite::params![
            new_offset,
            add_tokens_in,
            add_tokens_out,
            add_tokens_cache_read,
            add_tokens_cache_write,
            add_cost_usd,
            last_activity_at,
            id,
        ],
    )?;
    if n == 0 { return Err(AppError::NotFound(format!("session {id}"))); }
    Ok(())
}

/// Sums cost / tokens for sessions whose `started_at` is in `[since, now]`,
/// regardless of status. Used by the usage strip.
pub fn usage_since(&self, since: i64) -> AppResult<UsageSummary> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT COALESCE(SUM(tokens_in),0), COALESCE(SUM(tokens_out),0),
                COALESCE(SUM(tokens_cache_read),0), COALESCE(SUM(tokens_cache_write),0),
                COALESCE(SUM(cost_usd),0)
         FROM sessions WHERE started_at >= ?1"
    )?;
    let row = stmt.query_row(rusqlite::params![since], |r| {
        Ok(UsageSummary {
            tokens_in: r.get(0)?,
            tokens_out: r.get(1)?,
            tokens_cache_read: r.get(2)?,
            tokens_cache_write: r.get(3)?,
            cost_usd: r.get(4)?,
        })
    })?;
    Ok(row)
}
```

And add the type at the top of the file (near `Session`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageSummary {
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub tokens_cache_read: i64,
    pub tokens_cache_write: i64,
    pub cost_usd: f64,
}
```

- [ ] **Step 2: Add tests at the bottom of the same file**

Inside `mod tests { ... }`:

```rust
#[test]
fn apply_usage_delta_accumulates() {
    let r = make();
    let s = r.insert(new_sess("/p")).unwrap();
    r.apply_usage_delta(&s.id, 100, 10, 20, 1, 2, 0.05, 12345).unwrap();
    r.apply_usage_delta(&s.id, 200, 5, 5, 0, 0, 0.01, 23456).unwrap();
    let got = r.get(&s.id).unwrap();
    assert_eq!(got.tokens_in, 15);
    assert_eq!(got.tokens_out, 25);
    assert_eq!(got.jsonl_offset, 200);
    assert!((got.cost_usd - 0.06).abs() < 1e-9);
    assert_eq!(got.last_activity_at, 23456);
}

#[test]
fn usage_since_sums_recent_only() {
    let r = make();
    let a = r.insert(new_sess("/a")).unwrap();
    let b = r.insert(new_sess("/b")).unwrap();
    r.apply_usage_delta(&a.id, 0, 100, 0, 0, 0, 0.10, 0).unwrap();
    r.apply_usage_delta(&b.id, 0, 50, 0, 0, 0, 0.05, 0).unwrap();
    let summary = r.usage_since(0).unwrap();
    assert_eq!(summary.tokens_in, 150);
    assert!((summary.cost_usd - 0.15).abs() < 1e-9);
}

#[test]
fn set_jsonl_path_persists() {
    let r = make();
    let s = r.insert(new_sess("/p")).unwrap();
    r.set_jsonl_path(&s.id, "/some/path.jsonl").unwrap();
    assert_eq!(r.get(&s.id).unwrap().jsonl_path.as_deref(), Some("/some/path.jsonl"));
}
```

- [ ] **Step 3: Run tests, expect PASS**

```bash
cd C:/GitProjects/FastClaude/src-tauri && cargo test --lib session_registry::
```

Expected: 7 tests pass (4 from Plan 1 + 3 new).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/session_registry.rs
git commit -m "feat(registry): apply_usage_delta, usage_since, set_jsonl_path"
```

---

## Task 3: Extend the poller — JSONL discovery, cost, idle transitions

**Files:**
- Modify: `src-tauri/src/poller.rs`

The poller already marks dead sessions ended. Now also: discover the JSONL path on first tick after spawn, call `cost_reader::read_delta` on each tick if the file mtime advanced, mark a session `idle` if `last_activity_at` is older than `idle_threshold_seconds`.

- [ ] **Step 1: Replace `tick` with the richer version and adjust `run_loop`**

Replace `tick` and `run_loop` in `src-tauri/src/poller.rs` with:

```rust
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
    fn default() -> Self { Self::new() }
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
        // 1) liveness
        if !probe.alive(s.claude_pid as u32) {
            registry.mark_ended(&s.id, now)?;
            report.ended_ids.push(s.id);
            continue;
        }
        // 2) discover jsonl path if unknown
        let jsonl_path = match s.jsonl_path.clone() {
            Some(p) => Some(PathBuf::from(p)),
            None => find_jsonl_for(&s)
                .and_then(|p| {
                    let s_str = p.to_string_lossy().to_string();
                    let _ = registry.set_jsonl_path(&s.id, &s_str);
                    Some(p)
                }),
        };
        // 3) cost / idle
        let Some(jsonl) = jsonl_path else { continue };
        let mtime = match std::fs::metadata(&jsonl).and_then(|m| m.modified()) {
            Ok(t) => t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
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
/// is at or after the session's start_time.
fn find_jsonl_for(s: &Session) -> Option<PathBuf> {
    let root = recent_projects::default_claude_root().ok()?;
    // Encode the project_dir the same way Claude does.
    let encoded = encode_project_dir(&s.project_dir);
    let dir = root.join("projects").join(encoded);
    let mut best: Option<(PathBuf, i64)> = None;
    for entry in std::fs::read_dir(&dir).ok()? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") { continue; }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mod_time) = meta.modified() else { continue };
        let mtime = mod_time.duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64).unwrap_or(0);
        if mtime + 5 < s.started_at { continue; } // small slack
        if best.as_ref().map_or(true, |(_, t)| mtime > *t) {
            best = Some((path, mtime));
        }
    }
    best.map(|(p, _)| p)
}

/// Inverse of recent_projects::decode_name.
fn encode_project_dir(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let mut out = String::with_capacity(normalized.len() + 1);
    if let Some((drive, rest)) = normalized.split_once(":/") {
        out.push_str(drive);
        out.push_str("--");
        out.push_str(&rest.replace('/', "-"));
    } else if normalized.starts_with('/') {
        // /home/x → -home-x
        out.push_str(&normalized.replace('/', "-"));
    } else {
        out.push_str(&normalized.replace('/', "-"));
    }
    out
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
```

- [ ] **Step 2: Add encoding tests at the bottom of the test module**

Inside `mod tests { ... }`:

```rust
#[test]
fn encode_decode_round_trip_windows() {
    let dir = "C:/GitProjects/FastClaude";
    let encoded = encode_project_dir(dir);
    assert_eq!(encoded, "C--GitProjects-FastClaude");
}

#[test]
fn encode_unix_path() {
    let dir = "/home/tal/portfolio";
    let encoded = encode_project_dir(dir);
    assert_eq!(encoded, "-home-tal-portfolio");
}

#[test]
fn marks_dead_sessions_ended_only_with_cost_reader() {
    use crate::config::Config;
    use crate::session_registry::NewSession;
    use std::collections::HashSet;
    struct FakeProbe(HashSet<u32>);
    impl LivenessProbe for FakeProbe {
        fn alive(&mut self, pid: u32) -> bool { self.0.contains(&pid) }
    }
    let r = Registry::open_in_memory().unwrap();
    let cfg = Config::default();
    let alive = r.insert(NewSession {
        project_dir: "/p/a".into(), model: "claude-opus-4-7".into(),
        claude_pid: 100, terminal_pid: 99, terminal_window_handle: None,
    }).unwrap();
    let dead = r.insert(NewSession {
        project_dir: "/p/b".into(), model: "claude-opus-4-7".into(),
        claude_pid: 200, terminal_pid: 199, terminal_window_handle: None,
    }).unwrap();
    let mut probe = FakeProbe([100u32].into_iter().collect());
    let report = tick(&r, &mut probe, &cfg, 12345).unwrap();
    assert_eq!(report.ended_ids, vec![dead.id.clone()]);
    assert_eq!(r.list_active().unwrap()[0].id, alive.id);
}
```

(The single test from Plan 1 has been replaced with the version above so the `tick` signature change compiles.)

- [ ] **Step 3: Run tests, expect PASS**

```bash
cd C:/GitProjects/FastClaude/src-tauri && cargo test --lib poller::
```

Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/poller.rs
git commit -m "feat(poller): JSONL discovery, cost reading, idle transitions"
```

---

## Task 4: Wire new poller signature into `main.rs` and emit `usage-updated`

**Files:**
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/commands.rs`

The poller now needs the config and emits `usage-updated` whenever cost changed.

- [ ] **Step 1: Hold config in `Arc<Mutex<Config>>` everywhere**

In `src-tauri/src/commands.rs`, change `AppState::config` from `Mutex<Config>` to `Arc<Mutex<Config>>` so the poller and commands share the same handle:

```rust
pub struct AppState {
    pub registry: Arc<Registry>,
    pub spawner: Box<dyn Spawner>,
    pub focus: Box<dyn WindowFocus>,
    pub config: Arc<std::sync::Mutex<Config>>,
}
```

- [ ] **Step 2: Update `main.rs` to construct shared config and pass it to poller**

In `src-tauri/src/main.rs`, update the setup callback:

```rust
let cfg = config::load(&cfg_path).expect("load config");
let cfg_arc = Arc::new(Mutex::new(cfg));

let state = AppState {
    registry: registry.clone(),
    spawner: spawner::default_spawner(),
    focus: window_focus::default_focus(),
    config: cfg_arc.clone(),
};
app.manage(state);

let app_handle = app.handle().clone();
let registry_for_poller = registry.clone();
let cfg_for_poller = cfg_arc.clone();
tauri::async_runtime::spawn(async move {
    poller::run_loop(
        registry_for_poller,
        cfg_for_poller,
        std::time::Duration::from_secs(2),
        move |report| {
            if !report.ended_ids.is_empty() {
                let _ = app_handle.emit("session-changed", &report.ended_ids);
            }
            if report.usage_changed {
                let _ = app_handle.emit("usage-updated", ());
            }
        },
    )
    .await;
});
```

- [ ] **Step 3: Build to verify**

```bash
cd C:/GitProjects/FastClaude/src-tauri && cargo build
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/main.rs src-tauri/src/commands.rs
git commit -m "feat(app): share config Arc with poller; emit usage-updated"
```

---

## Task 5: New IPC commands — `get_usage_summary`, `set_config`

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/main.rs` (register new commands)

- [ ] **Step 1: Add commands at the bottom of `commands.rs`**

```rust
use crate::session_registry::UsageSummary;

#[tauri::command]
pub fn get_usage_summary(state: State<'_, AppState>, since: i64) -> AppResult<UsageSummary> {
    state.registry.usage_since(since)
}

#[tauri::command]
pub fn set_config(state: State<'_, AppState>, cfg: Config) -> AppResult<()> {
    let mut held = state.config.lock().unwrap();
    *held = cfg.clone();
    // Persist to disk in the user's app data dir.
    let dir = dirs::config_dir()
        .ok_or_else(|| crate::error::AppError::Other("no config dir".into()))?
        .join("FastClaude");
    std::fs::create_dir_all(&dir)?;
    crate::config::save(&dir.join("config.json"), &cfg)?;
    Ok(())
}
```

Add `dirs` to the use list at the top of the file:

```rust
use dirs as _; // pulled via crate::* indirectly; explicit use only for set_config above
```

(Actually `dirs::config_dir` is called via crate-path, no `use` needed.)

- [ ] **Step 2: Register both in `main.rs`**

In `tauri::generate_handler!`, add `commands::get_usage_summary` and `commands::set_config`.

- [ ] **Step 3: Build**

```bash
cd C:/GitProjects/FastClaude/src-tauri && cargo build
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/main.rs
git commit -m "feat(ipc): get_usage_summary and set_config commands"
```

---

## Task 6: Global hotkey (backend)

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/capabilities/default.json` (or whatever the capability file is named in the scaffold)

- [ ] **Step 1: Add the plugin dependency**

In `src-tauri/Cargo.toml` `[dependencies]`:

```toml
tauri-plugin-global-shortcut = "2"
```

- [ ] **Step 2: Register the plugin and bind the configured hotkey**

In `src-tauri/src/main.rs`, modify the builder:

```rust
use tauri_plugin_global_shortcut::{Builder as ShortcutBuilder, GlobalShortcutExt, Shortcut};

// inside main():
tauri::Builder::default()
    .plugin(tauri_plugin_opener::init())
    .plugin(ShortcutBuilder::new().build())
    .setup(|app| {
        // ... existing setup before this snippet ...
        let hotkey_str = cfg_arc.lock().unwrap().hotkey.clone();
        if let Ok(parsed) = hotkey_str.parse::<Shortcut>() {
            let app_handle_for_hk = app.handle().clone();
            app.global_shortcut().on_shortcut(parsed, move |_app, _shortcut, _event| {
                let _ = app_handle_for_hk.emit("hotkey-fired", ());
            }).ok();
        } else {
            eprintln!("invalid hotkey in config: {hotkey_str}");
        }
        Ok(())
    })
    // ...
```

- [ ] **Step 3: Add capability**

Find the capabilities JSON file (likely `src-tauri/capabilities/default.json`) and add the global-shortcut permission to its `permissions` array:

```json
"global-shortcut:allow-register",
"global-shortcut:allow-unregister",
"global-shortcut:allow-is-registered"
```

- [ ] **Step 4: Build and verify**

```bash
cd C:/GitProjects/FastClaude/src-tauri && cargo build
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/main.rs src-tauri/capabilities/
git commit -m "feat(hotkey): register global shortcut, emit hotkey-fired"
```

---

## Task 7: Frontend — Toaster + UsageStrip + IPC additions

**Files:**
- Modify: `src/App.tsx`, `src/lib/ipc.ts`, `src/types.ts`
- Create: `src/components/UsageStrip.tsx`

- [ ] **Step 1: Add types**

In `src/types.ts`, append:

```ts
export interface UsageSummary {
  tokens_in: number;
  tokens_out: number;
  tokens_cache_read: number;
  tokens_cache_write: number;
  cost_usd: number;
}
```

- [ ] **Step 2: Add IPC wrappers in `src/lib/ipc.ts`**

Append:

```ts
import type { UsageSummary, AppConfig } from "@/types";
// (UsageSummary added — keep existing imports too)

export async function getUsageSummary(since: number): Promise<UsageSummary> {
  return invoke<UsageSummary>("get_usage_summary", { since });
}

export async function setConfig(cfg: AppConfig): Promise<void> {
  return invoke<void>("set_config", { cfg });
}

export async function onUsageUpdated(handler: () => void): Promise<UnlistenFn> {
  return listen("usage-updated", () => handler());
}

export async function onHotkeyFired(handler: () => void): Promise<UnlistenFn> {
  return listen("hotkey-fired", () => handler());
}
```

- [ ] **Step 3: Create `src/components/UsageStrip.tsx`**

```tsx
import { useEffect, useState } from "react";
import { getUsageSummary, onUsageUpdated } from "@/lib/ipc";
import type { UsageSummary } from "@/types";

const SECS_PER_DAY = 86_400;

function startOfDayEpoch(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return Math.floor(d.getTime() / 1000);
}

function startOfWeekEpoch(): number {
  return startOfDayEpoch() - 6 * SECS_PER_DAY;
}

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

function fmtCost(n: number): string {
  return `$${n.toFixed(2)}`;
}

export function UsageStrip() {
  const [today, setToday] = useState<UsageSummary | null>(null);
  const [week, setWeek] = useState<UsageSummary | null>(null);

  function refresh() {
    getUsageSummary(startOfDayEpoch()).then(setToday).catch(() => {});
    getUsageSummary(startOfWeekEpoch()).then(setWeek).catch(() => {});
  }

  useEffect(() => {
    refresh();
    let unlisten: (() => void) | null = null;
    onUsageUpdated(refresh).then((fn) => { unlisten = fn; });
    const t = setInterval(refresh, 30_000);
    return () => { unlisten?.(); clearInterval(t); };
  }, []);

  if (!today || !week) return null;
  const totalTokens = today.tokens_in + today.tokens_out + today.tokens_cache_read + today.tokens_cache_write;

  return (
    <div className="flex gap-6 px-4 py-2 text-xs bg-muted/30 border-t border-border">
      <div><span className="text-muted-foreground">Today:</span> <strong>{fmtCost(today.cost_usd)}</strong></div>
      <div><span className="text-muted-foreground">This week:</span> <strong>{fmtCost(week.cost_usd)}</strong></div>
      <div><span className="text-muted-foreground">Today tokens:</span> <strong>{fmtTokens(totalTokens)}</strong></div>
    </div>
  );
}
```

- [ ] **Step 4: Wire `<Toaster />` + `<UsageStrip />` + hotkey listener into `App.tsx`**

```tsx
import { useEffect, useState } from "react";
import { Dashboard } from "@/components/Dashboard";
import { Settings } from "@/components/Settings";
import { UsageStrip } from "@/components/UsageStrip";
import { Toaster } from "@/components/ui/toaster";
import { onHotkeyFired } from "@/lib/ipc";

type View = "dashboard" | "settings";

export default function App() {
  const [view, setView] = useState<View>("dashboard");
  const [launchOpen, setLaunchOpen] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    onHotkeyFired(() => {
      setView("dashboard");
      setLaunchOpen(true);
    }).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, []);

  return (
    <>
      {view === "dashboard"
        ? <Dashboard onOpenSettings={() => setView("settings")} launchOpen={launchOpen} setLaunchOpen={setLaunchOpen} />
        : <Settings onBack={() => setView("dashboard")} />}
      <UsageStrip />
      <Toaster />
    </>
  );
}
```

- [ ] **Step 5: Update `Dashboard.tsx` props to accept onOpenSettings, launchOpen, setLaunchOpen**

In `src/components/Dashboard.tsx`, replace the component signature to receive props:

```tsx
export function Dashboard({
  onOpenSettings,
  launchOpen,
  setLaunchOpen,
}: {
  onOpenSettings: () => void;
  launchOpen: boolean;
  setLaunchOpen: (v: boolean) => void;
}) {
  // ... existing state and refresh logic, but use launchOpen/setLaunchOpen
  // instead of the local `open` state.
```

Replace the local `open` state with the prop. Add a Settings button in the top bar:

```tsx
<button onClick={onOpenSettings} className="px-3 py-1.5 rounded bg-secondary text-secondary-foreground text-sm">
  Settings
</button>
```

next to the existing Launch button.

Also change the existing onLaunched in LaunchDialog to call `setLaunchOpen(false)` instead of the previous local close.

- [ ] **Step 6: Build to verify**

```bash
cd C:/GitProjects/FastClaude && npm run build
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(frontend): UsageStrip, Toaster, hotkey wiring, view switch"
```

---

## Task 8: Frontend — Settings page

**Files:**
- Create: `src/components/Settings.tsx`

- [ ] **Step 1: Write the component**

```tsx
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useToast } from "@/hooks/use-toast";
import { getConfig, setConfig } from "@/lib/ipc";
import type { AppConfig } from "@/types";

export function Settings({ onBack }: { onBack: () => void }) {
  const { toast } = useToast();
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [draft, setDraft] = useState<AppConfig | null>(null);

  useEffect(() => {
    getConfig().then((c) => { setCfg(c); setDraft(c); }).catch(() => {});
  }, []);

  if (!draft || !cfg) return <div className="p-8">Loading...</div>;

  async function save() {
    if (!draft) return;
    try {
      await setConfig(draft);
      toast({ title: "Settings saved" });
      onBack();
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Failed to save", description: msg, variant: "destructive" });
    }
  }

  function field(label: string, value: string, onChange: (v: string) => void) {
    return (
      <label className="block">
        <div className="text-xs font-medium mb-1">{label}</div>
        <Input value={value} onChange={(e) => onChange(e.target.value)} />
      </label>
    );
  }

  return (
    <div className="min-h-screen bg-background text-foreground">
      <div className="flex items-center gap-3 px-4 py-3 border-b border-border">
        <button onClick={onBack} className="text-sm">← Back</button>
        <div className="font-semibold">Settings</div>
      </div>
      <div className="p-4 space-y-4 max-w-xl">
        {field("Terminal program (or 'auto')", draft.terminal_program, (v) => setDraft({ ...draft, terminal_program: v }))}
        {field("Default model", draft.default_model, (v) => setDraft({ ...draft, default_model: v }))}
        {field("Global hotkey", draft.hotkey, (v) => setDraft({ ...draft, hotkey: v }))}
        {field("Idle threshold (seconds)", String(draft.idle_threshold_seconds), (v) => {
          const n = parseInt(v, 10);
          if (!Number.isNaN(n) && n > 0) setDraft({ ...draft, idle_threshold_seconds: n });
        })}
        <div className="pt-4 flex gap-2 justify-end">
          <Button variant="ghost" onClick={onBack}>Cancel</Button>
          <Button onClick={save}>Save</Button>
        </div>
        <p className="text-xs text-muted-foreground pt-4">
          Hotkey changes take effect after restart. Per-model pricing is editable by hand
          in <code>%APPDATA%/FastClaude/config.json</code>.
        </p>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Build**

```bash
cd C:/GitProjects/FastClaude && npm run build
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src/components/Settings.tsx
git commit -m "feat(frontend): Settings page with terminal/model/hotkey/idle config"
```

---

## Task 9: Toast errors on IPC failures

**Files:**
- Modify: `src/components/SessionRow.tsx`, `src/components/LaunchDialog.tsx`

Replace the silent `catch { /* toast in Plan 2 */ }` blocks with actual toasts.

- [ ] **Step 1: Update `SessionRow.tsx`**

Add the toast hook and replace the catch blocks:

```tsx
import { useToast } from "@/hooks/use-toast";
// ... inside component:
const { toast } = useToast();
async function focus() {
  try { await focusSession(session.id); }
  catch (e: unknown) {
    const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
    toast({ title: "Couldn't focus session", description: msg, variant: "destructive" });
  }
  onChange();
}
async function kill() {
  try { await killSession(session.id); }
  catch (e: unknown) {
    const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
    toast({ title: "Couldn't kill session", description: msg, variant: "destructive" });
  }
  onChange();
}
```

- [ ] **Step 2: Update `LaunchDialog.tsx` to use the toast hook for parity**

The dialog already shows errors inline; keep that, but ALSO toast on success:

```tsx
const { toast } = useToast();
// after successful launchSession(...) call:
toast({ title: "Session launched" });
```

- [ ] **Step 3: Build**

```bash
cd C:/GitProjects/FastClaude && npm run build
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add src/components/SessionRow.tsx src/components/LaunchDialog.tsx
git commit -m "feat(frontend): toast on IPC failures and session launch"
```

---

## Task 10: End-to-end smoke (manual)

This is a hand checklist you run on your desktop after Tasks 1–9 are merged.

- [ ] **Step 1: Cost tracking works**

Run `npm run tauri dev`. Launch a session in a project folder where you've used claude before (or start a new conversation). Type a few prompts. Within ~2 sec the dashboard should show non-zero token counts and the bottom strip should show today/this-week cost.

- [ ] **Step 2: Idle detection**

Set `idle_threshold_seconds` to `30` in Settings, save. Launch a session, leave it untouched for 35 sec. The status dot on the row should go yellow ("idle"). Type a prompt → goes back to green ("running") within ~2 sec.

- [ ] **Step 3: Global hotkey**

Switch focus to any other app. Press Ctrl+Shift+C (or whatever your configured hotkey is). FastClaude should pop to the foreground and the Launch dialog should open.

- [ ] **Step 4: Settings save and persist**

Change the default model to `claude-sonnet-4-6`, save, restart the app. Open Launch dialog → model picker should default to sonnet.

- [ ] **Step 5: Toast on errors**

Launch a session in a folder without running `claude` available (or kill claude immediately). Click Focus → toast should appear with "Couldn't focus session" + reason.

- [ ] **Step 6: Document any failures**

Capture any error messages and either fix in a follow-up commit or add a "Known issues — Plan 2" note to the design spec.

---

## Self-review notes

Spec coverage check (Plan 2 portion):
- `cost_reader` module ✓ Task 1
- `apply_usage_delta` / `usage_since` / `set_jsonl_path` on registry ✓ Task 2
- Poller JSONL discovery + cost + idle ✓ Task 3
- `usage-updated` event ✓ Task 4
- IPC `get_usage_summary` / `set_config` ✓ Task 5
- Global hotkey + `hotkey-fired` event ✓ Task 6
- Frontend UsageStrip + Toaster + hotkey listener + view switch ✓ Tasks 7, 8
- Toast errors ✓ Task 9
- End-to-end manual smoke ✓ Task 10

Out of scope (Plan 3):
- macOS spawner & focus
- Linux spawner & focus
- Tabbed-terminal aware focus
- Multi-machine / cloud sessions
