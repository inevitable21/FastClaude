use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

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
    pub cost_usd: f64,
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
                tokens_cache_write INTEGER NOT NULL DEFAULT 0,
                cost_usd REAL NOT NULL DEFAULT 0
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
            cost_usd: 0.0,
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

    fn list_where(&self, where_clause: &str) -> AppResult<Vec<Session>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT id, project_dir, model, claude_pid, terminal_pid, terminal_window_handle,
                    started_at, ended_at, jsonl_path, jsonl_offset, status, last_activity_at,
                    tokens_in, tokens_out, tokens_cache_read, tokens_cache_write, cost_usd
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
                    tokens_in, tokens_out, tokens_cache_read, tokens_cache_write, cost_usd
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(row_to_session(row)?)
        } else {
            Err(AppError::NotFound(format!("session {id}")))
        }
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
        cost_usd: row.get(16)?,
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
    fn set_status_updates_status() {
        let r = make();
        let s = r.insert(new_sess("/p")).unwrap();
        r.set_status(&s.id, Status::Idle).unwrap();
        assert_eq!(r.get(&s.id).unwrap().status, Status::Idle);
    }
}
