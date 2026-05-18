# Auto-continue on 5h limit reset — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect Claude's 5-hour rate-limit signal in a session's JSONL and, at the parsed reset time, respawn the session via `claude --resume <id>` with a "continue" prompt — capped at 3 auto-resumes per chain and restart-safe.

**Architecture:** Extend three existing modules. `usage_reader` reports a `LimitEvent` in the same JSONL pass it already does. `poller` persists `next_resume_at` when it sees one (only on opt-in rows), and fires due resumes through the existing `Spawner` trait. `session_registry` gains six columns and a handful of CRUD methods. No new modules; the spawner is already capable via `SpawnRequest.resume`.

**Tech Stack:** Rust (rusqlite, sysinfo, chrono, serde, tokio), React + TypeScript + Tailwind, Tauri 2 IPC.

---

## File Structure

**Backend (Rust)**

| File | Change |
|---|---|
| `src-tauri/src/session_registry.rs` | Add six columns to schema + ALTER migrations + new `Session` fields + new methods: `insert_with_auto`, `set_auto_continue`, `set_resume_prompt`, `set_pending_resume`, `clear_pending_resume`, `record_resume_success`, `record_resume_failure`, `list_due_resumes`. |
| `src-tauri/src/config.rs` | Three new fields: `default_resume_prompt`, `default_auto_continue`, `default_resume_cap`. |
| `src-tauri/src/usage_reader.rs` | Add `LimitEvent` type + `limit_event` on `UsageDelta` + regex/pattern matching on assistant/system lines. |
| `src-tauri/src/poller.rs` | Extend `tick()` to (a) capture `limit_event` and persist pending_resume on opt-in rows, (b) iterate due resumes and call spawner. Extend `run_loop` to accept a `Spawner`. |
| `src-tauri/src/commands.rs` | Three new commands: `set_auto_continue`, `set_resume_prompt`, plus extend `LaunchInput` and `launch_session` to accept the new opt-in fields. |
| `src-tauri/src/main.rs` | Pass a spawner clone (Arc-wrapped) into `poller::run_loop`. |

**Frontend (React)**

| File | Change |
|---|---|
| `src/types.ts` | Add fields to `Session`, `AppConfig`, `LaunchInput`. |
| `src/lib/ipc.ts` | Add `setAutoContinue`, `setResumePrompt` wrappers. |
| `src/components/LaunchDialog.tsx` | New "auto-continue" checkbox + collapsible resume-prompt textarea. |
| `src/components/SessionRow.tsx` | New "auto-continue" pill (armed/off/firing/cap-reached states) wired to `setAutoContinue`. |
| `src/components/Settings.tsx` | New "Auto-continue" section with three controls. |

---

## Task 1: Add new columns to the sessions table schema

**Files:**
- Modify: `src-tauri/src/session_registry.rs:97-123` (init_schema)
- Modify: `src-tauri/src/session_registry.rs:48-66` (Session struct), `:68-75` (NewSession), `:355-375` (row_to_session)

- [ ] **Step 1: Write the failing test for column presence**

Add to the `tests` module in `src-tauri/src/session_registry.rs`:

```rust
#[test]
fn insert_defaults_auto_continue_fields() {
    let r = make();
    let s = r.insert(new_sess("/p")).unwrap();
    assert!(!s.auto_continue);
    assert_eq!(s.resume_prompt, None);
    assert_eq!(s.next_resume_at, None);
    assert_eq!(s.resume_count, 0);
    assert!(s.resume_cap >= 1, "default cap must be at least 1");
    assert_eq!(s.resumed_into, None);
    assert_eq!(s.resume_failures, 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p fastclaude_lib session_registry::tests::insert_defaults_auto_continue_fields -- --exact`

Expected: FAIL with "no field `auto_continue` on type `Session`".

- [ ] **Step 3: Extend `Session` struct, `NewSession`, schema, row_to_session, and `insert`**

In `src-tauri/src/session_registry.rs`, modify the `Session` struct (around line 48) to add fields at the end:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub id: String,
    pub project_dir: String,
    pub model: String,
    pub claude_pid: i64,
    pub terminal_pid: i64,
    pub terminal_window_handle: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub jsonl_path: Option<String>,
    pub jsonl_offset: i64,
    pub status: Status,
    pub last_activity_at: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub tokens_cache_read: i64,
    pub tokens_cache_write: i64,
    // NEW — auto-continue feature
    pub auto_continue: bool,
    pub resume_prompt: Option<String>,
    pub next_resume_at: Option<i64>,
    pub resume_count: i64,
    pub resume_cap: i64,
    pub resumed_into: Option<String>,
    pub resume_failures: i64,
}
```

Modify `NewSession` (around line 68):

```rust
#[derive(Debug, Clone)]
pub struct NewSession {
    pub project_dir: String,
    pub model: String,
    pub claude_pid: i64,
    pub terminal_pid: i64,
    pub terminal_window_handle: Option<String>,
    // NEW — defaults applied at insert time if zero/empty
    pub auto_continue: bool,
    pub resume_prompt: Option<String>,
    pub resume_cap: i64,
    /// Initial resume_count. Non-zero only for rows created as the
    /// continuation of a previous auto-resume (predecessor.resume_count + 1).
    pub resume_count: i64,
}
```

Modify `init_schema` to add columns AND a migration for existing DBs:

```rust
fn init_schema(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            project_dir TEXT NOT NULL,
            model TEXT NOT NULL,
            claude_pid INTEGER NOT NULL,
            terminal_pid INTEGER NOT NULL,
            terminal_window_handle TEXT,
            started_at INTEGER NOT NULL,
            ended_at INTEGER,
            jsonl_path TEXT,
            jsonl_offset INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL,
            last_activity_at INTEGER NOT NULL,
            tokens_in INTEGER NOT NULL DEFAULT 0,
            tokens_out INTEGER NOT NULL DEFAULT 0,
            tokens_cache_read INTEGER NOT NULL DEFAULT 0,
            tokens_cache_write INTEGER NOT NULL DEFAULT 0,
            auto_continue INTEGER NOT NULL DEFAULT 0,
            resume_prompt TEXT,
            next_resume_at INTEGER,
            resume_count INTEGER NOT NULL DEFAULT 0,
            resume_cap INTEGER NOT NULL DEFAULT 3,
            resumed_into TEXT,
            resume_failures INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_active
          ON sessions(ended_at) WHERE ended_at IS NULL;
        CREATE INDEX IF NOT EXISTS idx_sessions_pending_resume
          ON sessions(next_resume_at) WHERE next_resume_at IS NOT NULL;
        "#,
    )?;
    // Migrations for existing DBs. Each ALTER is wrapped so a duplicate-column
    // error on re-run is silently ignored.
    let migrations = [
        "ALTER TABLE sessions ADD COLUMN auto_continue INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE sessions ADD COLUMN resume_prompt TEXT",
        "ALTER TABLE sessions ADD COLUMN next_resume_at INTEGER",
        "ALTER TABLE sessions ADD COLUMN resume_count INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE sessions ADD COLUMN resume_cap INTEGER NOT NULL DEFAULT 3",
        "ALTER TABLE sessions ADD COLUMN resumed_into TEXT",
        "ALTER TABLE sessions ADD COLUMN resume_failures INTEGER NOT NULL DEFAULT 0",
    ];
    for sql in migrations {
        // SQLite returns error "duplicate column name" when re-applying; that's fine.
        let _ = conn.execute(sql, []);
    }
    Ok(())
}
```

Modify `row_to_session` (around line 355) to read the new columns:

```rust
fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let status_s: String = row.get(10)?;
    Ok(Session {
        id: row.get(0)?,
        project_dir: row.get(1)?,
        model: row.get(2)?,
        claude_pid: row.get(3)?,
        terminal_pid: row.get(4)?,
        terminal_window_handle: row.get(5)?,
        started_at: row.get(6)?,
        ended_at: row.get(7)?,
        jsonl_path: row.get(8)?,
        jsonl_offset: row.get(9)?,
        status: Status::parse(&status_s).unwrap_or(Status::Ended),
        last_activity_at: row.get(11)?,
        tokens_in: row.get(12)?,
        tokens_out: row.get(13)?,
        tokens_cache_read: row.get(14)?,
        tokens_cache_write: row.get(15)?,
        auto_continue: row.get::<_, i64>(16)? != 0,
        resume_prompt: row.get(17)?,
        next_resume_at: row.get(18)?,
        resume_count: row.get(19)?,
        resume_cap: row.get(20)?,
        resumed_into: row.get(21)?,
        resume_failures: row.get(22)?,
    })
}
```

Update both SELECT queries (`list_where` around line 191, and `get` around line 208) to include the new columns:

```rust
const SESSION_COLS: &str = "id, project_dir, model, claude_pid, terminal_pid, \
    terminal_window_handle, started_at, ended_at, jsonl_path, jsonl_offset, \
    status, last_activity_at, tokens_in, tokens_out, tokens_cache_read, \
    tokens_cache_write, auto_continue, resume_prompt, next_resume_at, \
    resume_count, resume_cap, resumed_into, resume_failures";
```

Replace the hard-coded SELECT lists in `list_where` and `get` with `format!("SELECT {SESSION_COLS} FROM sessions ...")`.

Modify `insert` (around line 125) to persist the new columns and populate the returned Session:

```rust
pub fn insert(&self, n: NewSession) -> AppResult<Session> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let resume_cap = if n.resume_cap > 0 { n.resume_cap } else { 3 };
    let s = Session {
        id: id.clone(),
        project_dir: n.project_dir,
        model: n.model,
        claude_pid: n.claude_pid,
        terminal_pid: n.terminal_pid,
        terminal_window_handle: n.terminal_window_handle,
        started_at: now,
        ended_at: None,
        jsonl_path: None,
        jsonl_offset: 0,
        status: Status::Running,
        last_activity_at: now,
        tokens_in: 0,
        tokens_out: 0,
        tokens_cache_read: 0,
        tokens_cache_write: 0,
        auto_continue: n.auto_continue,
        resume_prompt: n.resume_prompt,
        next_resume_at: None,
        resume_count: n.resume_count,
        resume_cap,
        resumed_into: None,
        resume_failures: 0,
    };
    let conn = self.conn.lock().unwrap();
    conn.execute(
        r#"
        INSERT INTO sessions
            (id, project_dir, model, claude_pid, terminal_pid, terminal_window_handle,
             started_at, status, last_activity_at,
             auto_continue, resume_prompt, resume_count, resume_cap)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        "#,
        params![
            s.id, s.project_dir, s.model, s.claude_pid, s.terminal_pid,
            s.terminal_window_handle, s.started_at, s.status.as_str(), s.last_activity_at,
            s.auto_continue as i64, s.resume_prompt, s.resume_count, s.resume_cap,
        ],
    )?;
    Ok(s)
}
```

Existing call sites in the same file (`new_sess` helper at line 385) and in `commands.rs:75` must be updated to include the new fields. Apply this minimal patch to the test helper:

```rust
fn new_sess(dir: &str) -> NewSession {
    NewSession {
        project_dir: dir.into(),
        model: "claude-opus-4-7".into(),
        claude_pid: 1234,
        terminal_pid: 1230,
        terminal_window_handle: Some("hwnd-abc".into()),
        auto_continue: false,
        resume_prompt: None,
        resume_cap: 3,
        resume_count: 0,
    }
}
```

And in `src-tauri/src/commands.rs:75` (inside `launch_session`), update the `NewSession` literal — Task 7 will overwrite this with the full opt-in flow; for now just append the four new fields with defaults:

```rust
let session = state.registry.insert(NewSession {
    project_dir: input.project_dir,
    model,
    claude_pid: result.claude_pid,
    terminal_pid: result.terminal_pid,
    terminal_window_handle: result.terminal_window_handle,
    auto_continue: false,
    resume_prompt: None,
    resume_cap: 3,
    resume_count: 0,
})?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p fastclaude_lib session_registry`

Expected: all session_registry tests pass, including the new `insert_defaults_auto_continue_fields`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/session_registry.rs src-tauri/src/commands.rs
git commit -m "feat(registry): add auto-continue columns to sessions schema"
```

---

## Task 2: Migration test — open an old-schema DB and read it

**Files:**
- Modify: `src-tauri/src/session_registry.rs` (tests module)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
#[test]
fn open_old_schema_db_runs_alter_migrations() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("legacy.db");
    // Hand-craft a pre-feature schema (no auto_continue columns).
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            project_dir TEXT NOT NULL,
            model TEXT NOT NULL,
            claude_pid INTEGER NOT NULL,
            terminal_pid INTEGER NOT NULL,
            terminal_window_handle TEXT,
            started_at INTEGER NOT NULL,
            ended_at INTEGER,
            jsonl_path TEXT,
            jsonl_offset INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL,
            last_activity_at INTEGER NOT NULL,
            tokens_in INTEGER NOT NULL DEFAULT 0,
            tokens_out INTEGER NOT NULL DEFAULT 0,
            tokens_cache_read INTEGER NOT NULL DEFAULT 0,
            tokens_cache_write INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO sessions
            (id, project_dir, model, claude_pid, terminal_pid,
             started_at, status, last_activity_at)
        VALUES ('legacy-id', '/p', 'claude-opus-4-7', 100, 99, 1000, 'running', 1000);
        "#,
    ).unwrap();
    drop(conn);

    // Reopen via Registry — should run ALTERs and read the legacy row.
    let r = Registry::open(&path).unwrap();
    let s = r.get("legacy-id").unwrap();
    assert!(!s.auto_continue);
    assert_eq!(s.resume_cap, 3);
    assert_eq!(s.resume_count, 0);
    assert_eq!(s.next_resume_at, None);
}
```

- [ ] **Step 2: Run test to verify it passes (the schema work in Task 1 should already cover this)**

Run: `cargo test -p fastclaude_lib session_registry::tests::open_old_schema_db_runs_alter_migrations -- --exact`

Expected: PASS.

If it fails because the ALTER block isn't there yet, re-check Task 1. The point of this test is to lock the migration behavior so future schema edits don't break old DBs.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/session_registry.rs
git commit -m "test(registry): cover ALTER migration of legacy schema"
```

---

## Task 3: Registry CRUD for auto-continue state changes

**Files:**
- Modify: `src-tauri/src/session_registry.rs` (impl Registry)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
#[test]
fn set_auto_continue_persists() {
    let r = make();
    let s = r.insert(new_sess("/p")).unwrap();
    r.set_auto_continue(&s.id, true).unwrap();
    assert!(r.get(&s.id).unwrap().auto_continue);
    r.set_auto_continue(&s.id, false).unwrap();
    assert!(!r.get(&s.id).unwrap().auto_continue);
}

#[test]
fn set_auto_continue_off_clears_pending_resume() {
    let r = make();
    let s = r.insert(new_sess("/p")).unwrap();
    r.set_auto_continue(&s.id, true).unwrap();
    r.set_pending_resume(&s.id, 2000).unwrap();
    assert_eq!(r.get(&s.id).unwrap().next_resume_at, Some(2000));
    r.set_auto_continue(&s.id, false).unwrap();
    assert_eq!(r.get(&s.id).unwrap().next_resume_at, None);
}

#[test]
fn set_resume_prompt_persists() {
    let r = make();
    let s = r.insert(new_sess("/p")).unwrap();
    r.set_resume_prompt(&s.id, Some("keep going")).unwrap();
    assert_eq!(r.get(&s.id).unwrap().resume_prompt.as_deref(), Some("keep going"));
    r.set_resume_prompt(&s.id, None).unwrap();
    assert_eq!(r.get(&s.id).unwrap().resume_prompt, None);
}

#[test]
fn set_pending_resume_only_when_auto_continue_on() {
    let r = make();
    let s = r.insert(new_sess("/p")).unwrap();
    // auto_continue is false by default → set_pending_resume is a no-op.
    let changed = r.set_pending_resume(&s.id, 2000).unwrap();
    assert!(!changed, "must not arm a session that hasn't opted in");
    assert_eq!(r.get(&s.id).unwrap().next_resume_at, None);

    r.set_auto_continue(&s.id, true).unwrap();
    let changed = r.set_pending_resume(&s.id, 2000).unwrap();
    assert!(changed);
    assert_eq!(r.get(&s.id).unwrap().next_resume_at, Some(2000));
}

#[test]
fn list_due_resumes_filters_correctly() {
    let r = make();
    let armed = r.insert(NewSession {
        project_dir: "/a".into(),
        model: "m".into(),
        claude_pid: 1,
        terminal_pid: 2,
        terminal_window_handle: None,
        auto_continue: true,
        resume_prompt: None,
        resume_cap: 3,
        resume_count: 0,
    }).unwrap();
    let _disarmed = r.insert(new_sess("/b")).unwrap();
    r.set_pending_resume(&armed.id, 1000).unwrap();

    // Cap-reached row: armed but resume_count == resume_cap
    let capped = r.insert(NewSession {
        project_dir: "/c".into(),
        model: "m".into(),
        claude_pid: 3,
        terminal_pid: 4,
        terminal_window_handle: None,
        auto_continue: true,
        resume_prompt: None,
        resume_cap: 1,
        resume_count: 1,
    }).unwrap();
    r.set_pending_resume(&capped.id, 1000).unwrap();

    let due = r.list_due_resumes(1500).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, armed.id);

    // Future reset time → not due
    let due = r.list_due_resumes(500).unwrap();
    assert!(due.is_empty());
}

#[test]
fn record_resume_success_clears_pending_and_resets_failures() {
    let r = make();
    let s = r.insert(NewSession {
        project_dir: "/p".into(),
        model: "m".into(),
        claude_pid: 1,
        terminal_pid: 2,
        terminal_window_handle: None,
        auto_continue: true,
        resume_prompt: None,
        resume_cap: 3,
        resume_count: 0,
    }).unwrap();
    r.set_pending_resume(&s.id, 1000).unwrap();
    r.record_resume_failure(&s.id, 5000).unwrap(); // bump failures, set retry
    r.record_resume_success(&s.id, "new-id").unwrap();
    let got = r.get(&s.id).unwrap();
    assert_eq!(got.next_resume_at, None);
    assert_eq!(got.resume_failures, 0);
    assert_eq!(got.resumed_into.as_deref(), Some("new-id"));
}

#[test]
fn record_resume_failure_increments_and_sets_next_retry() {
    let r = make();
    let s = r.insert(NewSession {
        project_dir: "/p".into(),
        model: "m".into(),
        claude_pid: 1,
        terminal_pid: 2,
        terminal_window_handle: None,
        auto_continue: true,
        resume_prompt: None,
        resume_cap: 3,
        resume_count: 0,
    }).unwrap();
    r.set_pending_resume(&s.id, 1000).unwrap();
    r.record_resume_failure(&s.id, 5000).unwrap();
    let got = r.get(&s.id).unwrap();
    assert_eq!(got.resume_failures, 1);
    assert_eq!(got.next_resume_at, Some(5000));
}

#[test]
fn mark_ended_clears_pending_resume() {
    let r = make();
    let s = r.insert(NewSession {
        project_dir: "/p".into(),
        model: "m".into(),
        claude_pid: 1,
        terminal_pid: 2,
        terminal_window_handle: None,
        auto_continue: true,
        resume_prompt: None,
        resume_cap: 3,
        resume_count: 0,
    }).unwrap();
    r.set_pending_resume(&s.id, 1000).unwrap();
    r.mark_ended(&s.id, 9999).unwrap();
    // mark_ended already exists — we extend it to clear next_resume_at.
    assert_eq!(r.get(&s.id).unwrap().next_resume_at, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fastclaude_lib session_registry::tests::set_auto_continue`

Expected: FAIL with "no method named `set_auto_continue`".

- [ ] **Step 3: Implement the methods**

Add these methods to `impl Registry` in `src-tauri/src/session_registry.rs`:

```rust
pub fn set_auto_continue(&self, id: &str, on: bool) -> AppResult<()> {
    let conn = self.conn.lock().unwrap();
    let sql = if on {
        "UPDATE sessions SET auto_continue = 1 WHERE id = ?1"
    } else {
        // Disarming clears any pending resume so the fire loop won't act.
        "UPDATE sessions SET auto_continue = 0, next_resume_at = NULL WHERE id = ?1"
    };
    let n = conn.execute(sql, params![id])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("session {id}")));
    }
    Ok(())
}

pub fn set_resume_prompt(&self, id: &str, prompt: Option<&str>) -> AppResult<()> {
    let conn = self.conn.lock().unwrap();
    let n = conn.execute(
        "UPDATE sessions SET resume_prompt = ?1 WHERE id = ?2",
        params![prompt, id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("session {id}")));
    }
    Ok(())
}

/// Persist `next_resume_at` only if the row is armed. Returns true when it
/// actually changed the row.
pub fn set_pending_resume(&self, id: &str, reset_at: i64) -> AppResult<bool> {
    let conn = self.conn.lock().unwrap();
    let n = conn.execute(
        "UPDATE sessions SET next_resume_at = ?1
         WHERE id = ?2 AND auto_continue = 1
           AND resume_count < resume_cap
           AND ended_at IS NULL",
        params![reset_at, id],
    )?;
    Ok(n > 0)
}

/// Rows whose pending resume time has passed and are eligible to fire.
pub fn list_due_resumes(&self, now: i64) -> AppResult<Vec<Session>> {
    self.list_where(&format!(
        "auto_continue = 1
           AND next_resume_at IS NOT NULL
           AND next_resume_at <= {now}
           AND resume_count < resume_cap
         ORDER BY next_resume_at ASC"
    ))
}

pub fn record_resume_success(&self, id: &str, new_id: &str) -> AppResult<()> {
    let conn = self.conn.lock().unwrap();
    let n = conn.execute(
        "UPDATE sessions
            SET resumed_into = ?1,
                next_resume_at = NULL,
                resume_failures = 0
          WHERE id = ?2",
        params![new_id, id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("session {id}")));
    }
    Ok(())
}

pub fn record_resume_failure(&self, id: &str, next_retry_at: i64) -> AppResult<()> {
    let conn = self.conn.lock().unwrap();
    let n = conn.execute(
        "UPDATE sessions
            SET resume_failures = resume_failures + 1,
                next_resume_at = ?1
          WHERE id = ?2",
        params![next_retry_at, id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("session {id}")));
    }
    Ok(())
}

pub fn give_up_resume(&self, id: &str) -> AppResult<()> {
    let conn = self.conn.lock().unwrap();
    let n = conn.execute(
        "UPDATE sessions SET next_resume_at = NULL WHERE id = ?1",
        params![id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("session {id}")));
    }
    Ok(())
}
```

Also modify the existing `mark_ended` method (around line 279) to clear `next_resume_at`:

```rust
pub fn mark_ended(&self, id: &str, ended_at: i64) -> AppResult<()> {
    let conn = self.conn.lock().unwrap();
    let n = conn.execute(
        "UPDATE sessions
            SET ended_at = ?1, status = 'ended', next_resume_at = NULL
          WHERE id = ?2",
        params![ended_at, id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("session {id}")));
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p fastclaude_lib session_registry`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/session_registry.rs
git commit -m "feat(registry): CRUD for auto-continue arming and resume scheduling"
```

---

## Task 4: Config defaults for auto-continue

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src-tauri/src/config.rs`:

```rust
#[test]
fn config_default_has_auto_continue_off_and_cap_three() {
    let cfg = Config::default();
    assert!(!cfg.default_auto_continue);
    assert_eq!(cfg.default_resume_prompt, "continue");
    assert_eq!(cfg.default_resume_cap, 3);
}

#[test]
fn load_defaults_auto_continue_fields_when_missing() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("c.json");
    std::fs::write(
        &path,
        br#"{"terminal_program":"auto","default_model":"claude-opus-4-7",
            "hotkey":"Ctrl+Shift+C","idle_threshold_seconds":300}"#,
    ).unwrap();
    let (cfg, _) = load(&path).unwrap();
    assert!(!cfg.default_auto_continue);
    assert_eq!(cfg.default_resume_prompt, "continue");
    assert_eq!(cfg.default_resume_cap, 3);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p fastclaude_lib config::tests::config_default_has_auto_continue`

Expected: FAIL with "no field `default_auto_continue`".

- [ ] **Step 3: Add the three config fields**

In `src-tauri/src/config.rs`, add to the `Config` struct (before the closing brace, after `launch_mode`):

```rust
    /// Default state for the LaunchDialog's auto-continue checkbox.
    #[serde(default)]
    pub default_auto_continue: bool,
    /// Default prompt sent to claude when an auto-resume fires.
    /// Per-session `resume_prompt` overrides this at fire time.
    #[serde(default = "default_resume_prompt_value")]
    pub default_resume_prompt: String,
    /// Max auto-resumes per session chain. Frozen onto new sessions at
    /// launch time so a later change does not retroactively re-arm.
    #[serde(default = "default_resume_cap_value")]
    pub default_resume_cap: i64,
```

Add the default-value functions outside the struct, near the bottom of the file but before `#[cfg(test)]`:

```rust
fn default_resume_prompt_value() -> String { "continue".into() }
fn default_resume_cap_value() -> i64 { 3 }
```

Modify `Default for Config` to include the new fields:

```rust
impl Default for Config {
    fn default() -> Self {
        Self {
            terminal_program: "auto".into(),
            default_model: "claude-opus-4-7".into(),
            hotkey: "Ctrl+Shift+C".into(),
            idle_threshold_seconds: 300,
            default_effort: String::new(),
            default_permission_mode: String::new(),
            default_extra_args: String::new(),
            default_prompt: String::new(),
            launch_on_login: false,
            launch_mode: LaunchMode::Window,
            default_auto_continue: false,
            default_resume_prompt: default_resume_prompt_value(),
            default_resume_cap: default_resume_cap_value(),
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p fastclaude_lib config`

Expected: all config tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/config.rs
git commit -m "feat(config): default_auto_continue, default_resume_prompt, default_resume_cap"
```

---

## Task 5: usage_reader detects rate-limit lines in JSONL

**Files:**
- Modify: `src-tauri/src/usage_reader.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src-tauri/src/usage_reader.rs`:

```rust
#[test]
fn detects_assistant_text_with_5h_limit_phrase() {
    let f = write_jsonl(&[
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"You've hit the 5-hour limit. Your session resets at 14:30 UTC."}],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
    ]);
    let d = read_delta(f.path(), 0).unwrap();
    let ev = d.limit_event.expect("limit event must be detected");
    // 14:30 UTC today, at-or-after detected_at.
    assert!(ev.reset_at > 0);
    assert!(ev.reset_at >= ev.detected_at);
}

#[test]
fn detects_system_rate_limit_error_line() {
    let f = write_jsonl(&[
        r#"{"type":"system","subtype":"error","content":"rate_limit_error: usage cap reached, resets at 09:00"}"#,
    ]);
    let d = read_delta(f.path(), 0).unwrap();
    let ev = d.limit_event.expect("system rate-limit must be detected");
    assert!(ev.reset_at > 0);
}

#[test]
fn falls_back_when_reset_time_unparseable() {
    let f = write_jsonl(&[
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"5-hour limit reached. Try again later."}]}}"#,
    ]);
    let d = read_delta(f.path(), 0).unwrap();
    let ev = d.limit_event.expect("limit event must be detected even without HH:MM");
    // Fallback: caller will apply last_activity_at + 5h + 60s; the reader
    // signals fallback by setting reset_at == 0 (sentinel).
    assert_eq!(ev.reset_at, 0);
}

#[test]
fn no_limit_event_on_normal_assistant_lines() {
    let f = write_jsonl(&[
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Sure, I can help with that."}],"usage":{"input_tokens":10,"output_tokens":5}}}"#,
    ]);
    let d = read_delta(f.path(), 0).unwrap();
    assert!(d.limit_event.is_none());
}

#[test]
fn limit_event_does_not_overwrite_token_tallies() {
    let f = write_jsonl(&[
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"normal"}],"usage":{"input_tokens":10,"output_tokens":5}}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"5-hour limit reached, resets at 14:30"}],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
    ]);
    let d = read_delta(f.path(), 0).unwrap();
    assert_eq!(d.tokens_in, 11);
    assert_eq!(d.tokens_out, 6);
    assert!(d.limit_event.is_some());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fastclaude_lib usage_reader::tests::detects_assistant_text_with_5h_limit_phrase`

Expected: FAIL with "no field `limit_event` on type `UsageDelta`".

- [ ] **Step 3: Implement detection in usage_reader**

Modify `src-tauri/src/usage_reader.rs`:

```rust
use crate::error::AppResult;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct UsageDelta {
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub tokens_cache_read: i64,
    pub tokens_cache_write: i64,
    pub new_offset: u64,
    pub limit_event: Option<LimitEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LimitEvent {
    /// Epoch seconds when claude said it'll be back. `0` is a sentinel
    /// meaning "we detected the limit but couldn't parse the time" — the
    /// caller should fall back to `last_activity_at + 5h + 60s`.
    pub reset_at: i64,
    /// Wall-clock at the moment we read the limit line.
    pub detected_at: i64,
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    typ: Option<String>,
    message: Option<Message>,
    /// Used by `type=system` lines that carry error text in a top-level field.
    content: Option<serde_json::Value>,
    subtype: Option<String>,
}

#[derive(Deserialize)]
struct Message {
    usage: Option<Usage>,
    content: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
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

pub fn read_delta(path: &Path, start_offset: u64) -> AppResult<UsageDelta> {
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
        delta.new_offset += line.len() as u64 + 1;
        if line.is_empty() {
            continue;
        }
        let parsed: Line = match serde_json::from_str(&line) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let typ = parsed.typ.as_deref().unwrap_or("");

        // Tally tokens for assistant lines.
        if typ == "assistant" {
            if let Some(Message { usage: Some(ref u), .. }) = parsed.message {
                delta.tokens_in += u.input_tokens;
                delta.tokens_out += u.output_tokens;
                delta.tokens_cache_read += u.cache_read_input_tokens;
                delta.tokens_cache_write += u.cache_creation_input_tokens;
            }
        }

        // Detect rate-limit on assistant text OR system error content.
        if delta.limit_event.is_none() {
            let text_blob = extract_text_for_limit_check(typ, &parsed);
            if let Some(text) = text_blob {
                if is_limit_text(&text) {
                    let reset_at = parse_reset_time_from(&text).unwrap_or(0);
                    delta.limit_event = Some(LimitEvent {
                        reset_at,
                        detected_at: chrono::Utc::now().timestamp(),
                    });
                }
            }
        }
    }

    if delta.new_offset > total_len {
        delta.new_offset = total_len;
    }
    Ok(delta)
}

/// Pulls a string from either an assistant message's content blocks or a
/// system line's top-level content. Returns None for lines we don't care
/// about (user, summary, etc.).
fn extract_text_for_limit_check(typ: &str, parsed: &Line) -> Option<String> {
    if typ == "assistant" {
        let content = parsed.message.as_ref().and_then(|m| m.content.as_ref())?;
        return Some(stringify_content(content));
    }
    if typ == "system" || typ == "error" {
        // Be lenient: the field could be a string or a structured blob.
        let content = parsed.content.as_ref()?;
        let subtype = parsed.subtype.as_deref().unwrap_or("");
        let mut blob = stringify_content(content);
        if !subtype.is_empty() {
            blob.push(' ');
            blob.push_str(subtype);
        }
        return Some(blob);
    }
    None
}

fn stringify_content(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut out = String::new();
            for item in arr {
                if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
                    out.push_str(t);
                    out.push(' ');
                }
            }
            out
        }
        _ => v.to_string(),
    }
}

fn is_limit_text(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.contains("5-hour limit")
        || lower.contains("five-hour limit")
        || lower.contains("5h limit")
        || lower.contains("usage limit")
        || lower.contains("usage cap")
        || lower.contains("rate_limit_error")
        || (lower.contains("limit reached") && lower.contains("claude"))
        || (lower.contains("limit") && lower.contains("resets at"))
}

/// Parse an HH:MM (24h) reset time from the message and convert to an epoch
/// timestamp on **today** (UTC). If the resulting time is already in the
/// past relative to wall-clock, roll it forward one day. Returns None if
/// no HH:MM is found.
fn parse_reset_time_from(s: &str) -> Option<i64> {
    use chrono::{Datelike, NaiveTime, TimeZone, Utc};
    // Match a 4–5 char clock like "14:30" or "9:00". Take the first hit.
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 4 < bytes.len() {
        if bytes[i].is_ascii_digit()
            && bytes.get(i + 1).map_or(false, |b| b.is_ascii_digit() || *b == b':')
        {
            // Look ahead for a colon within the next 1–2 chars and two digits after.
            let colon = if bytes[i + 1] == b':' { Some(i + 1) }
                else if bytes.get(i + 2) == Some(&b':') { Some(i + 2) }
                else { None };
            if let Some(c) = colon {
                if c + 2 < bytes.len()
                    && bytes[c + 1].is_ascii_digit()
                    && bytes[c + 2].is_ascii_digit()
                {
                    let h: u32 = std::str::from_utf8(&bytes[i..c]).ok()?.parse().ok()?;
                    let m: u32 = std::str::from_utf8(&bytes[c + 1..c + 3]).ok()?.parse().ok()?;
                    if h < 24 && m < 60 {
                        let now = Utc::now();
                        let today = now.date_naive();
                        let t = NaiveTime::from_hms_opt(h, m, 0)?;
                        let dt = chrono::NaiveDateTime::new(today, t);
                        let mut ts = Utc.from_utc_datetime(&dt).timestamp();
                        if ts < now.timestamp() {
                            ts += 24 * 3600;
                        }
                        return Some(ts);
                    }
                }
            }
        }
        i += 1;
    }
    None
}
```

Add to `Cargo.toml` only if not already present — `chrono` is already a dep.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p fastclaude_lib usage_reader`

Expected: all usage_reader tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/usage_reader.rs
git commit -m "feat(usage_reader): detect 5h-limit signal and parse reset time"
```

---

## Task 6: Poller — capture limit events and arm pending resumes

**Files:**
- Modify: `src-tauri/src/poller.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src-tauri/src/poller.rs`. We use a small `FakeJsonl` helper file and exercise the path via `tick`:

```rust
use crate::usage_reader::LimitEvent;

#[test]
fn arms_pending_resume_when_limit_event_seen_on_optin_row() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Build a registry with one opted-in active row.
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
    }).unwrap();

    // Point its jsonl_path at a temp file containing a rate-limit line.
    let mut jsonl = NamedTempFile::new().unwrap();
    writeln!(
        jsonl,
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"5-hour limit reached, resets at 23:30"}}],"usage":{{"input_tokens":1,"output_tokens":1}}}}}}"#
    ).unwrap();
    jsonl.flush().unwrap();
    r.set_jsonl_path(&s.id, &jsonl.path().to_string_lossy()).unwrap();

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
    let r = Registry::open_in_memory().unwrap();
    let cfg = Config::default();
    let s = r.insert(crate::session_registry::NewSession {
        project_dir: "/p".into(),
        model: "claude-opus-4-7".into(),
        claude_pid: 1,
        terminal_pid: 2,
        terminal_window_handle: None,
        auto_continue: false, // <-- not opted in
        resume_prompt: None,
        resume_cap: 3,
        resume_count: 0,
    }).unwrap();
    let mut jsonl = NamedTempFile::new().unwrap();
    writeln!(
        jsonl,
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"5-hour limit reached, resets at 14:30"}}],"usage":{{"input_tokens":1,"output_tokens":1}}}}}}"#
    ).unwrap();
    jsonl.flush().unwrap();
    r.set_jsonl_path(&s.id, &jsonl.path().to_string_lossy()).unwrap();

    let mut probe = FakeProbe([1u32].into_iter().collect());
    let _ = tick(&r, &mut probe, &cfg, 1000).unwrap();
    let got = r.get(&s.id).unwrap();
    assert!(got.next_resume_at.is_none());
}

#[test]
fn falls_back_to_last_activity_plus_5h_when_reset_unparseable() {
    use std::io::Write;
    use tempfile::NamedTempFile;
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
    }).unwrap();
    let mut jsonl = NamedTempFile::new().unwrap();
    writeln!(
        jsonl,
        r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"5-hour limit reached. Try again later."}}],"usage":{{"input_tokens":1,"output_tokens":1}}}}}}"#
    ).unwrap();
    jsonl.flush().unwrap();
    r.set_jsonl_path(&s.id, &jsonl.path().to_string_lossy()).unwrap();

    let mut probe = FakeProbe([1u32].into_iter().collect());
    let _ = tick(&r, &mut probe, &cfg, 1000).unwrap();
    let got = r.get(&s.id).unwrap();
    let expected = got.last_activity_at + 5 * 3600 + 60;
    assert_eq!(got.next_resume_at, Some(expected));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fastclaude_lib poller::tests::arms_pending_resume`

Expected: FAIL — `tick` does not yet handle `limit_event`.

- [ ] **Step 3: Wire limit-event handling into the tick function**

In `src-tauri/src/poller.rs`, modify the `tick` function — after the `apply_usage_delta` block, add:

```rust
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
            if s.status != Status::Running {
                registry.set_status(&s.id, Status::Running)?;
            }
            report.usage_changed = true;

            // NEW — arm pending resume if claude reported a rate-limit.
            if let Some(ev) = delta.limit_event {
                let reset_at = if ev.reset_at > 0 {
                    ev.reset_at
                } else {
                    // Fallback: last_activity_at + 5h + 60s.
                    mtime + 5 * 3600 + 60
                };
                // set_pending_resume is a no-op when auto_continue = 0.
                let _ = registry.set_pending_resume(&s.id, reset_at)?;
            }
        } else if now - s.last_activity_at > cfg.idle_threshold_seconds as i64
            && s.status != Status::Idle
        {
            registry.set_status(&s.id, Status::Idle)?;
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p fastclaude_lib poller`

Expected: all poller tests pass, including the three new ones.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/poller.rs
git commit -m "feat(poller): arm pending resume on rate-limit signal for opt-in sessions"
```

---

## Task 7: Poller — fire due resumes via spawner

**Files:**
- Modify: `src-tauri/src/poller.rs`
- Modify: `src-tauri/src/main.rs` (pass spawner into run_loop)

- [ ] **Step 1: Write the failing test**

Add a fake spawner and a test in `src-tauri/src/poller.rs` tests module:

```rust
use crate::spawner::{SpawnRequest, SpawnResult, Spawner};
use crate::error::AppResult as ResumeResult;
use std::sync::Mutex;

struct FakeSpawner {
    calls: Mutex<Vec<SpawnRequest>>,
    result: SpawnResult,
}
impl FakeSpawner {
    fn new(result: SpawnResult) -> Self {
        Self { calls: Mutex::new(Vec::new()), result }
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

#[test]
fn fires_due_resume_and_creates_successor_row() {
    let r = Arc::new(Registry::open_in_memory().unwrap());
    let cfg = Config::default();
    // Insert a session and arm it with a jsonl_path so a session_uuid exists.
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

    // Spawner saw a SpawnRequest with --resume <uuid> and the per-session prompt.
    let calls = spawner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].resume.as_deref(), Some("abc-1234"));
    assert_eq!(calls[0].prompt.as_deref(), Some("keep going"));
    assert_eq!(calls[0].model, "claude-opus-4-7");

    // Predecessor cleared.
    let pred = r.get(&s.id).unwrap();
    assert_eq!(pred.next_resume_at, None);
    assert!(pred.resumed_into.is_some());

    // Successor row inserted with resume_count + 1, same cap.
    let new_id = pred.resumed_into.unwrap();
    let succ = r.get(&new_id).unwrap();
    assert_eq!(succ.resume_count, 1);
    assert_eq!(succ.resume_cap, 3);
    assert!(succ.auto_continue);
}

#[test]
fn falls_back_to_global_resume_prompt_when_per_session_unset() {
    let r = Arc::new(Registry::open_in_memory().unwrap());
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
    let r = Arc::new(Registry::open_in_memory().unwrap());
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
    }).unwrap();
    r.set_jsonl_path(&s.id, "/tmp/abc.jsonl").unwrap();
    let _ = r.set_pending_resume(&s.id, 500); // no-op
    let spawner = FakeSpawner::new(SpawnResult {
        claude_pid: 42, terminal_pid: 41, terminal_window_handle: None,
    });
    let report = fire_due_resumes(&r, &spawner, &cfg, 1000).unwrap();
    assert!(report.fired_ids.is_empty());
    assert!(spawner.calls().is_empty());
}

#[test]
fn spawn_failure_bumps_failure_count_and_backs_off() {
    let r = Arc::new(Registry::open_in_memory().unwrap());
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
    }).unwrap();
    r.set_jsonl_path(&s.id, "/tmp/abc.jsonl").unwrap();
    r.set_pending_resume(&s.id, 500).unwrap();
    struct FailingSpawner;
    impl Spawner for FailingSpawner {
        fn spawn(&self, _req: &SpawnRequest) -> ResumeResult<SpawnResult> {
            Err(crate::error::AppError::Spawn("nope".into()))
        }
    }
    let report = fire_due_resumes(&r, &FailingSpawner, &cfg, 1000).unwrap();
    assert!(report.fired_ids.is_empty());
    let got = r.get(&s.id).unwrap();
    assert_eq!(got.resume_failures, 1);
    assert_eq!(got.next_resume_at, Some(1000 + 5 * 60), "5-min back-off scheduled");
}

#[test]
fn three_spawn_failures_give_up() {
    let r = Arc::new(Registry::open_in_memory().unwrap());
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
    }).unwrap();
    r.set_jsonl_path(&s.id, "/tmp/abc.jsonl").unwrap();
    r.set_pending_resume(&s.id, 500).unwrap();
    struct FailingSpawner;
    impl Spawner for FailingSpawner {
        fn spawn(&self, _req: &SpawnRequest) -> ResumeResult<SpawnResult> {
            Err(crate::error::AppError::Spawn("nope".into()))
        }
    }
    let mut now = 1000i64;
    for _ in 0..3 {
        let _ = fire_due_resumes(&r, &FailingSpawner, &cfg, now).unwrap();
        // simulate time advancing past the back-off window
        let row = r.get(&s.id).unwrap();
        now = row.next_resume_at.unwrap_or(now) + 1;
    }
    let got = r.get(&s.id).unwrap();
    assert_eq!(got.resume_failures, 3);
    assert_eq!(got.next_resume_at, None, "after 3 strikes we give up");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fastclaude_lib poller::tests::fires_due_resume_and_creates_successor_row`

Expected: FAIL — `fire_due_resumes` not defined.

- [ ] **Step 3: Implement fire_due_resumes and wire it into the loop**

Add a new public function to `src-tauri/src/poller.rs`:

```rust
#[derive(Debug, Default)]
pub struct FireReport {
    pub fired_ids: Vec<String>,
    pub failed_ids: Vec<(String, String)>, // (session_id, error_msg)
    pub gave_up_ids: Vec<String>,
}

const RESUME_BACKOFF_SECS: i64 = 5 * 60;
const RESUME_FAILURE_GIVE_UP: i64 = 3;

pub fn fire_due_resumes(
    registry: &Registry,
    spawner: &dyn crate::spawner::Spawner,
    cfg: &Config,
    now: i64,
) -> AppResult<FireReport> {
    let mut report = FireReport::default();
    for s in registry.list_due_resumes(now)? {
        // Derive session uuid from the jsonl filename stem.
        let Some(jsonl) = s.jsonl_path.as_deref() else {
            // No jsonl path → we can't form a --resume id. Defer one tick;
            // the poller's jsonl-finder may set it next round.
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
                })?;
                registry.record_resume_success(&s.id, &new_row.id)?;
                report.fired_ids.push(s.id.clone());
            }
            Err(e) => {
                let msg = format!("{e}");
                let new_failures = s.resume_failures + 1;
                if new_failures >= RESUME_FAILURE_GIVE_UP {
                    registry.record_resume_failure(&s.id, now)?;
                    registry.give_up_resume(&s.id)?;
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
```

Modify `run_loop` to accept a spawner and call `fire_due_resumes` once per tick:

```rust
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
```

Note the `Spawner` trait already requires `Send + Sync` (line 76 in spawner/mod.rs), so `Arc<dyn Spawner>` is OK.

Update `src-tauri/src/main.rs` — `run_loop` is now called with a spawner. AppState already holds `spawner: Box<dyn Spawner>`, but we need a clone-able handle. Refactor to `Arc<dyn Spawner>`:

In `src-tauri/src/commands.rs` change the AppState struct:

```rust
pub struct AppState {
    pub registry: Arc<Registry>,
    pub spawner: Arc<dyn Spawner>,
    pub focus: Box<dyn WindowFocus>,
    pub config: Arc<Mutex<Config>>,
    pub config_path: PathBuf,
    pub is_first_run: AtomicBool,
}
```

In `src-tauri/src/main.rs`, change construction:

```rust
let spawner_arc: Arc<dyn fastclaude_lib::spawner::Spawner> =
    Arc::from(spawner::default_spawner());
let state = AppState {
    registry: registry.clone(),
    spawner: spawner_arc.clone(),
    focus: window_focus::default_focus(),
    config: cfg_arc.clone(),
    config_path: cfg_path.clone(),
    is_first_run: AtomicBool::new(was_created),
};
app.manage(state);
// ...later when starting the poller:
let spawner_for_poller = spawner_arc.clone();
tauri::async_runtime::spawn(async move {
    poller::run_loop(
        registry_for_poller,
        spawner_for_poller,
        cfg_for_poller,
        std::time::Duration::from_secs(2),
        move |tick_report, fire_report| {
            let mut emit_changed = !tick_report.ended_ids.is_empty()
                || tick_report.usage_changed
                || !fire_report.fired_ids.is_empty()
                || !fire_report.gave_up_ids.is_empty();
            for id in &fire_report.fired_ids {
                let _ = app_handle.emit("auto-continue-fired", id);
            }
            for (id, msg) in &fire_report.failed_ids {
                let _ = app_handle.emit(
                    "auto-continue-failed",
                    serde_json::json!({ "id": id, "error": msg }),
                );
                emit_changed = true;
            }
            for id in &fire_report.gave_up_ids {
                let _ = app_handle.emit("auto-continue-gave-up", id);
            }
            if emit_changed {
                let _ = app_handle.emit("session-changed", &tick_report.ended_ids);
            }
        },
    ).await;
});
```

Note: `Arc::from(Box<T>)` works for `dyn Trait` only when the trait object is sized to be coerced; if the compiler complains, switch `default_spawner()` to return `Arc<dyn Spawner>` directly. Apply that change in `src-tauri/src/spawner/mod.rs:80`:

```rust
pub fn default_spawner() -> Arc<dyn Spawner> {
    #[cfg(target_os = "windows")]
    { Arc::new(windows::WindowsSpawner::new()) }
    #[cfg(target_os = "macos")]
    { Arc::new(macos::MacSpawner) }
    #[cfg(target_os = "linux")]
    { Arc::new(linux::LinuxSpawner) }
}
```

Add `use std::sync::Arc;` at the top of `spawner/mod.rs`.

`commands::launch_session` already uses `state.spawner.spawn(&req)` — `Arc<dyn Spawner>` derefs to `&dyn Spawner` transparently, no change needed.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p fastclaude_lib poller`

Expected: all poller tests pass. Also run `cargo build` to confirm the AppState refactor compiles app-wide.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/poller.rs src-tauri/src/main.rs \
        src-tauri/src/commands.rs src-tauri/src/spawner/mod.rs
git commit -m "feat(poller): fire due resumes via spawner with cap and backoff"
```

---

## Task 8: IPC commands — set_auto_continue and set_resume_prompt

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/main.rs` (register handlers)
- Modify: `src-tauri/src/commands.rs` (extend `LaunchInput` + `launch_session` to accept opt-in flags)

- [ ] **Step 1: Write the failing test for the launch path**

Add to `src-tauri/src/commands.rs` a `#[cfg(test)]` module if none exists, otherwise extend it. For this plan we test behavior through `Registry` round-trips (the IPC layer itself is thin):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_registry::Registry;

    #[test]
    fn launch_input_carries_auto_continue_flags() {
        let json = r#"{
            "project_dir": "/p",
            "auto_continue": true,
            "resume_prompt": "keep at it"
        }"#;
        let parsed: LaunchInput = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.auto_continue, Some(true));
        assert_eq!(parsed.resume_prompt.as_deref(), Some("keep at it"));
    }

    #[test]
    fn launch_input_defaults_are_none_when_omitted() {
        let json = r#"{ "project_dir": "/p" }"#;
        let parsed: LaunchInput = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.auto_continue, None);
        assert_eq!(parsed.resume_prompt, None);
    }

    #[test]
    fn registry_arm_disarm_through_methods_used_by_commands() {
        let r = Registry::open_in_memory().unwrap();
        let s = r.insert(crate::session_registry::NewSession {
            project_dir: "/p".into(),
            model: "m".into(),
            claude_pid: 1,
            terminal_pid: 2,
            terminal_window_handle: None,
            auto_continue: false,
            resume_prompt: None,
            resume_cap: 3,
            resume_count: 0,
        }).unwrap();
        r.set_auto_continue(&s.id, true).unwrap();
        assert!(r.get(&s.id).unwrap().auto_continue);
        r.set_resume_prompt(&s.id, Some("keep going")).unwrap();
        assert_eq!(r.get(&s.id).unwrap().resume_prompt.as_deref(), Some("keep going"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p fastclaude_lib commands`

Expected: FAIL with "no field `auto_continue` on type `LaunchInput`".

- [ ] **Step 3: Extend LaunchInput, launch_session, and add new commands**

In `src-tauri/src/commands.rs`, extend `LaunchInput` (around line 22):

```rust
#[derive(serde::Deserialize)]
pub struct LaunchInput {
    pub project_dir: String,
    pub model: Option<String>,
    pub prompt: Option<String>,
    #[serde(default)]
    pub resume: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub extra_args: Option<String>,
    /// NEW — pre-arm auto-continue at launch.
    #[serde(default)]
    pub auto_continue: Option<bool>,
    /// NEW — per-session override of the resume prompt. None at launch time
    /// means "fall back to config.default_resume_prompt at fire time".
    #[serde(default)]
    pub resume_prompt: Option<String>,
}
```

Modify `launch_session` (around line 53) to thread the new fields into the registry insert:

```rust
#[tauri::command]
pub fn launch_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: LaunchInput,
) -> AppResult<Session> {
    let cfg = state.config.lock().unwrap().clone();
    let model = input.model.unwrap_or(cfg.default_model.clone());
    let req = SpawnRequest {
        project_dir: input.project_dir.clone(),
        model: model.clone(),
        prompt: input.prompt,
        terminal_program: cfg.terminal_program.clone(),
        resume: input.resume,
        effort: input.effort.unwrap_or_else(|| cfg.default_effort.clone()),
        permission_mode: input
            .permission_mode
            .unwrap_or_else(|| cfg.default_permission_mode.clone()),
        extra_args: input
            .extra_args
            .unwrap_or_else(|| cfg.default_extra_args.clone()),
    };
    let result = state.spawner.spawn(&req)?;
    let session = state.registry.insert(NewSession {
        project_dir: input.project_dir,
        model,
        claude_pid: result.claude_pid,
        terminal_pid: result.terminal_pid,
        terminal_window_handle: result.terminal_window_handle,
        auto_continue: input.auto_continue.unwrap_or(cfg.default_auto_continue),
        resume_prompt: input.resume_prompt,
        resume_cap: cfg.default_resume_cap,
        resume_count: 0,
    })?;
    let _ = app.emit("session-changed", &session);
    Ok(session)
}
```

Add two new commands at the bottom of `src-tauri/src/commands.rs` (above the updater commands):

```rust
#[tauri::command]
pub fn set_auto_continue(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    on: bool,
) -> AppResult<()> {
    state.registry.set_auto_continue(&id, on)?;
    let _ = app.emit("session-changed", &id);
    Ok(())
}

#[tauri::command]
pub fn set_resume_prompt(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    prompt: Option<String>,
) -> AppResult<()> {
    state.registry.set_resume_prompt(&id, prompt.as_deref())?;
    let _ = app.emit("session-changed", &id);
    Ok(())
}
```

Register them in `src-tauri/src/main.rs` `invoke_handler!`:

```rust
.invoke_handler(tauri::generate_handler![
    commands::list_sessions,
    commands::list_all_sessions,
    commands::launch_session,
    commands::kill_session,
    commands::delete_session,
    commands::delete_sessions,
    commands::clear_ended_sessions,
    commands::focus_session,
    commands::recent_projects,
    commands::get_config,
    commands::set_config,
    commands::preview_launch_command,
    commands::get_first_run,
    commands::clear_first_run,
    commands::check_for_update,
    commands::install_update,
    commands::set_auto_continue,   // NEW
    commands::set_resume_prompt,   // NEW
])
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p fastclaude_lib && cargo build`

Expected: all tests pass and the binary builds.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/main.rs
git commit -m "feat(ipc): set_auto_continue and set_resume_prompt commands"
```

---

## Task 9: TypeScript types and IPC wrappers

**Files:**
- Modify: `src/types.ts`
- Modify: `src/lib/ipc.ts`

- [ ] **Step 1: Extend types**

Replace the content of `src/types.ts` with:

```typescript
export type SessionStatus = "running" | "idle" | "ended";

export interface Session {
  id: string;
  project_dir: string;
  model: string;
  claude_pid: number;
  terminal_pid: number;
  terminal_window_handle: string | null;
  started_at: number;
  ended_at: number | null;
  jsonl_path: string | null;
  jsonl_offset: number;
  status: SessionStatus;
  last_activity_at: number;
  tokens_in: number;
  tokens_out: number;
  tokens_cache_read: number;
  tokens_cache_write: number;
  auto_continue: boolean;
  resume_prompt: string | null;
  next_resume_at: number | null;
  resume_count: number;
  resume_cap: number;
  resumed_into: string | null;
  resume_failures: number;
}

export interface RecentProject {
  decoded_path: string;
  encoded_name: string;
  mtime: number;
  last_launched_at: number | null;
}

export type LaunchMode = "window" | "minimized" | "hidden";

export interface AppConfig {
  terminal_program: string;
  default_model: string;
  hotkey: string;
  idle_threshold_seconds: number;
  default_effort: string;
  default_permission_mode: string;
  default_extra_args: string;
  default_prompt: string;
  launch_on_login: boolean;
  launch_mode: LaunchMode;
  default_auto_continue: boolean;
  default_resume_prompt: string;
  default_resume_cap: number;
}

export interface LaunchInput {
  project_dir: string;
  model?: string;
  prompt?: string;
  resume?: string;
  effort?: string;
  permission_mode?: string;
  extra_args?: string;
  auto_continue?: boolean;
  resume_prompt?: string;
}

export interface UpdateInfo {
  version: string;
  notes: string | null;
}
```

- [ ] **Step 2: Add IPC wrappers**

Append to `src/lib/ipc.ts`:

```typescript
export async function setAutoContinue(id: string, on: boolean): Promise<void> {
  return invoke<void>("set_auto_continue", { id, on });
}

export async function setResumePrompt(id: string, prompt: string | null): Promise<void> {
  return invoke<void>("set_resume_prompt", { id, prompt });
}

export async function onAutoContinueFired(handler: (id: string) => void): Promise<UnlistenFn> {
  return listen<string>("auto-continue-fired", (e) => handler(e.payload));
}

export async function onAutoContinueFailed(
  handler: (payload: { id: string; error: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ id: string; error: string }>("auto-continue-failed", (e) => handler(e.payload));
}

export async function onAutoContinueGaveUp(
  handler: (id: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("auto-continue-gave-up", (e) => handler(e.payload));
}
```

- [ ] **Step 3: Verify TypeScript compiles**

Run: `npx tsc --noEmit`

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src/types.ts src/lib/ipc.ts
git commit -m "feat(ipc-ts): types and wrappers for auto-continue commands and events"
```

---

## Task 10: Settings UI — Auto-continue section

**Files:**
- Modify: `src/components/Settings.tsx`

- [ ] **Step 1: Add the section**

In `src/components/Settings.tsx`, insert a new `<Section>` immediately above the existing `<Section title="Theme">` block (around line 268):

```tsx
<Section title="Auto-continue">
  <div className="flex items-center justify-between">
    <div>
      <div className="text-sm">Default for new sessions</div>
      <div className="text-xs text-muted-foreground">
        Pre-arm the auto-continue checkbox when opening Launch.
      </div>
    </div>
    <input
      type="checkbox"
      className="h-4 w-4 accent-accent"
      checked={draft.default_auto_continue}
      onChange={(e) =>
        setDraft({ ...draft, default_auto_continue: e.target.checked })
      }
    />
  </div>
  <Field label="Default resume prompt">
    <Textarea
      value={draft.default_resume_prompt}
      onChange={(e) =>
        setDraft({ ...draft, default_resume_prompt: e.target.value })
      }
      placeholder="continue"
    />
  </Field>
  <Field label="Max auto-resumes per session (1–10)">
    <Input
      value={String(draft.default_resume_cap)}
      onChange={(e) => {
        const n = parseInt(e.target.value, 10);
        if (!Number.isNaN(n) && n >= 1 && n <= 10) {
          setDraft({ ...draft, default_resume_cap: n });
        }
      }}
    />
  </Field>
  <p className="text-xs text-muted-foreground">
    When a session hits its 5-hour limit, FastClaude will wait until the reset and
    relaunch with <code>claude --resume</code> plus this prompt. Capped to keep runaway loops in check.
  </p>
</Section>
```

- [ ] **Step 2: Verify the UI renders**

Run: `npm run tauri dev` and open Settings. Confirm the new section appears, checkbox toggles, textarea binds. Save → confirm config.json on disk contains the new fields.

(On Windows: `%APPDATA%\fastclaude\config.json` or wherever Tauri's `app_data_dir` resolves.)

Expected: section renders; Save persists.

- [ ] **Step 3: Commit**

```bash
git add src/components/Settings.tsx
git commit -m "feat(settings-ui): auto-continue section with cap and default prompt"
```

---

## Task 11: LaunchDialog — auto-continue checkbox

**Files:**
- Modify: `src/components/LaunchDialog.tsx`

- [ ] **Step 1: Add state + UI**

In `src/components/LaunchDialog.tsx`, add two new state variables alongside the existing ones (after `setExtraArgs` around line 52):

```tsx
const [autoContinue, setAutoContinue] = useState<boolean>(false);
const [resumePromptOverride, setResumePromptOverride] = useState<string>("");
const [showResumePrompt, setShowResumePrompt] = useState<boolean>(false);
```

In the existing `useEffect` that loads config (around line 83), set the default from config:

```tsx
useEffect(() => {
  if (!open) return;
  setErr(null);
  setRecentIndex(null);
  recentProjects(10).then(setRecents).catch(() => setRecents([]));
  getConfig()
    .then((c) => {
      setCfg(c);
      setModel(c.default_model);
      setEffort(c.default_effort);
      setPermissionMode(c.default_permission_mode);
      setExtraArgs(c.default_extra_args);
      setPrompt(c.default_prompt);
      setAutoContinue(c.default_auto_continue);    // NEW
      setResumePromptOverride("");                  // reset each open
      setShowResumePrompt(false);
    })
    .catch(() => {});
}, [open]);
```

Modify `submit` (around line 124) to pass the new fields to `launchSession`:

```tsx
await launchSession({
  project_dir: dir,
  model,
  prompt: prompt || undefined,
  effort,
  permission_mode: permissionMode,
  extra_args: extraArgs,
  auto_continue: autoContinue,
  resume_prompt: resumePromptOverride.trim() || undefined,
});
```

Add the new control inside the dialog body, immediately above the `{preview && ...}` block (around line 325). Use only existing UI primitives — no new components:

```tsx
<div className="rounded-md border border-border p-2 space-y-2">
  <label className="flex items-center gap-2 cursor-pointer">
    <input
      type="checkbox"
      className="h-4 w-4 accent-accent"
      checked={autoContinue}
      onChange={(e) => setAutoContinue(e.target.checked)}
    />
    <span className="text-sm">Auto-continue when the 5-hour limit resets</span>
  </label>
  {autoContinue && (
    <>
      <button
        type="button"
        onClick={() => setShowResumePrompt((v) => !v)}
        className="text-[11px] text-accent hover:underline"
      >
        {showResumePrompt ? "Use default resume prompt" : "Override resume prompt"}
      </button>
      {showResumePrompt && (
        <Textarea
          value={resumePromptOverride}
          onChange={(e) => setResumePromptOverride(e.target.value)}
          placeholder={cfg?.default_resume_prompt ?? "continue"}
          className="font-sans"
        />
      )}
    </>
  )}
</div>
```

- [ ] **Step 2: Verify the UI**

Run: `npm run tauri dev`. Open Launch:
- Checkbox respects `default_auto_continue` from Settings.
- Override link only shows when checkbox is on.
- Empty override → submit sends `resume_prompt: undefined` (uses global default at fire time).

Expected: behavior matches.

- [ ] **Step 3: Commit**

```bash
git add src/components/LaunchDialog.tsx
git commit -m "feat(launch-ui): auto-continue checkbox and per-session prompt override"
```

---

## Task 12: SessionRow — auto-continue pill

**Files:**
- Modify: `src/components/SessionRow.tsx`

- [ ] **Step 1: Add the pill**

In `src/components/SessionRow.tsx`, add an import:

```tsx
import { focusSession, killSession, setAutoContinue as setAutoContinueIpc } from "@/lib/ipc";
```

Add a small countdown helper near `elapsed`:

```tsx
function fmtCountdown(targetEpoch: number): string {
  const secs = Math.max(0, Math.floor(targetEpoch - Date.now() / 1000));
  if (secs <= 0) return "now";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${secs}s`;
}
```

Add the pill control before the Focus button in the JSX (around line 90, before the existing `<Button size="sm" variant="ghost" onClick={focus}>`):

```tsx
{(() => {
  const armed = session.auto_continue;
  const pending = session.next_resume_at !== null;
  const capReached = session.resume_count >= session.resume_cap;
  let label = "↻ off";
  let title = "Auto-continue is off. Click to arm.";
  let className = "border-border text-muted-foreground";
  if (armed && capReached) {
    label = "↻ cap";
    title = `Cap reached (${session.resume_count}/${session.resume_cap}). Toggle off and on to re-arm.`;
    className = "border-border text-muted-foreground opacity-60";
  } else if (armed && pending && session.next_resume_at !== null) {
    label = `↻ ${fmtCountdown(session.next_resume_at)}`;
    title = `Will resume at the reset (attempt ${session.resume_count + 1} of ${session.resume_cap}).`;
    className = "border-accent text-accent bg-accent/10";
  } else if (armed) {
    label = "↻ on";
    title = `Armed — will respawn when the 5h limit resets. (${session.resume_count}/${session.resume_cap} used)`;
    className = "border-accent text-accent bg-accent/10";
  }
  async function toggle() {
    try {
      await setAutoContinueIpc(session.id, !armed);
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as { message?: string })?.message ?? String(e);
      toast({ title: "Couldn't change auto-continue", description: msg, variant: "destructive" });
    }
    onChange();
  }
  return (
    <button
      title={title}
      onClick={toggle}
      className={`text-[10px] font-mono px-2 py-0.5 rounded-full border ${className} hover:brightness-110`}
    >
      {label}
    </button>
  );
})()}
```

- [ ] **Step 2: Add fire/failure toasts at the Dashboard level**

In `src/components/Dashboard.tsx`, extend the `useEffect` (around line 30) to listen for the new events and show toasts.

Add imports at the top:

```tsx
import { useToast } from "@/hooks/use-toast";
import {
  listSessions,
  onSessionChanged,
  getConfig,
  onAutoContinueFired,
  onAutoContinueFailed,
  onAutoContinueGaveUp,
} from "@/lib/ipc";
```

In the Dashboard function body:

```tsx
const { toast } = useToast();
```

Replace the existing `useEffect` for session-changed with one that also subscribes to the new events:

```tsx
useEffect(() => {
  refresh();
  const unlisteners: Array<() => void> = [];
  onSessionChanged(refresh).then((fn) => unlisteners.push(fn));
  onAutoContinueFired((id) => {
    refresh();
    listSessions().then((all) => {
      const s = all.find((x) => x.id === id);
      const name = s?.project_dir.split(/[\\/]/).filter(Boolean).pop() ?? "session";
      toast({ title: `Auto-continued ${name}` });
    });
  }).then((fn) => unlisteners.push(fn));
  onAutoContinueFailed(({ id, error }) => {
    refresh();
    toast({
      title: "Auto-continue failed",
      description: `${id.slice(0, 8)}…: ${error}`,
      variant: "destructive",
    });
  }).then((fn) => unlisteners.push(fn));
  onAutoContinueGaveUp((id) => {
    refresh();
    toast({
      title: "Auto-continue gave up",
      description: `${id.slice(0, 8)}… — 3 spawn failures in a row.`,
      variant: "destructive",
    });
  }).then((fn) => unlisteners.push(fn));
  const t = setInterval(refresh, 5000);
  return () => {
    for (const u of unlisteners) u();
    clearInterval(t);
  };
}, [refresh, toast]);
```

- [ ] **Step 3: Verify the UI**

Run: `npm run tauri dev`. Confirm:
- The pill renders on each session row, click toggles between `↻ off` and `↻ on`.
- Manually arm a session, then manually `UPDATE sessions SET next_resume_at = (strftime('%s', 'now') + 60)` in a sqlite shell on `%APPDATA%\fastclaude\state.db` (or wait for a real limit). The pill shows the countdown.

Expected: visual + behavioral match.

- [ ] **Step 4: Commit**

```bash
git add src/components/SessionRow.tsx src/components/Dashboard.tsx
git commit -m "feat(dashboard-ui): auto-continue pill and event toasts"
```

---

## Task 13: Manual smoke test + capture a real JSONL signal

**Files:**
- Create: `docs/superpowers/notes/auto-continue-jsonl-sample.md` (capture the real signal text for future maintainers)

This task does not require code changes unless the captured signal is shaped differently than the patterns in Task 5.

- [ ] **Step 1: Run FastClaude with a session and force a 5-hour limit hit**

Start a session, run a long task that will exhaust the window (or wait for an actual organic limit hit). When claude reports the limit, copy the relevant lines from the JSONL at `~/.claude/projects/<encoded>/<uuid>.jsonl`.

- [ ] **Step 2: Save the real example**

Write a short note documenting what you observed:

```markdown
# Real 5-hour limit JSONL example

Captured: <date>
Claude version: <version>

## Line(s) emitted on limit

```jsonl
<paste real line here>
```

## Reset time format observed

<HH:MM UTC | HH:MM with tz | relative "in N minutes" | other>

## Notes

<any patterns we should add to is_limit_text / parse_reset_time_from>
```

Save to `docs/superpowers/notes/auto-continue-jsonl-sample.md`.

- [ ] **Step 3: If the captured pattern differs from Task 5, extend `is_limit_text` / `parse_reset_time_from`**

If the real shape isn't matched by the patterns added in Task 5, append new clauses and add a fixture-based unit test in `src-tauri/src/usage_reader.rs` mirroring the real line.

- [ ] **Step 4: Verify auto-resume fires end-to-end**

1. Launch a fresh session with the new auto-continue checkbox enabled.
2. Run a small prompt to populate the JSONL.
3. Simulate the limit by manually appending a rate-limit line to the JSONL:

```bash
# PowerShell — adjust path
Add-Content -Path "$env:USERPROFILE\.claude\projects\<encoded>\<uuid>.jsonl" -Value '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"5-hour limit reached, resets at HH:MM"}],"usage":{"input_tokens":1,"output_tokens":1}}}'
```

Replace `HH:MM` with 1–2 minutes from now (UTC). Within ~4 seconds, the pill should show a countdown. After the reset hits, a new terminal opens running `claude --resume <uuid> "continue"`.

- [ ] **Step 5: Test restart recovery**

1. Arm a session, simulate the limit with a reset time 2 minutes from now.
2. Close FastClaude before the reset.
3. Wait until 2+ minutes after the reset.
4. Reopen FastClaude — within one poller tick, the auto-resume should fire.

- [ ] **Step 6: Commit the note**

```bash
git add docs/superpowers/notes/auto-continue-jsonl-sample.md
git commit -m "docs: real-world 5h-limit JSONL example for auto-continue feature"
```

---

## Self-Review

**Spec coverage check.** Mapping spec sections → tasks:

| Spec section | Task(s) |
|---|---|
| Goals: per-session opt-in | Task 1 (column), Task 8 (LaunchInput), Task 11 (checkbox), Task 12 (pill) |
| Goals: detect rate-limit from JSONL | Task 5 (usage_reader), Task 13 (real sample) |
| Goals: respawn via spawner with --resume | Task 7 (fire_due_resumes) |
| Goals: cap at N | Task 1 (resume_cap column), Task 3 (CRUD + cap check), Task 4 (config default), Task 7 (cap-blocks-fire test) |
| Goals: restart-safe | Covered by persisted columns + Task 7's loop running on startup (poller starts in main.rs setup) |
| Architecture: poller extension | Tasks 6, 7 |
| Architecture: registry extension | Tasks 1, 3 |
| Architecture: usage_reader extension | Task 5 |
| Architecture: spawner unchanged | Task 7 confirms `SpawnRequest.resume` already exists |
| Data model: 7 new columns | Task 1 |
| Data model: 3 new config fields | Task 4 |
| Data model: LimitEvent | Task 5 |
| Detection: 2 line shapes | Task 5's `is_limit_text` + `extract_text_for_limit_check` |
| Detection: fallback reset time | Task 5 test + Task 6 fallback test |
| Firing: derive uuid from jsonl filename | Task 7 `jsonl_session_id` |
| Firing: prompt resolution (per-session vs global) | Task 7 test |
| Firing: 5-min backoff + 3-strike give-up | Task 7 tests |
| Firing: per-chain cap | Task 7 successor inherits `resume_count + 1` |
| Startup recovery | Implicit — the poller's normal tick fires due rows. No separate startup code needed because `fire_due_resumes` runs on every tick and the first tick happens within 2s of boot. |
| UI: LaunchDialog | Task 11 |
| UI: SessionRow pill | Task 12 |
| UI: Settings section | Task 10 |
| UI: toasts | Task 12 |
| IPC: set_auto_continue / set_resume_prompt | Task 8 |
| IPC: events (fired/failed/gave-up) | Task 7 (emit) + Task 9 (subscribe) + Task 12 (handle) |
| Error handling: 9 scenarios | Tasks 3 (mark_ended clears pending), 5 (unparseable→fallback), 6 (fallback applied), 7 (spawn failure paths, cap blocks) |
| Testing: Rust unit | Tasks 1, 2, 3, 4, 5, 6, 7, 8 |
| Testing: frontend | Verified manually in Tasks 10–12; no automated frontend tests in this codebase, so we don't introduce them |
| Testing: manual smoke | Task 13 |

**Placeholder scan.** I searched for: TBD, TODO, "implement later", "appropriate error handling", "similar to Task N". None present.

**Type consistency check.** Cross-checked names:
- `auto_continue` (bool), `resume_prompt` (Option<String>), `next_resume_at` (Option<i64>), `resume_count` (i64), `resume_cap` (i64), `resumed_into` (Option<String>), `resume_failures` (i64) — same across `Session` struct, schema, NewSession, TS types.
- `set_auto_continue` / `set_resume_prompt` / `set_pending_resume` / `list_due_resumes` / `record_resume_success` / `record_resume_failure` / `give_up_resume` — names match across Tasks 3, 7, 8.
- `fire_due_resumes` returns `FireReport { fired_ids, failed_ids, gave_up_ids }`. Used consistently in Task 7's `run_loop` callback and in Task 12's event subscriptions.
- Tauri event names: `auto-continue-fired` (id payload), `auto-continue-failed` (`{id, error}`), `auto-continue-gave-up` (id payload). Same in main.rs emit (Task 7) and ipc.ts subscribers (Task 9) and Dashboard listeners (Task 12).
- `default_auto_continue` / `default_resume_prompt` / `default_resume_cap` — same in Config (Task 4), AppConfig TS (Task 9), Settings UI (Task 10), LaunchDialog default load (Task 11).

No inconsistencies.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-18-auto-continue-on-limit-reset.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
