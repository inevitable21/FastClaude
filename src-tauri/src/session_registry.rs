use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

/// Normalize a project directory string for equality comparisons across
/// what-the-user-typed (registry) vs. what-we-decoded-from-disk
/// (recent_projects). Forward slashes, lowercased drive letter, no trailing
/// slash. Windows is case-insensitive on path components but we lowercase
/// the whole thing — ASCII paths only matter here, so this is safe enough.
pub fn normalize_project_dir(s: &str) -> String {
    let mut s = s.replace('\\', "/").to_lowercase();
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Idle,
    Ended,
}

impl Status {
    fn as_str(&self) -> &'static str {
        match self {
            Status::Running => "running",
            Status::Idle => "idle",
            Status::Ended => "ended",
        }
    }
    fn parse(s: &str) -> AppResult<Self> {
        match s {
            "running" => Ok(Self::Running),
            "idle" => Ok(Self::Idle),
            "ended" => Ok(Self::Ended),
            other => Err(AppError::Invalid(format!("status {other}"))),
        }
    }
}

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
    pub subtask_id: Option<String>,
}

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
    /// If Some, the new row inherits this JSONL path (used when claude --resume
    /// reuses the predecessor's conversation file). None for fresh launches.
    pub jsonl_path: Option<String>,
    /// Starting byte offset for the inherited JSONL. Set so the poller doesn't
    /// re-tally the predecessor's tokens or re-detect its limit event.
    pub jsonl_offset: i64,
    pub subtask_id: Option<String>,
}

pub struct Registry {
    conn: Mutex<Connection>,
}

const SESSION_COLS: &str = "id, project_dir, model, claude_pid, terminal_pid, \
    terminal_window_handle, started_at, ended_at, jsonl_path, jsonl_offset, \
    status, last_activity_at, tokens_in, tokens_out, tokens_cache_read, \
    tokens_cache_write, auto_continue, resume_prompt, next_resume_at, \
    resume_count, resume_cap, resumed_into, resume_failures, subtask_id";

impl Registry {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::init_schema(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_schema(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn init_schema(conn: &Connection) -> AppResult<()> {
        // Step 1: create the table and the index on a column that has always existed.
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
                resume_failures INTEGER NOT NULL DEFAULT 0,
                subtask_id TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_active
              ON sessions(ended_at) WHERE ended_at IS NULL;
            "#,
        )?;
        // Step 2: ALTER migrations for existing DBs that pre-date the auto-continue
        // feature. Each statement is run individually so a duplicate-column error on
        // a fresh DB (where the column already exists from CREATE TABLE) is silently
        // ignored, leaving other migrations unaffected.
        let migrations = [
            "ALTER TABLE sessions ADD COLUMN auto_continue INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE sessions ADD COLUMN resume_prompt TEXT",
            "ALTER TABLE sessions ADD COLUMN next_resume_at INTEGER",
            "ALTER TABLE sessions ADD COLUMN resume_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE sessions ADD COLUMN resume_cap INTEGER NOT NULL DEFAULT 3",
            "ALTER TABLE sessions ADD COLUMN resumed_into TEXT",
            "ALTER TABLE sessions ADD COLUMN resume_failures INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE sessions ADD COLUMN subtask_id TEXT",
        ];
        for sql in migrations {
            let _ = conn.execute(sql, []);
        }
        // Step 3: create the index on next_resume_at only after the ALTER migrations
        // have guaranteed the column exists (whether this is a fresh or legacy DB).
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_sessions_pending_resume \
             ON sessions(next_resume_at) WHERE next_resume_at IS NOT NULL;",
        )?;
        Ok(())
    }

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
            jsonl_path: n.jsonl_path,
            jsonl_offset: n.jsonl_offset,
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
            subtask_id: n.subtask_id,
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO sessions
                (id, project_dir, model, claude_pid, terminal_pid, terminal_window_handle,
                 started_at, status, last_activity_at,
                 auto_continue, resume_prompt, resume_count, resume_cap,
                 jsonl_path, jsonl_offset, subtask_id)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            "#,
            params![
                s.id, s.project_dir, s.model, s.claude_pid, s.terminal_pid,
                s.terminal_window_handle, s.started_at, s.status.as_str(), s.last_activity_at,
                s.auto_continue as i64, s.resume_prompt, s.resume_count, s.resume_cap,
                s.jsonl_path, s.jsonl_offset, s.subtask_id,
            ],
        )?;
        Ok(s)
    }

    pub fn list_active(&self) -> AppResult<Vec<Session>> {
        self.list_where("ended_at IS NULL ORDER BY started_at DESC")
    }

    pub fn list_all(&self) -> AppResult<Vec<Session>> {
        self.list_where("1=1 ORDER BY started_at DESC")
    }

    /// Returns map of `normalized(project_dir) -> max(started_at)` across all
    /// sessions ever recorded. Used to rank the launch dialog's folder list by
    /// "last time the user actually launched a session here".
    pub fn last_launch_per_dir(&self) -> AppResult<HashMap<String, i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT project_dir, MAX(started_at) FROM sessions GROUP BY project_dir",
        )?;
        let rows = stmt.query_map([], |row| {
            let dir: String = row.get(0)?;
            let started: i64 = row.get(1)?;
            Ok((dir, started))
        })?;
        let mut out = HashMap::new();
        for r in rows {
            let (dir, started) = r?;
            out.insert(normalize_project_dir(&dir), started);
        }
        Ok(out)
    }

    fn list_where(&self, where_clause: &str) -> AppResult<Vec<Session>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {SESSION_COLS} FROM sessions WHERE {where_clause}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_session)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get(&self, id: &str) -> AppResult<Session> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {SESSION_COLS} FROM sessions WHERE id = ?1");
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(row_to_session(row)?)
        } else {
            Err(AppError::NotFound(format!("session {id}")))
        }
    }

    /// Removes a single session row by id. Refuses to delete a session that
    /// is still active (`ended_at IS NULL`) so we don't orphan the spawned
    /// process — the caller should kill it first via `kill_session`.
    pub fn delete(&self, id: &str) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM sessions WHERE id = ?1 AND ended_at IS NOT NULL",
            params![id],
        )?;
        if n == 0 {
            // Distinguish "doesn't exist" from "still running" so the UI can
            // surface a useful error if someone wires this up to an active row.
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM sessions WHERE id = ?1",
                    params![id],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            return Err(if exists {
                AppError::Invalid(format!("session {id} is still running"))
            } else {
                AppError::NotFound(format!("session {id}"))
            });
        }
        Ok(())
    }

    /// Bulk-deletes every ended session. Active sessions are preserved so the
    /// dashboard keeps showing what's currently running. Returns the number of
    /// rows removed.
    pub fn delete_all_ended(&self) -> AppResult<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM sessions WHERE ended_at IS NOT NULL", [])?;
        Ok(n)
    }

    /// Bulk-deletes the given ids, but only those that are already ended.
    /// Active sessions in the list are silently skipped — this matches `delete`
    /// rather than failing a 20-session group delete because one row happens
    /// to still be running. Returns the count actually removed.
    pub fn delete_many_ended(&self, ids: &[String]) -> AppResult<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self.conn.lock().unwrap();
        let placeholders = std::iter::repeat("?").take(ids.len()).collect::<Vec<_>>().join(",");
        let sql = format!(
            "DELETE FROM sessions WHERE ended_at IS NOT NULL AND id IN ({placeholders})"
        );
        let params = rusqlite::params_from_iter(ids.iter());
        let n = conn.execute(&sql, params)?;
        Ok(n)
    }

    pub fn mark_ended(&self, id: &str, ended_at: i64) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE sessions
                SET ended_at = ?1, status = 'ended'
              WHERE id = ?2",
            params![ended_at, id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("session {id}")));
        }
        Ok(())
    }

    pub fn set_status(&self, id: &str, status: Status) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE sessions SET status = ?1 WHERE id = ?2",
            params![status.as_str(), id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("session {id}")));
        }
        Ok(())
    }

    pub fn set_jsonl_path(&self, id: &str, path: &str) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE sessions SET jsonl_path = ?1 WHERE id = ?2",
            params![path, id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("session {id}")));
        }
        Ok(())
    }

    /// Arms or disarms auto-continue for a session.
    ///
    /// Disarming (`on = false`) also clears any pending `next_resume_at` in
    /// the same UPDATE — a disarmed session cannot fire even if a previous
    /// limit event scheduled one. Arming does not auto-schedule; a separate
    /// `set_pending_resume` call (driven by the poller detecting a rate
    /// limit) is needed for that.
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

    /// Persist `next_resume_at` only if the row is armed, active, and below cap.
    /// Returns true when it actually changed the row.
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

    /// Active opt-in rows whose pending resume time has passed and that are
    /// below their resume cap. Sorted oldest-due first.
    ///
    /// `now` is interpolated into the SQL via `format!` (consistent with the
    /// existing `list_where` helper). Safe — `i64` cannot carry SQL injection.
    pub fn list_due_resumes(&self, now: i64) -> AppResult<Vec<Session>> {
        self.list_where(&format!(
            "auto_continue = 1
               AND next_resume_at IS NOT NULL
               AND next_resume_at <= {now}
               AND resume_count < resume_cap
             ORDER BY next_resume_at ASC"
        ))
    }

    /// Records that an auto-resume successfully spawned a successor session.
    ///
    /// Clears `next_resume_at` (no more resumes pending on THIS row), sets
    /// `resumed_into = new_id` (forward pointer for chain visualization), and
    /// resets `resume_failures = 0`.
    ///
    /// Note: `resume_count` is intentionally NOT incremented on this row.
    /// The cap is enforced on the successor row, which is inserted with
    /// `resume_count = predecessor.resume_count + 1` (see Task 7's fire loop).
    /// The predecessor itself stays armed until its `claude_pid` dies and the
    /// poller marks it ended — at which point `mark_ended` clears its
    /// `next_resume_at`, preventing any further fire from this row.
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

    /// Permanently abandons the pending resume (e.g. max retries exceeded).
    ///
    /// Clears `next_resume_at` but leaves `auto_continue` and `resume_failures`
    /// untouched. Unlike `record_resume_failure`, this does NOT schedule a
    /// retry — the row will not fire again unless an external caller arms a
    /// new `next_resume_at`. Used by the fire loop after the 3-strike give-up.
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

    /// Records a terminal failure: increments `resume_failures` AND clears
    /// `next_resume_at` in a single UPDATE. Use when the cap of consecutive
    /// failures has been reached — the session should not retry without
    /// being manually re-armed.
    pub fn record_final_failure(&self, id: &str) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE sessions
                SET resume_failures = resume_failures + 1,
                    next_resume_at = NULL
              WHERE id = ?1",
            params![id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("session {id}")));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_usage_delta(
        &self,
        id: &str,
        new_offset: i64,
        add_tokens_in: i64,
        add_tokens_out: i64,
        add_tokens_cache_read: i64,
        add_tokens_cache_write: i64,
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
                last_activity_at = ?6
            WHERE id = ?7
            "#,
            params![
                new_offset,
                add_tokens_in,
                add_tokens_out,
                add_tokens_cache_read,
                add_tokens_cache_write,
                last_activity_at,
                id,
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("session {id}")));
        }
        Ok(())
    }

    /// Test-only helper: force `last_activity_at` to a specific value so that
    /// poller tests can guarantee `mtime > last_activity_at` without sleeping.
    #[cfg(test)]
    pub fn backdate_last_activity(&self, id: &str, ts: i64) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET last_activity_at = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
        Ok(())
    }
}

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
        subtask_id: row.get(23)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> Registry {
        Registry::open_in_memory().unwrap()
    }

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
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
        }
    }

    #[test]
    fn insert_then_get_round_trips() {
        let r = make();
        let inserted = r.insert(new_sess("/p/a")).unwrap();
        let fetched = r.get(&inserted.id).unwrap();
        assert_eq!(inserted, fetched);
        assert_eq!(fetched.project_dir, "/p/a");
        assert_eq!(fetched.status, Status::Running);
    }

    #[test]
    fn list_active_excludes_ended() {
        let r = make();
        let a = r.insert(new_sess("/p/a")).unwrap();
        let _b = r.insert(new_sess("/p/b")).unwrap();
        r.mark_ended(&a.id, 9999).unwrap();
        let active = r.list_active().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].project_dir, "/p/b");
    }

    #[test]
    fn mark_ended_unknown_id_returns_not_found() {
        let r = make();
        assert!(matches!(r.mark_ended("nope", 1), Err(AppError::NotFound(_))));
    }

    #[test]
    fn delete_removes_ended_session() {
        let r = make();
        let s = r.insert(new_sess("/p")).unwrap();
        r.mark_ended(&s.id, 9999).unwrap();
        r.delete(&s.id).unwrap();
        assert!(matches!(r.get(&s.id), Err(AppError::NotFound(_))));
    }

    #[test]
    fn delete_refuses_running_session() {
        let r = make();
        let s = r.insert(new_sess("/p")).unwrap();
        assert!(matches!(r.delete(&s.id), Err(AppError::Invalid(_))));
        // Row is still there.
        assert!(r.get(&s.id).is_ok());
    }

    #[test]
    fn delete_unknown_id_returns_not_found() {
        let r = make();
        assert!(matches!(r.delete("nope"), Err(AppError::NotFound(_))));
    }

    #[test]
    fn delete_many_ended_skips_active_rows() {
        let r = make();
        let a = r.insert(new_sess("/p/a")).unwrap();
        let b = r.insert(new_sess("/p/b")).unwrap();
        let c = r.insert(new_sess("/p/c")).unwrap();
        r.mark_ended(&a.id, 100).unwrap();
        r.mark_ended(&c.id, 300).unwrap();
        // b is still active; should be skipped, not error.
        let removed = r
            .delete_many_ended(&[a.id.clone(), b.id.clone(), c.id.clone()])
            .unwrap();
        assert_eq!(removed, 2);
        let all = r.list_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].project_dir, "/p/b");
    }

    #[test]
    fn delete_many_ended_empty_list_is_noop() {
        let r = make();
        let removed = r.delete_many_ended(&[]).unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn delete_all_ended_keeps_active_rows() {
        let r = make();
        let a = r.insert(new_sess("/p/a")).unwrap();
        let b = r.insert(new_sess("/p/b")).unwrap();
        let _c = r.insert(new_sess("/p/c")).unwrap(); // stays running
        r.mark_ended(&a.id, 100).unwrap();
        r.mark_ended(&b.id, 200).unwrap();
        let removed = r.delete_all_ended().unwrap();
        assert_eq!(removed, 2);
        let active = r.list_active().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].project_dir, "/p/c");
        // And the all-list reflects only the running one.
        let all = r.list_all().unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn set_status_updates_status() {
        let r = make();
        let s = r.insert(new_sess("/p")).unwrap();
        r.set_status(&s.id, Status::Idle).unwrap();
        assert_eq!(r.get(&s.id).unwrap().status, Status::Idle);
    }

    #[test]
    fn apply_usage_delta_accumulates() {
        let r = make();
        let s = r.insert(new_sess("/p")).unwrap();
        r.apply_usage_delta(&s.id, 100, 10, 20, 1, 2, 12345).unwrap();
        r.apply_usage_delta(&s.id, 200, 5, 5, 0, 0, 23456).unwrap();
        let got = r.get(&s.id).unwrap();
        assert_eq!(got.tokens_in, 15);
        assert_eq!(got.tokens_out, 25);
        assert_eq!(got.tokens_cache_read, 1);
        assert_eq!(got.tokens_cache_write, 2);
        assert_eq!(got.jsonl_offset, 200);
        assert_eq!(got.last_activity_at, 23456);
    }

    #[test]
    fn set_jsonl_path_persists() {
        let r = make();
        let s = r.insert(new_sess("/p")).unwrap();
        r.set_jsonl_path(&s.id, "/some/path.jsonl").unwrap();
        assert_eq!(r.get(&s.id).unwrap().jsonl_path.as_deref(), Some("/some/path.jsonl"));
    }

    #[test]
    fn insert_defaults_auto_continue_fields() {
        let r = make();
        let s = r.insert(new_sess("/p")).unwrap();
        assert!(!s.auto_continue);
        assert_eq!(s.resume_prompt, None);
        assert_eq!(s.next_resume_at, None);
        assert_eq!(s.resume_count, 0);
        assert_eq!(s.resume_cap, 3, "default cap is 3 when caller supplies 3");
        assert_eq!(s.resumed_into, None);
        assert_eq!(s.resume_failures, 0);
    }

    #[test]
    fn insert_falls_back_to_default_cap_when_caller_passes_zero() {
        let r = make();
        let bad = NewSession {
            project_dir: "/p".into(),
            model: "claude-opus-4-7".into(),
            claude_pid: 1,
            terminal_pid: 2,
            terminal_window_handle: None,
            auto_continue: false,
            resume_prompt: None,
            resume_cap: 0,
            resume_count: 0,
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
        };
        let s = r.insert(bad).unwrap();
        assert_eq!(s.resume_cap, 3, "0 must trip the fallback, not persist as 0");
    }

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
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
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
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
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
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
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
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
        }).unwrap();
        r.set_pending_resume(&s.id, 1000).unwrap();
        r.record_resume_failure(&s.id, 5000).unwrap();
        let got = r.get(&s.id).unwrap();
        assert_eq!(got.resume_failures, 1);
        assert_eq!(got.next_resume_at, Some(5000));
    }

    #[test]
    fn mark_ended_preserves_pending_resume() {
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
            jsonl_path: None,
            jsonl_offset: 0,
            subtask_id: None,
        }).unwrap();
        r.set_pending_resume(&s.id, 1000).unwrap();
        r.mark_ended(&s.id, 9999).unwrap();
        // mark_ended must preserve next_resume_at so the fire loop can act on
        // sessions whose claude died at the rate limit. User-initiated kills
        // clear it explicitly via set_auto_continue(false).
        assert_eq!(r.get(&s.id).unwrap().next_resume_at, Some(1000));
    }

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

    #[test]
    fn insert_and_get_round_trips_subtask_id() {
        let r = make();
        let mut new = new_sess("/p");
        new.subtask_id = Some("st-123".into());
        let s = r.insert(new).unwrap();
        assert_eq!(s.subtask_id.as_deref(), Some("st-123"));
        let fetched = r.get(&s.id).unwrap();
        assert_eq!(fetched.subtask_id.as_deref(), Some("st-123"));
    }

    #[test]
    fn open_legacy_db_adds_subtask_id_column() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("legacy.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        // Schema as of the auto-continue feature (no subtask_id column).
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
                tokens_cache_write INTEGER NOT NULL DEFAULT 0,
                auto_continue INTEGER NOT NULL DEFAULT 0,
                resume_prompt TEXT,
                next_resume_at INTEGER,
                resume_count INTEGER NOT NULL DEFAULT 0,
                resume_cap INTEGER NOT NULL DEFAULT 3,
                resumed_into TEXT,
                resume_failures INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO sessions
                (id, project_dir, model, claude_pid, terminal_pid,
                 started_at, status, last_activity_at)
            VALUES ('legacy', '/p', 'm', 1, 2, 1000, 'running', 1000);
            "#,
        ).unwrap();
        drop(conn);

        let r = Registry::open(&path).unwrap();
        let got = r.get("legacy").unwrap();
        assert_eq!(got.subtask_id, None);
    }
}
