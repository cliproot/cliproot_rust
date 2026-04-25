//! `cliproot wiki query` — two-phase retrieval over the compiled wiki.
//!
//! Phase F.  Phase 1 extracts 3–8 keywords from the user prompt via a cheap
//! Haiku call (JSON list), then uses deterministic
//! [`index::select_articles_for_compile`] to pick candidate articles.  Phase 2
//! feeds the selected bodies back to Haiku and asks for an answer with inline
//! `[cliproot:sha256-…]` citations preferred over `[[wikilinks]]`.
//!
//! Every query records a single `Activity(type=Research)` so answers are
//! auditable end-to-end.  With `--file-back`, the answer is persisted as a
//! `qa/<slug>.md` article and the cited clips get `CitedIn` artifact edges.

use std::fs;
use std::path::{Path, PathBuf};

use cliproot_core::model::{
    Activity, ActivityType, ArtifactType, BundleType, ClipArtifactRelationship, CrpBundle, CrpId,
};
use cliproot_store::Repository;
use serde::Serialize;

use super::article::{self, ArticleType};
use super::index::{self, IndexEntry};
use super::llm;
use super::state;

// ── Options + outcome ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct QueryOpts {
    pub file_back: bool,
    pub top_k: usize,
}

/// Injectable LLM call matching [`llm::call`].  Tests substitute a stub so no
/// real HTTP call is made.
pub type LlmCallFn =
    dyn Fn(&str, &str, &str, u32) -> Result<llm::LlmCallResult, Box<dyn std::error::Error>>;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum QueryOutcome {
    Answer {
        text: String,
        cited_clips: Vec<String>,
        consulted_articles: Vec<String>,
        qa_path: Option<PathBuf>,
    },
    BudgetExceeded(String),
    Skipped(String),
    Error(String),
}

// ── Constants ─────────────────────────────────────────────────────────────────

const ESTIMATED_TOKENS_PER_QUERY: u64 = 10_000;
const MAX_KEYWORDS_OUTPUT_TOKENS: u32 = 256;
const MAX_ANSWER_OUTPUT_TOKENS: u32 = 2_048;

// ── Entry points ──────────────────────────────────────────────────────────────

pub fn run_query(
    prompt: &str,
    cliproot_dir: &Path,
    repo: &Repository,
    opts: QueryOpts,
    llm_call: &LlmCallFn,
) -> QueryOutcome {
    match run_query_inner(prompt, cliproot_dir, repo, opts, llm_call) {
        Ok(outcome) => outcome,
        Err(e) => QueryOutcome::Error(e.to_string()),
    }
}

// ── Main flow ─────────────────────────────────────────────────────────────────

fn run_query_inner(
    prompt: &str,
    cliproot_dir: &Path,
    repo: &Repository,
    opts: QueryOpts,
    llm_call: &LlmCallFn,
) -> Result<QueryOutcome, Box<dyn std::error::Error>> {
    let knowledge_dir = cliproot_dir.join("knowledge");

    let cfg = repo.knowledge_config()?;
    if !cfg.level.allows_compile() {
        return Ok(QueryOutcome::Skipped(format!(
            "level {:?} does not allow query",
            cfg.level
        )));
    }

    let mut flush_state = state::load(&knowledge_dir)?;
    state::reset_budget_if_new_day(&mut flush_state);

    if let Err(e) = llm::check_budget(&flush_state, &cfg, ESTIMATED_TOKENS_PER_QUERY) {
        append_log_line(
            &knowledge_dir,
            &format!("BUDGET_EXCEEDED query {}", e.reason),
        );
        return Ok(QueryOutcome::BudgetExceeded(e.reason));
    }

    let existing_index = index::read(&knowledge_dir)?.unwrap_or_default();
    if existing_index.entries.is_empty() {
        return Ok(QueryOutcome::Skipped("empty wiki index".to_string()));
    }

    // Phase 1 — keyword extraction.
    let phase1_system = phase1_system_prompt();
    let phase1_user = format!("Question: {prompt}\n\nReturn a JSON array of 3-8 keywords.");
    let phase1_result = llm_call(
        &phase1_system,
        &phase1_user,
        &cfg.models.lint,
        MAX_KEYWORDS_OUTPUT_TOKENS,
    )?;
    let keywords = parse_keywords(&phase1_result.text);

    let selected: Vec<IndexEntry> = index::select_articles_for_compile(&existing_index, &keywords)
        .into_iter()
        .take(opts.top_k.max(1))
        .cloned()
        .collect();

    let article_bodies = read_selected_article_bodies(&knowledge_dir, &selected);

    // Phase 2 — answer with citations.
    let phase2_system = phase2_system_prompt();
    let phase2_user = build_answer_user_prompt(prompt, &article_bodies);
    let phase2_result = llm_call(
        &phase2_system,
        &phase2_user,
        &cfg.models.query,
        MAX_ANSWER_OUTPUT_TOKENS,
    )?;

    let answer_text = phase2_result.text.trim().to_string();
    let cited_clips = article::extract_citations_from_markdown(&answer_text);
    let consulted: Vec<String> = selected
        .iter()
        .map(|e| e.relative_path().display().to_string())
        .collect();

    // Optional file-back.
    let qa_path = if opts.file_back {
        let slug = article::canonical_key_from_title(prompt);
        let slug = if slug.is_empty() {
            format!("query-{}", chrono::Utc::now().format("%Y%m%d%H%M%S"))
        } else {
            slug
        };
        let write_res = article::write_article(
            &knowledge_dir,
            ArticleType::Qa,
            &slug,
            prompt,
            &format!("## Question\n\n{prompt}\n\n## Answer\n\n{answer_text}"),
            &[],
            &consulted,
            &cited_clips,
            None,
        )?;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let artifact = repo.add_artifact(
            Some(&write_res.path),
            None,
            Some(&format!("{slug}.md")),
            ArtifactType::Markdown,
            Some("text/markdown"),
            None,
            None,
            Some(serde_json::json!({
                "artifactType": "qa",
                "canonicalKey": write_res.canonical_key,
                "queryRun": today,
                "model": phase2_result.model,
            })),
        )?;
        let artifact_hash = artifact.artifact_hash.0.clone();
        for clip_hash in &cited_clips {
            let _ = repo.link_clip_artifact(
                clip_hash,
                &artifact_hash,
                ClipArtifactRelationship::CitedIn,
            );
        }
        Some(write_res.path)
    } else {
        None
    };

    // Record the Activity.
    record_query_activity(
        repo,
        prompt,
        &phase1_result,
        &phase2_result,
        &keywords,
        &consulted,
        &cited_clips,
    )?;

    // Update budget counters.
    let total_tokens = phase1_result.total_tokens + phase2_result.total_tokens;
    let total_cost = phase1_result.estimated_cost_usd + phase2_result.estimated_cost_usd;
    flush_state.daily_total_tokens += total_tokens;
    flush_state.daily_total_cost_usd += total_cost;
    state::save(&flush_state, &knowledge_dir)?;

    append_log_line(
        &knowledge_dir,
        &format!(
            "SUCCESS query cited={} consulted={} tokens={} cost=${:.4}",
            cited_clips.len(),
            consulted.len(),
            total_tokens,
            total_cost,
        ),
    );

    Ok(QueryOutcome::Answer {
        text: answer_text,
        cited_clips,
        consulted_articles: consulted,
        qa_path,
    })
}

// ── Prompt templates ──────────────────────────────────────────────────────────

fn phase1_system_prompt() -> String {
    "You extract search keywords from a natural-language question.  \
Return a JSON array of 3 to 8 lowercase keywords or short phrases that capture \
the salient concepts in the question.  Output ONLY the JSON array on a single \
line; no prose, no markdown fence.\n\n\
Example — Question: \"How does our OAuth flow handle PKCE?\"\n\
Output: [\"oauth\", \"pkce\", \"auth flow\"]"
        .to_string()
}

fn phase2_system_prompt() -> String {
    "You answer the user's question using the provided wiki articles.  \
When you state a fact or make a claim, cite the supporting source using \
`[cliproot:sha256-<full-hash>]` inline — prefer citations over `[[wikilinks]]`.  \
Copy citation hashes verbatim from the article bodies.  If the articles do not \
cover the question, say so directly; do not invent citations.  Keep the answer \
focused and under 400 words."
        .to_string()
}

fn build_answer_user_prompt(prompt: &str, article_bodies: &[(IndexEntry, String)]) -> String {
    let mut user = String::new();
    user.push_str(&format!("Question: {prompt}\n\n"));
    if article_bodies.is_empty() {
        user.push_str("No relevant articles were found in the wiki.\n");
    } else {
        user.push_str("Articles to consult:\n\n");
        for (entry, body) in article_bodies {
            user.push_str(&format!(
                "--- {} ---\n{body}\n\n",
                entry.relative_path().display()
            ));
        }
    }
    user
}

// ── Phase 1 parsing ───────────────────────────────────────────────────────────

/// Parse a JSON array of strings out of the phase-1 response.  Tolerates
/// markdown fences, leading/trailing prose, and falls back to a
/// whitespace-split of the raw text if no array is found.
fn parse_keywords(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if let (Some(start), Some(end)) = (trimmed.find('['), trimmed.rfind(']')) {
        if start < end {
            let slice = &trimmed[start..=end];
            if let Ok(vec) = serde_json::from_str::<Vec<String>>(slice) {
                return vec
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }
    // Fallback: split on commas/newlines so a malformed response still gives
    // the deterministic selector something to work with.
    trimmed
        .split([',', '\n'])
        .map(|s| {
            s.trim()
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

// ── Article body loading ──────────────────────────────────────────────────────

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

// ── CRP activity ──────────────────────────────────────────────────────────────

fn record_query_activity(
    repo: &Repository,
    prompt: &str,
    phase1: &llm::LlmCallResult,
    phase2: &llm::LlmCallResult,
    keywords: &[String],
    consulted_articles: &[String],
    cited_clip_hashes: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let activity = Activity {
        id: CrpId(format!("act-{}", uuid::Uuid::new_v4())),
        activity_type: ActivityType::Research,
        project_id: None,
        agent_id: None,
        prompt: Some(prompt.to_string()),
        parameters: Some(serde_json::json!({
            "operation": "query",
            "phase1Model": phase1.model,
            "phase1PromptHash": phase1.prompt_hash,
            "phase1InputTokens": phase1.input_tokens,
            "phase1OutputTokens": phase1.output_tokens,
            "model": phase2.model,
            "promptHash": phase2.prompt_hash,
            "inputTokens": phase2.input_tokens,
            "outputTokens": phase2.output_tokens,
            "keywords": keywords,
            "consultedArticles": consulted_articles,
        })),
        used_source_refs: cited_clip_hashes.to_vec(),
        generated_clip_refs: Vec::new(),
        created_at: now.clone(),
        ended_at: Some(now),
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
    use std::sync::{Arc, Mutex};

    fn make_wiki_cfg(repo: &Repository) {
        let mut cfg = repo.knowledge_config().unwrap();
        cfg.level = cliproot_store::KnowledgeLevel::Wiki;
        repo.set_knowledge_config(cfg).unwrap();
    }

    fn seed_index(knowledge_dir: &Path) {
        fs::create_dir_all(knowledge_dir).unwrap();
        article::write_article(
            knowledge_dir,
            ArticleType::Concept,
            "pkce-flow",
            "PKCE Flow",
            "PKCE prevents code-interception attacks in OAuth public clients. \
             See [cliproot:sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa].",
            &[],
            &[],
            &["sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()],
            None,
        )
        .unwrap();

        let idx = index::Index {
            schema_version: index::SCHEMA_VERSION,
            generated_at: "2026-04-14T00:00:00Z".to_string(),
            entries: vec![index::IndexEntry {
                uuid: "u1".to_string(),
                canonical_key: "pkce-flow".to_string(),
                title: "PKCE Flow".to_string(),
                article_type: ArticleType::Concept,
                tags: vec!["oauth".to_string(), "pkce".to_string()],
                last_seen: "2026-04-14".to_string(),
            }],
        };
        index::write(knowledge_dir, &idx).unwrap();
    }

    #[test]
    fn parse_keywords_plain_json() {
        let got = parse_keywords("[\"pkce\", \"oauth\", \"auth flow\"]");
        assert_eq!(got, vec!["pkce", "oauth", "auth flow"]);
    }

    #[test]
    fn parse_keywords_with_prose_wrapper() {
        let raw = "Here are the keywords: [\"pkce\", \"oauth\"] — done.";
        let got = parse_keywords(raw);
        assert_eq!(got, vec!["pkce", "oauth"]);
    }

    #[test]
    fn parse_keywords_with_markdown_fence() {
        let raw = "```json\n[\"pkce\", \"oauth\"]\n```";
        let got = parse_keywords(raw);
        assert_eq!(got, vec!["pkce", "oauth"]);
    }

    #[test]
    fn parse_keywords_fallback_on_malformed() {
        let raw = "pkce, oauth, auth flow";
        let got = parse_keywords(raw);
        assert!(got.contains(&"pkce".to_string()));
        assert!(got.contains(&"oauth".to_string()));
    }

    #[test]
    fn query_skipped_when_level_below_wiki() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let cliproot_dir = dir.path().join(".cliproot");
        let mock = |_: &str,
                    _: &str,
                    _: &str,
                    _: u32|
         -> Result<llm::LlmCallResult, Box<dyn std::error::Error>> {
            panic!("LLM must not be called when query is gated off")
        };
        let outcome = run_query(
            "any question",
            &cliproot_dir,
            &repo,
            QueryOpts {
                file_back: false,
                top_k: 6,
            },
            &mock,
        );
        assert!(matches!(outcome, QueryOutcome::Skipped(_)));
    }

    #[test]
    fn query_skipped_when_index_empty() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        make_wiki_cfg(&repo);
        let cliproot_dir = dir.path().join(".cliproot");

        let mock = |_: &str,
                    _: &str,
                    _: &str,
                    _: u32|
         -> Result<llm::LlmCallResult, Box<dyn std::error::Error>> {
            panic!("LLM must not be called when there is no wiki yet")
        };
        let outcome = run_query(
            "any question",
            &cliproot_dir,
            &repo,
            QueryOpts {
                file_back: false,
                top_k: 6,
            },
            &mock,
        );
        assert!(matches!(outcome, QueryOutcome::Skipped(_)));
    }

    #[test]
    fn query_budget_exceeded_short_circuits() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut cfg = repo.knowledge_config().unwrap();
        cfg.level = cliproot_store::KnowledgeLevel::Wiki;
        cfg.max_bg_tokens_per_day = 0;
        repo.set_knowledge_config(cfg).unwrap();

        let cliproot_dir = dir.path().join(".cliproot");
        let knowledge_dir = cliproot_dir.join("knowledge");
        seed_index(&knowledge_dir);

        let mock = |_: &str,
                    _: &str,
                    _: &str,
                    _: u32|
         -> Result<llm::LlmCallResult, Box<dyn std::error::Error>> {
            panic!("budget check should have short-circuited before the LLM call")
        };
        let outcome = run_query(
            "pkce?",
            &cliproot_dir,
            &repo,
            QueryOpts {
                file_back: false,
                top_k: 6,
            },
            &mock,
        );
        assert!(matches!(outcome, QueryOutcome::BudgetExceeded(_)));
    }

    #[test]
    fn query_answers_with_citations_and_records_activity() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        make_wiki_cfg(&repo);
        let cliproot_dir = dir.path().join(".cliproot");
        let knowledge_dir = cliproot_dir.join("knowledge");
        seed_index(&knowledge_dir);

        let call_count = Arc::new(Mutex::new(0u32));
        let call_count_mock = call_count.clone();
        let mock = move |_s: &str,
                         _u: &str,
                         model: &str,
                         _t: u32|
              -> Result<llm::LlmCallResult, Box<dyn std::error::Error>> {
            let mut n = call_count_mock.lock().unwrap();
            *n += 1;
            let text = if *n == 1 {
                r#"["pkce","oauth"]"#.to_string()
            } else {
                "PKCE binds the authorization code to the original requester \
                 [cliproot:sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa]."
                    .to_string()
            };
            Ok(llm::LlmCallResult {
                text,
                input_tokens: 100,
                output_tokens: 50,
                total_tokens: 150,
                estimated_cost_usd: 0.001,
                model: model.to_string(),
                prompt_hash: format!("hash-{n}"),
            })
        };

        let outcome = run_query(
            "how does our OAuth flow handle PKCE?",
            &cliproot_dir,
            &repo,
            QueryOpts {
                file_back: false,
                top_k: 6,
            },
            &mock,
        );

        match outcome {
            QueryOutcome::Answer {
                cited_clips,
                consulted_articles,
                qa_path,
                ..
            } => {
                assert_eq!(cited_clips.len(), 1);
                assert!(cited_clips[0].starts_with("sha256-"));
                assert!(consulted_articles
                    .iter()
                    .any(|p| p.contains("pkce-flow.md")));
                assert!(qa_path.is_none(), "file_back=false → no qa file");
            }
            other => panic!("expected Answer, got {other:?}"),
        }

        // State was updated — both phases billed.
        let st = state::load(&knowledge_dir).unwrap();
        assert!(st.daily_total_tokens >= 300, "both phases counted");

        // Two LLM calls happened: phase 1 + phase 2.
        assert_eq!(*call_count.lock().unwrap(), 2);
    }

    #[test]
    fn query_file_back_writes_qa_article() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        make_wiki_cfg(&repo);
        let cliproot_dir = dir.path().join(".cliproot");
        let knowledge_dir = cliproot_dir.join("knowledge");
        seed_index(&knowledge_dir);

        let call_count = Arc::new(Mutex::new(0u32));
        let call_count_mock = call_count.clone();
        let mock = move |_s: &str,
                         _u: &str,
                         model: &str,
                         _t: u32|
              -> Result<llm::LlmCallResult, Box<dyn std::error::Error>> {
            let mut n = call_count_mock.lock().unwrap();
            *n += 1;
            let text = if *n == 1 {
                r#"["pkce"]"#.to_string()
            } else {
                "Answer [cliproot:sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa].".to_string()
            };
            Ok(llm::LlmCallResult {
                text,
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                estimated_cost_usd: 0.0,
                model: model.to_string(),
                prompt_hash: "h".into(),
            })
        };

        let outcome = run_query(
            "How does PKCE work?",
            &cliproot_dir,
            &repo,
            QueryOpts {
                file_back: true,
                top_k: 6,
            },
            &mock,
        );

        let qa_path = match outcome {
            QueryOutcome::Answer { qa_path, .. } => qa_path.expect("qa path present"),
            other => panic!("expected Answer, got {other:?}"),
        };
        assert!(qa_path.exists());
        let body = fs::read_to_string(&qa_path).unwrap();
        assert!(body.contains("articleType: qa"));
        assert!(body.contains("canonicalKey: how-does-pkce-work"));
        let uuid_first = article::read_uuid_from_file(&qa_path).unwrap();

        // Rerun the same question — UUID must be preserved.
        *call_count.lock().unwrap() = 0;
        let _ = run_query(
            "How does PKCE work?",
            &cliproot_dir,
            &repo,
            QueryOpts {
                file_back: true,
                top_k: 6,
            },
            &mock,
        );
        let uuid_second = article::read_uuid_from_file(&qa_path).unwrap();
        assert_eq!(uuid_first, uuid_second);
    }
}
