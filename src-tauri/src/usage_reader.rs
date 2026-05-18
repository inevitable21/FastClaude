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

        // Tally tokens for assistant lines (unchanged behavior).
        if typ == "assistant" {
            if let Some(Message { usage: Some(ref u), .. }) = parsed.message {
                delta.tokens_in += u.input_tokens;
                delta.tokens_out += u.output_tokens;
                delta.tokens_cache_read += u.cache_read_input_tokens;
                delta.tokens_cache_write += u.cache_creation_input_tokens;
            }
        }

        // Detect rate-limit on assistant text OR system error content.
        // First-match-wins so a single limit line per delta call is captured.
        if delta.limit_event.is_none() {
            if let Some(text) = extract_text_for_limit_check(typ, &parsed) {
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
    use chrono::{NaiveTime, TimeZone, Utc};
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 4 < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let colon = if bytes[i + 1] == b':' { Some(i + 1) }
                else if i + 2 < bytes.len() && bytes[i + 2] == b':' { Some(i + 2) }
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
    fn tallies_assistant_usage() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":1000,"cache_read_input_tokens":200}}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":50,"output_tokens":25}}}"#,
        ]);
        let d = read_delta(f.path(), 0).unwrap();
        assert_eq!(d.tokens_in, 150);
        assert_eq!(d.tokens_out, 75);
        assert_eq!(d.tokens_cache_read, 200);
        assert_eq!(d.tokens_cache_write, 1000);
        assert!(d.new_offset > 0);
    }

    #[test]
    fn ignores_non_assistant_lines() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
            r#"{"type":"summary","content":"..."}"#,
        ]);
        let d = read_delta(f.path(), 0).unwrap();
        assert_eq!(d.tokens_in, 0);
        assert_eq!(d.tokens_out, 0);
    }

    #[test]
    fn skips_malformed_lines() {
        let f = write_jsonl(&[
            r#"not even json"#,
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":10,"output_tokens":5}}}"#,
        ]);
        let d = read_delta(f.path(), 0).unwrap();
        assert_eq!(d.tokens_in, 10);
        assert_eq!(d.tokens_out, 5);
    }

    #[test]
    fn resumes_from_offset() {
        let f = write_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":100,"output_tokens":50}}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":1,"output_tokens":2}}}"#,
        ]);
        let first = read_delta(f.path(), 0).unwrap();
        let line1_len = (
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":100,"output_tokens":50}}}"#
        ).len() as u64 + 1;
        let second = read_delta(f.path(), line1_len).unwrap();
        assert_eq!(second.tokens_in, 1);
        assert_eq!(second.tokens_out, 2);
        assert_eq!(first.tokens_in, 101);
    }

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
        // Sentinel: 0 signals "fallback applied at caller layer".
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
}
