use std::collections::HashSet;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use cliproot_store::Repository;
use serde::{Deserialize, Serialize};

use crate::transcript::hook_log::{
    build_enrichment, parse_hook_log, read_watermark, write_watermark, ConsolidationState,
    HookLogEntry,
};

// ── Config ────────────────────────────────────────────────────────────────

/// Consolidation-specific config read from `.cliproot/config.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConsolidationConfig {
    pub base_interval: usize,
    pub min_interval: usize,
    pub max_interval: usize,
    pub adaptive: bool,
    pub synthesis_lookback_entries: usize,
    pub synthesis_lookback_minutes: i64,
    pub synthesis_min_sources: usize,
    pub exclude_url_patterns: Vec<String>,
    pub exclude_file_patterns: Vec<String>,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            base_interval: 10,
            min_interval: 5,
            max_interval: 20,
            adaptive: true,
            synthesis_lookback_entries: 20,
            synthesis_lookback_minutes: 30,
            synthesis_min_sources: 2,
            exclude_url_patterns: vec![
                "crates.io".into(),
                "npmjs.com".into(),
                "pypi.org".into(),
                "pkg.go.dev".into(),
            ],
            exclude_file_patterns: vec![
                "target/".into(),
                "node_modules/".into(),
                ".git/".into(),
                "dist/".into(),
            ],
        }
    }
}

/// Wrapper to extract `consolidation` key from config.json.
#[derive(Debug, Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    consolidation: ConsolidationConfig,
}

pub fn load_config(cliproot_dir: &Path) -> ConsolidationConfig {
    let config_path = cliproot_dir.join("config.json");
    match fs::read_to_string(&config_path) {
        Ok(content) => serde_json::from_str::<ConfigFile>(&content)
            .map(|c| c.consolidation)
            .unwrap_or_default(),
        Err(_) => ConsolidationConfig::default(),
    }
}

// ── Candidate types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct SourceCandidate {
    pub url: String,
    pub title: Option<String>,
    pub times_accessed: usize,
    pub first_seen: String,
    pub tool: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileCandidate {
    pub path: String,
    pub times_read: usize,
    pub first_seen: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbableSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(rename = "type")]
    pub source_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SynthesisCandidate {
    pub file: String,
    pub tool: String,
    pub timestamp: String,
    pub probable_sources: Vec<ProbableSource>,
    pub source_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Candidates {
    pub sources: Vec<SourceCandidate>,
    pub files: Vec<FileCandidate>,
    pub syntheses: Vec<SynthesisCandidate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsolidationStats {
    pub total_events: usize,
    pub since_watermark: usize,
    pub unclipped_sources: usize,
    pub unclipped_files: usize,
    pub synthesis_candidates: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CandidateList {
    pub candidates: Candidates,
    pub stats: ConsolidationStats,
}

// ── Engine ────────────────────────────────────────────────────────────────

pub fn run_consolidation(
    cliproot_dir: &Path,
    session_id: &str,
    emergency: bool,
    commit: bool,
) -> Result<CandidateList, Box<dyn std::error::Error>> {
    let config = load_config(cliproot_dir);

    // 1. Read JSONL
    let log_path = cliproot_dir
        .join("agent-log")
        .join(format!("{session_id}.jsonl"));
    let all_entries = parse_hook_log(&log_path)?;
    let total_events = all_entries.len();

    // 2. Read watermark, slice entries
    let watermark = read_watermark(cliproot_dir, session_id);
    let slice = if watermark.line_count < all_entries.len() {
        &all_entries[watermark.line_count..]
    } else {
        &[]
    };
    let since_watermark = slice.len();

    // 3. Build enrichment on sliced entries
    let enrichment = build_enrichment(slice);

    // 4. Open repository for cross-referencing
    let project_root = cliproot_dir
        .parent()
        .ok_or("cannot determine project root from .cliproot dir")?;
    let repo = Repository::open(project_root)?;
    let project_root_str = project_root.to_string_lossy().to_string();

    // 5. Extract source candidates (unclipped URLs)
    let mut url_counts: std::collections::HashMap<String, (usize, DateTime<Utc>)> =
        std::collections::HashMap::new();
    for uf in &enrichment.urls_fetched {
        let entry = url_counts
            .entry(uf.url.clone())
            .or_insert((0, uf.timestamp));
        entry.0 += 1;
        if uf.timestamp < entry.1 {
            entry.1 = uf.timestamp;
        }
    }

    let mut source_candidates = Vec::new();
    for (url, (count, first_seen)) in &url_counts {
        if is_excluded_url(url, &config.exclude_url_patterns) {
            continue;
        }
        if repo.has_clip_for_uri(url)? {
            continue;
        }
        source_candidates.push(SourceCandidate {
            url: url.clone(),
            title: None,
            times_accessed: *count,
            first_seen: first_seen.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            tool: "WebFetch".to_string(),
        });
    }

    // 6. Extract file candidates (reference files outside project tree, not modified)
    let modified_set: HashSet<&str> = enrichment
        .files_modified
        .iter()
        .map(|s| s.as_str())
        .collect();
    let mut file_counts: std::collections::HashMap<String, (usize, DateTime<Utc>)> =
        std::collections::HashMap::new();
    for entry in slice {
        if entry.tool_name == "Read" {
            if let Some(path) = entry.tool_input.get("file_path").and_then(|v| v.as_str()) {
                let e = file_counts
                    .entry(path.to_string())
                    .or_insert((0, entry.timestamp));
                e.0 += 1;
                if entry.timestamp < e.1 {
                    e.1 = entry.timestamp;
                }
            }
        }
    }

    let mut file_candidates = Vec::new();
    for (path, (count, first_seen)) in &file_counts {
        if modified_set.contains(path.as_str()) {
            continue;
        }
        if is_excluded_file(path, &config.exclude_file_patterns) {
            continue;
        }
        // Only include files outside the project tree (reference files)
        if path.starts_with(&project_root_str) {
            continue;
        }
        file_candidates.push(FileCandidate {
            path: path.clone(),
            times_read: *count,
            first_seen: first_seen.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        });
    }

    // 7. Synthesis detection
    let synthesis_candidates =
        detect_syntheses(slice, &config, &repo, &modified_set, &project_root_str)?;

    // 8. Build result
    let result = CandidateList {
        stats: ConsolidationStats {
            total_events,
            since_watermark,
            unclipped_sources: source_candidates.len(),
            unclipped_files: file_candidates.len(),
            synthesis_candidates: synthesis_candidates.len(),
        },
        candidates: Candidates {
            sources: source_candidates,
            files: file_candidates,
            syntheses: synthesis_candidates,
        },
    };

    // 9. Commit watermark if requested
    if commit || emergency {
        let new_state = ConsolidationState {
            line_count: total_events,
            message_count: watermark.message_count, // preserved; hook handler updates this
            last_consolidation_ts: Some(
                Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            ),
        };
        write_watermark(cliproot_dir, session_id, &new_state)?;
    }

    // 10. Emergency: write candidate artifact
    if emergency && result.has_candidates() {
        let content = serde_json::to_string_pretty(&result)?;
        let file_name = format!("consolidation-candidates-{session_id}.json");
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
                "session_id": session_id,
            })),
        )?;
    }

    Ok(result)
}

impl CandidateList {
    pub fn has_candidates(&self) -> bool {
        !self.candidates.sources.is_empty()
            || !self.candidates.files.is_empty()
            || !self.candidates.syntheses.is_empty()
    }

    /// Format candidates as a human-readable summary for hook blocking output.
    pub fn format_block_reason(&self) -> String {
        let mut parts = Vec::new();

        if !self.candidates.sources.is_empty() {
            let mut section = String::from("  Sources:\n");
            for s in &self.candidates.sources {
                section.push_str(&format!("  - {} (accessed {}x)\n", s.url, s.times_accessed));
            }
            parts.push(section);
        }

        if !self.candidates.files.is_empty() {
            let mut section = String::from("  Files:\n");
            for f in &self.candidates.files {
                section.push_str(&format!("  - {} (read {}x)\n", f.path, f.times_read));
            }
            parts.push(section);
        }

        if !self.candidates.syntheses.is_empty() {
            let mut section = String::from("  Possible syntheses:\n");
            for s in &self.candidates.syntheses {
                section.push_str(&format!(
                    "  - {} drew from {} unhighlighted sources\n",
                    s.file, s.source_count
                ));
            }
            parts.push(section);
        }

        let total = self.candidates.sources.len()
            + self.candidates.files.len()
            + self.candidates.syntheses.len();

        format!(
            "Cliproot found {} source(s) you consulted but didn't highlight:\n\n{}\n\
             Review: for any source that informed your reasoning, highlight the key passage \
             with cliproot_clip. For syntheses, consider recording derivations with \
             cliproot_derive. Sources you don't highlight will still appear in the provenance \
             graph as consulted-but-not-cited.",
            total,
            parts.join("\n")
        )
    }
}

// ── Synthesis detection ───────────────────────────────────────────────────

fn detect_syntheses(
    entries: &[HookLogEntry],
    config: &ConsolidationConfig,
    repo: &Repository,
    modified_set: &HashSet<&str>,
    _project_root: &str,
) -> Result<Vec<SynthesisCandidate>, Box<dyn std::error::Error>> {
    let mut candidates = Vec::new();
    let lookback_duration = Duration::minutes(config.synthesis_lookback_minutes);

    for (i, entry) in entries.iter().enumerate() {
        // Only look at Write/Edit entries
        if entry.tool_name != "Write" && entry.tool_name != "Edit" {
            continue;
        }

        let written_path = match entry.tool_input.get("file_path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => continue,
        };

        // Skip config files, lock files, generated code
        if is_non_synthesis_target(written_path) {
            continue;
        }

        // Look back through preceding entries within the window
        let mut source_urls: HashSet<String> = HashSet::new();
        let mut source_paths: HashSet<String> = HashSet::new();
        let cutoff_time = entry.timestamp - lookback_duration;
        let start_idx = i.saturating_sub(config.synthesis_lookback_entries);

        for prev in entries
            .iter()
            .take(i)
            .skip(start_idx)
            .cloned()
            .collect::<Vec<_>>()
        {
            if prev.timestamp < cutoff_time {
                continue;
            }

            match prev.tool_name.as_str() {
                "WebFetch" => {
                    if let Some(url) = prev.tool_input.get("url").and_then(|v| v.as_str()) {
                        if !repo.has_clip_for_uri(url)? {
                            source_urls.insert(url.to_string());
                        }
                    }
                }
                "Read" => {
                    if let Some(path) = prev.tool_input.get("file_path").and_then(|v| v.as_str()) {
                        // Skip reads of the same file being written
                        if path == written_path {
                            continue;
                        }
                        // Skip files that are also being modified (editing, not citing)
                        if modified_set.contains(path) {
                            continue;
                        }
                        source_paths.insert(path.to_string());
                    }
                }
                _ => {}
            }
        }

        let source_count = source_urls.len() + source_paths.len();
        if source_count >= config.synthesis_min_sources {
            let mut probable_sources: Vec<ProbableSource> = Vec::new();
            for url in &source_urls {
                probable_sources.push(ProbableSource {
                    url: Some(url.clone()),
                    path: None,
                    source_type: "source".to_string(),
                });
            }
            for path in &source_paths {
                probable_sources.push(ProbableSource {
                    url: None,
                    path: Some(path.clone()),
                    source_type: "file".to_string(),
                });
            }

            candidates.push(SynthesisCandidate {
                file: written_path.to_string(),
                tool: entry.tool_name.clone(),
                timestamp: entry
                    .timestamp
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                source_count,
                probable_sources,
            });
        }
    }

    Ok(candidates)
}

// ── Filters ───────────────────────────────────────────────────────────────

fn is_excluded_url(url: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| url.contains(p.as_str()))
}

fn is_excluded_file(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| path.contains(p.as_str()))
}

fn is_non_synthesis_target(path: &str) -> bool {
    let lower = path.to_lowercase();
    // Config files
    if lower.ends_with(".json")
        && (lower.contains("config")
            || lower.contains("tsconfig")
            || lower.contains("package.json"))
    {
        return true;
    }
    // Lock files
    if lower.ends_with(".lock") || lower.contains("lock.") {
        return true;
    }
    // Generated code markers
    if lower.contains("/generated/") || lower.contains("/dist/") || lower.contains("/build/") {
        return true;
    }
    false
}

// ── CLI entry point ───────────────────────────────────────────────────────

pub fn run(
    session_id: &str,
    emergency: bool,
    commit: bool,
    format: &crate::OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    // Discover .cliproot/ from cwd
    let cwd = std::env::current_dir()?;
    let cliproot_dir = discover_cliproot_dir(&cwd)?;

    let result = run_consolidation(&cliproot_dir, session_id, emergency, commit)?;

    match format {
        crate::OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        _ => {
            if result.has_candidates() {
                println!("{}", result.format_block_reason());
            } else {
                println!("No unclipped sources found.");
            }
            println!(
                "\nStats: {} total events, {} since watermark, {} source candidates, {} file candidates, {} synthesis candidates",
                result.stats.total_events,
                result.stats.since_watermark,
                result.stats.unclipped_sources,
                result.stats.unclipped_files,
                result.stats.synthesis_candidates,
            );
        }
    }

    Ok(())
}

fn discover_cliproot_dir(cwd: &Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let mut dir = cwd.to_path_buf();
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
    use crate::transcript::hook_log::HookLogEntry;

    #[test]
    fn excluded_url_patterns() {
        let patterns = vec!["crates.io".into(), "npmjs.com".into()];
        assert!(is_excluded_url("https://crates.io/crates/serde", &patterns));
        assert!(is_excluded_url(
            "https://www.npmjs.com/package/foo",
            &patterns
        ));
        assert!(!is_excluded_url(
            "https://docs.rust-lang.org/book/",
            &patterns
        ));
    }

    #[test]
    fn excluded_file_patterns() {
        let patterns = vec!["target/".into(), "node_modules/".into()];
        assert!(is_excluded_file("/project/target/debug/foo", &patterns));
        assert!(!is_excluded_file("/project/src/main.rs", &patterns));
    }

    #[test]
    fn non_synthesis_targets() {
        assert!(is_non_synthesis_target("package.json"));
        assert!(is_non_synthesis_target("Cargo.lock"));
        assert!(is_non_synthesis_target("src/generated/schema.rs"));
        assert!(!is_non_synthesis_target("src/auth/pkce.rs"));
    }

    #[test]
    fn synthesis_detection_basic() {
        let now = Utc::now();
        let _entries = vec![
            HookLogEntry {
                timestamp: now - Duration::minutes(5),
                session_id: "s1".into(),
                tool_use_id: "t1".into(),
                tool_name: "WebFetch".into(),
                tool_input: serde_json::json!({"url": "https://example.com/rfc"}),
                tool_response: serde_json::json!({}),
                cwd: "/project".into(),
            },
            HookLogEntry {
                timestamp: now - Duration::minutes(4),
                session_id: "s1".into(),
                tool_use_id: "t2".into(),
                tool_name: "Read".into(),
                tool_input: serde_json::json!({"file_path": "/external/spec.txt"}),
                tool_response: serde_json::json!({}),
                cwd: "/project".into(),
            },
            HookLogEntry {
                timestamp: now - Duration::minutes(1),
                session_id: "s1".into(),
                tool_use_id: "t3".into(),
                tool_name: "Write".into(),
                tool_input: serde_json::json!({"file_path": "/project/src/impl.rs"}),
                tool_response: serde_json::json!({}),
                cwd: "/project".into(),
            },
        ];

        // Can't test with real repo here, but test the filter functions
        assert!(!is_non_synthesis_target("/project/src/impl.rs"));
    }

    #[test]
    fn candidate_list_has_candidates() {
        let empty = CandidateList {
            candidates: Candidates {
                sources: vec![],
                files: vec![],
                syntheses: vec![],
            },
            stats: ConsolidationStats {
                total_events: 0,
                since_watermark: 0,
                unclipped_sources: 0,
                unclipped_files: 0,
                synthesis_candidates: 0,
            },
        };
        assert!(!empty.has_candidates());

        let with_source = CandidateList {
            candidates: Candidates {
                sources: vec![SourceCandidate {
                    url: "https://example.com".into(),
                    title: None,
                    times_accessed: 1,
                    first_seen: "2026-04-11T14:00:00Z".into(),
                    tool: "WebFetch".into(),
                }],
                files: vec![],
                syntheses: vec![],
            },
            stats: ConsolidationStats {
                total_events: 1,
                since_watermark: 1,
                unclipped_sources: 1,
                unclipped_files: 0,
                synthesis_candidates: 0,
            },
        };
        assert!(with_source.has_candidates());
    }

    #[test]
    fn format_block_reason_includes_sources() {
        let list = CandidateList {
            candidates: Candidates {
                sources: vec![SourceCandidate {
                    url: "https://docs.rust-lang.org/book/ch10.html".into(),
                    title: None,
                    times_accessed: 2,
                    first_seen: "2026-04-11T14:00:00Z".into(),
                    tool: "WebFetch".into(),
                }],
                files: vec![],
                syntheses: vec![],
            },
            stats: ConsolidationStats {
                total_events: 10,
                since_watermark: 5,
                unclipped_sources: 1,
                unclipped_files: 0,
                synthesis_candidates: 0,
            },
        };
        let reason = list.format_block_reason();
        assert!(reason.contains("docs.rust-lang.org"));
        assert!(reason.contains("accessed 2x"));
        assert!(reason.contains("cliproot_clip"));
    }

    #[test]
    fn config_default_values() {
        let config = ConsolidationConfig::default();
        assert_eq!(config.base_interval, 10);
        assert_eq!(config.min_interval, 5);
        assert_eq!(config.max_interval, 20);
        assert!(config.adaptive);
        assert_eq!(config.synthesis_lookback_entries, 20);
        assert_eq!(config.synthesis_lookback_minutes, 30);
        assert_eq!(config.synthesis_min_sources, 2);
    }

    #[test]
    fn config_loads_from_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_json = serde_json::json!({
            "protocolVersion": "0.0.3",
            "consolidation": {
                "base_interval": 15,
                "min_interval": 3,
                "adaptive": false
            }
        });
        fs::write(
            dir.path().join("config.json"),
            serde_json::to_string_pretty(&config_json).unwrap(),
        )
        .unwrap();

        let config = load_config(dir.path());
        assert_eq!(config.base_interval, 15);
        assert_eq!(config.min_interval, 3);
        assert!(!config.adaptive);
        // Unset fields get defaults
        assert_eq!(config.max_interval, 20);
        assert_eq!(config.synthesis_lookback_entries, 20);
    }
}
