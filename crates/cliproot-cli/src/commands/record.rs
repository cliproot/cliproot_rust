use std::path::PathBuf;

use cliproot_store::Repository;

use crate::transcript::{
    activity_inferrer, clip_matcher, hook_log,
    parser::{self, SessionMeta},
    session_builder,
};
use crate::OutputFormat;

pub struct RecordOptions {
    pub session_id: Option<String>,
    pub session_dir: Option<String>,
    pub jsonl: Option<String>,
    pub hook_log_path: Option<String>,
    pub last: Option<u32>,
    pub project: Option<String>,
    pub include_subagents: bool,
    pub dry_run: bool,
    pub pack: bool,
    pub output: Option<String>,
}

pub fn run(opts: RecordOptions, format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;

    // Step 1: Resolve session JSONL path(s)
    let jsonl_paths = resolve_session_paths(&opts)?;

    if jsonl_paths.is_empty() {
        return Err("no session JSONL files found — specify --jsonl or --session-dir".into());
    }

    // Step 2: Parse transcripts and merge
    let mut all_events = Vec::new();
    let mut session_meta: Option<SessionMeta> = None;

    for jsonl_path in &jsonl_paths {
        if opts.include_subagents {
            // Check if there's a matching session directory (dir with same stem)
            let session_dir = jsonl_path.with_extension("");
            if session_dir.is_dir() {
                let (events, meta) = parser::parse_session_dir(&session_dir)?;
                all_events.extend(events);
                if session_meta.is_none() {
                    session_meta = Some(meta);
                }
                continue;
            }
        }

        let events = parser::parse_jsonl(jsonl_path)?;
        if session_meta.is_none() {
            let meta = parser::extract_session_meta(jsonl_path, &events)?;
            session_meta = Some(meta);
        }
        all_events.extend(events);
    }

    all_events.sort_by_key(|e| e.timestamp);

    let meta = session_meta.ok_or("could not extract session metadata")?;

    // Step 3: Match clips against repository
    let matched_clips = clip_matcher::match_clips(&all_events, &repo)?;

    // Step 4: Parse hook log if available
    let enrichment = resolve_hook_log(&opts, &repo, &meta)?;

    // Step 5: Infer activities
    let activities =
        activity_inferrer::infer_activities(&all_events, &matched_clips, enrichment.as_ref());

    // Step 6: Build design record
    let record = session_builder::build_design_record(meta, activities, matched_clips, enrichment);

    // Dry-run: just print what would be created
    if opts.dry_run {
        print_dry_run(&record, format);
        return Ok(());
    }

    // Step 7: Store in repository
    let stored = session_builder::store_design_record(&record, &repo, opts.project.as_deref())?;

    // Step 8: Optionally create a pack
    if opts.pack {
        let pack_output = opts
            .output
            .clone()
            .unwrap_or_else(|| format!("{}.cliprootpack", record.record_id));
        // Use existing pack create via CLI
        eprintln!(
            "Pack creation: run `cliproot pack create --root {} -o {}`",
            stored.record_id, pack_output
        );
    }

    // Print result
    match format {
        OutputFormat::Json => {
            let json = serde_json::json!({
                "record_id": record.record_id,
                "session_id": stored.session_id,
                "artifact_hash": stored.artifact_hash,
                "preview_path": stored.preview_path.display().to_string(),
                "stats": {
                    "turns": record.stats.turn_count,
                    "tool_calls": record.stats.tool_call_count,
                    "source_clips": record.stats.source_clip_count,
                    "derived_clips": record.stats.derived_clip_count,
                    "urls_fetched": record.stats.urls_fetched_count,
                    "files_read": record.stats.files_read_count,
                    "files_modified": record.stats.files_modified_count,
                    "subagents": record.stats.subagent_count,
                    "duration_secs": record.stats.duration_secs,
                }
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        _ => {
            println!("Design record created: {}", record.record_id);
            println!();
            let short_session = if record.session_meta.session_id.len() > 8 {
                &record.session_meta.session_id[..8]
            } else {
                &record.session_meta.session_id
            };
            let model = record.session_meta.model.as_deref().unwrap_or("unknown");
            let branch = record
                .session_meta
                .git_branch
                .as_deref()
                .unwrap_or("unknown");
            println!("  Session:     {} (Claude Code, {})", short_session, model);
            if let Some(secs) = record.stats.duration_secs {
                let duration = if secs >= 3600 {
                    format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
                } else {
                    format!("{}m", secs / 60)
                };
                if let (Some(start), Some(end)) =
                    (record.session_meta.started_at, record.session_meta.ended_at)
                {
                    println!(
                        "  Duration:    {} - {} ({})",
                        start.format("%H:%M"),
                        end.format("%H:%M"),
                        duration
                    );
                }
            }
            println!("  Branch:      {branch}");
            println!(
                "  Turns:       {} ({} prompts, {} tool calls)",
                record.stats.turn_count, record.stats.turn_count, record.stats.tool_call_count
            );
            println!(
                "  Sources:     {} clipped, {} derived",
                record.stats.source_clip_count, record.stats.derived_clip_count
            );
            if record.stats.files_read_count > 0 || record.stats.files_modified_count > 0 {
                println!(
                    "  Files:       {} read, {} modified",
                    record.stats.files_read_count, record.stats.files_modified_count
                );
            }
            if record.stats.subagent_count > 0 {
                println!("  Subagents:   {}", record.stats.subagent_count);
            }
            println!();
            println!("  Preview:     {}", stored.preview_path.display());
            println!("  Artifact:    {}", stored.artifact_hash);
        }
    }

    Ok(())
}

fn resolve_session_paths(opts: &RecordOptions) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    // Explicit JSONL path
    if let Some(ref jsonl) = opts.jsonl {
        return Ok(vec![PathBuf::from(jsonl)]);
    }

    // Explicit session directory
    if let Some(ref dir) = opts.session_dir {
        let dir = PathBuf::from(dir);
        if dir.is_file() {
            return Ok(vec![dir]);
        }
        let main_jsonl = dir.with_extension("jsonl");
        if main_jsonl.is_file() {
            return Ok(vec![main_jsonl]);
        }
        return Err(format!("no JSONL found at {}", dir.display()).into());
    }

    // Auto-detect from ~/.claude/projects/
    let cwd = std::env::current_dir()?;
    let project_dirs = parser::discover_session_dir(&cwd)?;
    if project_dirs.is_empty() {
        return Err(format!(
            "no Claude Code project found for {}. Use --jsonl or --session-dir.",
            cwd.display()
        )
        .into());
    }

    let mut all_sessions = Vec::new();
    for project_dir in &project_dirs {
        let sessions = parser::find_sessions(project_dir)?;
        all_sessions.extend(sessions);
    }

    if all_sessions.is_empty() {
        return Err("no session JSONL files found in Claude Code project directory".into());
    }

    // By session ID
    if let Some(ref session_id) = opts.session_id {
        let matching: Vec<_> = all_sessions
            .iter()
            .filter(|p| {
                p.file_stem()
                    .map(|s| s.to_string_lossy().contains(session_id.as_str()))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        if matching.is_empty() {
            return Err(format!("no session matching '{}' found", session_id).into());
        }
        return Ok(matching);
    }

    // --last N
    let count = opts.last.unwrap_or(1) as usize;
    Ok(all_sessions.into_iter().take(count).collect())
}

fn resolve_hook_log(
    opts: &RecordOptions,
    repo: &Repository,
    meta: &SessionMeta,
) -> Result<Option<hook_log::HookEnrichment>, Box<dyn std::error::Error>> {
    let log_path = if let Some(ref explicit_path) = opts.hook_log_path {
        Some(PathBuf::from(explicit_path))
    } else {
        hook_log::find_hook_log(repo.cliproot_dir(), &meta.session_id)
    };

    match log_path {
        Some(path) if path.is_file() => {
            let entries = hook_log::parse_hook_log(&path)?;
            Ok(Some(hook_log::build_enrichment(&entries)))
        }
        _ => Ok(None),
    }
}

fn print_dry_run(record: &session_builder::DesignRecord, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            let json = serde_json::json!({
                "dry_run": true,
                "record_id": record.record_id,
                "stats": {
                    "turns": record.stats.turn_count,
                    "tool_calls": record.stats.tool_call_count,
                    "source_clips": record.stats.source_clip_count,
                    "derived_clips": record.stats.derived_clip_count,
                    "urls_fetched": record.stats.urls_fetched_count,
                    "files_read": record.stats.files_read_count,
                    "files_modified": record.stats.files_modified_count,
                }
            });
            println!("{}", serde_json::to_string_pretty(&json).unwrap());
        }
        _ => {
            println!("Dry run — would create: {}", record.record_id);
            println!();
            println!("  Turns:       {}", record.stats.turn_count);
            println!("  Tool calls:  {}", record.stats.tool_call_count);
            println!(
                "  Clips:       {} source, {} derived",
                record.stats.source_clip_count, record.stats.derived_clip_count
            );
            println!("  URLs:        {}", record.stats.urls_fetched_count);
            println!(
                "  Files:       {} read, {} modified",
                record.stats.files_read_count, record.stats.files_modified_count
            );
            println!();
            println!("--- Preview markdown ---");
            println!("{}", record.markdown);
        }
    }
}
