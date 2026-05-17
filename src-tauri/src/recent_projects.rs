use crate::error::AppResult;
use crate::session_registry::normalize_project_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentProject {
    pub decoded_path: String,
    pub encoded_name: String,
    pub mtime: i64,
    /// Most recent FastClaude launch into this folder (epoch seconds), or
    /// `None` if the user has never launched a session here. Folders are
    /// sorted by this descending so "what I last actually used" floats up.
    pub last_launched_at: Option<i64>,
}

/// Best-effort decode of Claude's path-encoded folder name back to a real path.
/// Claude replaces path separators with `-`. Since `-` is also a valid char in
/// real paths, this is lossy. For Windows paths starting like `C--GitProjects-FastClaude`
/// we recognize the leading drive letter pattern and convert it to `C:/GitProjects/FastClaude`.
fn decode_name(name: &str) -> String {
    if let Some(rest) = windows_drive_prefix(name) {
        return format!("{}:/{}", &name[..1], rest.replace('-', "/"));
    }
    if name.starts_with('-') {
        return format!("/{}", name.trim_start_matches('-').replace('-', "/"));
    }
    name.replace('-', "/")
}

fn windows_drive_prefix(name: &str) -> Option<&str> {
    let bytes = name.as_bytes();
    if bytes.len() >= 4
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b'-'
        && bytes[2] == b'-'
    {
        Some(&name[3..])
    } else {
        None
    }
}

pub fn list(
    claude_root: &Path,
    limit: usize,
    launches: &HashMap<String, i64>,
) -> AppResult<Vec<RecentProject>> {
    let projects_dir = claude_root.join("projects");
    if !projects_dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&projects_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let mtime = entry
            .metadata()?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let decoded_path = decode_name(&name);
        let last_launched_at = launches.get(&normalize_project_dir(&decoded_path)).copied();
        out.push(RecentProject {
            decoded_path,
            encoded_name: name,
            mtime,
            last_launched_at,
        });
    }
    // Primary: last FastClaude launch (Some > None). Tiebreak: dir mtime.
    out.sort_by(|a, b| {
        b.last_launched_at
            .cmp(&a.last_launched_at)
            .then_with(|| b.mtime.cmp(&a.mtime))
    });
    out.truncate(limit);
    Ok(out)
}

/// Returns `~/.claude` for the current user. Override with `FASTCLAUDE_CLAUDE_DIR` for tests.
pub fn default_claude_root() -> AppResult<PathBuf> {
    if let Ok(p) = std::env::var("FASTCLAUDE_CLAUDE_DIR") {
        return Ok(PathBuf::from(p));
    }
    dirs::home_dir()
        .map(|h| h.join(".claude"))
        .ok_or_else(|| crate::error::AppError::Other("no home dir".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fixture() -> TempDir {
        use filetime::{set_file_mtime, FileTime};
        let dir = TempDir::new().unwrap();
        let projects = dir.path().join("projects");
        fs::create_dir_all(&projects).unwrap();
        let entries = [
            ("C--GitProjects-FastClaude", 1_000_000_000),
            ("-home-tal-portfolio",       1_000_000_100),
            ("D--projects-api",           1_000_000_200),
            ("E--newest",                 1_000_000_300),
        ];
        for (name, mtime) in entries {
            let p = projects.join(name);
            fs::create_dir(&p).unwrap();
            set_file_mtime(&p, FileTime::from_unix_time(mtime, 0)).unwrap();
        }
        dir
    }

    #[test]
    fn falls_back_to_mtime_when_no_launches_recorded() {
        let dir = fixture();
        let recents = list(dir.path(), 10, &HashMap::new()).unwrap();
        assert_eq!(recents.len(), 4);
        assert_eq!(recents[0].decoded_path, "E:/newest");
    }

    #[test]
    fn launched_folder_floats_above_newer_mtime() {
        // FastClaude was last launched in the OLDEST-mtime folder; it should
        // still appear first because the user actually used it.
        let dir = fixture();
        let mut launches = HashMap::new();
        launches.insert(
            normalize_project_dir("C:/GitProjects/FastClaude"),
            2_000_000_000,
        );
        let recents = list(dir.path(), 10, &launches).unwrap();
        assert_eq!(recents[0].decoded_path, "C:/GitProjects/FastClaude");
        assert_eq!(recents[0].last_launched_at, Some(2_000_000_000));
        // Never-launched folders fall back to mtime ordering after.
        assert_eq!(recents[1].decoded_path, "E:/newest");
    }

    #[test]
    fn most_recent_launch_wins_among_launched_folders() {
        let dir = fixture();
        let mut launches = HashMap::new();
        launches.insert(normalize_project_dir("C:/GitProjects/FastClaude"), 100);
        launches.insert(normalize_project_dir("D:/projects/api"), 500);
        let recents = list(dir.path(), 10, &launches).unwrap();
        assert_eq!(recents[0].decoded_path, "D:/projects/api");
        assert_eq!(recents[1].decoded_path, "C:/GitProjects/FastClaude");
    }

    #[test]
    fn launch_lookup_is_case_and_slash_insensitive() {
        let dir = fixture();
        let mut launches = HashMap::new();
        // User typed it backslashed and lowercased — should still match the
        // forward-slashed decoded path.
        launches.insert(normalize_project_dir(r"c:\gitprojects\fastclaude"), 9_999);
        let recents = list(dir.path(), 10, &launches).unwrap();
        assert_eq!(recents[0].decoded_path, "C:/GitProjects/FastClaude");
        assert_eq!(recents[0].last_launched_at, Some(9_999));
    }

    #[test]
    fn limit_truncates() {
        let dir = fixture();
        let recents = list(dir.path(), 2, &HashMap::new()).unwrap();
        assert_eq!(recents.len(), 2);
    }

    #[test]
    fn missing_projects_dir_returns_empty() {
        let dir = TempDir::new().unwrap();
        let recents = list(dir.path(), 10, &HashMap::new()).unwrap();
        assert!(recents.is_empty());
    }

    #[test]
    fn decode_windows_drive() {
        assert_eq!(decode_name("C--GitProjects-FastClaude"), "C:/GitProjects/FastClaude");
    }

    #[test]
    fn decode_unix_path() {
        assert_eq!(decode_name("-home-tal-portfolio"), "/home/tal/portfolio");
    }
}
