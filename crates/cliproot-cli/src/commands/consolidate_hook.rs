use std::fs;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use crate::commands::consolidate::{load_config, run_consolidation, ConsolidationConfig};
use crate::commands::harness::{
    emit_consolidation_block, emit_passthrough, parse_stop_input, Harness,
};
use crate::transcript::hook_log::{
    parse_hook_log, read_watermark, write_watermark, ConsolidationState,
};

/// Hook input from Claude Code Stop/PreCompact hooks.
/// Kept for backward compatibility; prefer harness-aware parsing via `parse_stop_input`.
#[derive(Deserialize)]
struct HookInput {
    #[allow(dead_code)]
    session_id: String,
    #[allow(dead_code)]
    cwd: String,
    #[allow(dead_code)]
    #[serde(default)]
    transcript_path: Option<String>,
    // Absorb unknown fields for forward-compatibility.
    #[serde(flatten)]
    _extra: serde_json::Value,
}

pub fn run(harness: Harness, emergency: bool) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Read stdin
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    // 2. Parse hook input using harness-aware dispatch
    let hook = parse_stop_input(harness, &input)?;

    // 3. Sanitize session_id
    if hook.session_id.is_empty()
        || hook.session_id.contains('/')
        || hook.session_id.contains('\\')
        || hook.session_id.contains("..")
        || hook.session_id.contains('\0')
    {
        return Err(format!("invalid session_id: {}", hook.session_id).into());
    }

    // 4. Discover .cliproot/
    let cliproot_dir = discover_cliproot_dir(&hook.cwd)?;
    let config = load_config(&cliproot_dir);

    // Optional: Handle Cursor preCompact observational signal
    // We drop a marker file that tells the next Stop hook to run in emergency mode
    if harness == Harness::Cursor && emergency {
        let marker_path = cliproot_dir
            .join("agent-log")
            .join(format!("precompact-hinted-{}", hook.session_id));
        let _ = fs::write(&marker_path, "");
    }

    // 5. Check adaptive interval (unless emergency)
    if !emergency {
        let watermark = read_watermark(&cliproot_dir, &hook.session_id);

        // Check if we have a preCompact hint marker (Cursor hint)
        let marker_path = cliproot_dir
            .join("agent-log")
            .join(format!("precompact-hinted-{}", hook.session_id));
        let was_precompact_hinted = marker_path.exists();
        if was_precompact_hinted {
            // Remove the marker after recognizing it
            let _ = fs::remove_file(&marker_path);
        }

        // Count human messages in transcript
        let message_count = match &hook.transcript_path {
            Some(path) => count_human_messages(path),
            None => 0,
        };

        let effective_interval =
            compute_effective_interval(&cliproot_dir, &hook.session_id, &config);

        let messages_since = message_count.saturating_sub(watermark.message_count);

        // If preCompact hinted, tighten interval
        let effective_interval = if was_precompact_hinted {
            effective_interval / 2
        } else {
            effective_interval
        };

        if messages_since < effective_interval {
            // Not at threshold — passthrough
            println!("{}", emit_passthrough(harness));
            return Ok(());
        }
    }

    // 6. Run consolidation
    let result = run_consolidation(&cliproot_dir, &hook.session_id, emergency, false)?;

    if result.has_candidates() {
        // Update watermark with current message count
        let message_count = match &hook.transcript_path {
            Some(path) => count_human_messages(path),
            None => 0,
        };
        let log_path = cliproot_dir
            .join("agent-log")
            .join(format!("{}.jsonl", hook.session_id));
        let total_lines = match fs::read_to_string(&log_path) {
            Ok(content) => content.lines().filter(|l| !l.trim().is_empty()).count(),
            Err(_) => 0,
        };

        let new_state = ConsolidationState {
            line_count: total_lines,
            message_count,
            last_consolidation_ts: Some(
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            ),
        };
        write_watermark(&cliproot_dir, &hook.session_id, &new_state)?;

        // Emergency: also write artifact
        if emergency {
            let content = serde_json::to_string_pretty(&result)?;
            let project_root = cliproot_dir
                .parent()
                .ok_or("cannot determine project root")?;
            let repo = cliproot_store::Repository::open(project_root)?;
            let file_name = format!("consolidation-candidates-{}.json", hook.session_id);
            repo.add_artifact(
                None,
                Some(content.as_bytes()),
                Some(&file_name),
                cliproot_core::model::ArtifactType::Json,
                Some("application/json"),
                None,
                None,
                Some(serde_json::json!({
                    "artifact_type": "consolidation_candidates",
                    "session_id": hook.session_id,
                })),
            )?;
        }

        // Output harness-appropriate blocking response
        let reason = result.format_block_reason();
        println!("{}", emit_consolidation_block(harness, &reason));
    } else {
        // No candidates — passthrough
        println!("{}", emit_passthrough(harness));
    }

    Ok(())
}

/// Count human messages in a transcript JSONL file (lightweight scan).
fn count_human_messages(transcript_path: &str) -> usize {
    let path = Path::new(transcript_path);
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    content
        .lines()
        .filter(|line| {
            // Quick check: line must contain "user" role to be a human message
            if !line.contains("\"role\"") {
                return false;
            }
            // Parse and check role field
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(msg) = val.get("message") {
                    return msg.get("role").and_then(|r| r.as_str()) == Some("user");
                }
            }
            false
        })
        .count()
}

/// Compute the effective consolidation interval based on source activity density.
fn compute_effective_interval(
    cliproot_dir: &Path,
    session_id: &str,
    config: &ConsolidationConfig,
) -> usize {
    if !config.adaptive {
        return config.base_interval;
    }

    let watermark = read_watermark(cliproot_dir, session_id);
    let log_path = cliproot_dir
        .join("agent-log")
        .join(format!("{session_id}.jsonl"));

    let entries = match parse_hook_log(&log_path) {
        Ok(e) => e,
        Err(_) => return config.base_interval,
    };

    // Count source events since watermark
    let source_events = entries
        .iter()
        .skip(watermark.line_count)
        .filter(|e| {
            e.tool_name == "WebFetch"
                || (e.tool_name == "Read"
                    && e.tool_input
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .map(|p| !p.contains("/target/") && !p.contains("/node_modules/"))
                        .unwrap_or(false))
        })
        .count();

    if source_events >= 8 {
        config.min_interval
    } else if source_events >= 4 {
        config.base_interval
    } else {
        config.max_interval
    }
}

fn discover_cliproot_dir(cwd: &str) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let mut dir = std::path::PathBuf::from(cwd);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_human_messages_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transcript.jsonl");
        fs::write(&path, "").unwrap();
        assert_eq!(count_human_messages(path.to_str().unwrap()), 0);
    }

    #[test]
    fn count_human_messages_mixed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let lines = [
            r#"{"uuid":"1","timestamp":"2026-04-11T14:00:00Z","message":{"role":"user","content":[{"type":"text","text":"hello"}]}}"#,
            r#"{"uuid":"2","timestamp":"2026-04-11T14:01:00Z","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
            r#"{"uuid":"3","timestamp":"2026-04-11T14:02:00Z","message":{"role":"user","content":[{"type":"text","text":"question"}]}}"#,
        ];
        fs::write(&path, lines.join("\n")).unwrap();
        assert_eq!(count_human_messages(path.to_str().unwrap()), 2);
    }

    #[test]
    fn count_missing_transcript() {
        assert_eq!(count_human_messages("/nonexistent/path.jsonl"), 0);
    }

    #[test]
    fn effective_interval_defaults() {
        let config = ConsolidationConfig::default();
        let dir = tempfile::tempdir().unwrap();
        let cliproot = dir.path();
        fs::create_dir_all(cliproot.join("agent-log")).unwrap();
        // No log file -> falls back to base_interval
        let interval = compute_effective_interval(cliproot, "nonexistent", &config);
        assert_eq!(interval, config.base_interval);
    }

    #[test]
    fn effective_interval_non_adaptive() {
        let config = ConsolidationConfig {
            adaptive: false,
            base_interval: 12,
            ..Default::default()
        };
        let dir = tempfile::tempdir().unwrap();
        let interval = compute_effective_interval(dir.path(), "test", &config);
        assert_eq!(interval, 12);
    }
}
