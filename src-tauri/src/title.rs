use crate::error::AppResult;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

const TITLE_MIN_WORDS: usize = 3;
const TITLE_MAX_WORDS: usize = 7;
const FIRST_USER_SCAN_LIMIT: usize = 50;

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    typ: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    role: Option<String>,
    content: Option<serde_json::Value>,
}

/// Read up to [`FIRST_USER_SCAN_LIMIT`] lines from a session's JSONL and return
/// the text of the first `type=user` line we find. Returns `None` if no user
/// message is present, the file can't be read, or the content is empty.
///
/// The scan limit guards against pathological JSONLs that lead with tool output
/// before any human input — we don't want to load megabytes just to title a
/// session.
pub fn extract_first_user_message(path: &Path) -> AppResult<Option<String>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    for (idx, line) in reader.lines().enumerate() {
        if idx >= FIRST_USER_SCAN_LIMIT {
            break;
        }
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.is_empty() {
            continue;
        }
        let parsed: Line = match serde_json::from_str(&line) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if parsed.typ.as_deref() != Some("user") {
            continue;
        }
        let msg = match parsed.message {
            Some(m) => m,
            None => continue,
        };
        if msg.role.as_deref() != Some("user") {
            continue;
        }
        let Some(content) = msg.content else { continue };
        let text = stringify_content(&content);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        return Ok(Some(trimmed.to_string()));
    }
    Ok(None)
}

fn stringify_content(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut out = String::new();
            for item in arr {
                // Skip tool_result blocks — they're machine-generated noise that
                // sometimes appears in synthetic "user" lines from claude --resume.
                if item.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    continue;
                }
                if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(t);
                }
            }
            out
        }
        _ => String::new(),
    }
}

/// Derive a concise 3-7 word title from a free-form message. Strips
/// command-style prefixes (`/foo`, leading `!`), collapses whitespace,
/// title-cases the first letter, drops trailing punctuation, and truncates to
/// [`TITLE_MAX_WORDS`] words with an ellipsis. Returns `None` when the input
/// has fewer than [`TITLE_MIN_WORDS`] usable words — in that case the caller
/// should keep the default title rather than persist something pointless like
/// "Hi".
pub fn derive_title(raw: &str) -> Option<String> {
    let cleaned = strip_command_prefix(raw);
    let words: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() < TITLE_MIN_WORDS {
        return None;
    }
    let truncated = words.len() > TITLE_MAX_WORDS;
    let take = words.len().min(TITLE_MAX_WORDS);
    let mut title = words[..take].join(" ");
    // Drop trailing punctuation a user might end a sentence with, so we don't
    // produce "Fix the login bug." or "Fix the login bug?".
    while title
        .chars()
        .last()
        .map_or(false, |c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?'))
    {
        title.pop();
    }
    if truncated {
        title.push('…');
    }
    capitalize_first(&mut title);
    if title.trim().is_empty() {
        return None;
    }
    Some(title)
}

fn strip_command_prefix(s: &str) -> String {
    let trimmed = s.trim_start();
    // `/<command> rest` — drop the slash command, keep the rest. A bare `/foo`
    // with no follow-on becomes empty, which derive_title then rejects.
    if let Some(rest) = trimmed.strip_prefix('/') {
        if let Some(idx) = rest.find(char::is_whitespace) {
            return rest[idx..].trim().to_string();
        }
        return String::new();
    }
    if let Some(rest) = trimmed.strip_prefix('!') {
        return rest.trim().to_string();
    }
    trimmed.to_string()
}

fn capitalize_first(s: &mut String) {
    if let Some(first) = s.chars().next() {
        if first.is_lowercase() {
            let upper: String = first.to_uppercase().collect();
            let tail: String = s.chars().skip(1).collect();
            *s = format!("{upper}{tail}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_jsonl(lines: &[&str]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        f.flush().unwrap();
        f
    }

    #[test]
    fn derives_3_to_7_words_from_a_sentence() {
        let t = derive_title("please add a login screen with email and password").unwrap();
        assert_eq!(t, "Please add a login screen with email…");
    }

    #[test]
    fn keeps_short_input_without_ellipsis() {
        let t = derive_title("Fix the login bug").unwrap();
        assert_eq!(t, "Fix the login bug");
    }

    #[test]
    fn rejects_inputs_shorter_than_min_words() {
        assert!(derive_title("hi").is_none());
        assert!(derive_title("hello there").is_none());
    }

    #[test]
    fn strips_trailing_sentence_punctuation() {
        assert_eq!(derive_title("Fix the bug.").unwrap(), "Fix the bug");
        assert_eq!(derive_title("Why does this break?").unwrap(), "Why does this break");
    }

    #[test]
    fn strips_slash_command_prefix() {
        let t = derive_title("/plan add login screen with email").unwrap();
        assert_eq!(t, "Add login screen with email");
    }

    #[test]
    fn strips_bang_prefix() {
        let t = derive_title("!run the migration on prod").unwrap();
        assert_eq!(t, "Run the migration on prod");
    }

    #[test]
    fn bare_slash_command_returns_none() {
        assert!(derive_title("/help").is_none());
    }

    #[test]
    fn collapses_whitespace_and_newlines() {
        let t = derive_title("refactor the\n\n auth     module").unwrap();
        assert_eq!(t, "Refactor the auth module");
    }

    #[test]
    fn extracts_first_user_message_from_string_content() {
        let f = write_jsonl(&[
            r#"{"type":"summary","content":"meta"}"#,
            r#"{"type":"user","message":{"role":"user","content":"please refactor the auth module"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
        ]);
        let got = extract_first_user_message(f.path()).unwrap();
        assert_eq!(got.as_deref(), Some("please refactor the auth module"));
    }

    #[test]
    fn extracts_first_user_message_from_block_content() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"please refactor the auth module"}]}}"#,
        ]);
        let got = extract_first_user_message(f.path()).unwrap();
        assert_eq!(got.as_deref(), Some("please refactor the auth module"));
    }

    #[test]
    fn skips_tool_result_synthetic_user_lines() {
        // claude --resume can synthesize a user line containing a tool_result;
        // we want the real human message that follows.
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"{json blob}"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":"the actual request"}}"#,
        ]);
        let got = extract_first_user_message(f.path()).unwrap();
        assert_eq!(got.as_deref(), Some("the actual request"));
    }

    #[test]
    fn returns_none_when_no_user_message() {
        let f = write_jsonl(&[
            r#"{"type":"summary","content":"meta"}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
        ]);
        let got = extract_first_user_message(f.path()).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn skips_malformed_lines() {
        let f = write_jsonl(&[
            r#"not json"#,
            r#"{"type":"user","message":{"role":"user","content":"valid message here"}}"#,
        ]);
        let got = extract_first_user_message(f.path()).unwrap();
        assert_eq!(got.as_deref(), Some("valid message here"));
    }
}
