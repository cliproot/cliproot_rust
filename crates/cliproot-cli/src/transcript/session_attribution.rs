use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use cliproot_core::{create_clip_hash, create_text_hash, model::*, ClipHashInput};
use cliproot_store::{PromptClipsMode, Repository, SessionAttributionConfig};

use super::parser::{extract_session_meta, parse_jsonl, TranscriptEventType};

// ── Outcome ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AttributionOutcome {
    Success {
        session_artifact_hash: String,
        prompt_sources_created: usize,
        prompt_clips_created: usize,
        edges_created: usize,
    },
    AlreadyRun,
    Skipped(String),
    Error(String),
}

impl std::fmt::Display for AttributionOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success {
                session_artifact_hash,
                prompt_sources_created,
                prompt_clips_created,
                edges_created,
            } => write!(
                f,
                "SUCCESS session={session_artifact_hash} prompts={prompt_sources_created} clips={prompt_clips_created} edges={edges_created}"
            ),
            Self::AlreadyRun => write!(f, "ALREADY_RUN"),
            Self::Skipped(reason) => write!(f, "SKIPPED {reason}"),
            Self::Error(reason) => write!(f, "ERROR {reason}"),
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run_session_attribution(
    _cliproot_dir: &Path,
    repo: &Repository,
    session_id: &str,
    transcript_path: &Path,
    config: &SessionAttributionConfig,
) -> AttributionOutcome {
    if !config.enabled {
        return AttributionOutcome::Skipped("disabled".to_string());
    }
    match run_inner(repo, session_id, transcript_path, config) {
        Ok(outcome) => outcome,
        Err(e) => AttributionOutcome::Error(e.to_string()),
    }
}

// ── Implementation ────────────────────────────────────────────────────────────

fn run_inner(
    repo: &Repository,
    session_id: &str,
    transcript_path: &Path,
    config: &SessionAttributionConfig,
) -> Result<AttributionOutcome, Box<dyn std::error::Error>> {
    // ── §5.1 step 1: confirm transcript exists and is non-empty ───────────────
    let metadata = fs::metadata(transcript_path)?;
    if metadata.len() == 0 {
        return Ok(AttributionOutcome::Skipped("empty transcript".to_string()));
    }

    // ── §5.1 step 2-3: register transcript as artifact ────────────────────────
    let artifact = repo.add_artifact(
        Some(transcript_path),
        None,
        Some(&format!("{session_id}.jsonl")),
        ArtifactType::Session,
        Some("application/x-ndjson"),
        None,
        None,
        Some(serde_json::json!({
            "artifactType": "session-transcript",
            "sessionId": session_id,
        })),
    )?;
    let session_artifact_hash = artifact.artifact_hash.0.clone();

    // short_hash: strip "sha256-" prefix, take first 16 chars
    let short_hash = session_artifact_hash
        .strip_prefix("sha256-")
        .unwrap_or(&session_artifact_hash)
        .chars()
        .take(16)
        .collect::<String>();

    // ── §5.1 step 4: idempotency check ────────────────────────────────────────
    let source_id_str = format!("src-session-{short_hash}");
    if repo.get_source_record(&source_id_str)?.is_some() {
        return Ok(AttributionOutcome::AlreadyRun);
    }

    // ── §5.1: parse transcript for session metadata and timestamps ────────────
    let events = parse_jsonl(transcript_path)?;
    let meta = extract_session_meta(transcript_path, &events)?;

    let started_at: Option<DateTime<Utc>> = meta.started_at;
    let ended_at: Option<DateTime<Utc>> = meta.ended_at;

    let iso_now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let short_session = short_session_id(session_id);

    // ── §5.1 step 4: resolve / create assistant agent ─────────────────────────
    let assistant_agent_id = CrpId("agent-claude-code".to_string());
    let assistant_agent = Agent {
        id: assistant_agent_id.clone(),
        agent_type: AgentType::Model,
        name: Some("claude-code".to_string()),
        uri: None,
    };

    // ── §5.1 step 4: build session SourceRecord ───────────────────────────────
    let title_date = started_at
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| iso_now.clone());

    let session_source = SourceRecord {
        id: CrpId(source_id_str.clone()),
        source_type: SourceType::AiAssisted,
        digital_source_type: Some("compositeWithTrainedAlgorithmicMedia".to_string()),
        title: Some(format!("Session {short_session} ({title_date})")),
        source_uri: Some(format!("cliproot://session/{session_artifact_hash}")),
        author_agent_id: Some(assistant_agent_id.clone()),
        created_at: Some(iso_now.clone()),
    };

    // ── §5.1 step 5: find clips produced in this session's time window ────────
    let all_clips_with_ts = repo.list_clips_with_created_at()?;
    let session_clips: Vec<(Clip, DateTime<Utc>)> = all_clips_with_ts
        .into_iter()
        .filter_map(|(clip, ts)| {
            let ts = ts?;
            let in_window =
                started_at.map_or(true, |s| ts >= s) && ended_at.map_or(true, |e| ts <= e);
            if in_window {
                Some((clip, ts))
            } else {
                None
            }
        })
        .collect();

    // ── §5.1 step 6: build WasGeneratedBy edges ───────────────────────────────
    let was_generated_by_edges: Vec<Edge> = session_clips
        .iter()
        .map(|(clip, _)| Edge {
            id: CrpId(format!("edge-{}", uuid::Uuid::new_v4())),
            edge_type: EdgeType::WasGeneratedBy,
            subject_ref: CrpId(clip.clip_hash.0.clone()),
            object_ref: session_source.id.clone(),
            transformation_type: None,
            agent_id: Some(assistant_agent_id.clone()),
            // 1.0 = explicit (session produced this clip)
            confidence: Some(1.0),
            created_at: iso_now.clone(),
        })
        .collect();

    let edge_count_a = was_generated_by_edges.len();

    // ── §5.1 step 7: store session-level bundle ───────────────────────────────
    let bundle_a = CrpBundle {
        protocol_version: "0.0.3".to_string(),
        bundle_type: BundleType::ProvenanceExport,
        created_at: iso_now.clone(),
        project: None,
        document: None,
        agents: vec![assistant_agent.clone()],
        sources: vec![session_source.clone()],
        clips: Vec::new(),
        artifacts: Vec::new(),
        clip_artifact_refs: Vec::new(),
        activities: Vec::new(),
        edges: was_generated_by_edges,
        reuse_events: Vec::new(),
        signatures: Vec::new(),
        registry: None,
    };
    repo.store_bundle(&bundle_a)?;

    // ── §5.2: per-turn attribution ────────────────────────────────────────────
    let (prompt_sources_created, prompt_clips_created, edge_count_b) = run_prompt_attribution(
        repo,
        session_id,
        transcript_path,
        &session_artifact_hash,
        &session_clips,
        &assistant_agent_id,
        &assistant_agent,
        &iso_now,
        config,
    )?;

    Ok(AttributionOutcome::Success {
        session_artifact_hash,
        prompt_sources_created,
        prompt_clips_created,
        edges_created: edge_count_a + edge_count_b,
    })
}

// ── §5.2 prompt attribution ───────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn run_prompt_attribution(
    repo: &Repository,
    session_id: &str,
    transcript_path: &Path,
    session_artifact_hash: &str,
    session_clips: &[(Clip, DateTime<Utc>)],
    assistant_agent_id: &CrpId,
    assistant_agent: &Agent,
    iso_now: &str,
    config: &SessionAttributionConfig,
) -> Result<(usize, usize, usize), Box<dyn std::error::Error>> {
    // Build line-number index: uuid → 1-based line number in JSONL.
    // §4.4: use JSONL line index for stable turn anchors.
    let raw_content = fs::read_to_string(transcript_path)?;
    let uuid_to_line = build_uuid_line_index(&raw_content);

    let events = parse_jsonl(transcript_path)?;
    let session_short = short_session_id(session_id);

    // Collect eligible user-prompt turns (§4.1).
    struct PromptTurn {
        text: String,
        timestamp: DateTime<Utc>,
        line_n: usize,
    }

    let mut turns: Vec<PromptTurn> = Vec::new();
    for event in &events {
        if event.event_type != TranscriptEventType::UserMessage {
            continue;
        }
        if event.agent_id.is_some() {
            // subagent — skip
            continue;
        }
        let Some(ref text) = event.message_text else {
            continue;
        };
        let text = text.trim().to_string();
        if text.len() < config.prompt_min_chars {
            continue;
        }
        // §4.1 step 3: skip harness-injected system patterns.
        if text.starts_with("<system-reminder>")
            || text.starts_with("<command-name>")
            || text.starts_with("<local-command-stdout>")
        {
            continue;
        }
        let line_n = uuid_to_line.get(&event.uuid).copied().unwrap_or(0);
        turns.push(PromptTurn {
            text,
            timestamp: event.timestamp,
            line_n,
        });
    }

    if turns.is_empty() {
        return Ok((0, 0, 0));
    }

    // Resolve / create user agent.
    let user_name = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string());
    let user_agent_id = CrpId("agent-user-local".to_string());
    let user_agent = Agent {
        id: user_agent_id.clone(),
        agent_type: AgentType::Person,
        name: Some(user_name),
        uri: None,
    };

    let mut sources: Vec<SourceRecord> = Vec::new();
    let mut clips_out: Vec<Clip> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let new_agents: Vec<Agent> = vec![assistant_agent.clone(), user_agent.clone()];

    for (i, turn) in turns.iter().enumerate() {
        let prompt_hash_short = sha256_short_12(&turn.text);
        let source_id_str = format!("src-prompt-{session_short}-{prompt_hash_short}");

        // Idempotency: skip if this prompt SourceRecord already exists.
        if repo.get_source_record(&source_id_str)?.is_some() {
            continue;
        }

        let n = turn.line_n;
        let turn_ts = turn
            .timestamp
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let prompt_source = SourceRecord {
            id: CrpId(source_id_str.clone()),
            source_type: SourceType::HumanAuthored,
            digital_source_type: Some("humanEditsAndPrompts".to_string()),
            title: Some(format!("Prompt {n} — {session_short}")),
            // §4.4: use transcript line index as the fragment anchor for stability.
            source_uri: Some(format!(
                "cliproot://session/{session_artifact_hash}#turn-{n}"
            )),
            author_agent_id: Some(user_agent_id.clone()),
            created_at: Some(turn_ts.clone()),
        };
        sources.push(prompt_source.clone());

        // §4.3: optional prompt Clip.
        if config.prompt_clips != PromptClipsMode::Off {
            let text_hash = create_text_hash(&turn.text);
            let clip_hash = create_clip_hash(ClipHashInput {
                text_hash: text_hash.clone(),
                source_refs: vec![source_id_str.clone()],
                text_quote_exact: None,
            });
            let clip_id = CrpId(format!("clip-prompt-{session_short}-{n}"));
            let prompt_clip = Clip {
                clip_hash,
                id: Some(clip_id),
                project_id: None,
                document_id: None,
                source_refs: vec![source_id_str.clone()],
                selectors: None,
                content: Some(turn.text.clone()),
                text_hash,
                // TODO(§16): set tags = ["type-prompt"] once Clip.tags is added in CRP 0.0.4.
                created_by_activity_id: None,
            };
            clips_out.push(prompt_clip);
        }

        // §4.5: find clips in window [t_n, t_{n+1}).
        let window_end = turns.get(i + 1).map(|t| t.timestamp);
        for (clip, clip_ts) in session_clips {
            let in_window =
                *clip_ts >= turn.timestamp && window_end.map_or(true, |end| *clip_ts < end);
            if !in_window {
                continue;
            }
            edges.push(Edge {
                id: CrpId(format!("edge-{}", uuid::Uuid::new_v4())),
                edge_type: EdgeType::WasDerivedFrom,
                subject_ref: CrpId(clip.clip_hash.0.clone()),
                object_ref: CrpId(source_id_str.clone()),
                transformation_type: None,
                agent_id: Some(assistant_agent_id.clone()),
                // 0.5 = heuristic (time-window-based, not explicit causal link; see §5.2.1)
                confidence: Some(0.5),
                created_at: iso_now.to_string(),
            });
        }
    }

    let prompt_sources_created = sources.len();
    let prompt_clips_created = clips_out.len();
    let edge_count = edges.len();

    if prompt_sources_created == 0 && prompt_clips_created == 0 && edge_count == 0 {
        return Ok((0, 0, 0));
    }

    let bundle_b = CrpBundle {
        protocol_version: "0.0.3".to_string(),
        bundle_type: BundleType::ProvenanceExport,
        created_at: iso_now.to_string(),
        project: None,
        document: None,
        agents: new_agents,
        sources,
        clips: clips_out,
        artifacts: Vec::new(),
        clip_artifact_refs: Vec::new(),
        activities: Vec::new(),
        edges,
        reuse_events: Vec::new(),
        signatures: Vec::new(),
        registry: None,
    };
    repo.store_bundle(&bundle_b)?;

    Ok((prompt_sources_created, prompt_clips_created, edge_count))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a map from event UUID to 1-based JSONL line number.
fn build_uuid_line_index(raw: &str) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (idx, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(uuid) = val.get("uuid").and_then(|v| v.as_str()) {
                map.insert(uuid.to_string(), idx + 1);
            }
        }
    }
    map
}

fn short_session_id(id: &str) -> &str {
    if id.len() > 8 {
        &id[..8]
    } else {
        id
    }
}

/// First 12 hex chars of the SHA-256 of the text (for prompt source IDs).
fn sha256_short_12(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(text.as_bytes());
    digest[..6].iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn make_repo() -> (TempDir, Repository) {
        let dir = tempfile::tempdir().unwrap();
        let repo = cliproot_store::Repository::init(dir.path()).unwrap();
        (dir, repo)
    }

    fn default_config() -> SessionAttributionConfig {
        SessionAttributionConfig::default()
    }

    fn write_jsonl(dir: &Path, name: &str, lines: &[serde_json::Value]) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut f = fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(f, "{}", serde_json::to_string(line).unwrap()).unwrap();
        }
        path
    }

    fn small_transcript(dir: &Path) -> std::path::PathBuf {
        let t0 = "2026-04-01T10:00:00.000Z";
        let t1 = "2026-04-01T10:00:05.000Z";
        write_jsonl(
            dir,
            "session.jsonl",
            &[
                serde_json::json!({
                    "uuid": "u1",
                    "timestamp": t0,
                    "type": "user",
                    "sessionId": "test-session-id",
                    "message": {"role": "user", "content": "Implement the OAuth2 PKCE flow"}
                }),
                serde_json::json!({
                    "uuid": "a1",
                    "timestamp": t1,
                    "type": "assistant",
                    "message": {"role": "assistant", "content": "Sure, here is the implementation."}
                }),
            ],
        )
    }

    // ── §3.3 test 1 ───────────────────────────────────────────────────────────

    #[test]
    fn registers_session_artifact_with_session_type() {
        let (dir, repo) = make_repo();
        let tp = small_transcript(dir.path());
        let cfg = default_config();

        let outcome = run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "test-session-id",
            &tp,
            &cfg,
        );
        assert!(
            matches!(outcome, AttributionOutcome::Success { .. }),
            "unexpected outcome: {outcome}"
        );

        let artifacts = repo.list_artifacts(None).unwrap();
        let session_artifact = artifacts
            .iter()
            .find(|a| a.artifact_type == ArtifactType::Session);
        assert!(session_artifact.is_some(), "session artifact not found");
        let a = session_artifact.unwrap();
        assert!(
            a.file_name.ends_with(".jsonl"),
            "file_name should end with .jsonl, got {}",
            a.file_name
        );
    }

    // ── §3.3 test 2 ───────────────────────────────────────────────────────────

    #[test]
    fn creates_session_source_record_with_c2pa_type() {
        let (dir, repo) = make_repo();
        let tp = small_transcript(dir.path());
        let cfg = default_config();

        run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "test-session-id",
            &tp,
            &cfg,
        );

        // Derive the expected source id the same way the impl does.
        let artifact = repo.list_artifacts(None).unwrap();
        let a = artifact
            .iter()
            .find(|a| a.artifact_type == ArtifactType::Session)
            .unwrap();
        let short_hash: String = a
            .artifact_hash
            .0
            .strip_prefix("sha256-")
            .unwrap_or(&a.artifact_hash.0)
            .chars()
            .take(16)
            .collect();
        let source_id = format!("src-session-{short_hash}");

        let src = repo
            .get_source_record(&source_id)
            .unwrap()
            .expect("source record not found");
        assert_eq!(
            src.digital_source_type.as_deref(),
            Some("compositeWithTrainedAlgorithmicMedia"),
        );
        assert!(
            src.source_uri
                .as_deref()
                .unwrap_or("")
                .starts_with("cliproot://session/sha256-"),
            "unexpected source_uri: {:?}",
            src.source_uri
        );
    }

    // ── §3.3 test 3 ───────────────────────────────────────────────────────────

    #[test]
    fn links_clips_via_was_generated_by() {
        let (dir, repo) = make_repo();
        let tp = small_transcript(dir.path());
        let cfg = default_config();

        // Seed a clip with a created_at inside the session window.
        let text = "OAuth2 PKCE flow detail";
        let text_hash = create_text_hash(text);
        let src = SourceRecord {
            id: CrpId("src-test-001".to_string()),
            source_type: SourceType::ExternalQuoted,
            digital_source_type: None,
            title: None,
            source_uri: Some("https://example.com".to_string()),
            author_agent_id: None,
            created_at: None,
        };
        let clip_hash = create_clip_hash(ClipHashInput {
            text_hash: text_hash.clone(),
            source_refs: vec!["src-test-001".to_string()],
            text_quote_exact: None,
        });
        let clip = Clip {
            clip_hash: clip_hash.clone(),
            id: None,
            project_id: None,
            document_id: None,
            source_refs: vec!["src-test-001".to_string()],
            selectors: None,
            content: Some(text.to_string()),
            text_hash,
            created_by_activity_id: None,
        };
        let seed_bundle = CrpBundle {
            protocol_version: "0.0.3".to_string(),
            bundle_type: BundleType::ProvenanceExport,
            created_at: "2026-04-01T10:00:03.000Z".to_string(),
            project: None,
            document: None,
            agents: Vec::new(),
            sources: vec![src],
            clips: vec![clip],
            artifacts: Vec::new(),
            clip_artifact_refs: Vec::new(),
            activities: Vec::new(),
            edges: Vec::new(),
            reuse_events: Vec::new(),
            signatures: Vec::new(),
            registry: None,
        };
        repo.store_bundle(&seed_bundle).unwrap();

        run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "test-session-id",
            &tp,
            &cfg,
        );

        let edges = repo.get_edges_for_subject(&clip_hash.0).unwrap();
        let was_gen = edges
            .iter()
            .any(|e| e.edge_type == EdgeType::WasGeneratedBy);
        assert!(was_gen, "expected WasGeneratedBy edge on seeded clip");
    }

    // ── §3.3 test 4 ───────────────────────────────────────────────────────────

    #[test]
    fn idempotent_on_rerun() {
        let (dir, repo) = make_repo();
        let tp = small_transcript(dir.path());
        let cfg = default_config();

        run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "test-session-id",
            &tp,
            &cfg,
        );
        let artifacts_after_first = repo.list_artifacts(None).unwrap().len();

        let outcome2 = run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "test-session-id",
            &tp,
            &cfg,
        );
        assert!(
            matches!(outcome2, AttributionOutcome::AlreadyRun),
            "expected AlreadyRun on second call, got: {outcome2}"
        );
        let artifacts_after_second = repo.list_artifacts(None).unwrap().len();
        assert_eq!(
            artifacts_after_first, artifacts_after_second,
            "artifact count changed on second run"
        );
    }

    // ── §4.8 test 1 ───────────────────────────────────────────────────────────

    #[test]
    fn extracts_prompts_above_threshold() {
        let (dir, repo) = make_repo();
        let path = write_jsonl(
            dir.path(),
            "multi.jsonl",
            &[
                // 5 chars — below default threshold of 32
                serde_json::json!({
                    "uuid": "u1", "timestamp": "2026-04-01T10:00:00.000Z",
                    "type": "user", "sessionId": "sid",
                    "message": {"role": "user", "content": "short"}
                }),
                // 50 chars — above threshold
                serde_json::json!({
                    "uuid": "u2", "timestamp": "2026-04-01T10:01:00.000Z",
                    "type": "user", "sessionId": "sid",
                    "message": {"role": "user", "content": "Implement the full OAuth2 PKCE authorization flow here"}
                }),
                // 100 chars — above threshold
                serde_json::json!({
                    "uuid": "u3", "timestamp": "2026-04-01T10:02:00.000Z",
                    "type": "user", "sessionId": "sid",
                    "message": {"role": "user", "content": "Please add comprehensive unit tests covering edge cases for the token refresh mechanism in auth.rs"}
                }),
            ],
        );

        let cfg = default_config();
        run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "sid",
            &path,
            &cfg,
        );

        // Two prompts should have SourceRecords (not the 5-char one).
        let short_hash: String = {
            let artifacts = repo.list_artifacts(None).unwrap();
            let a = artifacts
                .iter()
                .find(|a| a.artifact_type == ArtifactType::Session)
                .unwrap();
            a.artifact_hash
                .0
                .strip_prefix("sha256-")
                .unwrap_or(&a.artifact_hash.0)
                .chars()
                .take(8)
                .collect()
        };
        let session_short = &short_hash[..8.min(short_hash.len())];
        // Just verify the outcome contains the right counts.
        // Re-run to get the outcome (first run already stored).
        // We need to count how many src-prompt-* records exist.
        // Query indirectly: re-run with a NEW transcript for a different session
        // and count the returned prompt_sources_created value.
        let path2 = write_jsonl(
            dir.path(),
            "multi2.jsonl",
            &[
                serde_json::json!({
                    "uuid": "v1", "timestamp": "2026-04-02T10:00:00.000Z",
                    "type": "user", "sessionId": "sid2",
                    "message": {"role": "user", "content": "short"}
                }),
                serde_json::json!({
                    "uuid": "v2", "timestamp": "2026-04-02T10:01:00.000Z",
                    "type": "user", "sessionId": "sid2",
                    "message": {"role": "user", "content": "Implement the full OAuth2 PKCE authorization flow here"}
                }),
                serde_json::json!({
                    "uuid": "v3", "timestamp": "2026-04-02T10:02:00.000Z",
                    "type": "user", "sessionId": "sid2",
                    "message": {"role": "user", "content": "Please add comprehensive unit tests covering edge cases for the token refresh mechanism in auth.rs"}
                }),
            ],
        );
        let outcome = run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "sid2",
            &path2,
            &cfg,
        );
        match outcome {
            AttributionOutcome::Success {
                prompt_sources_created,
                ..
            } => {
                assert_eq!(
                    prompt_sources_created, 2,
                    "expected 2 prompt sources (5-char prompt filtered out)"
                );
            }
            other => panic!("unexpected outcome: {other}"),
        }
        let _ = session_short;
    }

    // ── §4.8 test 2 ───────────────────────────────────────────────────────────

    #[test]
    fn skips_system_reminder_prompts() {
        let (dir, repo) = make_repo();
        let path = write_jsonl(
            dir.path(),
            "sysrem.jsonl",
            &[serde_json::json!({
                "uuid": "u1", "timestamp": "2026-04-01T10:00:00.000Z",
                "type": "user", "sessionId": "sid",
                "message": {
                    "role": "user",
                    "content": "<system-reminder>This is a system-injected reminder that should be filtered out.</system-reminder>"
                }
            })],
        );
        let cfg = default_config();
        let outcome = run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "sid",
            &path,
            &cfg,
        );
        match outcome {
            AttributionOutcome::Success {
                prompt_sources_created,
                ..
            } => {
                assert_eq!(prompt_sources_created, 0);
            }
            other => panic!("unexpected outcome: {other}"),
        }
    }

    // ── §4.8 test 3 ───────────────────────────────────────────────────────────

    #[test]
    fn prompt_source_uri_uses_session_fragment() {
        let (dir, repo) = make_repo();
        let path = write_jsonl(
            dir.path(),
            "frag.jsonl",
            &[serde_json::json!({
                "uuid": "u1", "timestamp": "2026-04-01T10:00:00.000Z",
                "type": "user", "sessionId": "frag-session",
                "message": {"role": "user", "content": "Implement the OAuth2 PKCE authorization flow end to end"}
            })],
        );
        let cfg = default_config();
        run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "frag-session",
            &path,
            &cfg,
        );

        // Find a prompt source record via the artifact hash.
        let artifacts = repo.list_artifacts(None).unwrap();
        let a = artifacts
            .iter()
            .find(|a| a.artifact_type == ArtifactType::Session)
            .unwrap();
        let short_hash: String = a
            .artifact_hash
            .0
            .strip_prefix("sha256-")
            .unwrap_or(&a.artifact_hash.0)
            .chars()
            .take(8)
            .collect();
        let session_short = &short_hash;
        // Find a prompt source for this session.
        let prompt_hash =
            sha256_short_12("Implement the OAuth2 PKCE authorization flow end to end");
        let source_id = format!("src-prompt-{session_short}-{prompt_hash}");
        // The source id uses first 8 chars of session, need to use the actual session_short from session_id.
        let session_short2 = short_session_id("frag-session");
        let source_id2 = format!("src-prompt-{session_short2}-{prompt_hash}");
        let src = repo
            .get_source_record(&source_id2)
            .unwrap()
            .expect("prompt source not found");
        let uri = src.source_uri.unwrap_or_default();
        assert!(
            uri.starts_with("cliproot://session/sha256-"),
            "URI should start with cliproot://session/sha256-, got: {uri}"
        );
        assert!(
            uri.contains("#turn-"),
            "URI should contain #turn-, got: {uri}"
        );
        let _ = source_id;
    }

    // ── §4.8 test 4 ───────────────────────────────────────────────────────────

    #[test]
    fn prompt_clip_off_skips_clip_keeps_source() {
        let (dir, repo) = make_repo();
        let path = write_jsonl(
            dir.path(),
            "off.jsonl",
            &[serde_json::json!({
                "uuid": "u1", "timestamp": "2026-04-01T10:00:00.000Z",
                "type": "user", "sessionId": "off-session",
                "message": {"role": "user", "content": "Implement the OAuth2 PKCE authorization flow end to end"}
            })],
        );
        let mut cfg = default_config();
        cfg.prompt_clips = PromptClipsMode::Off;

        let outcome = run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "off-session",
            &path,
            &cfg,
        );
        match outcome {
            AttributionOutcome::Success {
                prompt_sources_created,
                prompt_clips_created,
                ..
            } => {
                assert_eq!(prompt_sources_created, 1, "expected 1 source record");
                assert_eq!(
                    prompt_clips_created, 0,
                    "expected 0 clips with prompt_clips=Off"
                );
            }
            other => panic!("unexpected: {other}"),
        }
    }

    // ── §4.8 test 5 ───────────────────────────────────────────────────────────

    #[test]
    fn derives_edge_from_prompt_to_clip_in_window() {
        let (dir, repo) = make_repo();

        let t_prompt1 = "2026-04-01T10:00:00.000Z";
        let t_clip1 = "2026-04-01T10:00:02.000Z"; // in window of prompt1
        let t_prompt2 = "2026-04-01T10:05:00.000Z";
        let t_clip2 = "2026-04-01T10:05:03.000Z"; // in window of prompt2

        let path = write_jsonl(
            dir.path(),
            "derive.jsonl",
            &[
                serde_json::json!({
                    "uuid": "u1", "timestamp": t_prompt1,
                    "type": "user", "sessionId": "derive-session",
                    "message": {"role": "user", "content": "Please implement the OAuth2 PKCE flow in Rust with tests"}
                }),
                serde_json::json!({
                    "uuid": "u2", "timestamp": t_prompt2,
                    "type": "user", "sessionId": "derive-session",
                    "message": {"role": "user", "content": "Now add the token refresh mechanism with automatic retry logic"}
                }),
                serde_json::json!({
                    "uuid": "a1", "timestamp": "2026-04-01T10:05:10.000Z",
                    "type": "assistant", "sessionId": "derive-session",
                    "message": {"role": "assistant", "content": "Done."}
                }),
            ],
        );

        // Seed two clips at the respective timestamps.
        fn make_clip(text: &str, source_id: &str, _ts: &str) -> (SourceRecord, Clip) {
            let th = create_text_hash(text);
            let ch = create_clip_hash(ClipHashInput {
                text_hash: th.clone(),
                source_refs: vec![source_id.to_string()],
                text_quote_exact: None,
            });
            let src = SourceRecord {
                id: CrpId(source_id.to_string()),
                source_type: SourceType::ExternalQuoted,
                digital_source_type: None,
                title: None,
                source_uri: Some("https://example.com".to_string()),
                author_agent_id: None,
                created_at: None,
            };
            let clip = Clip {
                clip_hash: ch,
                id: None,
                project_id: None,
                document_id: None,
                source_refs: vec![source_id.to_string()],
                selectors: None,
                content: Some(text.to_string()),
                text_hash: th,
                created_by_activity_id: None,
            };
            (src, clip)
        }

        let (src1, clip1) = make_clip("PKCE code verifier detail", "src-clip1", t_clip1);
        let (src2, clip2) = make_clip("Token refresh retry logic", "src-clip2", t_clip2);
        let clip1_hash = clip1.clip_hash.0.clone();
        let clip2_hash = clip2.clip_hash.0.clone();

        for (src, clip, ts) in [(src1, clip1, t_clip1), (src2, clip2, t_clip2)] {
            repo.store_bundle(&CrpBundle {
                protocol_version: "0.0.3".to_string(),
                bundle_type: BundleType::ProvenanceExport,
                created_at: ts.to_string(),
                project: None,
                document: None,
                agents: Vec::new(),
                sources: vec![src],
                clips: vec![clip],
                artifacts: Vec::new(),
                clip_artifact_refs: Vec::new(),
                activities: Vec::new(),
                edges: Vec::new(),
                reuse_events: Vec::new(),
                signatures: Vec::new(),
                registry: None,
            })
            .unwrap();
        }

        let cfg = default_config();
        run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "derive-session",
            &path,
            &cfg,
        );

        // clip1 should have WasDerivedFrom pointing to prompt1's source.
        let edges1 = repo.get_edges_for_subject(&clip1_hash).unwrap();
        let derived1 = edges1
            .iter()
            .find(|e| e.edge_type == EdgeType::WasDerivedFrom);
        assert!(derived1.is_some(), "clip1 should have WasDerivedFrom edge");
        let obj1 = derived1.unwrap().object_ref.0.clone();

        // clip2 should have WasDerivedFrom pointing to prompt2's source (different from clip1's).
        let edges2 = repo.get_edges_for_subject(&clip2_hash).unwrap();
        let derived2 = edges2
            .iter()
            .find(|e| e.edge_type == EdgeType::WasDerivedFrom);
        assert!(derived2.is_some(), "clip2 should have WasDerivedFrom edge");
        let obj2 = derived2.unwrap().object_ref.0.clone();

        assert_ne!(
            obj1, obj2,
            "clip1 and clip2 should point to different prompts"
        );
    }

    // ── §4.8 test 6 ───────────────────────────────────────────────────────────

    #[test]
    fn idempotent_on_rerun_no_duplicate_sources() {
        let (dir, repo) = make_repo();
        let path = write_jsonl(
            dir.path(),
            "idem2.jsonl",
            &[serde_json::json!({
                "uuid": "u1", "timestamp": "2026-04-01T10:00:00.000Z",
                "type": "user", "sessionId": "idem-session",
                "message": {"role": "user", "content": "Implement the OAuth2 PKCE authorization flow end to end"}
            })],
        );
        let cfg = default_config();

        let outcome1 = run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "idem-session",
            &path,
            &cfg,
        );
        assert!(matches!(outcome1, AttributionOutcome::Success { .. }));

        // Second run: should return AlreadyRun — no new sources.
        let outcome2 = run_session_attribution(
            dir.path().join(".cliproot").as_path(),
            &repo,
            "idem-session",
            &path,
            &cfg,
        );
        assert!(
            matches!(outcome2, AttributionOutcome::AlreadyRun),
            "expected AlreadyRun, got: {outcome2}"
        );
    }
}
