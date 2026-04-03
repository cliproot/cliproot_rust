use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// A parsed entry from the capture-hook agent log.
#[derive(Debug, Clone)]
pub struct HookLogEntry {
    pub timestamp: DateTime<Utc>,
    #[allow(dead_code)]
    pub session_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    #[allow(dead_code)]
    pub tool_response: serde_json::Value,
    #[allow(dead_code)]
    pub cwd: String,
}

/// Enriched data extracted from hook logs that supplements transcript parsing.
#[derive(Debug, Clone, Default)]
pub struct HookEnrichment {
    /// URLs fetched (from WebFetch tool calls) — includes ones not captured as clips.
    pub urls_fetched: Vec<UrlFetched>,
    /// Files that were read during the session.
    pub files_read: Vec<String>,
    /// Files that were written or edited during the session.
    pub files_modified: Vec<String>,
    /// Bash commands executed.
    pub bash_commands: Vec<String>,
    /// Raw entries indexed by tool_use_id for cross-referencing.
    pub entries_by_tool_use_id: HashMap<String, HookLogEntry>,
}

#[derive(Debug, Clone)]
pub struct UrlFetched {
    pub url: String,
    #[allow(dead_code)]
    pub timestamp: DateTime<Utc>,
}

#[derive(Deserialize)]
struct RawLogEntry {
    ts: String,
    session_id: String,
    tool_use_id: String,
    tool_name: String,
    tool_input: serde_json::Value,
    tool_response: serde_json::Value,
    cwd: String,
}

/// Parse a hook log JSONL file from .cliproot/agent-log/.
pub fn parse_hook_log(path: &Path) -> Result<Vec<HookLogEntry>, Box<dyn std::error::Error>> {
    let data = fs::read_to_string(path)?;
    let mut entries = Vec::new();

    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let raw: RawLogEntry = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let timestamp = DateTime::parse_from_rfc3339(&raw.ts)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        entries.push(HookLogEntry {
            timestamp,
            session_id: raw.session_id,
            tool_use_id: raw.tool_use_id,
            tool_name: raw.tool_name,
            tool_input: raw.tool_input,
            tool_response: raw.tool_response,
            cwd: raw.cwd,
        });
    }

    entries.sort_by_key(|e| e.timestamp);
    Ok(entries)
}

/// Build enrichment data from parsed hook log entries.
pub fn build_enrichment(entries: &[HookLogEntry]) -> HookEnrichment {
    let mut enrichment = HookEnrichment::default();
    let mut seen_files_read = std::collections::HashSet::new();
    let mut seen_files_modified = std::collections::HashSet::new();

    for entry in entries {
        enrichment
            .entries_by_tool_use_id
            .insert(entry.tool_use_id.clone(), entry.clone());

        match entry.tool_name.as_str() {
            "WebFetch" => {
                if let Some(url) = entry.tool_input.get("url").and_then(|v| v.as_str()) {
                    enrichment.urls_fetched.push(UrlFetched {
                        url: url.to_string(),
                        timestamp: entry.timestamp,
                    });
                }
            }
            "Read" => {
                if let Some(path) = entry.tool_input.get("file_path").and_then(|v| v.as_str()) {
                    if seen_files_read.insert(path.to_string()) {
                        enrichment.files_read.push(path.to_string());
                    }
                }
            }
            "Write" | "Edit" => {
                let path_key = if entry.tool_name == "Write" {
                    "file_path"
                } else {
                    "file_path"
                };
                if let Some(path) = entry.tool_input.get(path_key).and_then(|v| v.as_str()) {
                    if seen_files_modified.insert(path.to_string()) {
                        enrichment.files_modified.push(path.to_string());
                    }
                }
            }
            "Bash" => {
                if let Some(cmd) = entry.tool_input.get("command").and_then(|v| v.as_str()) {
                    enrichment.bash_commands.push(cmd.to_string());
                }
            }
            _ => {}
        }
    }

    enrichment
}

/// Find the hook log file for a given session ID in .cliproot/agent-log/.
pub fn find_hook_log(cliproot_dir: &Path, session_id: &str) -> Option<std::path::PathBuf> {
    let log_path = cliproot_dir
        .join("agent-log")
        .join(format!("{session_id}.jsonl"));
    if log_path.is_file() {
        Some(log_path)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hook_log_entry() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.jsonl");
        let entry = serde_json::json!({
            "ts": "2026-04-01T13:20:15Z",
            "session_id": "test-123",
            "tool_use_id": "toolu_01",
            "tool_name": "WebFetch",
            "tool_input": {"url": "https://example.com"},
            "tool_response": {"status": "ok"},
            "cwd": "/tmp"
        });
        fs::write(&log_path, serde_json::to_string(&entry).unwrap()).unwrap();

        let entries = parse_hook_log(&log_path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tool_name, "WebFetch");
        assert_eq!(entries[0].session_id, "test-123");
    }

    #[test]
    fn builds_enrichment() {
        let entries = vec![
            HookLogEntry {
                timestamp: Utc::now(),
                session_id: "s1".into(),
                tool_use_id: "t1".into(),
                tool_name: "WebFetch".into(),
                tool_input: serde_json::json!({"url": "https://example.com"}),
                tool_response: serde_json::json!({}),
                cwd: "/tmp".into(),
            },
            HookLogEntry {
                timestamp: Utc::now(),
                session_id: "s1".into(),
                tool_use_id: "t2".into(),
                tool_name: "Read".into(),
                tool_input: serde_json::json!({"file_path": "/src/main.rs"}),
                tool_response: serde_json::json!({}),
                cwd: "/tmp".into(),
            },
            HookLogEntry {
                timestamp: Utc::now(),
                session_id: "s1".into(),
                tool_use_id: "t3".into(),
                tool_name: "Edit".into(),
                tool_input: serde_json::json!({"file_path": "/src/lib.rs"}),
                tool_response: serde_json::json!({}),
                cwd: "/tmp".into(),
            },
        ];

        let enrichment = build_enrichment(&entries);
        assert_eq!(enrichment.urls_fetched.len(), 1);
        assert_eq!(enrichment.urls_fetched[0].url, "https://example.com");
        assert_eq!(enrichment.files_read, vec!["/src/main.rs"]);
        assert_eq!(enrichment.files_modified, vec!["/src/lib.rs"]);
    }

    #[test]
    fn deduplicates_files() {
        let entries = vec![
            HookLogEntry {
                timestamp: Utc::now(),
                session_id: "s1".into(),
                tool_use_id: "t1".into(),
                tool_name: "Read".into(),
                tool_input: serde_json::json!({"file_path": "/src/main.rs"}),
                tool_response: serde_json::json!({}),
                cwd: "/tmp".into(),
            },
            HookLogEntry {
                timestamp: Utc::now(),
                session_id: "s1".into(),
                tool_use_id: "t2".into(),
                tool_name: "Read".into(),
                tool_input: serde_json::json!({"file_path": "/src/main.rs"}),
                tool_response: serde_json::json!({}),
                cwd: "/tmp".into(),
            },
        ];

        let enrichment = build_enrichment(&entries);
        assert_eq!(enrichment.files_read.len(), 1);
    }
}
