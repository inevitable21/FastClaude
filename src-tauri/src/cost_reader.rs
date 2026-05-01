use crate::config::Pricing;
use crate::error::AppResult;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct UsageDelta {
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub tokens_cache_read: i64,
    pub tokens_cache_write: i64,
    pub cost_usd: f64,
    pub new_offset: u64,
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    typ: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    usage: Option<Usage>,
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

pub fn read_delta(
    path: &Path,
    start_offset: u64,
    model: &str,
    pricing: &HashMap<String, Pricing>,
) -> AppResult<UsageDelta> {
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
        if parsed.typ.as_deref() != Some("assistant") {
            continue;
        }
        let Some(Message { usage: Some(u) }) = parsed.message else { continue };
        delta.tokens_in += u.input_tokens;
        delta.tokens_out += u.output_tokens;
        delta.tokens_cache_read += u.cache_read_input_tokens;
        delta.tokens_cache_write += u.cache_creation_input_tokens;
    }

    if delta.new_offset > total_len {
        delta.new_offset = total_len;
    }

    if let Some(p) = pricing.get(model) {
        delta.cost_usd = (delta.tokens_in as f64) * p.input / 1_000_000.0
            + (delta.tokens_out as f64) * p.output / 1_000_000.0
            + (delta.tokens_cache_read as f64) * p.cache_read / 1_000_000.0
            + (delta.tokens_cache_write as f64) * p.cache_write / 1_000_000.0;
    }
    Ok(delta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn pricing() -> HashMap<String, Pricing> {
        let mut p = HashMap::new();
        p.insert(
            "claude-opus-4-7".into(),
            Pricing { input: 15.0, output: 75.0, cache_read: 1.5, cache_write: 18.75 },
        );
        p
    }

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
        let d = read_delta(f.path(), 0, "claude-opus-4-7", &pricing()).unwrap();
        assert_eq!(d.tokens_in, 150);
        assert_eq!(d.tokens_out, 75);
        assert_eq!(d.tokens_cache_read, 200);
        assert_eq!(d.tokens_cache_write, 1000);
        let expect = (150.0 * 15.0 + 75.0 * 75.0 + 200.0 * 1.5 + 1000.0 * 18.75) / 1_000_000.0;
        assert!((d.cost_usd - expect).abs() < 1e-9);
        assert!(d.new_offset > 0);
    }

    #[test]
    fn ignores_non_assistant_lines() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
            r#"{"type":"summary","content":"..."}"#,
        ]);
        let d = read_delta(f.path(), 0, "claude-opus-4-7", &pricing()).unwrap();
        assert_eq!(d.tokens_in, 0);
        assert_eq!(d.cost_usd, 0.0);
    }

    #[test]
    fn skips_malformed_lines() {
        let f = write_jsonl(&[
            r#"not even json"#,
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":10,"output_tokens":5}}}"#,
        ]);
        let d = read_delta(f.path(), 0, "claude-opus-4-7", &pricing()).unwrap();
        assert_eq!(d.tokens_in, 10);
        assert_eq!(d.tokens_out, 5);
    }

    #[test]
    fn resumes_from_offset() {
        let f = write_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":100,"output_tokens":50}}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":1,"output_tokens":2}}}"#,
        ]);
        let first = read_delta(f.path(), 0, "claude-opus-4-7", &pricing()).unwrap();
        let line1_len = (
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":100,"output_tokens":50}}}"#
        ).len() as u64 + 1;
        let second = read_delta(f.path(), line1_len, "claude-opus-4-7", &pricing()).unwrap();
        assert_eq!(second.tokens_in, 1);
        assert_eq!(second.tokens_out, 2);
        assert_eq!(first.tokens_in, 101);
    }

    #[test]
    fn missing_model_pricing_zeroes_cost() {
        let f = write_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":100,"output_tokens":50}}}"#,
        ]);
        let d = read_delta(f.path(), 0, "claude-unknown", &pricing()).unwrap();
        assert_eq!(d.tokens_in, 100);
        assert_eq!(d.cost_usd, 0.0);
    }
}
