//! `cliproot wiki lint` — Karpathy's 7 structural checks unified with
//! cliproot's provenance invariants.
//!
//! Phase F.  Runs a set of checks over `.cliproot/knowledge/` and reports
//! any findings.  The load-bearing invariant (per part 2 §4.2) is check #2
//! (broken `[cliproot:sha256-...]` citations); #1 wikilinks are informational.
//!
//! Check catalogue:
//!   1. Broken `[[wikilinks]]`           — free
//!   2. Broken `[cliproot:sha256-...]`    — free  (load-bearing)
//!   3. Orphan pages (no inbound refs)   — free
//!   4. Orphan sources (daily not compiled) — free
//!   5. Stale articles (body hash drifted from frontmatter contentHash) — free
//!   6. Sparse articles (< 200 words)    — free
//!   7. Missing backlinks                — free
//!   8. Uncovered claims (via `cliproot doc coverage`) — free (skipped under --structural-only)
//!   9. Pairwise contradictions          — ~5K tokens Haiku (opt-in via --contradictions)

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use cliproot_core::matching::CoverageStatus;
use cliproot_core::model::{Activity, ActivityType, BundleType, CrpBundle, CrpId};
use cliproot_store::Repository;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::article::{self, ArticleType};
use super::index;
use super::llm;
use super::state;

// ── Options + outcome ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct LintOpts {
    pub structural_only: bool,
    pub contradictions: bool,
    pub write_report: bool,
}

/// Injectable LLM call matching [`llm::call`].
pub type LlmCallFn =
    dyn Fn(&str, &str, &str, u32) -> Result<llm::LlmCallResult, Box<dyn std::error::Error>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CheckId {
    WikilinksBroken,
    CitationsBroken,
    OrphanPages,
    OrphanSources,
    StaleArticles,
    SparseArticles,
    MissingBacklinks,
    UncoveredClaims,
    Contradictions,
}

impl CheckId {
    pub fn number(&self) -> u8 {
        match self {
            Self::WikilinksBroken => 1,
            Self::CitationsBroken => 2,
            Self::OrphanPages => 3,
            Self::OrphanSources => 4,
            Self::StaleArticles => 5,
            Self::SparseArticles => 6,
            Self::MissingBacklinks => 7,
            Self::UncoveredClaims => 8,
            Self::Contradictions => 9,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WikilinksBroken => "wikilinks",
            Self::CitationsBroken => "citations",
            Self::OrphanPages => "orphan-pages",
            Self::OrphanSources => "orphan-sources",
            Self::StaleArticles => "stale",
            Self::SparseArticles => "sparse",
            Self::MissingBacklinks => "backlinks",
            Self::UncoveredClaims => "coverage",
            Self::Contradictions => "contradictions",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub id: CheckId,
    pub findings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LintReport {
    pub checks: Vec<CheckResult>,
    pub generated_at: String,
    pub report_path: Option<PathBuf>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run_lint(
    cliproot_dir: &Path,
    repo: &Repository,
    opts: LintOpts,
    llm_call: &LlmCallFn,
) -> Result<LintReport, Box<dyn std::error::Error>> {
    let knowledge_dir = cliproot_dir.join("knowledge");
    if !knowledge_dir.exists() {
        return Ok(LintReport {
            checks: Vec::new(),
            generated_at: now_iso(),
            report_path: None,
        });
    }

    let articles = collect_articles(&knowledge_dir)?;
    let idx = index::read(&knowledge_dir)?.unwrap_or_default();

    let mut checks = Vec::new();
    checks.push(check_wikilinks(&articles, &idx));
    checks.push(check_citations(&articles, repo)?);
    checks.push(check_orphan_pages(&articles));
    checks.push(check_orphan_sources(&knowledge_dir)?);
    checks.push(check_stale_articles(&articles));
    checks.push(check_sparse_articles(&articles));
    checks.push(check_missing_backlinks(&articles));

    if !opts.structural_only {
        checks.push(check_uncovered_claims(&articles, repo));
    }

    if opts.contradictions {
        checks.push(check_contradictions(
            &knowledge_dir,
            repo,
            &articles,
            llm_call,
        )?);
    }

    let generated_at = now_iso();
    let report_path = if opts.write_report {
        Some(write_report_file(&knowledge_dir, &checks, &generated_at)?)
    } else {
        None
    };

    Ok(LintReport {
        checks,
        generated_at,
        report_path,
    })
}

// ── Article loading ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LoadedArticle {
    path: PathBuf,
    rel_path: String,
    article_type: ArticleType,
    slug: String,
    uuid: Option<String>,
    frontmatter_content_hash: Option<String>,
    body: String,
    raw: String,
}

impl LoadedArticle {
    fn word_count(&self) -> usize {
        self.body.split_whitespace().count()
    }
    fn citations(&self) -> Vec<String> {
        article::extract_citations_from_markdown(&self.body)
    }
    fn wikilinks(&self) -> Vec<String> {
        extract_wikilinks(&self.body)
    }
}

fn collect_articles(
    knowledge_dir: &Path,
) -> Result<Vec<LoadedArticle>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for (subdir, kind) in [
        ("concepts", ArticleType::Concept),
        ("connections", ArticleType::Connection),
        ("qa", ArticleType::Qa),
    ] {
        let dir = knowledge_dir.join(subdir);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let raw = fs::read_to_string(&path)?;
            let (body, uuid, ch) = split_frontmatter_and_body(&raw);
            let slug = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let rel_path = format!(
                "{subdir}/{}",
                path.file_name().and_then(|s| s.to_str()).unwrap_or("")
            );
            out.push(LoadedArticle {
                path,
                rel_path,
                article_type: kind,
                slug,
                uuid,
                frontmatter_content_hash: ch,
                body,
                raw,
            });
        }
    }
    Ok(out)
}

fn split_frontmatter_and_body(raw: &str) -> (String, Option<String>, Option<String>) {
    let mut lines = raw.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (raw.to_string(), None, None);
    }
    let mut uuid: Option<String> = None;
    let mut ch: Option<String> = None;
    let mut rest_lines: Vec<&str> = Vec::new();
    let mut in_fm = true;
    for line in lines {
        if in_fm {
            if line.trim() == "---" {
                in_fm = false;
                continue;
            }
            if let Some(v) = line.strip_prefix("uuid:") {
                uuid = Some(v.trim().trim_matches('"').to_string());
            } else if let Some(v) = line.strip_prefix("contentHash:") {
                ch = Some(v.trim().trim_matches('"').to_string());
            }
        } else {
            rest_lines.push(line);
        }
    }
    let body = rest_lines.join("\n").trim_start().to_string();
    (body, uuid, ch)
}

// ── Check implementations ─────────────────────────────────────────────────────

fn check_wikilinks(articles: &[LoadedArticle], idx: &index::Index) -> CheckResult {
    let known_slugs: BTreeSet<String> = idx
        .entries
        .iter()
        .map(|e| e.canonical_key.clone())
        .collect();
    let known_titles: BTreeSet<String> =
        idx.entries.iter().map(|e| e.title.to_lowercase()).collect();
    let mut findings = Vec::new();
    for a in articles {
        for link in a.wikilinks() {
            let slug = article::canonical_key_from_title(&link);
            if !known_slugs.contains(&slug) && !known_titles.contains(&link.to_lowercase()) {
                findings.push(format!("{}: broken [[{link}]]", a.rel_path));
            }
        }
    }
    CheckResult {
        id: CheckId::WikilinksBroken,
        findings,
    }
}

fn check_citations(
    articles: &[LoadedArticle],
    repo: &Repository,
) -> Result<CheckResult, Box<dyn std::error::Error>> {
    let mut findings = Vec::new();
    for a in articles {
        for hash in a.citations() {
            match repo.get_clip(&hash) {
                Ok(Some(_)) => {}
                Ok(None) => findings.push(format!("{}: missing clip {hash}", a.rel_path)),
                Err(e) => findings.push(format!("{}: {hash} ({e})", a.rel_path)),
            }
        }
    }
    Ok(CheckResult {
        id: CheckId::CitationsBroken,
        findings,
    })
}

fn check_orphan_pages(articles: &[LoadedArticle]) -> CheckResult {
    let mut inbound: BTreeSet<String> = BTreeSet::new();
    for a in articles {
        for link in a.wikilinks() {
            inbound.insert(article::canonical_key_from_title(&link));
        }
    }
    let mut findings = Vec::new();
    for a in articles {
        if !inbound.contains(&a.slug) {
            findings.push(format!("{}: zero inbound wikilinks", a.rel_path));
        }
    }
    CheckResult {
        id: CheckId::OrphanPages,
        findings,
    }
}

fn check_orphan_sources(knowledge_dir: &Path) -> Result<CheckResult, Box<dyn std::error::Error>> {
    let mut findings = Vec::new();
    let st = state::load(knowledge_dir).unwrap_or_default();
    let daily_dir = knowledge_dir.join("daily");
    let Ok(entries) = fs::read_dir(&daily_dir) else {
        return Ok(CheckResult {
            id: CheckId::OrphanSources,
            findings,
        });
    };
    let mut dailies: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .collect();
    dailies.sort();
    // Anything older than the most-recent daily that has not influenced a
    // compile is an orphan.  We have no per-daily compile record; a pragmatic
    // proxy: if `last_compile_hash` is None and any daily exists, every daily
    // is an orphan.
    if st.last_compile_hash.is_none() {
        for p in &dailies {
            findings.push(format!(
                "daily/{}: never compiled",
                p.file_name().and_then(|s| s.to_str()).unwrap_or("")
            ));
        }
    }
    Ok(CheckResult {
        id: CheckId::OrphanSources,
        findings,
    })
}

fn check_stale_articles(articles: &[LoadedArticle]) -> CheckResult {
    let mut findings = Vec::new();
    for a in articles {
        let Some(fm_hash) = a.frontmatter_content_hash.as_ref() else {
            continue;
        };
        let expected = format!("sha256-{}", sha256_b64url(a.body.as_bytes()));
        if *fm_hash != expected {
            findings.push(format!(
                "{}: body drifted from frontmatter contentHash",
                a.rel_path
            ));
        }
    }
    CheckResult {
        id: CheckId::StaleArticles,
        findings,
    }
}

fn check_sparse_articles(articles: &[LoadedArticle]) -> CheckResult {
    let mut findings = Vec::new();
    for a in articles {
        let wc = a.word_count();
        if wc < 200 {
            findings.push(format!("{}: {wc} words (< 200)", a.rel_path));
        }
    }
    CheckResult {
        id: CheckId::SparseArticles,
        findings,
    }
}

fn check_missing_backlinks(articles: &[LoadedArticle]) -> CheckResult {
    // If article A wikilinks to article B (by slug), B should wikilink back.
    use std::collections::BTreeMap;
    let by_slug: BTreeMap<String, &LoadedArticle> =
        articles.iter().map(|a| (a.slug.clone(), a)).collect();
    let mut findings = Vec::new();
    for a in articles {
        for link in a.wikilinks() {
            let target_slug = article::canonical_key_from_title(&link);
            let Some(target) = by_slug.get(&target_slug) else {
                continue;
            };
            let back_slugs: BTreeSet<String> = target
                .wikilinks()
                .into_iter()
                .map(|l| article::canonical_key_from_title(&l))
                .collect();
            if !back_slugs.contains(&a.slug) {
                findings.push(format!(
                    "{} → {} (no backlink)",
                    a.rel_path, target.rel_path
                ));
            }
        }
    }
    CheckResult {
        id: CheckId::MissingBacklinks,
        findings,
    }
}

fn check_uncovered_claims(articles: &[LoadedArticle], repo: &Repository) -> CheckResult {
    let mut findings = Vec::new();
    for a in articles {
        let Ok(report) = repo.doctor(&a.body, 0.4) else {
            continue;
        };
        for p in &report.paragraph_reports {
            if matches!(p.status, CoverageStatus::Uncovered) {
                findings.push(format!(
                    "{}: P{} uncovered — {}",
                    a.rel_path,
                    p.index + 1,
                    truncate(&p.text_preview, 80)
                ));
            }
        }
    }
    CheckResult {
        id: CheckId::UncoveredClaims,
        findings,
    }
}

fn check_contradictions(
    knowledge_dir: &Path,
    repo: &Repository,
    articles: &[LoadedArticle],
    llm_call: &LlmCallFn,
) -> Result<CheckResult, Box<dyn std::error::Error>> {
    let cfg = repo.knowledge_config()?;
    let mut st = state::load(knowledge_dir).unwrap_or_default();
    state::reset_budget_if_new_day(&mut st);

    const ESTIMATED_TOKENS: u64 = 5_000;
    if let Err(e) = llm::check_budget(&st, &cfg, ESTIMATED_TOKENS) {
        return Ok(CheckResult {
            id: CheckId::Contradictions,
            findings: vec![format!("BUDGET_EXCEEDED {}", e.reason)],
        });
    }

    if articles.len() < 2 {
        return Ok(CheckResult {
            id: CheckId::Contradictions,
            findings: Vec::new(),
        });
    }

    let system = "You are a wiki editor scanning a small set of articles for \
        direct factual contradictions.  Return, as a JSON array of strings, \
        any contradictions you find (each string describes one contradiction \
        and names the two articles by relative path).  If there are no \
        contradictions, return `[]`.  Do NOT include commentary, headers, \
        or prose — only a JSON array."
        .to_string();

    let mut user = String::from("Articles:\n\n");
    for a in articles {
        user.push_str(&format!("--- {} ---\n{}\n\n", a.rel_path, a.body));
    }

    let result = llm_call(&system, &user, &cfg.models.lint, 1024)?;
    let findings = parse_contradiction_response(&result.text);

    // Record a Verify activity for auditability.
    record_verify_activity(repo, &result, articles)?;

    st.daily_total_tokens += result.total_tokens;
    st.daily_total_cost_usd += result.estimated_cost_usd;
    let _ = state::save(&st, knowledge_dir);

    Ok(CheckResult {
        id: CheckId::Contradictions,
        findings,
    })
}

fn parse_contradiction_response(text: &str) -> Vec<String> {
    // Try strict JSON first; fall back to a brace-scan if the model wrapped
    // its array in prose despite instructions.
    if let Ok(v) = serde_json::from_str::<Vec<String>>(text.trim()) {
        return v;
    }
    if let (Some(l), Some(r)) = (text.find('['), text.rfind(']')) {
        if l < r {
            if let Ok(v) = serde_json::from_str::<Vec<String>>(&text[l..=r]) {
                return v;
            }
        }
    }
    Vec::new()
}

fn record_verify_activity(
    repo: &Repository,
    result: &llm::LlmCallResult,
    articles: &[LoadedArticle],
) -> Result<(), Box<dyn std::error::Error>> {
    let now = now_iso();
    let activity = Activity {
        id: CrpId(format!("act-{}", uuid::Uuid::new_v4())),
        activity_type: ActivityType::Review,
        project_id: None,
        agent_id: None,
        prompt: None,
        parameters: Some(serde_json::json!({
            "operation": "wiki-lint:contradictions",
            "promptHash": result.prompt_hash,
            "model": result.model,
            "inputTokens": result.input_tokens,
            "outputTokens": result.output_tokens,
            "articleCount": articles.len(),
        })),
        used_source_refs: Vec::new(),
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

// ── Report writer ─────────────────────────────────────────────────────────────

fn write_report_file(
    knowledge_dir: &Path,
    checks: &[CheckResult],
    generated_at: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let reports_dir = knowledge_dir.join("reports");
    fs::create_dir_all(&reports_dir)?;
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let path = reports_dir.join(format!("wiki-lint-{date}.md"));
    let mut body = String::new();
    body.push_str(&format!("# Wiki lint — {generated_at}\n\n"));
    for c in checks {
        body.push_str(&format!(
            "## #{} {} ({} finding{})\n\n",
            c.id.number(),
            c.id.as_str(),
            c.findings.len(),
            if c.findings.len() == 1 { "" } else { "s" },
        ));
        if c.findings.is_empty() {
            body.push_str("(clean)\n\n");
        } else {
            for f in &c.findings {
                body.push_str(&format!("- {f}\n"));
            }
            body.push('\n');
        }
    }
    fs::write(&path, body)?;
    Ok(path)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_wikilinks(body: &str) -> Vec<String> {
    // Hand-rolled scanner for `[[...]]` that excludes Markdown image/link
    // syntax.  Ignores empty or embedded-colon targets (e.g. the
    // `[cliproot:sha256-...]` citation form is single-bracket, not matched here).
    let bytes = body.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if &bytes[i..i + 2] == b"[[" {
            let start = i + 2;
            let Some(end_rel) = body[start..].find("]]") else {
                break;
            };
            let text = &body[start..start + end_rel];
            if !text.is_empty() && !text.contains('\n') {
                let link = text.split('|').next().unwrap_or("").trim().to_string();
                if !link.is_empty() && !out.contains(&link) {
                    out.push(link);
                }
            }
            i = start + end_rel + 2;
        } else {
            i += 1;
        }
    }
    out
}

fn sha256_b64url(data: &[u8]) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let mut hasher = Sha256::new();
    hasher.update(data);
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo_and_wiki() -> (tempfile::TempDir, Repository, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut cfg = repo.knowledge_config().unwrap();
        cfg.level = cliproot_store::KnowledgeLevel::Wiki;
        repo.set_knowledge_config(cfg).unwrap();
        let cliproot_dir = dir.path().join(".cliproot");
        (dir, repo, cliproot_dir)
    }

    fn panic_llm(
        _: &str,
        _: &str,
        _: &str,
        _: u32,
    ) -> Result<llm::LlmCallResult, Box<dyn std::error::Error>> {
        panic!("LLM must not be called in structural-only lint");
    }

    #[test]
    fn wikilinks_extract_basic() {
        let body = "Related: [[PKCE Flow]], [[OAuth 2.0 | oauth]], and `[not-a-wikilink]`.";
        let got = extract_wikilinks(body);
        assert!(got.contains(&"PKCE Flow".to_string()));
        assert!(got.contains(&"OAuth 2.0".to_string()));
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn wikilinks_ignore_cliproot_citations() {
        let body = "Cite [cliproot:sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa]";
        assert!(extract_wikilinks(body).is_empty());
    }

    #[test]
    fn sparse_flagged_below_200_words() {
        let (_dir, repo, cliproot_dir) = init_repo_and_wiki();
        let knowledge_dir = cliproot_dir.join("knowledge");
        article::write_article(
            &knowledge_dir,
            ArticleType::Concept,
            "sparse",
            "Sparse",
            "Three whole words.",
            &[],
            &[],
            None,
        )
        .unwrap();
        let report = run_lint(
            &cliproot_dir,
            &repo,
            LintOpts {
                structural_only: true,
                contradictions: false,
                write_report: false,
            },
            &panic_llm,
        )
        .unwrap();
        let sparse = report
            .checks
            .iter()
            .find(|c| c.id == CheckId::SparseArticles)
            .unwrap();
        assert_eq!(sparse.findings.len(), 1);
        assert!(sparse.findings[0].contains("concepts/sparse.md"));
    }

    #[test]
    fn stale_flagged_when_body_tampered() {
        let (_dir, repo, cliproot_dir) = init_repo_and_wiki();
        let knowledge_dir = cliproot_dir.join("knowledge");
        let res = article::write_article(
            &knowledge_dir,
            ArticleType::Concept,
            "pkce",
            "PKCE",
            "Body at write time.",
            &[],
            &[],
            None,
        )
        .unwrap();
        // Tamper: rewrite the file body WITHOUT touching the frontmatter hash.
        let raw = fs::read_to_string(&res.path).unwrap();
        let (fm_end, _) = raw
            .match_indices("\n---\n")
            .nth(0)
            .map(|(i, _)| (i + 5, ()))
            .unwrap();
        let mut tampered = raw[..fm_end].to_string();
        tampered.push_str("\nDrastically different body now.\n");
        fs::write(&res.path, tampered).unwrap();

        let report = run_lint(
            &cliproot_dir,
            &repo,
            LintOpts {
                structural_only: true,
                contradictions: false,
                write_report: false,
            },
            &panic_llm,
        )
        .unwrap();
        let stale = report
            .checks
            .iter()
            .find(|c| c.id == CheckId::StaleArticles)
            .unwrap();
        assert_eq!(stale.findings.len(), 1, "findings: {:?}", stale.findings);
    }

    #[test]
    fn citations_flagged_when_clip_missing() {
        let (_dir, repo, cliproot_dir) = init_repo_and_wiki();
        let knowledge_dir = cliproot_dir.join("knowledge");
        let bogus = "sha256-deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        article::write_article(
            &knowledge_dir,
            ArticleType::Concept,
            "pkce",
            "PKCE",
            &format!(
                "Body with [cliproot:{bogus}] inline. Plus filler to pass sparse check. {}",
                "word ".repeat(210)
            ),
            &[],
            &[bogus.to_string()],
            None,
        )
        .unwrap();
        let report = run_lint(
            &cliproot_dir,
            &repo,
            LintOpts {
                structural_only: true,
                contradictions: false,
                write_report: false,
            },
            &panic_llm,
        )
        .unwrap();
        let citations = report
            .checks
            .iter()
            .find(|c| c.id == CheckId::CitationsBroken)
            .unwrap();
        assert_eq!(citations.findings.len(), 1);
        assert!(citations.findings[0].contains(bogus));
    }

    #[test]
    fn uncovered_claims_skipped_under_structural_only() {
        let (_dir, repo, cliproot_dir) = init_repo_and_wiki();
        let knowledge_dir = cliproot_dir.join("knowledge");
        article::write_article(
            &knowledge_dir,
            ArticleType::Concept,
            "pkce",
            "PKCE",
            "A claim without any citation.",
            &[],
            &[],
            None,
        )
        .unwrap();

        let report = run_lint(
            &cliproot_dir,
            &repo,
            LintOpts {
                structural_only: true,
                contradictions: false,
                write_report: false,
            },
            &panic_llm,
        )
        .unwrap();
        assert!(report
            .checks
            .iter()
            .all(|c| c.id != CheckId::UncoveredClaims));
    }

    #[test]
    fn contradictions_records_activity_and_parses_response() {
        let (_dir, repo, cliproot_dir) = init_repo_and_wiki();
        let knowledge_dir = cliproot_dir.join("knowledge");

        for (slug, body) in [
            ("a", "PKCE is required for public clients."),
            ("b", "PKCE is not required for public clients."),
        ] {
            article::write_article(
                &knowledge_dir,
                ArticleType::Concept,
                slug,
                slug,
                &format!("{body} {}", "word ".repeat(210)),
                &[],
                &[],
                None,
            )
            .unwrap();
        }

        let mock = |_: &str, _: &str, model: &str, _: u32| {
            Ok(llm::LlmCallResult {
                text:
                    "[\"concepts/a.md vs concepts/b.md: direct contradiction on PKCE requirement\"]"
                        .to_string(),
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                estimated_cost_usd: 0.0001,
                model: model.to_string(),
                prompt_hash: "h".into(),
            })
        };

        let report = run_lint(
            &cliproot_dir,
            &repo,
            LintOpts {
                structural_only: true,
                contradictions: true,
                write_report: false,
            },
            &mock,
        )
        .unwrap();
        let c = report
            .checks
            .iter()
            .find(|c| c.id == CheckId::Contradictions)
            .unwrap();
        assert_eq!(c.findings.len(), 1);
        assert!(c.findings[0].contains("PKCE"));
    }

    #[test]
    fn write_report_creates_markdown_file() {
        let (_dir, repo, cliproot_dir) = init_repo_and_wiki();
        let knowledge_dir = cliproot_dir.join("knowledge");
        article::write_article(
            &knowledge_dir,
            ArticleType::Concept,
            "pkce",
            "PKCE",
            "Tiny body.",
            &[],
            &[],
            None,
        )
        .unwrap();
        let report = run_lint(
            &cliproot_dir,
            &repo,
            LintOpts {
                structural_only: true,
                contradictions: false,
                write_report: true,
            },
            &panic_llm,
        )
        .unwrap();
        let path = report.report_path.expect("report path");
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("# Wiki lint"));
        assert!(content.contains("## #6 sparse"));
    }
}
