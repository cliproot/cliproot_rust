//! Harness-aware hook dispatch.
//!
//! Normalizes input from Claude Code, Cursor, and Codex into a common format,
//! and handles harness-specific response emission.

use clap::ValueEnum;
use serde::Deserialize;

/// The AI harness (agent tool-calling environment) that emitted the hook event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum Harness {
    /// Claude Code (Anthropic) - default
    #[value(name = "claude-code")]
    #[default]
    ClaudeCode,

    /// Cursor IDE with hooks
    #[value(name = "cursor")]
    Cursor,
    /// OpenAI Codex CLI
    #[value(name = "codex")]
    Codex,
}





/// Normalized hook input event, common across all harnesses.
#[derive(Debug, Clone)]
pub struct NormalizedHookEvent {
    pub session_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_response: serde_json::Value,
    pub tool_use_id: String,
    pub cwd: String,
    pub transcript_path: Option<String>,
}

/// Claude Code hook input format (PostToolUse).
#[derive(Deserialize)]
pub struct ClaudeCodePostToolUse {
    pub session_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_response: serde_json::Value,
    pub tool_use_id: String,
    pub cwd: String,
    #[serde(flatten)]
    _extra: serde_json::Value,
}

/// Claude Code hook input format (Stop/PreCompact).
#[derive(Deserialize)]
pub struct ClaudeCodeStop {
    pub session_id: String,
    pub cwd: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(flatten)]
    _extra: serde_json::Value,
}

/// Cursor hook input format (postToolUse / stop / preCompact).
/// Schema: <https://cursor.com/docs/hooks>
#[derive(Deserialize)]
pub struct CursorHookInput {
    pub session_id: String,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default, rename = "tool_input")]
    pub tool_input: Option<serde_json::Value>,
    #[serde(default, rename = "tool_output")]
    pub tool_output: Option<serde_json::Value>,
    #[serde(default, rename = "tool_use_id")]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Cursor uses `tool_output` instead of `tool_response`
    #[serde(flatten)]
    _extra: serde_json::Value,
}

/// Codex hook input format.
/// Schema mirrors Claude Code for compatibility.
#[derive(Deserialize)]
pub struct CodexHookInput {
    pub session_id: String,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_response: Option<serde_json::Value>,
    #[serde(default, rename = "tool_use_id")]
    pub tool_use_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(flatten)]
    _extra: serde_json::Value,
}

/// Parse Claude Code PostToolUse JSON into normalized event.
pub fn parse_claude_code_post_tool_use(
    input: &str,
) -> Result<NormalizedHookEvent, Box<dyn std::error::Error>> {
    let hook: ClaudeCodePostToolUse = serde_json::from_str(input)?;

    Ok(NormalizedHookEvent {
        session_id: hook.session_id,
        tool_name: hook.tool_name,
        tool_input: hook.tool_input,
        tool_response: hook.tool_response,
        tool_use_id: hook.tool_use_id,
        cwd: hook.cwd,
        transcript_path: None,
    })
}

/// Parse Claude Code Stop/PreCompact JSON into normalized event.
pub fn parse_claude_code_stop(
    input: &str,
) -> Result<NormalizedHookEvent, Box<dyn std::error::Error>> {
    let hook: ClaudeCodeStop = serde_json::from_str(input)?;

    Ok(NormalizedHookEvent {
        session_id: hook.session_id,
        tool_name: String::new(), // Not meaningful for Stop hooks
        tool_input: serde_json::Value::Null,
        tool_response: serde_json::Value::Null,
        tool_use_id: String::new(),
        cwd: hook.cwd,
        transcript_path: hook.transcript_path,
    })
}

/// Parse Cursor hook JSON into normalized event.
pub fn parse_cursor_hook(
    input: &str,
) -> Result<NormalizedHookEvent, Box<dyn std::error::Error>> {
    let hook: CursorHookInput = serde_json::from_str(input)?;

    Ok(NormalizedHookEvent {
        session_id: hook.session_id,
        tool_name: hook.tool_name.unwrap_or_default(),
        tool_input: hook.tool_input.unwrap_or(serde_json::Value::Null),
        // Cursor uses `tool_output` instead of `tool_response`
        tool_response: hook.tool_output.unwrap_or(serde_json::Value::Null),
        tool_use_id: hook.tool_use_id.unwrap_or_default(),
        cwd: hook.cwd.unwrap_or_default(),
        transcript_path: None,
    })
}

/// Parse Codex hook JSON into normalized event.
pub fn parse_codex_hook(
    input: &str,
) -> Result<NormalizedHookEvent, Box<dyn std::error::Error>> {
    let hook: CodexHookInput = serde_json::from_str(input)?;

    Ok(NormalizedHookEvent {
        session_id: hook.session_id,
        tool_name: hook.tool_name.unwrap_or_default(),
        tool_input: hook.tool_input.unwrap_or(serde_json::Value::Null),
        tool_response: hook.tool_response.unwrap_or(serde_json::Value::Null),
        tool_use_id: hook.tool_use_id.unwrap_or_default(),
        cwd: hook.cwd.unwrap_or_default(),
        transcript_path: hook.transcript_path,
    })
}

/// Dispatch to the correct parser based on harness.
pub fn parse_hook_input(
    harness: Harness,
    input: &str,
) -> Result<NormalizedHookEvent, Box<dyn std::error::Error>> {
    match harness {
        Harness::ClaudeCode => parse_claude_code_post_tool_use(input),
        Harness::Cursor => parse_cursor_hook(input),
        Harness::Codex => parse_codex_hook(input),
    }
}

/// Parse a Stop/PreCompact hook input based on harness.
pub fn parse_stop_input(
    harness: Harness,
    input: &str,
) -> Result<NormalizedHookEvent, Box<dyn std::error::Error>> {
    match harness {
        Harness::ClaudeCode => parse_claude_code_stop(input),
        // Cursor and Codex use similar structure to Claude Code for stop events
        Harness::Cursor => {
            let hook: CursorHookInput = serde_json::from_str(input)?;
            Ok(NormalizedHookEvent {
                session_id: hook.session_id,
                tool_name: String::new(),
                tool_input: serde_json::Value::Null,
                tool_response: serde_json::Value::Null,
                tool_use_id: String::new(),
                cwd: hook.cwd.unwrap_or_default(),
                transcript_path: None,
            })
        }
        Harness::Codex => {
            let hook: CodexHookInput = serde_json::from_str(input)?;
            Ok(NormalizedHookEvent {
                session_id: hook.session_id,
                tool_name: String::new(),
                tool_input: serde_json::Value::Null,
                tool_response: serde_json::Value::Null,
                tool_use_id: String::new(),
                cwd: hook.cwd.unwrap_or_default(),
                transcript_path: hook.transcript_path,
            })
        }
    }
}

/// Emit a consolidation block response appropriate for the harness.
///
/// - Claude Code / Codex: `{"decision": "block", "reason": "..."}`
/// - Cursor: `{"followup_message": "..."}`
pub fn emit_consolidation_block(harness: Harness, reason: &str) -> String {
    match harness {
        Harness::ClaudeCode | Harness::Codex => {
            serde_json::json!({
                "decision": "block",
                "reason": reason,
            })
            .to_string()
        }
        Harness::Cursor => {
            serde_json::json!({
                "followup_message": reason,
            })
            .to_string()
        }
    }
}

/// Emit an empty passthrough response (no block needed).
pub fn emit_passthrough(_harness: Harness) -> &'static str {
    "{}"
}

/// Emit a Cursor preCompact observational response (cannot block, only observe).
#[allow(dead_code)]
pub fn emit_cursor_precompact_observation(message: &str) -> String {
    serde_json::json!({
        "user_message": message,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Claude Code parsing ────────────────────────────────────────────────

    #[test]
    fn parse_claude_post_tool_use() {
        let input = r#"{
            "session_id": "sess-123",
            "tool_name": "WebFetch",
            "tool_input": {"url": "https://example.com"},
            "tool_response": {"status": "ok", "content": "..."},
            "tool_use_id": "toolu_01ABC",
            "cwd": "/home/user/project"
        }"#;

        let event = parse_claude_code_post_tool_use(input).unwrap();
        assert_eq!(event.session_id, "sess-123");
        assert_eq!(event.tool_name, "WebFetch");
        assert_eq!(event.tool_use_id, "toolu_01ABC");
        assert_eq!(event.cwd, "/home/user/project");
    }

    #[test]
    fn parse_claude_stop() {
        let input = r#"{
            "session_id": "sess-456",
            "cwd": "/home/user/project",
            "transcript_path": "/tmp/transcript.jsonl"
        }"#;

        let event = parse_claude_code_stop(input).unwrap();
        assert_eq!(event.session_id, "sess-456");
        assert_eq!(event.cwd, "/home/user/project");
        assert_eq!(event.transcript_path, Some("/tmp/transcript.jsonl".to_string()));
    }

    // ── Cursor parsing ───────────────────────────────────────────────────

    #[test]
    fn parse_cursor_post_tool_use() {
        let input = r#"{
            "session_id": "sess-789",
            "tool_name": "WebFetch",
            "tool_input": {"url": "https://cursor.com"},
            "tool_output": {"status": "ok"},
            "tool_use_id": "tool_cus_123",
            "cwd": "/home/user/cursor-project"
        }"#;

        let event = parse_cursor_hook(input).unwrap();
        assert_eq!(event.session_id, "sess-789");
        assert_eq!(event.tool_name, "WebFetch");
        // Cursor uses tool_output -> normalized to tool_response
        assert_eq!(event.tool_response["status"], "ok");
        assert_eq!(event.tool_use_id, "tool_cus_123");
    }

    // ── Codex parsing ──────────────────────────────────────────────────────

    #[test]
    fn test_parse_codex_hook() {
        let input = r#"{
            "session_id": "sess-codex-001",
            "tool_name": "Read",
            "tool_input": {"file_path": "/home/user/main.rs"},
            "tool_response": {"content": "fn main() {}"},
            "tool_use_id": "call_codex_abc",
            "cwd": "/home/user/codex-project",
            "transcript_path": "/tmp/codex-transcript.jsonl"
        }"#;

        let event = parse_codex_hook(input).unwrap();
        assert_eq!(event.session_id, "sess-codex-001");
        assert_eq!(event.tool_name, "Read");
        assert_eq!(event.transcript_path, Some("/tmp/codex-transcript.jsonl".to_string()));
    }

    // ── Response emission ────────────────────────────────────────────────

    #[test]
    fn emit_claude_block() {
        let output = emit_consolidation_block(Harness::ClaudeCode, "Consolidation needed");
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["decision"], "block");
        assert_eq!(parsed["reason"], "Consolidation needed");
    }

    #[test]
    fn emit_cursor_followup() {
        let output = emit_consolidation_block(Harness::Cursor, "Time to consolidate");
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["followup_message"], "Time to consolidate");
        assert!(parsed.get("decision").is_none());
    }

    #[test]
    fn emit_codex_block() {
        let output = emit_consolidation_block(Harness::Codex, "Codex-style block");
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["decision"], "block");
        assert_eq!(parsed["reason"], "Codex-style block");
    }

    #[test]
    fn emit_cursor_observation() {
        let output = emit_cursor_precompact_observation("Noted pre-compact");
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["user_message"], "Noted pre-compact");
    }

}
