use std::collections::HashMap;

use cliproot_core::Clip;
use cliproot_store::Repository;

use super::parser::{TranscriptEvent, TranscriptEventType};

/// A matched clip from the transcript linked to a cliproot-stored clip.
#[derive(Debug, Clone)]
pub struct MatchedClip {
    /// The clip hash from .cliproot/index.db.
    pub clip_hash: String,
    /// The full clip record.
    pub clip: Clip,
    /// The transcript tool_use event UUID that produced this clip.
    pub tool_use_uuid: String,
    /// Whether this is a derived clip (from cliproot_derive) vs a source clip.
    pub is_derived: bool,
}

/// MCP tool name patterns for cliproot clip/derive operations.
const CLIP_TOOL_PATTERNS: &[&str] = &["clip", "cliproot_clip"];
const DERIVE_TOOL_PATTERNS: &[&str] = &["derive", "cliproot_derive"];

/// Match transcript tool calls against stored clips in the repository.
///
/// Scans for MCP tool calls that look like cliproot operations, extracts
/// clip hashes from tool results, and cross-references against index.db.
pub fn match_clips(
    events: &[TranscriptEvent],
    repo: &Repository,
) -> Result<Vec<MatchedClip>, Box<dyn std::error::Error>> {
    // Build a map of tool_use_id → ToolUse event for cross-referencing with results.
    let tool_uses: HashMap<&str, &TranscriptEvent> = events
        .iter()
        .filter(|e| e.event_type == TranscriptEventType::ToolUse)
        .map(|e| (e.uuid.as_str(), e))
        .collect();

    // Build a map of tool_use_id → tool result content.
    let tool_results: HashMap<&str, &str> = events
        .iter()
        .filter(|e| e.event_type == TranscriptEventType::ToolResult)
        .filter_map(|e| {
            let output = e.tool_output.as_deref()?;
            Some((e.uuid.as_str(), output))
        })
        .collect();

    let mut matched = Vec::new();

    for (tool_use_id, event) in &tool_uses {
        let tool_name = match &event.tool_name {
            Some(name) => name,
            None => continue,
        };

        let is_clip = is_cliproot_tool(tool_name, CLIP_TOOL_PATTERNS);
        let is_derive = is_cliproot_tool(tool_name, DERIVE_TOOL_PATTERNS);

        if !is_clip && !is_derive {
            continue;
        }

        // Try to extract clip hash from the tool result
        let result_text = match tool_results.get(tool_use_id) {
            Some(text) => *text,
            None => continue,
        };

        let clip_hash = match extract_clip_hash(result_text) {
            Some(hash) => hash,
            None => continue,
        };

        // Look up the clip in the repository
        if let Some(clip) = repo.get_clip(&clip_hash)? {
            matched.push(MatchedClip {
                clip_hash,
                clip,
                tool_use_uuid: tool_use_id.to_string(),
                is_derived: is_derive,
            });
        }
    }

    Ok(matched)
}

/// Check if a tool name matches cliproot patterns.
/// Handles MCP prefixed names like `mcp__cliproot__clip` and direct names.
fn is_cliproot_tool(tool_name: &str, patterns: &[&str]) -> bool {
    // Direct match
    if patterns.contains(&tool_name) {
        return true;
    }
    // MCP-prefixed match: mcp__<server>__<tool>
    // Match the last segment against our patterns
    if let Some(last_segment) = tool_name.rsplit("__").next() {
        if patterns.contains(&last_segment) {
            // Verify it's actually a cliproot server
            return tool_name.contains("cliproot");
        }
    }
    false
}

/// Extract a sha256-* clip hash from a tool result string.
fn extract_clip_hash(result: &str) -> Option<String> {
    // Look for sha256-* pattern in the result text
    for word in result.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
        if cleaned.starts_with("sha256-") && cleaned.len() >= 50 {
            return Some(cleaned.to_string());
        }
    }
    // Also try JSON parsing — the result might be structured
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(result) {
        if let Some(hash) = json.get("clipHash").and_then(|v| v.as_str()) {
            if hash.starts_with("sha256-") {
                return Some(hash.to_string());
            }
        }
        if let Some(hash) = json.get("clip_hash").and_then(|v| v.as_str()) {
            if hash.starts_with("sha256-") {
                return Some(hash.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_mcp_clip_tool() {
        assert!(is_cliproot_tool("mcp__cliproot__clip", CLIP_TOOL_PATTERNS));
        assert!(is_cliproot_tool(
            "mcp__cliproot__derive",
            DERIVE_TOOL_PATTERNS
        ));
        assert!(!is_cliproot_tool(
            "mcp__cliproot__clip",
            DERIVE_TOOL_PATTERNS
        ));
        assert!(!is_cliproot_tool("mcp__other__clip", CLIP_TOOL_PATTERNS));
    }

    #[test]
    fn detects_direct_tool_name() {
        assert!(is_cliproot_tool("clip", CLIP_TOOL_PATTERNS));
        assert!(is_cliproot_tool("cliproot_clip", CLIP_TOOL_PATTERNS));
        assert!(is_cliproot_tool("derive", DERIVE_TOOL_PATTERNS));
    }

    #[test]
    fn extracts_hash_from_text() {
        let result = "Clip created: sha256-abc123def456ghi789jkl012mno345pqr678stu901vwx234y";
        let hash = extract_clip_hash(result).unwrap();
        assert!(hash.starts_with("sha256-"));
    }

    #[test]
    fn extracts_hash_from_json() {
        let result = r#"{"clipHash": "sha256-abc123def456ghi789jkl012mno345pqr678stu901vwx234y"}"#;
        let hash = extract_clip_hash(result).unwrap();
        assert_eq!(
            hash,
            "sha256-abc123def456ghi789jkl012mno345pqr678stu901vwx234y"
        );
    }

    #[test]
    fn no_hash_in_unrelated_text() {
        assert!(extract_clip_hash("File written successfully").is_none());
    }
}
