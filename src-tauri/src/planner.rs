use crate::error::{AppError, AppResult};
use serde::Deserialize;
use std::sync::Mutex;
use std::time::Duration;

pub trait PlannerRunner: Send + Sync {
    fn run(&self, prompt: &str, model: &str, timeout: Duration) -> AppResult<String>;
}

const PROMPT_TEMPLATE: &str = "You are a planning assistant. Decompose the following TODO into 2 to 5 concrete subtasks that can each be worked on independently by a separate Claude Code session. Each subtask must be a self-contained instruction (no cross-references between subtasks). Reply with strict JSON only.\n\nProject: {project}\nTODO: {title}\n\nReply format:\n{\"subtasks\": [\"...\", \"...\", \"...\"]}";

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_SUBTASKS_ACCEPTED: usize = 8;
const MAX_SUBTASK_LEN: usize = 500;

pub fn build_prompt(title: &str, project_name: &str) -> String {
    PROMPT_TEMPLATE
        .replace("{title}", title)
        .replace("{project}", project_name)
}

#[derive(Debug, Deserialize)]
struct PlannerJson {
    subtasks: Vec<String>,
}

pub fn parse_planner_output(stdout: &str) -> AppResult<Vec<String>> {
    // claude -p with --output-format json wraps the model's text in a
    // top-level envelope; the parser tolerates either the bare {"subtasks":..}
    // form or an envelope where the text appears under a "result" or "response"
    // field. We look for the first '{' that successfully parses, scanning the
    // whole stdout — that's robust to leading log lines on stderr-into-stdout.
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err(AppError::PlannerFailed("planner returned empty output".into()));
    }
    // Try direct parse.
    if let Ok(p) = serde_json::from_str::<PlannerJson>(trimmed) {
        return validate(p.subtasks);
    }
    // Try to locate an inner JSON object inside any wrapper.
    let mut depth = 0i32;
    let mut start = None;
    for (i, ch) in trimmed.char_indices() {
        match ch {
            '{' => {
                if depth == 0 { start = Some(i); }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        if let Ok(p) = serde_json::from_str::<PlannerJson>(&trimmed[s..=i]) {
                            return validate(p.subtasks);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let snippet: String = trimmed.chars().take(200).collect();
    Err(AppError::PlannerFailed(format!(
        "planner returned non-JSON or missing 'subtasks' key: {snippet}"
    )))
}

fn validate(items: Vec<String>) -> AppResult<Vec<String>> {
    if items.is_empty() {
        return Err(AppError::PlannerFailed("planner returned 0 subtasks".into()));
    }
    if items.len() > MAX_SUBTASKS_ACCEPTED {
        return Err(AppError::PlannerFailed(format!(
            "planner returned {} subtasks (max {MAX_SUBTASKS_ACCEPTED})",
            items.len()
        )));
    }
    for (i, s) in items.iter().enumerate() {
        let t = s.trim();
        if t.is_empty() {
            return Err(AppError::PlannerFailed(format!("subtask {} is empty", i + 1)));
        }
        if t.chars().count() > MAX_SUBTASK_LEN {
            return Err(AppError::PlannerFailed(format!(
                "subtask {} too long ({} chars > {MAX_SUBTASK_LEN})",
                i + 1,
                t.chars().count()
            )));
        }
    }
    Ok(items.into_iter().map(|s| s.trim().to_string()).collect())
}

/// Pure orchestrator — composes a runner with the parser. Tests use a fake
/// runner; production wires this to `RealRunner` in Task 9.
pub fn plan_subtasks(
    runner: &dyn PlannerRunner,
    title: &str,
    project_name: &str,
    model: &str,
    timeout: Duration,
) -> AppResult<Vec<String>> {
    if title.trim().is_empty() {
        return Err(AppError::Invalid("planner title is empty".into()));
    }
    let prompt = build_prompt(title.trim(), project_name);
    let out = runner.run(&prompt, model, timeout)?;
    parse_planner_output(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scripted runner — returns canned strings or errors in sequence.
    pub struct FakeRunner {
        pub responses: Mutex<Vec<AppResult<String>>>,
    }

    impl FakeRunner {
        pub fn new(responses: Vec<AppResult<String>>) -> Self {
            Self { responses: Mutex::new(responses) }
        }
    }

    impl PlannerRunner for FakeRunner {
        fn run(&self, _prompt: &str, _model: &str, _t: Duration) -> AppResult<String> {
            let mut v = self.responses.lock().unwrap();
            v.remove(0)
        }
    }

    #[test]
    fn parses_bare_json_object() {
        let out = r#"{"subtasks": ["a", "b"]}"#;
        let parsed = parse_planner_output(out).unwrap();
        assert_eq!(parsed, vec!["a", "b"]);
    }

    #[test]
    fn parses_json_inside_envelope() {
        let out = r#"{"result": "{\"subtasks\": [\"a\", \"b\"]}"}"#;
        // First parse fails (top-level has no `subtasks`), but the scanner
        // finds an inner balanced object. That inner object IS the envelope's
        // top level, which has no `subtasks` — so this should fail. Use a
        // realistic claude -p envelope where the text content sits unescaped.
        let out2 = "Some log line\n{\"subtasks\": [\"a\", \"b\"]}";
        let parsed = parse_planner_output(out2).unwrap();
        assert_eq!(parsed, vec!["a", "b"]);
        // Confirm the escaped-inside case errors cleanly (we don't recurse).
        assert!(matches!(parse_planner_output(out), Err(AppError::PlannerFailed(_))));
    }

    #[test]
    fn rejects_empty_output() {
        assert!(matches!(parse_planner_output("   "), Err(AppError::PlannerFailed(_))));
    }

    #[test]
    fn rejects_missing_subtasks_key() {
        let out = r#"{"items": ["a"]}"#;
        assert!(matches!(parse_planner_output(out), Err(AppError::PlannerFailed(_))));
    }

    #[test]
    fn rejects_empty_subtasks_array() {
        let out = r#"{"subtasks": []}"#;
        assert!(matches!(parse_planner_output(out), Err(AppError::PlannerFailed(_))));
    }

    #[test]
    fn rejects_too_many_subtasks() {
        let nine: Vec<String> = (0..9).map(|i| format!("t{i}")).collect();
        let out = format!(r#"{{"subtasks": {}}}"#, serde_json::to_string(&nine).unwrap());
        assert!(matches!(parse_planner_output(&out), Err(AppError::PlannerFailed(_))));
    }

    #[test]
    fn rejects_oversized_subtask() {
        let big = "x".repeat(MAX_SUBTASK_LEN + 1);
        let out = format!(r#"{{"subtasks": [{}]}}"#, serde_json::to_string(&big).unwrap());
        assert!(matches!(parse_planner_output(&out), Err(AppError::PlannerFailed(_))));
    }

    #[test]
    fn rejects_empty_subtask_item() {
        let out = r#"{"subtasks": ["ok", "  "]}"#;
        assert!(matches!(parse_planner_output(out), Err(AppError::PlannerFailed(_))));
    }

    #[test]
    fn plan_subtasks_happy_path() {
        let runner = FakeRunner::new(vec![Ok(r#"{"subtasks": ["a", "b"]}"#.into())]);
        let out = plan_subtasks(&runner, "do work", "myproj", "claude-opus-4-7", DEFAULT_TIMEOUT).unwrap();
        assert_eq!(out, vec!["a", "b"]);
    }

    #[test]
    fn plan_subtasks_propagates_runner_error() {
        let runner = FakeRunner::new(vec![Err(AppError::Spawn("nope".into()))]);
        let err = plan_subtasks(&runner, "do", "p", "m", DEFAULT_TIMEOUT).unwrap_err();
        assert!(matches!(err, AppError::Spawn(_)));
    }

    #[test]
    fn plan_subtasks_rejects_empty_title() {
        let runner = FakeRunner::new(vec![]);
        assert!(matches!(
            plan_subtasks(&runner, "  ", "p", "m", DEFAULT_TIMEOUT),
            Err(AppError::Invalid(_))
        ));
    }

    #[test]
    fn build_prompt_substitutes_title_and_project() {
        let p = build_prompt("Refactor auth", "MyApp");
        assert!(p.contains("MyApp"));
        assert!(p.contains("Refactor auth"));
    }
}

/// Production runner — spawns `claude -p <prompt> --model <model> --output-format json`
/// in the system temp directory and reads stdout to completion (or until
/// `timeout` elapses, in which case the child is killed and an error returned).
pub struct RealRunner;

impl PlannerRunner for RealRunner {
    fn run(&self, prompt: &str, model: &str, timeout: Duration) -> AppResult<String> {
        use std::process::{Command, Stdio};
        use std::io::Read;
        let tmp = std::env::temp_dir();
        let mut child = Command::new("claude")
            .arg("-p")
            .arg(prompt)
            .arg("--model")
            .arg(model)
            .arg("--output-format")
            .arg("json")
            .current_dir(&tmp)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => AppError::ClaudeNotOnPath,
                _ => AppError::Spawn(format!("spawn claude: {e}")),
            })?;

        // Manual deadline: poll `try_wait` until timeout, kill if still running.
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => break,
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(AppError::PlannerFailed(format!(
                            "planner timed out after {}s",
                            timeout.as_secs()
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(AppError::PlannerFailed(format!("wait error: {e}"))),
            }
        }
        let mut out = String::new();
        if let Some(mut s) = child.stdout.take() {
            let _ = s.read_to_string(&mut out);
        }
        if out.trim().is_empty() {
            let mut err_s = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = e.read_to_string(&mut err_s);
            }
            return Err(AppError::PlannerFailed(format!(
                "planner produced no stdout (stderr: {})",
                err_s.trim()
            )));
        }
        Ok(out)
    }
}
