use std::fs;
use std::path::Path;

use cliproot_core::model::{
    ActivityType, ArtifactType, ClipArtifactRelationship, CrpBundle, CrpId,
};
use cliproot_store::Repository;

use super::article;
use super::llm;
use super::state::{self, FlushState};

// ── Outcome type ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum FlushOutcome {
    Success {
        digest_path: String,
        tokens_used: u64,
    },
    Skipped(String),
    BudgetExceeded(String),
    Error(String),
}

impl std::fmt::Display for FlushOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success {
                digest_path,
                tokens_used,
            } => {
                write!(f, "SUCCESS digest={digest_path} tokens={tokens_used}")
            }
            Self::Skipped(reason) => write!(f, "SKIPPED {reason}"),
            Self::BudgetExceeded(reason) => write!(f, "BUDGET_EXCEEDED {reason}"),
            Self::Error(reason) => write!(f, "ERROR {reason}"),
        }
    }
}

// ── Constants ─────────────────────────────────────────────────────────────────

const MAX_INPUT_LINES: usize = 200;
const MAX_INPUT_CHARS: usize = 8_192;
/// Rough upper-bound token estimate for budget pre-check (system + user + output).
const ESTIMATED_TOKENS_PER_FLUSH: u64 = 4_000;
const MAX_OUTPUT_TOKENS: u32 = 2_048;

// ── Entry point ───────────────────────────────────────────────────────────────

/// Run the daily flush: synthesise a digest from today's agent-log entries,
/// write it to `.cliproot/knowledge/daily/YYYY-MM-DD.md`, and record
/// provenance in the CRP repository.
pub fn run_flush(cliproot_dir: &Path, repo: &Repository) -> FlushOutcome {
    match run_flush_inner(cliproot_dir, repo) {
        Ok(outcome) => outcome,
        Err(e) => FlushOutcome::Error(e.to_string()),
    }
}

fn run_flush_inner(
    cliproot_dir: &Path,
    repo: &Repository,
) -> Result<FlushOutcome, Box<dyn std::error::Error>> {
    let knowledge_dir = cliproot_dir.join("knowledge");
    let log_dir = cliproot_dir.join("agent-log");

    // ── Load config and state ──────────────────────────────────────────────
    let cfg = repo.knowledge_config()?;
    let mut flush_state = state::load(&knowledge_dir)?;
    state::reset_budget_if_new_day(&mut flush_state);

    // ── Hash-gate: skip if no new lines ───────────────────────────────────
    let new_line_count = state::total_new_lines(&flush_state, &log_dir)?;
    if new_line_count == 0 {
        return Ok(FlushOutcome::Skipped("no new log entries".to_string()));
    }

    // ── Budget pre-check ──────────────────────────────────────────────────
    if let Err(e) = llm::check_budget(&flush_state, &cfg, ESTIMATED_TOKENS_PER_FLUSH) {
        append_log_line(&knowledge_dir, &format!("BUDGET_EXCEEDED {}", e.reason));
        return Ok(FlushOutcome::BudgetExceeded(e.reason));
    }

    // ── Collect new JSONL lines ────────────────────────────────────────────
    let (new_lines, clip_hashes) = collect_new_lines(&log_dir, &flush_state);

    // ── Build LLM prompt ──────────────────────────────────────────────────
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let system = flush_system_prompt();
    let user = build_user_prompt(&today, &new_lines);

    // ── Call Anthropic API ────────────────────────────────────────────────
    let model = &cfg.models.flush;
    let result = llm::call(&system, &user, model, MAX_OUTPUT_TOKENS)?;

    // ── Write daily digest ────────────────────────────────────────────────
    let digest_path = article::write_daily_digest(&knowledge_dir, &today, &result.text, None)?;

    // ── Register artifact in CRP repo ─────────────────────────────────────
    let file_name = format!("{today}.md");
    let artifact = repo.add_artifact(
        Some(&digest_path),
        None,
        Some(&file_name),
        ArtifactType::Markdown,
        Some("text/markdown"),
        None,
        None,
        Some(serde_json::json!({
            "artifactType": "daily-digest",
            "date": today,
            "model": model,
        })),
    )?;
    let artifact_hash = artifact.artifact_hash.0.clone();

    // ── Link clips to artifact (GeneratedFrom) ────────────────────────────
    let mut linked_clip_hashes: Vec<String> = Vec::new();
    for clip_hash in &clip_hashes {
        if repo
            .link_clip_artifact(
                clip_hash,
                &artifact_hash,
                ClipArtifactRelationship::GeneratedFrom,
            )
            .is_ok()
        {
            linked_clip_hashes.push(clip_hash.clone());
        } // else: clip not in repo — skip silently
    }

    // ── Record LLM call as a CRP Activity ────────────────────────────────
    record_flush_activity(repo, &result, &linked_clip_hashes)?;

    // ── Update flush state ────────────────────────────────────────────────
    update_state_watermarks(&mut flush_state, &log_dir);
    flush_state.daily_total_tokens += result.total_tokens;
    flush_state.daily_total_cost_usd += result.estimated_cost_usd;
    state::save(&flush_state, &knowledge_dir)?;

    // ── Append log line ───────────────────────────────────────────────────
    append_log_line(
        &knowledge_dir,
        &format!(
            "SUCCESS digest={} tokens={} cost=${:.4}",
            digest_path.display(),
            result.total_tokens,
            result.estimated_cost_usd,
        ),
    );

    Ok(FlushOutcome::Success {
        digest_path: digest_path.display().to_string(),
        tokens_used: result.total_tokens,
    })
}

// ── CRP activity recording ────────────────────────────────────────────────────

fn record_flush_activity(
    repo: &Repository,
    result: &llm::LlmCallResult,
    used_clip_hashes: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let activity = cliproot_core::model::Activity {
        id: CrpId(format!("act-{}", uuid::Uuid::new_v4())),
        activity_type: ActivityType::Derive,
        project_id: None,
        agent_id: None,
        prompt: None,
        parameters: Some(serde_json::json!({
            "promptHash": result.prompt_hash,
            "model": result.model,
            "maxTokens": MAX_OUTPUT_TOKENS,
            "inputTokens": result.input_tokens,
            "outputTokens": result.output_tokens,
        })),
        used_source_refs: used_clip_hashes.to_vec(),
        generated_clip_refs: Vec::new(),
        created_at: now.clone(),
        ended_at: Some(now),
    };

    let bundle = CrpBundle {
        protocol_version: "0.0.3".to_string(),
        bundle_type: cliproot_core::model::BundleType::Derivation,
        created_at: activity.created_at.clone(),
        project: None,
        document: None,
        agents: Vec::new(),
        sources: Vec::new(),
        clips: Vec::new(),
        artifacts: Vec::new(),
        clip_artifact_refs: Vec::new(),
        activities: vec![activity],
        edges: Vec::new(),
        reuse_events: Vec::new(),
        signatures: Vec::new(),
        registry: None,
    };

    repo.store_bundle(&bundle)?;
    Ok(())
}

// ── Log input collection ──────────────────────────────────────────────────────

/// Collect new JSONL log lines (since watermarks) across all agent-log files.
/// Returns `(lines_for_prompt, clip_hashes_seen)`.
fn collect_new_lines(log_dir: &Path, flush_state: &FlushState) -> (Vec<String>, Vec<String>) {
    let mut all_new: Vec<String> = Vec::new();
    let mut clip_hashes: Vec<String> = Vec::new();

    let Ok(entries) = fs::read_dir(log_dir) else {
        return (all_new, clip_hashes);
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(ext) = path.extension() else {
            continue;
        };
        if ext != "jsonl" {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.starts_with("watermark-") || stem.starts_with("precompact-hinted-") {
            continue;
        }

        let watermark = flush_state
            .last_flushed_line_counts
            .get(stem)
            .copied()
            .unwrap_or(0);

        let content = fs::read_to_string(&path).unwrap_or_default();
        let new_lines: Vec<&str> = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .skip(watermark as usize)
            .collect();

        for line in &new_lines {
            // Extract any sha256- hashes from the line for clip linking
            extract_clip_hashes_from_line(line, &mut clip_hashes);
            all_new.push(line.to_string());
        }
    }

    // Deduplicate clip hashes
    clip_hashes.sort();
    clip_hashes.dedup();

    // Cap at MAX_INPUT_LINES and MAX_INPUT_CHARS
    let mut capped: Vec<String> = Vec::new();
    let mut total_chars: usize = 0;
    for line in all_new {
        if capped.len() >= MAX_INPUT_LINES {
            break;
        }
        total_chars += line.len() + 1;
        if total_chars > MAX_INPUT_CHARS && !capped.is_empty() {
            break;
        }
        capped.push(line);
    }

    (capped, clip_hashes)
}

/// Scan a JSONL line for `sha256-<base64url>` patterns (clip hashes).
fn extract_clip_hashes_from_line(line: &str, out: &mut Vec<String>) {
    let mut rest = line;
    while let Some(idx) = rest.find("sha256-") {
        rest = &rest[idx..];
        // Collect alphanumeric + base64url chars after "sha256-"
        let hash_part: String = rest["sha256-".len()..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if hash_part.len() >= 40 {
            out.push(format!("sha256-{hash_part}"));
        }
        rest = &rest["sha256-".len()..];
    }
}

// ── State update helpers ──────────────────────────────────────────────────────

/// Update watermark counts to the current line counts in the log directory.
fn update_state_watermarks(flush_state: &mut FlushState, log_dir: &Path) {
    let Ok(entries) = fs::read_dir(log_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(ext) = path.extension() else {
            continue;
        };
        if ext != "jsonl" {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.starts_with("watermark-") || stem.starts_with("precompact-hinted-") {
            continue;
        }
        let content = fs::read_to_string(&path).unwrap_or_default();
        let count = content.lines().filter(|l| !l.trim().is_empty()).count() as u64;
        flush_state
            .last_flushed_line_counts
            .insert(stem.to_string(), count);
    }
}

// ── Prompt templates ──────────────────────────────────────────────────────────

fn flush_system_prompt() -> String {
    "You are a knowledge curator for a software development session. \
Given the raw tool call log from today's AI-assisted coding work, write a concise daily digest.\n\n\
Format your response as Markdown with these sections:\n\
## Summary\n\
One paragraph: what was worked on today.\n\n\
## Key Decisions\n\
Bullet list of significant technical decisions made.\n\n\
## Sources Consulted\n\
Bullet list of URLs, files, or documents referenced (from WebFetch and Read calls).\n\n\
## Open Questions\n\
Any unresolved issues or threads to pick up next session.\n\n\
Be factual. Do not invent details not present in the log. Omit sections with no content."
        .to_string()
}

fn build_user_prompt(date: &str, lines: &[String]) -> String {
    let log_text = lines.join("\n");
    format!("Today's date: {date}\n\nSession log (tool calls):\n{log_text}")
}

// ── Log file helper ───────────────────────────────────────────────────────────
// (Public to the crate: `flush_hook::run_background_impl` uses this to surface
// errors from the detached Stop-hook child, whose stderr is /dev/null.)

pub(crate) fn append_log_line(knowledge_dir: &Path, message: &str) {
    let log_path = knowledge_dir.join("log.md");
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let line = format!("- `{timestamp}` {message}\n");
    let _ = fs::create_dir_all(knowledge_dir);
    // Append to existing log or create new
    let existing = fs::read_to_string(&log_path).unwrap_or_default();
    let _ = fs::write(&log_path, format!("{existing}{line}"));
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_clip_hashes_finds_sha256() {
        let line =
            r#"{"clip_hash":"sha256-abc123def456abc123def456abc123def456abc123d","tool":"Read"}"#;
        let mut out = Vec::new();
        extract_clip_hashes_from_line(line, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].starts_with("sha256-"));
    }

    #[test]
    fn extract_clip_hashes_ignores_short() {
        let line = r#"{"x":"sha256-tooshort"}"#;
        let mut out = Vec::new();
        extract_clip_hashes_from_line(line, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn collect_new_lines_caps_at_max() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("agent-log");
        fs::create_dir_all(&log_dir).unwrap();

        // Write 300 lines
        let content = (0..300)
            .map(|i| format!("{{\"i\":{i}}}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(log_dir.join("sess.jsonl"), content).unwrap();

        let state = FlushState::default();
        let (lines, _) = collect_new_lines(&log_dir, &state);
        assert!(lines.len() <= MAX_INPUT_LINES);
    }

    #[test]
    fn collect_new_lines_respects_watermark() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("agent-log");
        fs::create_dir_all(&log_dir).unwrap();

        fs::write(
            log_dir.join("sess.jsonl"),
            "{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n",
        )
        .unwrap();

        let mut state = FlushState::default();
        state.last_flushed_line_counts.insert("sess".to_string(), 2);

        let (lines, _) = collect_new_lines(&log_dir, &state);
        assert_eq!(lines.len(), 1); // only {"c":3}
    }

    #[test]
    fn flush_outcome_budget_exceeded_written_to_log() {
        let dir = tempfile::tempdir().unwrap();
        // init creates .cliproot/ for us
        let repo = cliproot_store::Repository::init(dir.path()).unwrap();
        let cliproot_dir = dir.path().join(".cliproot");

        // Write one log line so hash-gate passes
        let log_dir = cliproot_dir.join("agent-log");
        fs::create_dir_all(&log_dir).unwrap();
        fs::write(log_dir.join("s.jsonl"), "{\"tool\":\"Read\"}\n").unwrap();

        // Set budget to 0 via config
        let mut cfg = repo.knowledge_config().unwrap();
        cfg.max_bg_tokens_per_day = 0;
        repo.set_knowledge_config(cfg).unwrap();

        let outcome = run_flush(&cliproot_dir, &repo);
        assert!(matches!(outcome, FlushOutcome::BudgetExceeded(_)));

        // log.md should mention BUDGET_EXCEEDED
        let log = fs::read_to_string(cliproot_dir.join("knowledge/log.md")).unwrap();
        assert!(log.contains("BUDGET_EXCEEDED"));
    }

    #[test]
    fn flush_outcome_skipped_no_new_lines() {
        let dir = tempfile::tempdir().unwrap();
        let repo = cliproot_store::Repository::init(dir.path()).unwrap();
        let cliproot_dir = dir.path().join(".cliproot");
        // No log files → skipped
        let outcome = run_flush(&cliproot_dir, &repo);
        assert!(matches!(outcome, FlushOutcome::Skipped(_)));
    }
}
