use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoState {
    Pending,
    Ongoing,
    Finished,
}

impl TodoState {
    fn as_str(self) -> &'static str {
        match self {
            TodoState::Pending => "pending",
            TodoState::Ongoing => "ongoing",
            TodoState::Finished => "finished",
        }
    }
    fn parse(s: &str) -> AppResult<Self> {
        match s {
            "pending" => Ok(Self::Pending),
            "ongoing" => Ok(Self::Ongoing),
            "finished" => Ok(Self::Finished),
            other => Err(AppError::Invalid(format!("todo state {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerStatus {
    Idle,
    Planning,
    Planned,
    PlannerFailed,
}

impl PlannerStatus {
    fn as_str(self) -> &'static str {
        match self {
            PlannerStatus::Idle => "idle",
            PlannerStatus::Planning => "planning",
            PlannerStatus::Planned => "planned",
            PlannerStatus::PlannerFailed => "planner_failed",
        }
    }
    fn parse(s: &str) -> AppResult<Self> {
        match s {
            "idle" => Ok(Self::Idle),
            "planning" => Ok(Self::Planning),
            "planned" => Ok(Self::Planned),
            "planner_failed" => Ok(Self::PlannerFailed),
            other => Err(AppError::Invalid(format!("planner status {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Todo {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub state: TodoState,
    pub planner_status: PlannerStatus,
    pub planner_error: Option<String>,
    pub auto_suggest_done_at: Option<i64>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubtaskOrigin {
    Planner,
    Manual,
}

impl SubtaskOrigin {
    fn as_str(self) -> &'static str {
        match self {
            SubtaskOrigin::Planner => "planner",
            SubtaskOrigin::Manual => "manual",
        }
    }
    fn parse(s: &str) -> AppResult<Self> {
        match s {
            "planner" => Ok(Self::Planner),
            "manual" => Ok(Self::Manual),
            other => Err(AppError::Invalid(format!("subtask origin {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Subtask {
    pub id: String,
    pub todo_id: String,
    pub ord: i64,
    pub text: String,
    pub session_id: Option<String>,
    pub origin: SubtaskOrigin,
    pub created_at: i64,
}

pub struct Todos {
    conn: Mutex<Connection>,
}

const TODO_COLS: &str = "id, project_id, title, state, planner_status, planner_error, \
    auto_suggest_done_at, created_at, completed_at";
const SUBTASK_COLS: &str = "id, todo_id, ord, text, session_id, origin, created_at";

impl Todos {
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
            CREATE TABLE IF NOT EXISTS todos (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                title TEXT NOT NULL,
                state TEXT NOT NULL,
                planner_status TEXT NOT NULL,
                planner_error TEXT,
                auto_suggest_done_at INTEGER,
                created_at INTEGER NOT NULL,
                completed_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_todos_project ON todos(project_id);

            CREATE TABLE IF NOT EXISTS subtasks (
                id TEXT PRIMARY KEY,
                todo_id TEXT NOT NULL,
                ord INTEGER NOT NULL,
                text TEXT NOT NULL,
                session_id TEXT,
                origin TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(todo_id) REFERENCES todos(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_subtasks_todo ON subtasks(todo_id);
            "#,
        )?;
        // Enable FK enforcement so the CASCADE above actually fires.
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(())
    }

    pub fn create_todo(&self, project_id: &str, title: &str) -> AppResult<Todo> {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return Err(AppError::Invalid("todo title is empty".into()));
        }
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        let t = Todo {
            id: id.clone(),
            project_id: project_id.into(),
            title: trimmed.to_string(),
            state: TodoState::Pending,
            planner_status: PlannerStatus::Idle,
            planner_error: None,
            auto_suggest_done_at: None,
            created_at: now,
            completed_at: None,
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO todos (id, project_id, title, state, planner_status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                t.id, t.project_id, t.title,
                t.state.as_str(), t.planner_status.as_str(), t.created_at
            ],
        )?;
        Ok(t)
    }

    pub fn get_todo(&self, id: &str) -> AppResult<Todo> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {TODO_COLS} FROM todos WHERE id = ?1");
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            Ok(row_to_todo(r)?)
        } else {
            Err(AppError::NotFound(format!("todo {id}")))
        }
    }

    pub fn list_todos_for_project(&self, project_id: &str) -> AppResult<Vec<Todo>> {
        let conn = self.conn.lock().unwrap();
        // rowid DESC ties-breaks within the same Unix second so the most
        // recently created todo always appears first.
        let sql = format!(
            "SELECT {TODO_COLS} FROM todos WHERE project_id = ?1 \
             ORDER BY created_at DESC, rowid DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![project_id], row_to_todo)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn list_subtasks(&self, todo_id: &str) -> AppResult<Vec<Subtask>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {SUBTASK_COLS} FROM subtasks WHERE todo_id = ?1 ORDER BY ord ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![todo_id], row_to_subtask)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn replace_subtasks(
        &self,
        todo_id: &str,
        texts: &[String],
    ) -> AppResult<Vec<Subtask>> {
        let conn = self.conn.lock().unwrap();
        let launched: i64 = conn.query_row(
            "SELECT COUNT(*) FROM subtasks WHERE todo_id = ?1 AND session_id IS NOT NULL",
            params![todo_id],
            |r| r.get(0),
        )?;
        if launched > 0 {
            return Err(AppError::Invalid(
                "cannot replace subtasks after sessions launched".into(),
            ));
        }
        conn.execute("DELETE FROM subtasks WHERE todo_id = ?1", params![todo_id])?;
        let now = chrono::Utc::now().timestamp();
        let mut out = Vec::with_capacity(texts.len());
        for (i, text) in texts.iter().enumerate() {
            let s = Subtask {
                id: Uuid::new_v4().to_string(),
                todo_id: todo_id.into(),
                ord: i as i64,
                text: text.clone(),
                session_id: None,
                origin: SubtaskOrigin::Planner,
                created_at: now,
            };
            conn.execute(
                "INSERT INTO subtasks (id, todo_id, ord, text, session_id, origin, created_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6)",
                params![s.id, s.todo_id, s.ord, s.text, s.origin.as_str(), s.created_at],
            )?;
            out.push(s);
        }
        Ok(out)
    }

    pub fn add_manual_subtask(&self, todo_id: &str, text: &str) -> AppResult<Subtask> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(AppError::Invalid("subtask text is empty".into()));
        }
        let conn = self.conn.lock().unwrap();
        let next_ord: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(ord) + 1, 0) FROM subtasks WHERE todo_id = ?1",
                params![todo_id],
                |r| r.get(0),
            )?;
        let s = Subtask {
            id: Uuid::new_v4().to_string(),
            todo_id: todo_id.into(),
            ord: next_ord,
            text: trimmed.into(),
            session_id: None,
            origin: SubtaskOrigin::Manual,
            created_at: chrono::Utc::now().timestamp(),
        };
        conn.execute(
            "INSERT INTO subtasks (id, todo_id, ord, text, session_id, origin, created_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6)",
            params![s.id, s.todo_id, s.ord, s.text, s.origin.as_str(), s.created_at],
        )?;
        Ok(s)
    }

    pub fn edit_subtask(&self, id: &str, text: &str) -> AppResult<()> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(AppError::Invalid("subtask text is empty".into()));
        }
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE subtasks SET text = ?1 WHERE id = ?2",
            params![trimmed, id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("subtask {id}")));
        }
        Ok(())
    }

    pub fn delete_subtask(&self, id: &str) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM subtasks WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(AppError::NotFound(format!("subtask {id}")));
        }
        Ok(())
    }

    pub fn reorder_subtasks(&self, todo_id: &str, ordered_ids: &[String]) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let mut existing: Vec<String> = {
            let mut stmt = conn.prepare("SELECT id FROM subtasks WHERE todo_id = ?1")?;
            let rows = stmt.query_map(params![todo_id], |r| r.get::<_, String>(0))?;
            let mut v = Vec::new();
            for r in rows { v.push(r?); }
            v
        };
        existing.sort();
        let mut requested = ordered_ids.to_vec();
        requested.sort();
        if existing != requested {
            return Err(AppError::Invalid(
                "reorder ids do not match the subtask set for this todo".into(),
            ));
        }
        let tx = conn.unchecked_transaction()?;
        for (i, id) in ordered_ids.iter().enumerate() {
            tx.execute(
                "UPDATE subtasks SET ord = ?1 WHERE id = ?2",
                params![i as i64, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn attach_session(&self, subtask_id: &str, session_id: &str) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE subtasks SET session_id = ?1 WHERE id = ?2",
            params![session_id, subtask_id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("subtask {subtask_id}")));
        }
        Ok(())
    }

    pub fn delete_todo(&self, id: &str) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        // Manual cascade — SQLite's PRAGMA foreign_keys is per-connection and
        // PRAGMA was already enabled in init_schema, but spell it out anyway
        // for clarity and so the test passes on any connection state.
        conn.execute("DELETE FROM subtasks WHERE todo_id = ?1", params![id])?;
        let n = conn.execute("DELETE FROM todos WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(AppError::NotFound(format!("todo {id}")));
        }
        Ok(())
    }
}

fn row_to_todo(row: &rusqlite::Row<'_>) -> rusqlite::Result<Todo> {
    let state_s: String = row.get(3)?;
    let ps_s: String = row.get(4)?;
    Ok(Todo {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        state: TodoState::parse(&state_s).unwrap_or(TodoState::Pending),
        planner_status: PlannerStatus::parse(&ps_s).unwrap_or(PlannerStatus::Idle),
        planner_error: row.get(5)?,
        auto_suggest_done_at: row.get(6)?,
        created_at: row.get(7)?,
        completed_at: row.get(8)?,
    })
}

fn row_to_subtask(row: &rusqlite::Row<'_>) -> rusqlite::Result<Subtask> {
    let origin_s: String = row.get(5)?;
    Ok(Subtask {
        id: row.get(0)?,
        todo_id: row.get(1)?,
        ord: row.get(2)?,
        text: row.get(3)?,
        session_id: row.get(4)?,
        origin: SubtaskOrigin::parse(&origin_s).unwrap_or(SubtaskOrigin::Planner),
        created_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> Todos {
        Todos::open_in_memory().unwrap()
    }

    #[test]
    fn create_todo_persists_and_round_trips() {
        let t = make();
        let a = t.create_todo("proj-1", "Refactor auth").unwrap();
        let b = t.get_todo(&a.id).unwrap();
        assert_eq!(a, b);
        assert_eq!(b.title, "Refactor auth");
        assert_eq!(b.state, TodoState::Pending);
        assert_eq!(b.planner_status, PlannerStatus::Idle);
    }

    #[test]
    fn create_todo_rejects_empty_title() {
        let t = make();
        assert!(matches!(t.create_todo("p", "  "), Err(AppError::Invalid(_))));
    }

    #[test]
    fn list_todos_returns_only_project_rows_newest_first() {
        let t = make();
        let a = t.create_todo("p1", "first").unwrap();
        let _b = t.create_todo("p2", "other").unwrap();
        let c = t.create_todo("p1", "second").unwrap();
        let listed = t.list_todos_for_project("p1").unwrap();
        let ids: Vec<_> = listed.iter().map(|x| x.id.clone()).collect();
        assert_eq!(ids, vec![c.id, a.id]);
    }

    #[test]
    fn replace_subtasks_writes_planner_origin_with_sequential_ord() {
        let t = make();
        let todo = t.create_todo("p", "do work").unwrap();
        let written = t
            .replace_subtasks(&todo.id, &["one".into(), "two".into(), "three".into()])
            .unwrap();
        assert_eq!(written.len(), 3);
        for (i, s) in written.iter().enumerate() {
            assert_eq!(s.ord, i as i64);
            assert_eq!(s.origin, SubtaskOrigin::Planner);
            assert_eq!(s.todo_id, todo.id);
            assert!(s.session_id.is_none());
        }
    }

    #[test]
    fn replace_subtasks_overwrites_existing_when_none_launched() {
        let t = make();
        let todo = t.create_todo("p", "do work").unwrap();
        t.replace_subtasks(&todo.id, &["a".into(), "b".into()]).unwrap();
        t.replace_subtasks(&todo.id, &["x".into()]).unwrap();
        let subs = t.list_subtasks(&todo.id).unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].text, "x");
    }

    #[test]
    fn replace_subtasks_refuses_when_any_already_launched() {
        let t = make();
        let todo = t.create_todo("p", "do work").unwrap();
        let subs = t.replace_subtasks(&todo.id, &["a".into(), "b".into()]).unwrap();
        t.attach_session(&subs[0].id, "sess-1").unwrap();
        let err = t.replace_subtasks(&todo.id, &["x".into()]);
        assert!(matches!(err, Err(AppError::Invalid(_))));
    }

    #[test]
    fn add_manual_subtask_appends_after_existing_max_ord() {
        let t = make();
        let todo = t.create_todo("p", "do work").unwrap();
        t.replace_subtasks(&todo.id, &["a".into(), "b".into()]).unwrap();
        let manual = t.add_manual_subtask(&todo.id, "c").unwrap();
        assert_eq!(manual.ord, 2);
        assert_eq!(manual.origin, SubtaskOrigin::Manual);
    }

    #[test]
    fn edit_subtask_persists_text_change() {
        let t = make();
        let todo = t.create_todo("p", "do work").unwrap();
        let subs = t.replace_subtasks(&todo.id, &["a".into()]).unwrap();
        t.edit_subtask(&subs[0].id, "updated").unwrap();
        let again = t.list_subtasks(&todo.id).unwrap();
        assert_eq!(again[0].text, "updated");
    }

    #[test]
    fn delete_subtask_removes_row() {
        let t = make();
        let todo = t.create_todo("p", "do work").unwrap();
        let subs = t.replace_subtasks(&todo.id, &["a".into(), "b".into()]).unwrap();
        t.delete_subtask(&subs[0].id).unwrap();
        let remaining = t.list_subtasks(&todo.id).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].text, "b");
    }

    #[test]
    fn reorder_subtasks_renumbers_ord() {
        let t = make();
        let todo = t.create_todo("p", "do work").unwrap();
        let subs = t.replace_subtasks(&todo.id, &["a".into(), "b".into(), "c".into()]).unwrap();
        let new_order = vec![subs[2].id.clone(), subs[0].id.clone(), subs[1].id.clone()];
        t.reorder_subtasks(&todo.id, &new_order).unwrap();
        let listed = t.list_subtasks(&todo.id).unwrap();
        let texts: Vec<_> = listed.iter().map(|s| s.text.clone()).collect();
        assert_eq!(texts, vec!["c", "a", "b"]);
        for (i, s) in listed.iter().enumerate() {
            assert_eq!(s.ord, i as i64);
        }
    }

    #[test]
    fn reorder_subtasks_refuses_when_set_does_not_match() {
        let t = make();
        let todo = t.create_todo("p", "do work").unwrap();
        let subs = t.replace_subtasks(&todo.id, &["a".into(), "b".into()]).unwrap();
        let bad = vec![subs[0].id.clone()];
        assert!(matches!(
            t.reorder_subtasks(&todo.id, &bad),
            Err(AppError::Invalid(_))
        ));
    }

    #[test]
    fn delete_todo_cascade_removes_subtasks() {
        let t = make();
        let todo = t.create_todo("p", "do work").unwrap();
        t.replace_subtasks(&todo.id, &["a".into(), "b".into()]).unwrap();
        t.delete_todo(&todo.id).unwrap();
        assert!(matches!(t.get_todo(&todo.id), Err(AppError::NotFound(_))));
        let subs = t.list_subtasks(&todo.id).unwrap();
        assert!(subs.is_empty());
    }
}
