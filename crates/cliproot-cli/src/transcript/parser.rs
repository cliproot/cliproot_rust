use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// A parsed event from a Claude Code session JSONL transcript.
#[derive(Debug, Clone)]
pub struct TranscriptEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: TranscriptEventType,
    /// None for main session, Some(agent-id) for subagent messages.
    pub agent_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_output: Option<String>,
    pub message_text: Option<String>,
    pub uuid: String,
    #[allow(dead_code)]
    pub parent_uuid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptEventType {
    UserMessage,
    AssistantMessage,
    ToolUse,
    ToolResult,
}

/// Metadata about a Claude Code session extracted from the transcript.
#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub session_id: String,
    pub git_branch: Option<String>,
    #[allow(dead_code)]
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
}

/// Metadata about a subagent from its .meta.json file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubagentMeta {
    #[serde(default)]
    _agent_type: Option<String>,
    #[serde(default)]
    _description: Option<String>,
}

// ── Raw JSONL structures ──

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLine {
    uuid: String,
    #[serde(default)]
    parent_uuid: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(rename = "type", default)]
    line_type: Option<String>,
    #[serde(default)]
    message: Option<RawMessage>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(rename = "gitBranch", default)]
    git_branch: Option<String>,
    #[serde(rename = "isSidechain", default)]
    _is_sidechain: bool,
    #[serde(flatten)]
    _extra: serde_json::Value,
}

#[derive(Deserialize)]
struct RawMessage {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: serde_json::Value,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<serde_json::Value>,
    #[serde(default)]
    text: Option<String>,
    // tool_result fields
    #[serde(default)]
    tool_use_id: Option<String>,
    #[serde(default)]
    content: Option<serde_json::Value>,
}

/// Parse a single Claude Code session JSONL file into an ordered event stream.
pub fn parse_jsonl(path: &Path) -> Result<Vec<TranscriptEvent>, Box<dyn std::error::Error>> {
    let data = fs::read_to_string(path)?;
    parse_jsonl_str(&data, None)
}

/// Parse JSONL content string, optionally tagging events with an agent_id.
fn parse_jsonl_str(
    data: &str,
    agent_id: Option<&str>,
) -> Result<Vec<TranscriptEvent>, Box<dyn std::error::Error>> {
    let mut events = Vec::new();

    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let raw: RawLine = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => continue, // skip malformed lines
        };

        // Skip non-message line types (file-history-snapshot, last-prompt, etc.)
        match raw.line_type.as_deref() {
            Some("user") | Some("assistant") => {}
            Some(_) => continue,
            None => continue,
        }

        let Some(message) = raw.message else {
            continue;
        };

        let timestamp = raw
            .timestamp
            .as_deref()
            .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let Some(timestamp) = timestamp else {
            continue;
        };

        let role = message.role.as_deref().unwrap_or("");

        match role {
            "user" => {
                // User messages can contain text and tool_result blocks
                let (text_parts, tool_results) = extract_content_blocks(&message.content);

                if !text_parts.is_empty() {
                    events.push(TranscriptEvent {
                        timestamp,
                        event_type: TranscriptEventType::UserMessage,
                        agent_id: agent_id.map(|s| s.to_string()),
                        tool_name: None,
                        tool_input: None,
                        tool_output: None,
                        message_text: Some(text_parts.join("\n")),
                        uuid: raw.uuid.clone(),
                        parent_uuid: raw.parent_uuid.clone(),
                    });
                }

                for tr in tool_results {
                    events.push(TranscriptEvent {
                        timestamp,
                        event_type: TranscriptEventType::ToolResult,
                        agent_id: agent_id.map(|s| s.to_string()),
                        tool_name: None, // matched by tool_use_id
                        tool_input: None,
                        tool_output: Some(extract_tool_result_content(&tr.content)),
                        message_text: None,
                        uuid: tr.tool_use_id.unwrap_or_default(),
                        parent_uuid: Some(raw.uuid.clone()),
                    });
                }
            }
            "assistant" => {
                let (text_parts, tool_uses) = extract_assistant_blocks(&message.content);

                if !text_parts.is_empty() {
                    events.push(TranscriptEvent {
                        timestamp,
                        event_type: TranscriptEventType::AssistantMessage,
                        agent_id: agent_id.map(|s| s.to_string()),
                        tool_name: None,
                        tool_input: None,
                        tool_output: None,
                        message_text: Some(text_parts.join("\n")),
                        uuid: raw.uuid.clone(),
                        parent_uuid: raw.parent_uuid.clone(),
                    });
                }

                for tu in tool_uses {
                    events.push(TranscriptEvent {
                        timestamp,
                        event_type: TranscriptEventType::ToolUse,
                        agent_id: agent_id.map(|s| s.to_string()),
                        tool_name: Some(tu.name.unwrap_or_default()),
                        tool_input: tu.input,
                        tool_output: None,
                        message_text: None,
                        uuid: tu.id.unwrap_or_default(),
                        parent_uuid: Some(raw.uuid.clone()),
                    });
                }
            }
            _ => {}
        }
    }

    events.sort_by_key(|e| e.timestamp);
    Ok(events)
}

/// Extract session metadata from parsed transcript events and raw JSONL.
pub fn extract_session_meta(
    path: &Path,
    events: &[TranscriptEvent],
) -> Result<SessionMeta, Box<dyn std::error::Error>> {
    let data = fs::read_to_string(path)?;
    let mut session_id = None;
    let mut git_branch = None;
    let mut cwd = None;
    let mut model = None;

    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let raw: RawLine = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if session_id.is_none() {
            session_id = raw.session_id;
        }
        if git_branch.is_none() {
            git_branch = raw.git_branch;
        }
        if cwd.is_none() {
            cwd = raw.cwd;
        }
        if model.is_none() {
            if let Some(ref msg) = raw.message {
                if msg.model.is_some() {
                    model = msg.model.clone();
                }
            }
        }
        // Break early once we have all metadata
        if session_id.is_some() && git_branch.is_some() && cwd.is_some() && model.is_some() {
            break;
        }
    }

    let started_at = events.first().map(|e| e.timestamp);
    let ended_at = events.last().map(|e| e.timestamp);

    Ok(SessionMeta {
        session_id: session_id.unwrap_or_default(),
        git_branch,
        cwd,
        model,
        started_at,
        ended_at,
    })
}

/// Parse a session directory, merging subagent transcripts into the main timeline.
pub fn parse_session_dir(
    session_dir: &Path,
) -> Result<(Vec<TranscriptEvent>, SessionMeta), Box<dyn std::error::Error>> {
    // Find the main JSONL — it's the .jsonl file directly in session_dir's parent,
    // or the largest .jsonl in session_dir itself.
    let main_jsonl = find_main_jsonl(session_dir)?;
    let mut events = parse_jsonl(&main_jsonl)?;
    let meta = extract_session_meta(&main_jsonl, &events)?;

    // Merge subagent transcripts
    let subagents_dir = session_dir.join("subagents");
    if subagents_dir.is_dir() {
        for entry in fs::read_dir(&subagents_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "jsonl") {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                let agent_id = stem.to_string();
                let sub_events = parse_jsonl_str(&fs::read_to_string(&path)?, Some(&agent_id))?;
                events.extend(sub_events);
            }
        }
    }

    events.sort_by_key(|e| e.timestamp);
    Ok((events, meta))
}

/// Find the main JSONL file in a session directory.
fn find_main_jsonl(session_dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // The session_dir might be the directory containing subagents/, tool-results/, etc.
    // The main JSONL is typically at <session_dir>.jsonl (sibling to the dir)
    // or could be a .jsonl file directly passed.
    if session_dir.is_file() && session_dir.extension().map_or(false, |ext| ext == "jsonl") {
        return Ok(session_dir.to_path_buf());
    }

    // Try <session_dir>.jsonl as a sibling file
    let sibling = session_dir.with_extension("jsonl");
    if sibling.is_file() {
        return Ok(sibling);
    }

    // Try files inside the directory
    if session_dir.is_dir() {
        let mut jsonl_files: Vec<_> = fs::read_dir(session_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map_or(false, |ext| ext == "jsonl"))
            .filter(|p| {
                // Exclude subagent files
                !p.to_string_lossy().contains("subagents")
            })
            .collect();
        // Pick the largest file (most likely the main transcript)
        jsonl_files.sort_by_key(|p| std::cmp::Reverse(p.metadata().map(|m| m.len()).unwrap_or(0)));
        if let Some(path) = jsonl_files.first() {
            return Ok(path.clone());
        }
    }

    Err(format!("no JSONL file found in {}", session_dir.display()).into())
}

/// Discover the Claude Code session directory for the given working directory.
/// Searches ~/.claude/projects/ for a directory whose path hash matches.
pub fn discover_session_dir(cwd: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let home = home_dir()?;
    let projects_dir = home.join(".claude").join("projects");
    if !projects_dir.is_dir() {
        return Err("~/.claude/projects/ not found".into());
    }

    // Claude Code hashes the project path: /Volumes/DevSpace/src/foo → -Volumes-DevSpace-src-foo
    // Non-alphanumeric characters (/, _, spaces, etc.) are all replaced with hyphens.
    let cwd_str = cwd.to_string_lossy();
    let path_hash: String = cwd_str
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    // Also try without leading dash
    let path_hash_no_leading = path_hash.strip_prefix('-').unwrap_or(&path_hash);

    let mut candidates = Vec::new();
    for entry in fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let dir_name = entry.file_name().to_string_lossy().to_string();
        if dir_name == path_hash || dir_name == path_hash_no_leading {
            let project_dir = entry.path();
            if project_dir.is_dir() {
                candidates.push(project_dir);
            }
        }
    }

    Ok(candidates)
}

/// Find session JSONL files within a Claude Code project directory, sorted newest first.
pub fn find_sessions(project_dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut sessions: Vec<PathBuf> = fs::read_dir(project_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |ext| ext == "jsonl"))
        .collect();

    // Sort by modification time, newest first
    sessions.sort_by(|a, b| {
        let ma = a.metadata().and_then(|m| m.modified()).ok();
        let mb = b.metadata().and_then(|m| m.modified()).ok();
        mb.cmp(&ma)
    });

    Ok(sessions)
}

fn home_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| "HOME not set".into())
}

// ── Content extraction helpers ──

fn extract_content_blocks(content: &serde_json::Value) -> (Vec<String>, Vec<ContentBlock>) {
    let mut texts = Vec::new();
    let mut tool_results = Vec::new();

    match content {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                texts.push(trimmed.to_string());
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                if let Ok(block) = serde_json::from_value::<ContentBlock>(item.clone()) {
                    match block.block_type.as_str() {
                        "text" => {
                            if let Some(ref text) = block.text {
                                let trimmed = text.trim();
                                if !trimmed.is_empty() {
                                    texts.push(trimmed.to_string());
                                }
                            }
                        }
                        "tool_result" => {
                            tool_results.push(block);
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }

    (texts, tool_results)
}

fn extract_assistant_blocks(content: &serde_json::Value) -> (Vec<String>, Vec<ContentBlock>) {
    let mut texts = Vec::new();
    let mut tool_uses = Vec::new();

    match content {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                texts.push(trimmed.to_string());
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                if let Ok(block) = serde_json::from_value::<ContentBlock>(item.clone()) {
                    match block.block_type.as_str() {
                        "text" => {
                            if let Some(ref text) = block.text {
                                let trimmed = text.trim();
                                if !trimmed.is_empty() {
                                    texts.push(trimmed.to_string());
                                }
                            }
                        }
                        "tool_use" => {
                            tool_uses.push(block);
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }

    (texts, tool_uses)
}

fn extract_tool_result_content(content: &Option<serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => {
            let mut parts = Vec::new();
            for item in arr {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    parts.push(text.to_string());
                }
            }
            parts.join("\n")
        }
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_jsonl(lines: &[serde_json::Value]) -> String {
        lines
            .iter()
            .map(|v| serde_json::to_string(v).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn parses_user_message() {
        let data = make_jsonl(&[serde_json::json!({
            "uuid": "u1",
            "timestamp": "2026-04-01T13:00:00.000Z",
            "type": "user",
            "message": {
                "role": "user",
                "content": "Research OAuth2 PKCE flow"
            }
        })]);

        let events = parse_jsonl_str(&data, None).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, TranscriptEventType::UserMessage);
        assert_eq!(
            events[0].message_text.as_deref(),
            Some("Research OAuth2 PKCE flow")
        );
    }

    #[test]
    fn parses_assistant_with_tool_use() {
        let data = make_jsonl(&[serde_json::json!({
            "uuid": "a1",
            "timestamp": "2026-04-01T13:00:01.000Z",
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Let me research that."},
                    {
                        "type": "tool_use",
                        "id": "toolu_01",
                        "name": "mcp__cliproot__clip",
                        "input": {"url": "https://example.com", "quote": "test"}
                    }
                ]
            }
        })]);

        let events = parse_jsonl_str(&data, None).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, TranscriptEventType::AssistantMessage);
        assert_eq!(events[1].event_type, TranscriptEventType::ToolUse);
        assert_eq!(events[1].tool_name.as_deref(), Some("mcp__cliproot__clip"));
    }

    #[test]
    fn parses_tool_result() {
        let data = make_jsonl(&[serde_json::json!({
            "uuid": "u2",
            "timestamp": "2026-04-01T13:00:02.000Z",
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01",
                        "content": "Clip created: sha256-abc123"
                    }
                ]
            }
        })]);

        let events = parse_jsonl_str(&data, None).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, TranscriptEventType::ToolResult);
        assert_eq!(
            events[0].tool_output.as_deref(),
            Some("Clip created: sha256-abc123")
        );
    }

    #[test]
    fn skips_non_message_types() {
        let data = make_jsonl(&[
            serde_json::json!({
                "uuid": "f1",
                "timestamp": "2026-04-01T13:00:00.000Z",
                "type": "file-history-snapshot",
                "message": null
            }),
            serde_json::json!({
                "uuid": "u1",
                "timestamp": "2026-04-01T13:00:01.000Z",
                "type": "user",
                "message": {"role": "user", "content": "hello"}
            }),
        ]);

        let events = parse_jsonl_str(&data, None).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, TranscriptEventType::UserMessage);
    }

    #[test]
    fn tags_subagent_events() {
        let data = make_jsonl(&[serde_json::json!({
            "uuid": "s1",
            "timestamp": "2026-04-01T13:00:00.000Z",
            "type": "assistant",
            "message": {"role": "assistant", "content": "Exploring..."}
        })]);

        let events = parse_jsonl_str(&data, Some("agent-explore-1")).unwrap();
        assert_eq!(events[0].agent_id.as_deref(), Some("agent-explore-1"));
    }

    #[test]
    fn sorts_by_timestamp() {
        let data = make_jsonl(&[
            serde_json::json!({
                "uuid": "u2",
                "timestamp": "2026-04-01T13:00:05.000Z",
                "type": "user",
                "message": {"role": "user", "content": "second"}
            }),
            serde_json::json!({
                "uuid": "u1",
                "timestamp": "2026-04-01T13:00:01.000Z",
                "type": "user",
                "message": {"role": "user", "content": "first"}
            }),
        ]);

        let events = parse_jsonl_str(&data, None).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].message_text.as_deref(), Some("first"));
        assert_eq!(events[1].message_text.as_deref(), Some("second"));
    }
}
