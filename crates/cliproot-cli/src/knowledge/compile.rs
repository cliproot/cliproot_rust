//! `cliproot wiki compile` — materialise the wiki.
//!
//! Phase D.  Runs either manually from the CLI or as the chained second stage
//! of a background flush (after `flush::run_flush` succeeds).  Reads today's
//! daily digest + the existing `index.md` + any substring-matched prior
//! articles, hands them to Claude-Sonnet-4-6 to synthesise updated
//! concept/connection/qa articles, then:
//!
//! 1. Parses the LLM response into per-file sections.
//! 2. Writes each section via [`article::write_article`], which preserves
//!    UUIDs across recompiles.
//! 3. Scans each written body for inline `[cliproot:sha256-...]` citations and
//!    creates `CitedIn` edges in the CRP repo.
//! 4. Registers each article as a Markdown artifact.
//! 5. Rebuilds `index.md` deterministically from the articles on disk (the
//!    LLM does not author the index).
//! 6. Emits a single `Activity(type=Derive)` covering the whole run.
//! 7. Updates `state.last_compile_hash` so the next PostFlush run no-ops.
//!
//! # Prompt caching — deferred to Phase F
//!
//! The compile prompt is large enough that Anthropic prompt-caching would help
//! in theory, but our cadence is at most once per day so the cache-hit payoff
//! is negligible.  Adding `cache_control` blocks would require refactoring
//! [`llm::call`]'s wire format.  Revisit alongside Phase F's multi-call
//! features (query, wiki-lint).

use std::fs;
use std::path::{Path, PathBuf};

use cliproot_core::model::{
    Activity, ActivityType, ArtifactType, BundleType, ClipArtifactRelationship, CrpBundle, CrpId,
};
use cliproot_store::Repository;
use sha2::{Digest, Sha256};

use super::article::{self, ArticleType, ArticleWriteResult};
use super::index::{self, Index, IndexEntry};
use super::llm;
use super::state;

// ── Outcome + trigger ─────────────────────────────────────────────────────────

/// Why the compile ran.  Affects only the time-of-day gate — all other
/// decisions (idempotency, budget) apply equally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileTrigger {
    /// `cliproot compile` invoked from the CLI.  Ignores `compile_after_hour`.
    Manual,
    /// Chained from a successful flush.  Honours `compile_after_hour`.
    PostFlush,
}

#[derive(Debug)]
pub enum CompileOutcome {
    Success {
        articles_written: Vec<String>,
        tokens_used: u64,
    },
    Skipped(String),
    BudgetExceeded(String),
    Error(String),
}

impl std::fmt::Display for CompileOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success {
                articles_written,
                tokens_used,
            } => write!(
                f,
                "SUCCESS articles={} tokens={tokens_used}",
                articles_written.len()
            ),
            Self::Skipped(r) => write!(f, "SKIPPED {r}"),
            Self::BudgetExceeded(r) => write!(f, "BUDGET_EXCEEDED {r}"),
            Self::Error(r) => write!(f, "ERROR {r}"),
        }
    }
}

// ── Constants ─────────────────────────────────────────────────────────────────

/// Upper bound on tokens charged against the daily budget for a single
/// compile.  Intentionally generous — real usage is ~20–40k combined.
const ESTIMATED_TOKENS_PER_COMPILE: u64 = 40_000;
const MAX_OUTPUT_TOKENS: u32 = 8_192;

// ── LLM indirection for tests ─────────────────────────────────────────────────

/// Callable matching the signature of [`llm::call`].  Tests substitute a
/// stub so no real HTTP call is made.
pub type LlmCallFn =
    dyn Fn(&str, &str, &str, u32) -> Result<llm::LlmCallResult, Box<dyn std::error::Error>>;

// ── Entry points ──────────────────────────────────────────────────────────────

/// Run a compile using the real Anthropic API.  Intended callers: the
/// `cliproot compile` command and the in-process chain from `flush_hook`.
pub fn run_compile(
    cliproot_dir: &Path,
    repo: &Repository,
    trigger: CompileTrigger,
) -> CompileOutcome {
    run_compile_with_llm(cliproot_dir, repo, trigger, &|s, u, m, t| {
        llm::call(s, u, m, t)
    })
}

/// Same as [`run_compile`] but with an injectable LLM call.  Pub(crate)
/// because test code lives in `tests/` integration style, outside this
/// module.  Kept pub so integration tests can reach it.
pub fn run_compile_with_llm(
    cliproot_dir: &Path,
    repo: &Repository,
    trigger: CompileTrigger,
    llm_call: &LlmCallFn,
) -> CompileOutcome {
    match run_compile_inner(cliproot_dir, repo, trigger, llm_call) {
        Ok(outcome) => outcome,
        Err(e) => CompileOutcome::Error(e.to_string()),
    }
}

// ── Main flow ─────────────────────────────────────────────────────────────────

fn run_compile_inner(
    cliproot_dir: &Path,
    repo: &Repository,
    trigger: CompileTrigger,
    llm_call: &LlmCallFn,
) -> Result<CompileOutcome, Box<dyn std::error::Error>> {
    let knowledge_dir = cliproot_dir.join("knowledge");

    // 1. Config + gating.
    let cfg = repo.knowledge_config()?;
    if !cfg.level.allows_compile() {
        return Ok(CompileOutcome::Skipped(format!(
            "level {:?} does not allow compile",
            cfg.level
        )));
    }
    if trigger == CompileTrigger::PostFlush {
        let hour_now = chrono::Local::now().format("%H").to_string();
        if let Ok(h) = hour_now.parse::<u8>() {
            if h < cfg.compile_after_hour {
                return Ok(CompileOutcome::Skipped(format!(
                    "before compile_after_hour (now {h}, threshold {})",
                    cfg.compile_after_hour
                )));
            }
        }
    }

    let mut flush_state = state::load(&knowledge_dir)?;
    state::reset_budget_if_new_day(&mut flush_state);

    // 2. Locate the daily we will feed into compile.
    let daily_path = match latest_daily(&knowledge_dir)? {
        Some(p) => p,
        None => {
            return Ok(CompileOutcome::Skipped("no daily corpus yet".to_string()));
        }
    };
    let daily_body = fs::read_to_string(&daily_path)?;

    // 3. Idempotency: fingerprint today's daily + the entire article corpus.
    let corpus_hash = compute_corpus_hash(&knowledge_dir, &daily_body)?;
    if !flush_state.needs_compile(&corpus_hash) {
        return Ok(CompileOutcome::Skipped(
            "no changes since last compile".to_string(),
        ));
    }

    // 4. Budget pre-check.
    if let Err(e) = llm::check_budget(&flush_state, &cfg, ESTIMATED_TOKENS_PER_COMPILE) {
        append_log_line(&knowledge_dir, &format!("BUDGET_EXCEEDED {}", e.reason));
        return Ok(CompileOutcome::BudgetExceeded(e.reason));
    }

    // 5. Load existing index + pick articles to include as context.
    let existing_index = index::read(&knowledge_dir)?.unwrap_or_default();
    let daily_concepts = extract_concept_hints(&daily_body);
    let selected: Vec<IndexEntry> =
        index::select_articles_for_compile(&existing_index, &daily_concepts)
            .into_iter()
            .cloned()
            .collect();
    let article_bodies = read_selected_article_bodies(&knowledge_dir, &selected);

    // 6. Build + send the prompt.
    let article_types = super::article_types::load_article_types(cliproot_dir);
    let system = compile_system_prompt();
    let user = build_user_prompt(
        &daily_path,
        &daily_body,
        &existing_index,
        &article_bodies,
        &article_types,
    );
    let result = llm_call(&system, &user, &cfg.models.compile, MAX_OUTPUT_TOKENS)?;

    // 7. Parse the response into per-file sections.
    let sections = parse_compile_response(&result.text);
    if sections.is_empty() {
        append_log_line(
            &knowledge_dir,
            "NO_SECTIONS compile response contained no `### FILE:` blocks",
        );
        return Ok(CompileOutcome::Error(
            "compile response contained no `### FILE:` blocks".to_string(),
        ));
    }

    // 8. Write articles + link citations.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut written: Vec<ArticleWriteResult> = Vec::new();
    let mut all_linked_clip_hashes: Vec<String> = Vec::new();
    let daily_source_ref = relative_daily_source(&daily_path);

    for section in &sections {
        let Some((article_type, slug)) = parse_file_header(&section.header) else {
            continue;
        };
        let write_res = article::write_article(
            &knowledge_dir,
            article_type,
            &slug,
            &section.title,
            &section.body,
            &[daily_source_ref.clone()],
            &section.clip_hashes_from_body(),
            None,
        )?;

        // Register as artifact.
        let artifact = repo.add_artifact(
            Some(&write_res.path),
            None,
            Some(&format!("{slug}.md")),
            ArtifactType::Markdown,
            Some("text/markdown"),
            None,
            None,
            Some(serde_json::json!({
                "artifactType": article_type.as_slug(),
                "canonicalKey": write_res.canonical_key,
                "compileRun": today,
                "model": result.model,
            })),
        )?;
        let artifact_hash = artifact.artifact_hash.0.clone();

        // Link citations.
        for clip_hash in section.clip_hashes_from_body() {
            if repo
                .link_clip_artifact(
                    &clip_hash,
                    &artifact_hash,
                    ClipArtifactRelationship::CitedIn,
                )
                .is_ok()
                && !all_linked_clip_hashes.contains(&clip_hash)
            {
                all_linked_clip_hashes.push(clip_hash);
            }
        }

        written.push(write_res);
    }

    // 9. Rebuild index.md from everything on disk.
    let rebuilt_index = rebuild_index(&knowledge_dir, &today)?;
    index::write(&knowledge_dir, &rebuilt_index)?;

    // 10. Record a single Activity for the whole compile run.
    record_compile_activity(repo, &result, &written, &all_linked_clip_hashes)?;

    // 11. Update state + log line.  Recompute the corpus hash _after_ writing
    // so that a subsequent run (which will read the now-present articles) sees
    // the same fingerprint and short-circuits via `needs_compile`.
    let post_corpus_hash = compute_corpus_hash(&knowledge_dir, &daily_body)?;
    flush_state.last_compile_hash = Some(post_corpus_hash);
    flush_state.daily_total_tokens += result.total_tokens;
    flush_state.daily_total_cost_usd += result.estimated_cost_usd;
    state::save(&flush_state, &knowledge_dir)?;

    append_log_line(
        &knowledge_dir,
        &format!(
            "SUCCESS compile articles={} tokens={} cost=${:.4}",
            written.len(),
            result.total_tokens,
            result.estimated_cost_usd,
        ),
    );

    Ok(CompileOutcome::Success {
        articles_written: written
            .into_iter()
            .map(|r| r.path.display().to_string())
            .collect(),
        tokens_used: result.total_tokens,
    })
}

// ── Daily location ────────────────────────────────────────────────────────────

fn latest_daily(knowledge_dir: &Path) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let daily_dir = knowledge_dir.join("daily");
    let Ok(entries) = fs::read_dir(&daily_dir) else {
        return Ok(None);
    };
    // Filenames are YYYY-MM-DD.md; string-sort == date-sort.
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .collect();
    candidates.sort();
    Ok(candidates.last().cloned())
}

fn relative_daily_source(daily_path: &Path) -> String {
    daily_path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|n| format!("daily/{n}"))
        .unwrap_or_else(|| daily_path.display().to_string())
}

// ── Corpus hash ───────────────────────────────────────────────────────────────

/// SHA-256 of: sorted `(relpath, contentHash)` pairs across every article on
/// disk, concatenated with today's daily body hash.  Gives us a single
/// fingerprint that changes iff any article's body or the daily has changed.
fn compute_corpus_hash(
    knowledge_dir: &Path,
    daily_body: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut rows: Vec<(String, String)> = Vec::new();
    for subdir in ["concepts", "connections", "qa"] {
        let dir = knowledge_dir.join(subdir);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let content = fs::read_to_string(&path).unwrap_or_default();
            // Use the article's contentHash frontmatter if present; otherwise
            // hash the whole file (covers legacy imports).
            let ch = article::read_content_hash_from_file(&path).unwrap_or_else(|| {
                let mut h = Sha256::new();
                h.update(content.as_bytes());
                format!("sha256-raw-{}", hex::encode(h.finalize()))
            });
            let rel = format!(
                "{subdir}/{}",
                path.file_name().and_then(|s| s.to_str()).unwrap_or("")
            );
            rows.push((rel, ch));
        }
    }
    rows.sort();

    let mut hasher = Sha256::new();
    for (rel, ch) in &rows {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(ch.as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(b"DAILY\0");
    hasher.update(daily_body.as_bytes());
    Ok(hex_encode(&hasher.finalize()))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

// Tiny inline `hex` module; we avoid pulling a new dep for one routine.
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        super::hex_encode(bytes.as_ref())
    }
}

// ── Concept extraction from daily ─────────────────────────────────────────────

/// Very lightweight extraction: headings + bullet-list items from the daily
/// body give us enough substrings to feed `index::select_articles_for_compile`.
fn extract_concept_hints(daily_body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in daily_body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            out.push(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("- ") {
            out.push(rest.trim().to_string());
        }
    }
    out
}

fn read_selected_article_bodies(
    knowledge_dir: &Path,
    entries: &[IndexEntry],
) -> Vec<(IndexEntry, String)> {
    let mut out = Vec::new();
    for e in entries {
        let path = knowledge_dir.join(e.relative_path());
        if let Ok(body) = fs::read_to_string(&path) {
            out.push((e.clone(), body));
        }
    }
    out
}

// ── Prompt templates ──────────────────────────────────────────────────────────

fn compile_system_prompt() -> String {
    "You are a knowledge curator compiling durable wiki articles from the \
user's daily digest.  Given today's digest plus any existing articles that \
touch the same topics, produce updated article bodies.\n\n\
Use the user's own terminology — mirror the vocabulary already present in \
their digests and existing articles.  Avoid importing domain-specific jargon \
unless it appears in the source material.\n\n\
Response format — STRICT:\n\
For each article you create or update, emit exactly one section:\n\
\n\
### FILE: <subdir>/<slug>.md\n\
TITLE: <human-readable title>\n\
TAGS: <tag1>, <tag2>\n\
BODY:\n\
<markdown body with inline `[cliproot:sha256-...]` citations>\n\
\n\
Where `<subdir>` is one of `concepts`, `connections`, `qa`, and `<slug>` is a \
kebab-case identifier.  Preserve clip citations verbatim from the daily log.  \
Do NOT emit an index.md section — the pipeline rebuilds the index \
deterministically.  Do NOT emit prose outside these sections."
        .to_string()
}

fn build_user_prompt(
    daily_path: &Path,
    daily_body: &str,
    existing_index: &Index,
    article_bodies: &[(IndexEntry, String)],
    article_types: &[String],
) -> String {
    let mut user = String::new();
    if !article_types.is_empty() {
        user.push_str(&format!(
            "Known article types in the user's vocabulary: {}\n\n",
            article_types.join(", ")
        ));
    }
    user.push_str(&format!(
        "Today's daily digest ({}):\n{daily_body}\n\n",
        daily_path.display()
    ));

    user.push_str("Existing index (titles and uuids):\n");
    if existing_index.entries.is_empty() {
        user.push_str("(none)\n");
    } else {
        for e in &existing_index.entries {
            user.push_str(&format!(
                "- {} ({}) [{}]\n",
                e.title,
                e.article_type.as_slug(),
                e.uuid
            ));
        }
    }
    user.push('\n');

    if !article_bodies.is_empty() {
        user.push_str("Existing article bodies (for reference — update these in place if you touch them):\n\n");
        for (entry, body) in article_bodies {
            user.push_str(&format!(
                "--- {} ---\n{body}\n\n",
                entry.relative_path().display()
            ));
        }
    }

    user
}

// ── Response parsing ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct CompileSection {
    header: String, // the `### FILE: ...` line content, sans prefix
    title: String,
    #[allow(dead_code)]
    tags: Vec<String>,
    body: String,
}

impl CompileSection {
    fn clip_hashes_from_body(&self) -> Vec<String> {
        article::extract_citations_from_markdown(&self.body)
    }
}

fn parse_compile_response(raw: &str) -> Vec<CompileSection> {
    let marker = "### FILE: ";
    let mut sections: Vec<CompileSection> = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None; // header, body lines

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix(marker) {
            if let Some((hdr, body)) = current.take() {
                sections.push(finish_section(hdr, body));
            }
            current = Some((rest.trim().to_string(), Vec::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push(line.to_string());
        }
    }
    if let Some((hdr, body)) = current.take() {
        sections.push(finish_section(hdr, body));
    }
    sections
}

fn finish_section(header: String, body_lines: Vec<String>) -> CompileSection {
    let mut title = String::new();
    let mut tags: Vec<String> = Vec::new();
    let mut body_start: Option<usize> = None;

    for (i, line) in body_lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("TITLE:") {
            title = rest.trim().to_string();
        } else if let Some(rest) = trimmed.strip_prefix("TAGS:") {
            tags = rest
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        } else if trimmed == "BODY:" {
            body_start = Some(i + 1);
            break;
        }
    }

    let body = match body_start {
        Some(idx) => body_lines[idx..].join("\n").trim_end().to_string(),
        None => body_lines.join("\n").trim_end().to_string(),
    };

    CompileSection {
        header,
        title,
        tags,
        body,
    }
}

fn parse_file_header(header: &str) -> Option<(ArticleType, String)> {
    // Expect "<subdir>/<slug>.md".  Strip any leading `./`, trim whitespace.
    let h = header.trim_start_matches("./").trim();
    let (subdir, rest) = h.split_once('/')?;
    let slug = rest.strip_suffix(".md")?.to_string();
    if slug.is_empty() {
        return None;
    }
    let kind = match subdir {
        "concepts" => ArticleType::Concept,
        "connections" => ArticleType::Connection,
        "qa" => ArticleType::Qa,
        _ => return None,
    };
    Some((kind, slug))
}

// ── Index rebuild ─────────────────────────────────────────────────────────────

/// Walk the three article subdirectories and rebuild `Index` from what is
/// currently on disk.  Does NOT call the LLM — we trust what write_article
/// persisted.
fn rebuild_index(knowledge_dir: &Path, today: &str) -> Result<Index, Box<dyn std::error::Error>> {
    let mut entries: Vec<IndexEntry> = Vec::new();
    for (subdir, kind) in [
        ("concepts", ArticleType::Concept),
        ("connections", ArticleType::Connection),
        ("qa", ArticleType::Qa),
    ] {
        let dir = knowledge_dir.join(subdir);
        let Ok(items) = fs::read_dir(&dir) else {
            continue;
        };
        for item in items.flatten() {
            let path = item.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let content = fs::read_to_string(&path).unwrap_or_default();
            let title =
                parse_frontmatter_field(&content, "title").unwrap_or_else(|| stem.to_string());
            let uuid = parse_frontmatter_field(&content, "uuid").unwrap_or_default();
            let canonical_key = parse_frontmatter_field(&content, "canonicalKey")
                .unwrap_or_else(|| stem.to_string());
            entries.push(IndexEntry {
                uuid,
                canonical_key,
                title,
                article_type: kind,
                tags: Vec::new(),
                last_seen: today.to_string(),
            });
        }
    }
    entries.sort_by(|a, b| a.title.cmp(&b.title));

    Ok(Index {
        schema_version: index::SCHEMA_VERSION,
        generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        entries,
    })
}

/// Local duplicate of the article-module private helper.  Kept private to
/// this module to avoid widening `article`'s public surface for one caller.
fn parse_frontmatter_field(content: &str, field: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let prefix = format!("{field}:");
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some(rest) = line.strip_prefix(&prefix) {
            return Some(rest.trim().trim_matches('"').to_string());
        }
    }
    None
}

// ── CRP activity ──────────────────────────────────────────────────────────────

fn record_compile_activity(
    repo: &Repository,
    result: &llm::LlmCallResult,
    written: &[ArticleWriteResult],
    used_clip_hashes: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let activity = Activity {
        id: CrpId(format!("act-{}", uuid::Uuid::new_v4())),
        activity_type: ActivityType::Derive,
        project_id: None,
        agent_id: None,
        prompt: None,
        parameters: Some(serde_json::json!({
            "operation": "compile",
            "promptHash": result.prompt_hash,
            "model": result.model,
            "maxTokens": MAX_OUTPUT_TOKENS,
            "inputTokens": result.input_tokens,
            "outputTokens": result.output_tokens,
            "articlesWritten": written.len(),
        })),
        used_source_refs: used_clip_hashes.to_vec(),
        generated_clip_refs: Vec::new(),
        created_at: now.clone(),
        ended_at: Some(now.clone()),
    };

    let bundle = CrpBundle {
        protocol_version: "0.0.3".to_string(),
        bundle_type: BundleType::Derivation,
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

// ── Log helper ────────────────────────────────────────────────────────────────

fn append_log_line(knowledge_dir: &Path, message: &str) {
    let log_path = knowledge_dir.join("log.md");
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let line = format!("- `{timestamp}` {message}\n");
    let _ = fs::create_dir_all(knowledge_dir);
    let existing = fs::read_to_string(&log_path).unwrap_or_default();
    let _ = fs::write(&log_path, format!("{existing}{line}"));
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_compile_response_single_section() {
        let raw = "### FILE: concepts/pkce-flow.md\n\
            TITLE: PKCE Flow\n\
            TAGS: oauth, pkce\n\
            BODY:\n\
            ## Overview\n\
            PKCE is... [cliproot:sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa].\n";
        let got = parse_compile_response(raw);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].header, "concepts/pkce-flow.md");
        assert_eq!(got[0].title, "PKCE Flow");
        assert_eq!(got[0].tags, vec!["oauth", "pkce"]);
        assert!(got[0].body.contains("PKCE is..."));
    }

    #[test]
    fn parse_compile_response_multiple_sections() {
        let raw = "### FILE: concepts/a.md\n\
            TITLE: A\n\
            TAGS: x\n\
            BODY:\n\
            body A\n\
            ### FILE: concepts/b.md\n\
            TITLE: B\n\
            TAGS:\n\
            BODY:\n\
            body B\n";
        let got = parse_compile_response(raw);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].title, "A");
        assert_eq!(got[1].title, "B");
    }

    #[test]
    fn parse_file_header_concepts() {
        let (kind, slug) = parse_file_header("concepts/pkce-flow.md").unwrap();
        assert_eq!(kind, ArticleType::Concept);
        assert_eq!(slug, "pkce-flow");
    }

    #[test]
    fn parse_file_header_connections_and_qa() {
        assert_eq!(
            parse_file_header("connections/oauth-vs-oidc.md").unwrap().0,
            ArticleType::Connection
        );
        assert_eq!(
            parse_file_header("qa/what-is-pkce.md").unwrap().0,
            ArticleType::Qa
        );
    }

    #[test]
    fn parse_file_header_rejects_unknown_subdir() {
        assert!(parse_file_header("misc/x.md").is_none());
        assert!(parse_file_header("concepts/.md").is_none());
        assert!(parse_file_header("concepts/x.txt").is_none());
    }

    #[test]
    fn extract_concept_hints_picks_headings_and_bullets() {
        let daily = "## Summary\nSome text.\n## Key Decisions\n- Use PKCE\n- Pick OAuth 2.0\n## Sources Consulted\n";
        let got = extract_concept_hints(daily);
        assert!(got.contains(&"Summary".to_string()));
        assert!(got.contains(&"Use PKCE".to_string()));
        assert!(got.contains(&"Pick OAuth 2.0".to_string()));
    }

    #[test]
    fn latest_daily_picks_largest_name() {
        let dir = tempfile::tempdir().unwrap();
        let knowledge_dir = dir.path().join("knowledge");
        let daily_dir = knowledge_dir.join("daily");
        fs::create_dir_all(&daily_dir).unwrap();
        fs::write(daily_dir.join("2026-04-10.md"), "old").unwrap();
        fs::write(daily_dir.join("2026-04-12.md"), "newer").unwrap();
        fs::write(daily_dir.join("2026-04-11.md"), "mid").unwrap();
        let got = latest_daily(&knowledge_dir).unwrap().unwrap();
        assert!(got.to_string_lossy().ends_with("2026-04-12.md"));
    }

    #[test]
    fn corpus_hash_stable_across_runs() {
        let dir = tempfile::tempdir().unwrap();
        let knowledge_dir = dir.path().join("knowledge");
        fs::create_dir_all(&knowledge_dir).unwrap();
        let h1 = compute_corpus_hash(&knowledge_dir, "daily body").unwrap();
        let h2 = compute_corpus_hash(&knowledge_dir, "daily body").unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn corpus_hash_changes_with_daily_body() {
        let dir = tempfile::tempdir().unwrap();
        let knowledge_dir = dir.path().join("knowledge");
        fs::create_dir_all(&knowledge_dir).unwrap();
        let h1 = compute_corpus_hash(&knowledge_dir, "body A").unwrap();
        let h2 = compute_corpus_hash(&knowledge_dir, "body B").unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn corpus_hash_changes_when_article_added() {
        let dir = tempfile::tempdir().unwrap();
        let knowledge_dir = dir.path().join("knowledge");
        fs::create_dir_all(&knowledge_dir).unwrap();

        let h_before = compute_corpus_hash(&knowledge_dir, "daily").unwrap();
        let _ = article::write_article(
            &knowledge_dir,
            ArticleType::Concept,
            "pkce",
            "PKCE",
            "body",
            &[],
            &[],
            None,
        )
        .unwrap();
        let h_after = compute_corpus_hash(&knowledge_dir, "daily").unwrap();
        assert_ne!(h_before, h_after);
    }

    #[test]
    fn skipped_when_level_below_wiki() {
        let dir = tempfile::tempdir().unwrap();
        let repo = cliproot_store::Repository::init(dir.path()).unwrap();
        let cliproot_dir = dir.path().join(".cliproot");

        // Default level is Curator — below Wiki, so compile is a no-op.
        let outcome = run_compile(&cliproot_dir, &repo, CompileTrigger::Manual);
        assert!(matches!(outcome, CompileOutcome::Skipped(_)));
    }

    #[test]
    fn skipped_when_no_daily_exists() {
        let dir = tempfile::tempdir().unwrap();
        let repo = cliproot_store::Repository::init(dir.path()).unwrap();
        let cliproot_dir = dir.path().join(".cliproot");

        let mut cfg = repo.knowledge_config().unwrap();
        cfg.level = cliproot_store::KnowledgeLevel::Wiki;
        repo.set_knowledge_config(cfg).unwrap();

        let outcome = run_compile(&cliproot_dir, &repo, CompileTrigger::Manual);
        match outcome {
            CompileOutcome::Skipped(r) => assert!(r.contains("no daily")),
            other => panic!("expected Skipped, got {other:?}"),
        }
    }

    #[test]
    fn compile_idempotent_second_run_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let repo = cliproot_store::Repository::init(dir.path()).unwrap();
        let cliproot_dir = dir.path().join(".cliproot");
        let knowledge_dir = cliproot_dir.join("knowledge");

        // Level = Wiki.
        let mut cfg = repo.knowledge_config().unwrap();
        cfg.level = cliproot_store::KnowledgeLevel::Wiki;
        repo.set_knowledge_config(cfg).unwrap();

        // Write a daily so compile has something to operate on.
        article::write_daily_digest(
            &knowledge_dir,
            "2026-04-13",
            "## Summary\nWorked on PKCE.",
            None,
        )
        .unwrap();

        // Mock LLM emits exactly one article + no other output.
        let mock = |_: &str, _: &str, model: &str, _: u32| {
            Ok(llm::LlmCallResult {
                text: "### FILE: concepts/pkce-flow.md\n\
                       TITLE: PKCE Flow\n\
                       TAGS: oauth\n\
                       BODY:\n\
                       PKCE body.\n"
                    .to_string(),
                input_tokens: 100,
                output_tokens: 50,
                total_tokens: 150,
                estimated_cost_usd: 0.001,
                model: model.to_string(),
                prompt_hash: "testhash".to_string(),
            })
        };

        let first = run_compile_with_llm(&cliproot_dir, &repo, CompileTrigger::Manual, &mock);
        assert!(
            matches!(first, CompileOutcome::Success { .. }),
            "first: {first:?}"
        );

        // Stub that would fail if called — proves second run skips LLM.
        let mock_fail = |_: &str,
                         _: &str,
                         _: &str,
                         _: u32|
         -> Result<llm::LlmCallResult, Box<dyn std::error::Error>> {
            Err("LLM must not be called on an idempotent second compile".into())
        };
        let second = run_compile_with_llm(&cliproot_dir, &repo, CompileTrigger::Manual, &mock_fail);
        match second {
            CompileOutcome::Skipped(r) => assert!(r.contains("no changes")),
            other => panic!("expected Skipped on second run, got {other:?}"),
        }
    }

    #[test]
    fn compile_writes_article_and_index() {
        let dir = tempfile::tempdir().unwrap();
        let repo = cliproot_store::Repository::init(dir.path()).unwrap();
        let cliproot_dir = dir.path().join(".cliproot");
        let knowledge_dir = cliproot_dir.join("knowledge");

        let mut cfg = repo.knowledge_config().unwrap();
        cfg.level = cliproot_store::KnowledgeLevel::Wiki;
        repo.set_knowledge_config(cfg).unwrap();

        article::write_daily_digest(&knowledge_dir, "2026-04-13", "## Work\n- PKCE", None).unwrap();

        let mock = |_: &str, _: &str, model: &str, _: u32| {
            Ok(llm::LlmCallResult {
                text: "### FILE: concepts/pkce-flow.md\n\
                       TITLE: PKCE Flow\n\
                       TAGS: oauth\n\
                       BODY:\n\
                       Cite [cliproot:sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa] here.\n"
                    .to_string(),
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                estimated_cost_usd: 0.0,
                model: model.to_string(),
                prompt_hash: "h".to_string(),
            })
        };

        let outcome = run_compile_with_llm(&cliproot_dir, &repo, CompileTrigger::Manual, &mock);
        assert!(matches!(outcome, CompileOutcome::Success { .. }));

        // Article exists.
        let article_path = knowledge_dir.join("concepts/pkce-flow.md");
        assert!(article_path.exists());
        let body = fs::read_to_string(&article_path).unwrap();
        assert!(body.contains("uuid:"));
        assert!(body.contains("canonicalKey: pkce-flow"));

        // Index rebuilt.
        let idx = index::read(&knowledge_dir).unwrap().unwrap();
        assert_eq!(idx.entries.len(), 1);
        assert_eq!(idx.entries[0].title, "PKCE Flow");

        // State updated.
        let st = state::load(&knowledge_dir).unwrap();
        assert!(st.last_compile_hash.is_some());
    }

    #[test]
    fn compile_preserves_uuid_across_reruns() {
        let dir = tempfile::tempdir().unwrap();
        let repo = cliproot_store::Repository::init(dir.path()).unwrap();
        let cliproot_dir = dir.path().join(".cliproot");
        let knowledge_dir = cliproot_dir.join("knowledge");

        let mut cfg = repo.knowledge_config().unwrap();
        cfg.level = cliproot_store::KnowledgeLevel::Wiki;
        repo.set_knowledge_config(cfg).unwrap();

        article::write_daily_digest(&knowledge_dir, "2026-04-13", "## Day one", None).unwrap();

        let mock_v1 = |_: &str, _: &str, model: &str, _: u32| {
            Ok(llm::LlmCallResult {
                text: "### FILE: concepts/pkce-flow.md\n\
                       TITLE: PKCE Flow\nTAGS:\nBODY:\nV1\n"
                    .into(),
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
                estimated_cost_usd: 0.0,
                model: model.to_string(),
                prompt_hash: "h1".into(),
            })
        };
        let _ = run_compile_with_llm(&cliproot_dir, &repo, CompileTrigger::Manual, &mock_v1);
        let uuid1 =
            article::read_uuid_from_file(&knowledge_dir.join("concepts/pkce-flow.md")).unwrap();

        // Modify daily so idempotency key changes, then recompile.
        article::write_daily_digest(&knowledge_dir, "2026-04-13", "## Day two — changed", None)
            .unwrap();
        let mock_v2 = |_: &str, _: &str, model: &str, _: u32| {
            Ok(llm::LlmCallResult {
                text: "### FILE: concepts/pkce-flow.md\n\
                       TITLE: PKCE Flow\nTAGS:\nBODY:\nV2 updated body\n"
                    .into(),
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
                estimated_cost_usd: 0.0,
                model: model.to_string(),
                prompt_hash: "h2".into(),
            })
        };
        let _ = run_compile_with_llm(&cliproot_dir, &repo, CompileTrigger::Manual, &mock_v2);
        let uuid2 =
            article::read_uuid_from_file(&knowledge_dir.join("concepts/pkce-flow.md")).unwrap();

        assert_eq!(uuid1, uuid2, "UUID must persist across recompiles");
        // Body content changed.
        let body = fs::read_to_string(knowledge_dir.join("concepts/pkce-flow.md")).unwrap();
        assert!(body.contains("V2 updated body"));
    }

    #[test]
    fn compile_budget_exceeded_logs_and_returns() {
        let dir = tempfile::tempdir().unwrap();
        let repo = cliproot_store::Repository::init(dir.path()).unwrap();
        let cliproot_dir = dir.path().join(".cliproot");
        let knowledge_dir = cliproot_dir.join("knowledge");

        let mut cfg = repo.knowledge_config().unwrap();
        cfg.level = cliproot_store::KnowledgeLevel::Wiki;
        cfg.max_bg_tokens_per_day = 0;
        repo.set_knowledge_config(cfg).unwrap();

        article::write_daily_digest(&knowledge_dir, "2026-04-13", "## Work", None).unwrap();

        let mock_never_called = |_: &str,
                                 _: &str,
                                 _: &str,
                                 _: u32|
         -> Result<llm::LlmCallResult, Box<dyn std::error::Error>> {
            panic!("budget check should have short-circuited before the LLM call");
        };
        let outcome = run_compile_with_llm(
            &cliproot_dir,
            &repo,
            CompileTrigger::Manual,
            &mock_never_called,
        );
        assert!(matches!(outcome, CompileOutcome::BudgetExceeded(_)));
        let log = fs::read_to_string(knowledge_dir.join("log.md")).unwrap();
        assert!(log.contains("BUDGET_EXCEEDED"));
    }
}
