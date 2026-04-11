use std::collections::HashSet;

use chrono::{DateTime, Utc};

use super::clip_matcher::MatchedClip;
use super::hook_log::HookEnrichment;
use super::parser::{TranscriptEvent, TranscriptEventType};

/// An inferred activity from one "turn" (user prompt → next user prompt) in the transcript.
#[derive(Debug, Clone)]
pub struct InferredActivity {
    /// The user's prompt text for this turn.
    pub prompt: String,
    /// Inferred activity type string (research, derive, plan, implement, review).
    pub activity_type: String,
    /// Clip hashes produced in this turn (source clips).
    pub source_clip_hashes: Vec<String>,
    /// Clip hashes produced in this turn (derived clips).
    pub derived_clip_hashes: Vec<String>,
    /// Tool calls made in this turn (tool_name, uuid).
    pub tool_calls: Vec<(String, String)>,
    /// Assistant reasoning text (first few sentences).
    pub reasoning_summary: Option<String>,
    /// URLs fetched in this turn (from hook enrichment).
    pub urls_fetched: Vec<String>,
    /// Files read in this turn.
    pub files_read: Vec<String>,
    /// Files modified in this turn.
    pub files_modified: Vec<String>,
    /// Subagent IDs active in this turn.
    pub subagent_ids: Vec<String>,
    /// Start timestamp.
    pub started_at: DateTime<Utc>,
    /// End timestamp.
    #[allow(dead_code)]
    pub ended_at: DateTime<Utc>,
}

/// Segment the transcript into turns and infer activities.
pub fn infer_activities(
    events: &[TranscriptEvent],
    matched_clips: &[MatchedClip],
    enrichment: Option<&HookEnrichment>,
) -> Vec<InferredActivity> {
    let turns = segment_into_turns(events);
    let clip_by_uuid: std::collections::HashMap<&str, &MatchedClip> = matched_clips
        .iter()
        .map(|mc| (mc.tool_use_uuid.as_str(), mc))
        .collect();

    turns
        .into_iter()
        .map(|turn| build_activity(turn, &clip_by_uuid, enrichment))
        .collect()
}

/// A raw turn: everything between one user prompt and the next.
struct Turn<'a> {
    prompt: String,
    events: Vec<&'a TranscriptEvent>,
    started_at: DateTime<Utc>,
    ended_at: DateTime<Utc>,
}

fn segment_into_turns(events: &[TranscriptEvent]) -> Vec<Turn<'_>> {
    let mut turns = Vec::new();
    let mut current_prompt: Option<String> = None;
    let mut current_events: Vec<&TranscriptEvent> = Vec::new();
    let mut turn_start: Option<DateTime<Utc>> = None;

    for event in events {
        if event.event_type == TranscriptEventType::UserMessage && event.agent_id.is_none() {
            // Flush the previous turn
            if let Some(prompt) = current_prompt.take() {
                let started = turn_start.unwrap_or_else(Utc::now);
                let ended = current_events
                    .last()
                    .map(|e| e.timestamp)
                    .unwrap_or(started);
                turns.push(Turn {
                    prompt,
                    events: std::mem::take(&mut current_events),
                    started_at: started,
                    ended_at: ended,
                });
            }
            current_prompt = event.message_text.clone();
            turn_start = Some(event.timestamp);
            current_events.clear();
        } else {
            current_events.push(event);
        }
    }

    // Flush last turn
    if let Some(prompt) = current_prompt {
        let started = turn_start.unwrap_or_else(Utc::now);
        let ended = current_events
            .last()
            .map(|e| e.timestamp)
            .unwrap_or(started);
        turns.push(Turn {
            prompt,
            events: current_events,
            started_at: started,
            ended_at: ended,
        });
    }

    turns
}

fn build_activity<'a>(
    turn: Turn<'a>,
    clip_by_uuid: &std::collections::HashMap<&str, &MatchedClip>,
    enrichment: Option<&HookEnrichment>,
) -> InferredActivity {
    let mut source_clip_hashes = Vec::new();
    let mut derived_clip_hashes = Vec::new();
    let mut tool_calls = Vec::new();
    let mut reasoning_parts = Vec::new();
    let mut subagent_ids = HashSet::new();
    let mut urls_fetched = Vec::new();
    let mut files_read = Vec::new();
    let mut files_modified = Vec::new();

    for event in &turn.events {
        if let Some(ref agent_id) = event.agent_id {
            subagent_ids.insert(agent_id.clone());
        }

        match event.event_type {
            TranscriptEventType::ToolUse => {
                if let Some(ref tool_name) = event.tool_name {
                    tool_calls.push((tool_name.clone(), event.uuid.clone()));

                    // Check if this tool call produced a matched clip
                    if let Some(mc) = clip_by_uuid.get(event.uuid.as_str()) {
                        if mc.is_derived {
                            derived_clip_hashes.push(mc.clip_hash.clone());
                        } else {
                            source_clip_hashes.push(mc.clip_hash.clone());
                        }
                    }

                    // Extract file/URL info from tool input if no hook enrichment
                    if enrichment.is_none() {
                        extract_tool_metadata(
                            tool_name,
                            event.tool_input.as_ref(),
                            &mut urls_fetched,
                            &mut files_read,
                            &mut files_modified,
                        );
                    }
                }
            }
            TranscriptEventType::AssistantMessage => {
                if event.agent_id.is_none() {
                    if let Some(ref text) = event.message_text {
                        reasoning_parts.push(text.clone());
                    }
                }
            }
            _ => {}
        }
    }

    // If we have hook enrichment, use it for file/URL data (more complete)
    if let Some(enrichment) = enrichment {
        for event in &turn.events {
            if event.event_type == TranscriptEventType::ToolUse {
                if let Some(hook_entry) = enrichment.entries_by_tool_use_id.get(event.uuid.as_str())
                {
                    match hook_entry.tool_name.as_str() {
                        "WebFetch" => {
                            if let Some(url) =
                                hook_entry.tool_input.get("url").and_then(|v| v.as_str())
                            {
                                urls_fetched.push(url.to_string());
                            }
                        }
                        "Read" => {
                            if let Some(path) = hook_entry
                                .tool_input
                                .get("file_path")
                                .and_then(|v| v.as_str())
                            {
                                files_read.push(path.to_string());
                            }
                        }
                        "Write" | "Edit" => {
                            if let Some(path) = hook_entry
                                .tool_input
                                .get("file_path")
                                .and_then(|v| v.as_str())
                            {
                                files_modified.push(path.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // Deduplicate
    dedup(&mut urls_fetched);
    dedup(&mut files_read);
    dedup(&mut files_modified);

    let reasoning_summary = summarize_reasoning(&reasoning_parts);
    let activity_type = infer_activity_type(
        &source_clip_hashes,
        &derived_clip_hashes,
        &tool_calls,
        &files_modified,
    );

    InferredActivity {
        prompt: turn.prompt,
        activity_type,
        source_clip_hashes,
        derived_clip_hashes,
        tool_calls,
        reasoning_summary,
        urls_fetched,
        files_read,
        files_modified,
        subagent_ids: subagent_ids.into_iter().collect(),
        started_at: turn.started_at,
        ended_at: turn.ended_at,
    }
}

fn extract_tool_metadata(
    tool_name: &str,
    tool_input: Option<&serde_json::Value>,
    urls: &mut Vec<String>,
    reads: &mut Vec<String>,
    writes: &mut Vec<String>,
) {
    let Some(input) = tool_input else { return };
    match tool_name {
        "WebFetch" => {
            if let Some(url) = input.get("url").and_then(|v| v.as_str()) {
                urls.push(url.to_string());
            }
        }
        "Read" => {
            if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                reads.push(path.to_string());
            }
        }
        "Write" | "Edit" => {
            if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                writes.push(path.to_string());
            }
        }
        _ => {}
    }
}

fn infer_activity_type(
    source_clips: &[String],
    derived_clips: &[String],
    tool_calls: &[(String, String)],
    files_modified: &[String],
) -> String {
    if !derived_clips.is_empty() {
        return "derive".to_string();
    }
    if !source_clips.is_empty() {
        return "research".to_string();
    }
    // Look at tool patterns
    let has_writes = tool_calls
        .iter()
        .any(|(name, _)| name == "Write" || name == "Edit");
    let has_reads = tool_calls
        .iter()
        .any(|(name, _)| name == "Read" || name.contains("Glob") || name.contains("Grep"));

    if has_writes || !files_modified.is_empty() {
        return "edit".to_string();
    }
    if has_reads {
        return "review".to_string();
    }
    "research".to_string()
}

/// Summarize assistant reasoning: take the first 2-3 sentences.
fn summarize_reasoning(parts: &[String]) -> Option<String> {
    if parts.is_empty() {
        return None;
    }
    let combined = parts.join(" ");
    let sentences: Vec<&str> = combined
        .split(['.', '!', '?'])
        .filter(|s| !s.trim().is_empty())
        .take(3)
        .collect();
    if sentences.is_empty() {
        return None;
    }
    let summary = sentences
        .iter()
        .map(|s| s.trim())
        .collect::<Vec<_>>()
        .join(". ");
    Some(format!("{summary}."))
}

fn dedup(v: &mut Vec<String>) {
    let mut seen = HashSet::new();
    v.retain(|item| seen.insert(item.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::parser::TranscriptEventType;

    fn make_event(
        event_type: TranscriptEventType,
        ts_offset_secs: i64,
        text: Option<&str>,
        tool_name: Option<&str>,
        agent_id: Option<&str>,
    ) -> TranscriptEvent {
        let base = DateTime::parse_from_rfc3339("2026-04-01T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        TranscriptEvent {
            timestamp: base + chrono::Duration::seconds(ts_offset_secs),
            event_type,
            agent_id: agent_id.map(|s| s.to_string()),
            tool_name: tool_name.map(|s| s.to_string()),
            tool_input: None,
            tool_output: None,
            message_text: text.map(|s| s.to_string()),
            uuid: format!("uuid-{ts_offset_secs}"),
            parent_uuid: None,
        }
    }

    #[test]
    fn segments_into_turns() {
        let events = vec![
            make_event(
                TranscriptEventType::UserMessage,
                0,
                Some("first"),
                None,
                None,
            ),
            make_event(
                TranscriptEventType::AssistantMessage,
                1,
                Some("response 1"),
                None,
                None,
            ),
            make_event(
                TranscriptEventType::UserMessage,
                5,
                Some("second"),
                None,
                None,
            ),
            make_event(
                TranscriptEventType::AssistantMessage,
                6,
                Some("response 2"),
                None,
                None,
            ),
        ];

        let turns = segment_into_turns(&events);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].prompt, "first");
        assert_eq!(turns[1].prompt, "second");
    }

    #[test]
    fn infers_research_type() {
        assert_eq!(
            infer_activity_type(&["hash1".into()], &[], &[], &[]),
            "research"
        );
    }

    #[test]
    fn infers_derive_type() {
        assert_eq!(
            infer_activity_type(&[], &["hash1".into()], &[], &[]),
            "derive"
        );
    }

    #[test]
    fn infers_edit_type() {
        assert_eq!(
            infer_activity_type(
                &[],
                &[],
                &[("Write".into(), "id".into())],
                &["/foo.rs".into()]
            ),
            "edit"
        );
    }

    #[test]
    fn summarizes_reasoning() {
        let parts = vec!["First sentence. Second sentence. Third sentence. Fourth.".to_string()];
        let summary = summarize_reasoning(&parts).unwrap();
        assert!(summary.contains("First sentence"));
        assert!(summary.contains("Third sentence"));
        assert!(!summary.contains("Fourth"));
    }
}
