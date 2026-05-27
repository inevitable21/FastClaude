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
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            return Err(AppError::Invalid("project path is empty".into()));
        }
        // Reject non-path inputs at the data layer so the launch path never
        // sees a `norm_path` it can't actually `cd` into. Without this guard,
        // a user typing "asdasd" into the LaunchDialog folder input would
        // upsert a permanent project row whose later TODO launches all fail
        // silently because `wt -d asdasd` can't set a real working directory.
        let p = Path::new(trimmed);
        if !p.is_absolute() {
            return Err(AppError::Invalid(format!(
                "project path must be absolute: {trimmed:?}"
            )));
        }
        if !p.is_dir() {
            return Err(AppError::Invalid(format!(
                "project folder does not exist: {trimmed}"
            )));
        }
        let norm = normalize_project_dir(trimmed);
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

    pub fn list_visible(&self) -> AppResult<Vec<Project>> {
        // Tie-break on rowid DESC so creation order within the same Unix
        // second is deterministic (newer inserts first).
        self.list_where("hidden = 0 ORDER BY pinned DESC, created_at DESC, rowid DESC")
    }

    pub fn list_hidden(&self) -> AppResult<Vec<Project>> {
        self.list_where("hidden = 1 ORDER BY created_at DESC, rowid DESC")
    }

    fn list_where(&self, where_clause: &str) -> AppResult<Vec<Project>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {COLS} FROM projects WHERE {where_clause}");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_project)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn set_display_name(&self, id: &str, name: &str) -> AppResult<()> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::Invalid("display name is empty".into()));
        }
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE projects SET display_name = ?1 WHERE id = ?2",
            params![trimmed, id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("project {id}")));
        }
        Ok(())
    }

    pub fn set_pinned(&self, id: &str, on: bool) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE projects SET pinned = ?1 WHERE id = ?2",
            params![on as i64, id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("project {id}")));
        }
        Ok(())
    }

    pub fn set_hidden(&self, id: &str, on: bool) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE projects SET hidden = ?1 WHERE id = ?2",
            params![on as i64, id],
        )?;
        if n == 0 {
            return Err(AppError::NotFound(format!("project {id}")));
        }
        Ok(())
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(AppError::NotFound(format!("project {id}")));
        }
        Ok(())
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
    use tempfile::TempDir;

    fn make() -> Projects {
        Projects::open_in_memory().unwrap()
    }

    /// Create a directory inside `root` and return its absolute path as a
    /// `String`. Validation in [`Projects::upsert_for_path`] requires real
    /// on-disk directories, so every test creates one rather than passing a
    /// fake string. `name` becomes the directory's basename, which is what
    /// `default_display_name` keys on for display.
    fn make_dir(root: &TempDir, name: &str) -> String {
        let p = root.path().join(name);
        std::fs::create_dir_all(&p).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn upsert_creates_then_returns_same_row() {
        let p = make();
        let tmp = TempDir::new().unwrap();
        let path = make_dir(&tmp, "myapp");
        let a = p.upsert_for_path(&path).unwrap();
        let b = p.upsert_for_path(&path).unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(a.display_name, "myapp");
    }

    #[test]
    fn upsert_normalizes_path_variants() {
        // Same on-disk folder, two textual representations: with native
        // separators and with forward slashes. Both must collapse to the
        // same `norm_path` and return the same project id.
        let p = make();
        let tmp = TempDir::new().unwrap();
        let dir = make_dir(&tmp, "myapp");
        let with_forward = dir.replace('\\', "/");
        let a = p.upsert_for_path(&dir).unwrap();
        let b = p.upsert_for_path(&with_forward).unwrap();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn upsert_rejects_empty_path() {
        let p = make();
        assert!(matches!(p.upsert_for_path(""), Err(AppError::Invalid(_))));
        assert!(matches!(p.upsert_for_path("   "), Err(AppError::Invalid(_))));
    }

    #[test]
    fn upsert_rejects_non_absolute_path() {
        // The whole point of the on-disk guard: stop "asdasd" /
        // "newmessanger" from ever becoming permanent project rows.
        let p = make();
        assert!(matches!(p.upsert_for_path("asdasd"), Err(AppError::Invalid(_))));
        assert!(matches!(
            p.upsert_for_path("relative/path"),
            Err(AppError::Invalid(_))
        ));
    }

    #[test]
    fn upsert_rejects_absolute_path_that_doesnt_exist() {
        let p = make();
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let res = p.upsert_for_path(&missing.to_string_lossy());
        assert!(matches!(res, Err(AppError::Invalid(_))));
    }

    #[test]
    fn get_returns_not_found_for_unknown_id() {
        let p = make();
        assert!(matches!(p.get("nope"), Err(AppError::NotFound(_))));
    }

    #[test]
    fn list_orders_pinned_first_then_created_desc() {
        let p = make();
        let tmp = TempDir::new().unwrap();
        let a = p.upsert_for_path(&make_dir(&tmp, "a")).unwrap();
        let b = p.upsert_for_path(&make_dir(&tmp, "b")).unwrap();
        let c = p.upsert_for_path(&make_dir(&tmp, "c")).unwrap();
        p.set_pinned(&b.id, true).unwrap();
        let listed = p.list_visible().unwrap();
        let ids: Vec<_> = listed.iter().map(|x| x.id.clone()).collect();
        // b first (pinned), then c, a in reverse-created order
        assert_eq!(ids, vec![b.id, c.id, a.id]);
    }

    #[test]
    fn list_visible_excludes_hidden() {
        let p = make();
        let tmp = TempDir::new().unwrap();
        let a = p.upsert_for_path(&make_dir(&tmp, "a")).unwrap();
        let b = p.upsert_for_path(&make_dir(&tmp, "b")).unwrap();
        p.set_hidden(&b.id, true).unwrap();
        let listed = p.list_visible().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, a.id);
    }

    #[test]
    fn list_hidden_returns_only_hidden() {
        let p = make();
        let tmp = TempDir::new().unwrap();
        let _a = p.upsert_for_path(&make_dir(&tmp, "a")).unwrap();
        let b = p.upsert_for_path(&make_dir(&tmp, "b")).unwrap();
        p.set_hidden(&b.id, true).unwrap();
        let listed = p.list_hidden().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, b.id);
    }

    #[test]
    fn set_display_name_persists_and_rejects_empty() {
        let p = make();
        let tmp = TempDir::new().unwrap();
        let a = p.upsert_for_path(&make_dir(&tmp, "a")).unwrap();
        p.set_display_name(&a.id, "Alpha").unwrap();
        assert_eq!(p.get(&a.id).unwrap().display_name, "Alpha");
        assert!(matches!(p.set_display_name(&a.id, "  "), Err(AppError::Invalid(_))));
    }

    #[test]
    fn delete_removes_row_and_returns_not_found_after() {
        let p = make();
        let tmp = TempDir::new().unwrap();
        let a = p.upsert_for_path(&make_dir(&tmp, "a")).unwrap();
        p.delete(&a.id).unwrap();
        assert!(matches!(p.get(&a.id), Err(AppError::NotFound(_))));
    }
}
