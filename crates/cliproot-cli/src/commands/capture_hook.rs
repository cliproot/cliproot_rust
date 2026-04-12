use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;

use serde::Serialize;

use crate::commands::harness::{parse_hook_input, Harness};

const MAX_STRING_BYTES: usize = 50_000;

/// Tools to always capture.
const CAPTURED_TOOLS: &[&str] = &["WebFetch", "Read", "Write", "Edit", "Bash", "Agent"];

/// MCP tool prefix — any tool name starting with this is captured.
const MCP_TOOL_PREFIX: &str = "mcp__cliproot__";

/// Tools explicitly skipped (noisy, high-frequency).
const SKIPPED_TOOLS: &[&str] = &["Glob", "Grep", "ToolSearch"];

#[derive(Serialize)]
struct LogEntry {
    ts: String,
    session_id: String,
    tool_use_id: String,
    tool_name: String,
    tool_input: serde_json::Value,
    tool_response: serde_json::Value,
    cwd: String,
}

pub fn run(harness: Harness) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Read stdin
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    // 2. Parse using harness-aware dispatch
    let hook = parse_hook_input(harness, &input)?;

    // 3. Filter
    if !should_capture(&hook.tool_name) {
        return Ok(());
    }

    // 4. Sanitize session_id before using as filename
    let session_id = sanitize_session_id(&hook.session_id)?;

    // 5. Find .cliproot/
    let cliproot_dir = discover_cliproot_dir(&hook.cwd)?;
    let log_dir = cliproot_dir.join("agent-log");
    fs::create_dir_all(&log_dir)?;

    // 6. Build log entry
    let entry = LogEntry {
        ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        session_id: session_id.to_string(),
        tool_use_id: hook.tool_use_id,
        tool_name: hook.tool_name,
        tool_input: truncate_strings(hook.tool_input),
        tool_response: truncate_strings(hook.tool_response),
        cwd: hook.cwd,
    };

    // 7. Append JSONL
    let log_path = log_dir.join(format!("{session_id}.jsonl"));
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let line = serde_json::to_string(&entry)?;
    writeln!(file, "{line}")?;

    Ok(())
}

fn should_capture(tool_name: &str) -> bool {
    if tool_name.starts_with(MCP_TOOL_PREFIX) {
        return true;
    }
    if SKIPPED_TOOLS.contains(&tool_name) {
        return false;
    }
    CAPTURED_TOOLS.contains(&tool_name)
}

fn sanitize_session_id(id: &str) -> Result<&str, Box<dyn std::error::Error>> {
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.contains('\0')
    {
        return Err(format!("invalid session_id: {id}").into());
    }
    Ok(id)
}

fn discover_cliproot_dir(cwd: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut dir = PathBuf::from(cwd);
    loop {
        let candidate = dir.join(".cliproot");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        if !dir.pop() {
            return Err("no .cliproot/ directory found in any ancestor".into());
        }
    }
}

fn truncate_strings(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            if s.len() > MAX_STRING_BYTES {
                let truncated = floor_char_boundary(&s, MAX_STRING_BYTES);
                serde_json::Value::String(format!("{truncated} [truncated]"))
            } else {
                serde_json::Value::String(s)
            }
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(truncate_strings).collect())
        }
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, truncate_strings(v)))
                .collect(),
        ),
        other => other,
    }
}

/// Find the largest byte index <= max that is a char boundary.
fn floor_char_boundary(s: &str, max: usize) -> &str {
    if max >= s.len() {
        return s;
    }
    let mut i = max;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    &s[..i]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── should_capture ─────────────────────────────────────────────────────

    #[test]
    fn captures_write() {
        assert!(should_capture("Write"));
    }

    #[test]
    fn captures_web_fetch() {
        assert!(should_capture("WebFetch"));
    }

    #[test]
    fn captures_mcp_tool() {
        assert!(should_capture("mcp__cliproot__clip"));
        assert!(should_capture("mcp__cliproot__derive"));
    }

    #[test]
    fn skips_glob() {
        assert!(!should_capture("Glob"));
    }

    #[test]
    fn skips_grep() {
        assert!(!should_capture("Grep"));
    }

    #[test]
    fn skips_unknown_tool() {
        assert!(!should_capture("SomeNewTool"));
    }

    // ── sanitize_session_id ────────────────────────────────────────────────

    #[test]
    fn valid_session_id() {
        assert_eq!(sanitize_session_id("abc-123").unwrap(), "abc-123");
    }

    #[test]
    fn rejects_slash() {
        assert!(sanitize_session_id("../etc/passwd").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(sanitize_session_id("").is_err());
    }

    #[test]
    fn rejects_null_byte() {
        assert!(sanitize_session_id("abc\0def").is_err());
    }

    // ── truncate_strings ───────────────────────────────────────────────────

    #[test]
    fn short_string_unchanged() {
        let v = serde_json::json!("hello");
        assert_eq!(truncate_strings(v), serde_json::json!("hello"));
    }

    #[test]
    fn long_string_truncated() {
        let long = "x".repeat(60_000);
        let result = truncate_strings(serde_json::json!(long));
        let s = result.as_str().unwrap();
        assert!(s.ends_with(" [truncated]"));
        assert!(s.len() < 55_000);
    }

    #[test]
    fn nested_object_truncated() {
        let long = "x".repeat(60_000);
        let v = serde_json::json!({"content": long, "short": "ok"});
        let result = truncate_strings(v);
        assert!(result["content"]
            .as_str()
            .unwrap()
            .ends_with(" [truncated]"));
        assert_eq!(result["short"], "ok");
    }

    #[test]
    fn non_string_values_unchanged() {
        let v = serde_json::json!({"num": 42, "flag": true, "nothing": null});
        let result = truncate_strings(v.clone());
        assert_eq!(result, v);
    }

    // ── discover_cliproot_dir ──────────────────────────────────────────────

    #[test]
    fn finds_cliproot_in_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let cliproot = dir.path().join(".cliproot");
        std::fs::create_dir_all(&cliproot).unwrap();
        let sub = dir.path().join("a/b/c");
        std::fs::create_dir_all(&sub).unwrap();

        let found = discover_cliproot_dir(sub.to_str().unwrap()).unwrap();
        assert_eq!(found, cliproot);
    }

    #[test]
    fn error_when_no_cliproot() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover_cliproot_dir(dir.path().to_str().unwrap()).is_err());
    }

    // ── full capture flow ──────────────────────────────────────────────────

    #[test]
    fn full_capture_roundtrip() {
        use crate::commands::harness::{parse_hook_input, Harness};

        let dir = tempfile::tempdir().unwrap();
        let cliproot = dir.path().join(".cliproot");
        std::fs::create_dir_all(&cliproot).unwrap();

        let input = serde_json::json!({
            "session_id": "test-session-123",
            "transcript_path": "/tmp/transcript.jsonl",
            "cwd": dir.path().to_str().unwrap(),
            "permission_mode": "default",
            "hook_event_name": "PostToolUse",
            "tool_name": "Write",
            "tool_input": {"file_path": "/tmp/foo.rs", "content": "fn main() {}"},
            "tool_response": {"status": "ok"},
            "tool_use_id": "toolu_01ABC"
        });

        // Parse using harness dispatcher (Claude Code format)
        let hook = parse_hook_input(Harness::ClaudeCode, &input.to_string()).unwrap();
        assert!(should_capture(&hook.tool_name));

        // Write
        let log_dir = cliproot.join("agent-log");
        std::fs::create_dir_all(&log_dir).unwrap();
        let entry = LogEntry {
            ts: "2026-04-01T00:00:00Z".to_string(),
            session_id: hook.session_id.clone(),
            tool_use_id: hook.tool_use_id,
            tool_name: hook.tool_name.clone(),
            tool_input: truncate_strings(hook.tool_input),
            tool_response: truncate_strings(hook.tool_response),
            cwd: hook.cwd.clone(),
        };
        let log_path = log_dir.join(format!("{}.jsonl", hook.session_id));
        let line = serde_json::to_string(&entry).unwrap();
        std::fs::write(&log_path, format!("{line}\n")).unwrap();

        // Verify
        let content = std::fs::read_to_string(&log_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["tool_name"], "Write");
        assert_eq!(parsed["session_id"], "test-session-123");
        assert_eq!(parsed["tool_input"]["file_path"], "/tmp/foo.rs");
    }

    #[test]
    fn cursor_capture_roundtrip() {
        use crate::commands::harness::{parse_hook_input, Harness};

        let dir = tempfile::tempdir().unwrap();
        let cliproot = dir.path().join(".cliproot");
        std::fs::create_dir_all(&cliproot).unwrap();

        // Cursor uses tool_output instead of tool_response
        let input = serde_json::json!({
            "session_id": "cursor-session-456",
            "tool_name": "WebFetch",
            "tool_input": {"url": "https://example.com"},
            "tool_output": {"content": "<html>...</html>"},
            "tool_use_id": "tool_cus_xyz",
            "cwd": dir.path().to_str().unwrap()
        });

        // Parse using harness dispatcher (Cursor format)
        let hook = parse_hook_input(Harness::Cursor, &input.to_string()).unwrap();
        assert!(should_capture(&hook.tool_name));

        // Verify normalization: Cursor's tool_output becomes tool_response
        assert_eq!(hook.tool_response["content"], "<html>...</html>");

        // Write
        let log_dir = cliproot.join("agent-log");
        std::fs::create_dir_all(&log_dir).unwrap();
        let entry = LogEntry {
            ts: "2026-04-01T00:00:00Z".to_string(),
            session_id: hook.session_id.clone(),
            tool_use_id: hook.tool_use_id.clone(),
            tool_name: hook.tool_name.clone(),
            tool_input: truncate_strings(hook.tool_input),
            tool_response: truncate_strings(hook.tool_response),
            cwd: hook.cwd.clone(),
        };
        let log_path = log_dir.join(format!("{}.jsonl", hook.session_id));
        let line = serde_json::to_string(&entry).unwrap();
        std::fs::write(&log_path, format!("{line}\n")).unwrap();

        // Verify
        let content = std::fs::read_to_string(&log_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["tool_name"], "WebFetch");
        assert_eq!(parsed["session_id"], "cursor-session-456");
        // Cursor's tool_output got normalized to tool_response
        assert_eq!(parsed["tool_response"]["content"], "<html>...</html>");
    }

    #[test]
    fn codex_capture_roundtrip() {
        use crate::commands::harness::{parse_hook_input, Harness};

        let dir = tempfile::tempdir().unwrap();
        let cliproot = dir.path().join(".cliproot");
        std::fs::create_dir_all(&cliproot).unwrap();

        // Codex format (similar to Claude but with optional fields)
        let input = serde_json::json!({
            "session_id": "codex-session-789",
            "tool_name": "Read",
            "tool_input": {"file_path": "/home/user/src/main.rs"},
            "tool_response": {"content": "fn main() { println!(\"Hello\"); }"},
            "tool_use_id": "call_abc123",
            "cwd": dir.path().to_str().unwrap(),
            "transcript_path": "/tmp/codex-transcript.jsonl"
        });

        // Parse using harness dispatcher (Codex format)
        let hook = parse_hook_input(Harness::Codex, &input.to_string()).unwrap();
        assert!(should_capture(&hook.tool_name));
        assert_eq!(hook.tool_name, "Read");
        assert_eq!(hook.tool_input["file_path"], "/home/user/src/main.rs");

        // Write
        let log_dir = cliproot.join("agent-log");
        std::fs::create_dir_all(&log_dir).unwrap();
        let entry = LogEntry {
            ts: "2026-04-01T00:00:00Z".to_string(),
            session_id: hook.session_id.clone(),
            tool_use_id: hook.tool_use_id.clone(),
            tool_name: hook.tool_name.clone(),
            tool_input: truncate_strings(hook.tool_input),
            tool_response: truncate_strings(hook.tool_response),
            cwd: hook.cwd.clone(),
        };
        let log_path = log_dir.join(format!("{}.jsonl", hook.session_id));
        let line = serde_json::to_string(&entry).unwrap();
        std::fs::write(&log_path, format!("{line}\n")).unwrap();

        // Verify
        let content = std::fs::read_to_string(&log_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["tool_name"], "Read");
        assert_eq!(parsed["session_id"], "codex-session-789");
    }
}
