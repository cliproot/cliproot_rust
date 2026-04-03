use std::fmt::Write;

use cliproot_core::ActivityType;
use cliproot_store::Repository;

use super::activity_inferrer::InferredActivity;
use super::clip_matcher::MatchedClip;
use super::hook_log::HookEnrichment;
use super::parser::SessionMeta;

/// A fully reconstructed design record ready for storage and rendering.
#[derive(Debug, Clone)]
pub struct DesignRecord {
    pub record_id: String,
    pub session_meta: SessionMeta,
    pub activities: Vec<InferredActivity>,
    #[allow(dead_code)]
    pub matched_clips: Vec<MatchedClip>,
    #[allow(dead_code)]
    pub enrichment: Option<HookEnrichment>,
    /// The rendered markdown design record.
    pub markdown: String,
    /// Summary statistics.
    pub stats: RecordStats,
}

#[derive(Debug, Clone)]
pub struct RecordStats {
    pub turn_count: usize,
    pub tool_call_count: usize,
    pub source_clip_count: usize,
    pub derived_clip_count: usize,
    pub urls_fetched_count: usize,
    pub files_read_count: usize,
    pub files_modified_count: usize,
    pub subagent_count: usize,
    pub duration_secs: Option<i64>,
}

/// Build a design record from inferred activities and metadata.
pub fn build_design_record(
    session_meta: SessionMeta,
    activities: Vec<InferredActivity>,
    matched_clips: Vec<MatchedClip>,
    enrichment: Option<HookEnrichment>,
) -> DesignRecord {
    let record_id = format!(
        "rec-{}",
        short_session_id(&session_meta.session_id)
    );
    let stats = compute_stats(&activities, &matched_clips, &enrichment, &session_meta);
    let markdown = render_markdown(&record_id, &session_meta, &activities, &matched_clips, &enrichment, &stats);

    DesignRecord {
        record_id,
        session_meta,
        activities,
        matched_clips,
        enrichment,
        markdown,
        stats,
    }
}

/// Store the design record as a session + artifact in the cliproot repository.
pub fn store_design_record(
    record: &DesignRecord,
    repo: &Repository,
    project_id: Option<&str>,
) -> Result<StoredRecord, Box<dyn std::error::Error>> {
    // Start a session
    let metadata = serde_json::json!({
        "reconstructed": true,
        "source_session": record.session_meta.session_id,
        "model": record.session_meta.model,
        "git_branch": record.session_meta.git_branch,
    });
    let session = repo.start_session(project_id, Some("claude-code"), Some(metadata))?;

    // Create activities and link clips
    for inferred in &record.activities {
        let activity_type: ActivityType =
            serde_json::from_value(serde_json::Value::String(inferred.activity_type.clone()))
                .unwrap_or(ActivityType::Research);

        let activity = repo.start_activity(
            activity_type,
            project_id,
            Some("claude-code"),
            Some(inferred.prompt.clone()),
            None,
            Some(&session.session_id),
        )?;

        // Link clips to the activity
        for hash in inferred
            .source_clip_hashes
            .iter()
            .chain(inferred.derived_clip_hashes.iter())
        {
            let used_refs: Vec<String> = Vec::new();
            let _ = repo.record_clip_tracking(hash, Some(activity.id.as_str()), Some(&session.session_id), &used_refs);
        }

        repo.end_activity(activity.id.as_str())?;
    }

    // Store the markdown as an artifact
    let md_bytes = record.markdown.as_bytes();
    let artifact = repo.add_artifact(
        None,
        Some(md_bytes),
        Some(&format!("{}.md", record.record_id)),
        cliproot_core::ArtifactType::Markdown,
        Some("text/markdown"),
        Some(&record.record_id),
        project_id,
        None,
    )?;

    // Also write to .cliproot/records/ for easy access
    let records_dir = repo.cliproot_dir().join("records");
    std::fs::create_dir_all(&records_dir)?;
    let preview_path = records_dir.join(format!("{}.md", record.record_id));
    std::fs::write(&preview_path, &record.markdown)?;

    // End the session
    let session = repo.end_session(&session.session_id)?;

    Ok(StoredRecord {
        session_id: session.session_id,
        record_id: record.record_id.clone(),
        artifact_hash: artifact.artifact_hash.0,
        preview_path,
    })
}

#[derive(Debug)]
pub struct StoredRecord {
    pub session_id: String,
    pub record_id: String,
    pub artifact_hash: String,
    pub preview_path: std::path::PathBuf,
}

// ── Markdown rendering ──

fn render_markdown(
    record_id: &str,
    meta: &SessionMeta,
    activities: &[InferredActivity],
    matched_clips: &[MatchedClip],
    enrichment: &Option<HookEnrichment>,
    stats: &RecordStats,
) -> String {
    let mut md = String::new();

    // Header
    let date = meta
        .started_at
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let branch = meta.git_branch.as_deref().unwrap_or("unknown");
    let model = meta.model.as_deref().unwrap_or("unknown");
    let duration = stats.duration_secs.map(format_duration).unwrap_or_else(|| "unknown".to_string());

    writeln!(md, "# Design Record: {record_id}").unwrap();
    writeln!(
        md,
        "**Session**: {} | **Date**: {} | **Branch**: {}",
        short_session_id(&meta.session_id),
        date,
        branch
    )
    .unwrap();
    writeln!(
        md,
        "**Duration**: {} | **Model**: {}",
        duration, model
    )
    .unwrap();
    writeln!(md).unwrap();

    // Summary stats
    writeln!(md, "## Summary").unwrap();
    writeln!(md, "- **Turns**: {}", stats.turn_count).unwrap();
    writeln!(md, "- **Tool calls**: {}", stats.tool_call_count).unwrap();
    writeln!(
        md,
        "- **Sources clipped**: {} ({} derived)",
        stats.source_clip_count, stats.derived_clip_count
    )
    .unwrap();
    if stats.urls_fetched_count > 0 {
        writeln!(md, "- **URLs fetched**: {}", stats.urls_fetched_count).unwrap();
    }
    if stats.files_read_count > 0 {
        writeln!(md, "- **Files read**: {}", stats.files_read_count).unwrap();
    }
    if stats.files_modified_count > 0 {
        writeln!(md, "- **Files modified**: {}", stats.files_modified_count).unwrap();
    }
    if stats.subagent_count > 0 {
        writeln!(md, "- **Subagents**: {}", stats.subagent_count).unwrap();
    }
    writeln!(md).unwrap();

    // Exploration timeline
    writeln!(md, "## Exploration Timeline").unwrap();
    writeln!(md).unwrap();

    for (i, activity) in activities.iter().enumerate() {
        let time = activity.started_at.format("%H:%M");
        writeln!(md, "### Turn {}: \"{}\"", i + 1, truncate_prompt(&activity.prompt, 80)).unwrap();
        writeln!(md, "*{} | {}*", time, activity.activity_type).unwrap();
        writeln!(md).unwrap();

        // Sources consulted
        if !activity.source_clip_hashes.is_empty() {
            writeln!(md, "**Sources clipped**:").unwrap();
            for hash in &activity.source_clip_hashes {
                if let Some(mc) = matched_clips.iter().find(|mc| mc.clip_hash == *hash) {
                    let source = mc
                        .clip
                        .source_refs
                        .first()
                        .map(|s| s.as_str())
                        .unwrap_or("unknown");
                    let content_preview = mc
                        .clip
                        .content
                        .as_deref()
                        .map(|c| truncate_prompt(c, 100))
                        .unwrap_or_default();
                    writeln!(md, "- [{}]({}) — \"{}\"", source, source, content_preview).unwrap();
                } else {
                    writeln!(md, "- `{}`", short_hash(hash)).unwrap();
                }
            }
            writeln!(md).unwrap();
        }

        // Derived clips
        if !activity.derived_clip_hashes.is_empty() {
            writeln!(md, "**Derivations**:").unwrap();
            for hash in &activity.derived_clip_hashes {
                if let Some(mc) = matched_clips.iter().find(|mc| mc.clip_hash == *hash) {
                    let content_preview = mc
                        .clip
                        .content
                        .as_deref()
                        .map(|c| truncate_prompt(c, 120))
                        .unwrap_or_default();
                    writeln!(md, "- `{}`: \"{}\"", short_hash(hash), content_preview).unwrap();
                } else {
                    writeln!(md, "- `{}`", short_hash(hash)).unwrap();
                }
            }
            writeln!(md).unwrap();
        }

        // URLs fetched
        if !activity.urls_fetched.is_empty() {
            writeln!(md, "**URLs fetched**:").unwrap();
            for url in &activity.urls_fetched {
                writeln!(md, "- {url}").unwrap();
            }
            writeln!(md).unwrap();
        }

        // Files
        if !activity.files_modified.is_empty() {
            writeln!(md, "**Files modified**:").unwrap();
            for f in &activity.files_modified {
                writeln!(md, "- `{f}`").unwrap();
            }
            writeln!(md).unwrap();
        }

        if !activity.files_read.is_empty() && activity.files_modified.is_empty() {
            writeln!(md, "**Files examined**:").unwrap();
            for f in &activity.files_read {
                writeln!(md, "- `{f}`").unwrap();
            }
            writeln!(md).unwrap();
        }

        // Reasoning summary
        if let Some(ref summary) = activity.reasoning_summary {
            writeln!(md, "**Key finding**: {summary}").unwrap();
            writeln!(md).unwrap();
        }
    }

    // Files touched summary
    let all_files_read: Vec<&str> = activities
        .iter()
        .flat_map(|a| a.files_read.iter().map(|s| s.as_str()))
        .collect();
    let all_files_modified: Vec<&str> = activities
        .iter()
        .flat_map(|a| a.files_modified.iter().map(|s| s.as_str()))
        .collect();

    if !all_files_read.is_empty() || !all_files_modified.is_empty() {
        writeln!(md, "## Files Touched").unwrap();
        let mut seen = std::collections::HashSet::new();
        for f in &all_files_modified {
            if seen.insert(*f) {
                writeln!(md, "- `{f}` (modified)").unwrap();
            }
        }
        for f in &all_files_read {
            if seen.insert(*f) {
                writeln!(md, "- `{f}` (read)").unwrap();
            }
        }
        writeln!(md).unwrap();
    }

    // Unclipped URLs (from enrichment)
    if let Some(ref enrichment) = enrichment {
        let clipped_urls: std::collections::HashSet<&str> = matched_clips
            .iter()
            .flat_map(|mc| mc.clip.source_refs.iter().map(|s| s.as_str()))
            .collect();
        let unclipped: Vec<_> = enrichment
            .urls_fetched
            .iter()
            .filter(|u| !clipped_urls.contains(u.url.as_str()))
            .collect();
        if !unclipped.is_empty() {
            writeln!(md, "## URLs Fetched (Not Clipped)").unwrap();
            for u in &unclipped {
                writeln!(md, "- {}", u.url).unwrap();
            }
            writeln!(md).unwrap();
        }
    }

    md
}

fn compute_stats(
    activities: &[InferredActivity],
    matched_clips: &[MatchedClip],
    enrichment: &Option<HookEnrichment>,
    meta: &SessionMeta,
) -> RecordStats {
    let source_clip_count = matched_clips.iter().filter(|mc| !mc.is_derived).count();
    let derived_clip_count = matched_clips.iter().filter(|mc| mc.is_derived).count();
    let tool_call_count: usize = activities.iter().map(|a| a.tool_calls.len()).sum();
    let mut all_urls = std::collections::HashSet::new();
    let mut all_reads = std::collections::HashSet::new();
    let mut all_writes = std::collections::HashSet::new();
    let mut all_subagents = std::collections::HashSet::new();

    for a in activities {
        for u in &a.urls_fetched {
            all_urls.insert(u.clone());
        }
        for f in &a.files_read {
            all_reads.insert(f.clone());
        }
        for f in &a.files_modified {
            all_writes.insert(f.clone());
        }
        for s in &a.subagent_ids {
            all_subagents.insert(s.clone());
        }
    }

    // Merge enrichment-level URLs if available
    if let Some(ref enrichment) = enrichment {
        for u in &enrichment.urls_fetched {
            all_urls.insert(u.url.clone());
        }
    }

    let duration_secs = match (meta.started_at, meta.ended_at) {
        (Some(start), Some(end)) => Some((end - start).num_seconds()),
        _ => None,
    };

    RecordStats {
        turn_count: activities.len(),
        tool_call_count,
        source_clip_count,
        derived_clip_count,
        urls_fetched_count: all_urls.len(),
        files_read_count: all_reads.len(),
        files_modified_count: all_writes.len(),
        subagent_count: all_subagents.len(),
        duration_secs,
    }
}

fn short_session_id(id: &str) -> &str {
    if id.len() > 8 {
        &id[..8]
    } else {
        id
    }
}

fn short_hash(hash: &str) -> &str {
    if hash.len() > 20 {
        &hash[..20]
    } else {
        hash
    }
}

fn truncate_prompt(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        // Find the largest byte index <= max_chars that is a char boundary
        let mut i = max_chars;
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
        format!("{}...", &s[..i])
    }
}

fn format_duration(secs: i64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}
