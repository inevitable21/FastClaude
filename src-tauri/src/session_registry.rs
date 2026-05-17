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
}

#[derive(Debug, Clone)]
pub struct NewSession {
    pub project_dir: String,
    pub model: String,
    pub claude_pid: i64,
    pub terminal_pid: i64,
    pub terminal_window_handle: Option<String>,
}

pub struct Registry {
    conn: Mutex<Connection>,
}

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
                tokens_cache_write INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_active
              ON sessions(ended_at) WHERE ended_at IS NULL;
            "#,
        )?;
        Ok(())
    }

    pub fn insert(&self, n: NewSession) -> AppResult<Session> {
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
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
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            INSERT INTO sessions
                (id, project_dir, model, claude_pid, terminal_pid, terminal_window_handle,
                 started_at, status, last_activity_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                s.id, s.project_dir, s.model, s.claude_pid, s.terminal_pid,
                s.terminal_window_handle, s.started_at, s.status.as_str(), s.last_activity_at,
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
            "SELECT id, project_dir, model, claude_pid, terminal_pid, terminal_window_handle,
                    started_at, ended_at, jsonl_path, jsonl_offset, status, last_activity_at,
                    tokens_in, tokens_out, tokens_cache_read, tokens_cache_write
             FROM sessions WHERE {where_clause}"
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
        let mut stmt = conn.prepare(
            "SELECT id, project_dir, model, claude_pid, terminal_pid, terminal_window_handle,
                    started_at, ended_at, jsonl_path, jsonl_offset, status, last_activity_at,
                    tokens_in, tokens_out, tokens_cache_read, tokens_cache_write
             FROM sessions WHERE id = ?1",
        )?;
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
            "UPDATE sessions SET ended_at = ?1, status = 'ended' WHERE id = ?2",
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
}
