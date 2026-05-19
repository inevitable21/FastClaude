use crate::error::{AppError, AppResult};
use crate::session_registry::normalize_project_dir;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub norm_path: String,
    pub display_name: String,
    pub pinned: bool,
    pub hidden: bool,
    pub created_at: i64,
}

pub struct Projects {
    conn: Mutex<Connection>,
}

const COLS: &str = "id, norm_path, display_name, pinned, hidden, created_at";

impl Projects {
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
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                norm_path TEXT NOT NULL UNIQUE,
                display_name TEXT NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0,
                hidden INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_projects_visible
              ON projects(hidden) WHERE hidden = 0;
            "#,
        )?;
        Ok(())
    }

    pub fn upsert_for_path(&self, raw_path: &str) -> AppResult<Project> {
        let norm = normalize_project_dir(raw_path);
        if norm.is_empty() {
            return Err(AppError::Invalid("project path is empty".into()));
        }
        let conn = self.conn.lock().unwrap();
        let existing: Option<Project> = conn
            .query_row(
                &format!("SELECT {COLS} FROM projects WHERE norm_path = ?1"),
                params![norm],
                row_to_project,
            )
            .ok();
        if let Some(p) = existing {
            return Ok(p);
        }
        let id = Uuid::new_v4().to_string();
        let display = default_display_name(&norm);
        let created_at = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO projects (id, norm_path, display_name, pinned, hidden, created_at)
             VALUES (?1, ?2, ?3, 0, 0, ?4)",
            params![id, norm, display, created_at],
        )?;
        Ok(Project {
            id,
            norm_path: norm,
            display_name: display,
            pinned: false,
            hidden: false,
            created_at,
        })
    }

    pub fn get(&self, id: &str) -> AppResult<Project> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {COLS} FROM projects WHERE id = ?1");
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            Ok(row_to_project(r)?)
        } else {
            Err(AppError::NotFound(format!("project {id}")))
        }
    }
}

fn default_display_name(norm_path: &str) -> String {
    norm_path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(norm_path)
        .to_string()
}

fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        norm_path: row.get(1)?,
        display_name: row.get(2)?,
        pinned: row.get::<_, i64>(3)? != 0,
        hidden: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> Projects {
        Projects::open_in_memory().unwrap()
    }

    #[test]
    fn upsert_creates_then_returns_same_row() {
        let p = make();
        let a = p.upsert_for_path("C:/Code/MyApp").unwrap();
        let b = p.upsert_for_path("C:/Code/MyApp").unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(a.display_name, "myapp");
    }

    #[test]
    fn upsert_normalizes_path_variants() {
        let p = make();
        let a = p.upsert_for_path("C:\\Code\\MyApp").unwrap();
        let b = p.upsert_for_path("c:/code/myapp/").unwrap();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn upsert_rejects_empty_path() {
        let p = make();
        assert!(matches!(p.upsert_for_path(""), Err(AppError::Invalid(_))));
    }

    #[test]
    fn get_returns_not_found_for_unknown_id() {
        let p = make();
        assert!(matches!(p.get("nope"), Err(AppError::NotFound(_))));
    }
}
