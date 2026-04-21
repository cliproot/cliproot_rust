//! `index.md` — master catalog of wiki articles.
//!
//! Written by `cliproot wiki compile` at the end of each compile run; read (raw,
//! without parsing) by `cliproot hook session-start` to inject a summary into
//! Claude Code's context.
//!
//! ### On-disk format
//!
//! YAML frontmatter block with schema metadata, then a markdown table body:
//!
//! ```text
//! ---
//! schemaVersion: 1
//! generatedAt: 2026-04-14T18:03:00Z
//! articleCount: 3
//! ---
//!
//! # Wiki index
//!
//! | concept | uuid | type | tags | last_seen |
//! |---|---|---|---|---|
//! | PKCE Flow | 4d3b9a35-... | concept | oauth, pkce | 2026-04-13 |
//! ```
//!
//! We deliberately avoid a markdown/YAML parser dependency — both directions
//! use simple line splitting.  Session-start injection does not even parse the
//! body; it takes the first N bytes verbatim.
//!
//! Phase D — paired with `knowledge::compile`.

use std::fs;
use std::path::{Path, PathBuf};

use super::article::ArticleType;

pub const INDEX_FILENAME: &str = "index.md";
pub const SCHEMA_VERSION: u32 = 1;

/// One row in the wiki index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    /// Persistent UUID (matches the article's frontmatter `uuid`).
    pub uuid: String,
    /// Slug used as the filename stem (e.g. `pkce-flow`).
    pub canonical_key: String,
    /// Human-readable title.
    pub title: String,
    /// Article type — determines the subdirectory.
    pub article_type: ArticleType,
    /// Free-form tags, alphanumeric preferred.
    pub tags: Vec<String>,
    /// Local YYYY-MM-DD date of the most recent compile that touched this
    /// article.
    pub last_seen: String,
}

impl IndexEntry {
    /// Path where this entry's article lives, relative to the knowledge root.
    pub fn relative_path(&self) -> PathBuf {
        let subdir = self.article_type.subdir().unwrap_or(".");
        PathBuf::from(subdir).join(format!("{}.md", self.canonical_key))
    }
}

/// The parsed `index.md` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub schema_version: u32,
    pub generated_at: String,
    pub entries: Vec<IndexEntry>,
}

impl Default for Index {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            generated_at: chrono::Utc::now().to_rfc3339(),
            entries: Vec::new(),
        }
    }
}

/// Read `<knowledge_dir>/index.md`.  Returns `Ok(None)` if the file does not
/// exist — the compile pipeline treats absence as "first run, everything is
/// new".
pub fn read(knowledge_dir: &Path) -> Result<Option<Index>, Box<dyn std::error::Error>> {
    let path = knowledge_dir.join(INDEX_FILENAME);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    let idx = parse(&raw)?;
    Ok(Some(idx))
}

/// Write `<knowledge_dir>/index.md`.  Overwrites unconditionally; the compile
/// pipeline is responsible for preserving / merging entries before calling.
pub fn write(knowledge_dir: &Path, index: &Index) -> Result<PathBuf, Box<dyn std::error::Error>> {
    fs::create_dir_all(knowledge_dir)?;
    let path = knowledge_dir.join(INDEX_FILENAME);
    fs::write(&path, render(index))?;
    Ok(path)
}

/// Given the current index and a list of concept strings mentioned in today's
/// daily log, return the subset of entries whose title or tags substring-match
/// any of the concepts.  When the full corpus has fewer than 50 entries, all
/// entries are returned regardless — the LLM context is small enough to hold
/// everything at that scale and "load all" beats missing a relevant article.
pub fn select_articles_for_compile<'a>(
    index: &'a Index,
    daily_concepts: &[String],
) -> Vec<&'a IndexEntry> {
    if index.entries.len() < 50 {
        return index.entries.iter().collect();
    }

    let needles: Vec<String> = daily_concepts
        .iter()
        .map(|s| s.to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    index
        .entries
        .iter()
        .filter(|e| {
            let hay_title = e.title.to_lowercase();
            let hay_tags: Vec<String> = e.tags.iter().map(|t| t.to_lowercase()).collect();
            needles
                .iter()
                .any(|n| hay_title.contains(n) || hay_tags.iter().any(|t| t.contains(n)))
        })
        .collect()
}

// ── rendering ─────────────────────────────────────────────────────────────────

fn render(index: &Index) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("schemaVersion: {}\n", index.schema_version));
    out.push_str(&format!("generatedAt: {}\n", index.generated_at));
    out.push_str(&format!("articleCount: {}\n", index.entries.len()));
    out.push_str("---\n\n# Wiki index\n\n");
    out.push_str("| concept | uuid | type | tags | last_seen |\n");
    out.push_str("|---|---|---|---|---|\n");
    for e in &index.entries {
        out.push_str(&format!(
            "| [{title}]({target}) | {uuid} | {kind} | {tags} | {last_seen} |\n",
            title = escape_link_text(&e.title),
            target = relative_link_target(e),
            uuid = e.uuid,
            kind = e.article_type.as_slug(),
            tags = escape_cell(&e.tags.join(", ")),
            last_seen = e.last_seen,
        ));
    }
    out
}

fn escape_cell(s: &str) -> String {
    // Pipe characters inside cells break markdown tables; escape them.
    s.replace('|', "\\|").replace('\n', " ")
}

fn relative_link_target(e: &IndexEntry) -> String {
    // Forward-slash path so the link works on all platforms regardless of
    // PathBuf's native separator.  Slugs are [a-z0-9-] so no URL encoding.
    let subdir = e.article_type.subdir().unwrap_or(".");
    format!("{subdir}/{slug}.md", slug = e.canonical_key)
}

fn escape_link_text(s: &str) -> String {
    // Escape brackets so titles like "Array[T]" don't confuse the link parser.
    escape_cell(s).replace('[', "\\[").replace(']', "\\]")
}

// ── parsing ───────────────────────────────────────────────────────────────────

fn parse(raw: &str) -> Result<Index, Box<dyn std::error::Error>> {
    let mut lines = raw.lines();

    if lines.next().map(str::trim) != Some("---") {
        return Err("index.md missing YAML frontmatter opening delimiter".into());
    }

    let mut schema_version: u32 = SCHEMA_VERSION;
    let mut generated_at = String::new();
    for line in lines.by_ref() {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("schemaVersion:") {
            schema_version = rest.trim().parse().unwrap_or(SCHEMA_VERSION);
        } else if let Some(rest) = trimmed.strip_prefix("generatedAt:") {
            generated_at = rest.trim().to_string();
        }
        // articleCount is informational; re-derived on write.
    }

    // Parse body table rows.  Header is the line with "| concept | uuid |"
    // followed by a separator `|---|...|`; real rows start after that.
    let mut entries = Vec::new();
    let mut saw_header = false;
    let mut saw_separator = false;
    for line in lines {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        if !saw_header {
            if trimmed.contains("concept") && trimmed.contains("uuid") {
                saw_header = true;
            }
            continue;
        }
        if !saw_separator {
            if trimmed.contains("---") {
                saw_separator = true;
            }
            continue;
        }
        if let Some(entry) = parse_row(trimmed) {
            entries.push(entry);
        }
    }

    Ok(Index {
        schema_version,
        generated_at,
        entries,
    })
}

fn parse_row(line: &str) -> Option<IndexEntry> {
    // A row looks like `| title | uuid | type | tag1, tag2 | 2026-04-13 |`.
    // Split on `|` and trim; require at least 5 cells.
    let cells: Vec<String> = line
        .trim_matches('|')
        .split('|')
        .map(|c| c.trim().replace("\\|", "|"))
        .collect();
    if cells.len() < 5 {
        return None;
    }
    let (title, link_target) = parse_markdown_link(&cells[0]);
    let uuid = cells[1].clone();
    let kind = parse_article_type(&cells[2])?;
    let tags: Vec<String> = cells[3]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let last_seen = cells[4].clone();

    // Prefer the link target's filename stem — it is the canonical identity
    // on disk.  Fall back to title-derived slug for legacy indexes that
    // predate the linked-title format.
    let canonical_key = link_target
        .as_deref()
        .and_then(|t| {
            std::path::Path::new(t)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(String::from)
        })
        .unwrap_or_else(|| super::article::canonical_key_from_title(&title));

    Some(IndexEntry {
        uuid,
        canonical_key,
        title,
        article_type: kind,
        tags,
        last_seen,
    })
}

fn parse_markdown_link(cell: &str) -> (String, Option<String>) {
    // Accept `[text](target)` and return `(text, Some(target))`; otherwise
    // return `(cell, None)` so legacy plain-title indexes still parse after
    // upgrade.
    let trimmed = cell.trim();
    if let Some(rest) = trimmed.strip_prefix('[') {
        if let Some(close) = rest.find("](") {
            let text = &rest[..close];
            let after = &rest[close + 2..];
            if let Some(target) = after.strip_suffix(')') {
                return (
                    text.replace("\\]", "]").replace("\\[", "["),
                    Some(target.to_string()),
                );
            }
        }
    }
    (cell.to_string(), None)
}

fn parse_article_type(s: &str) -> Option<ArticleType> {
    match s.trim() {
        "concept" => Some(ArticleType::Concept),
        "connection" => Some(ArticleType::Connection),
        "qa" => Some(ArticleType::Qa),
        "daily-digest" => Some(ArticleType::DailyDigest),
        "index" => Some(ArticleType::Index),
        _ => None,
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_index() -> Index {
        Index {
            schema_version: 1,
            generated_at: "2026-04-14T18:03:00Z".to_string(),
            entries: vec![
                IndexEntry {
                    uuid: "4d3b9a35-aaaa-0000-0000-000000000001".to_string(),
                    canonical_key: "pkce-flow".to_string(),
                    title: "PKCE Flow".to_string(),
                    article_type: ArticleType::Concept,
                    tags: vec!["oauth".to_string(), "pkce".to_string()],
                    last_seen: "2026-04-13".to_string(),
                },
                IndexEntry {
                    uuid: "4d3b9a35-bbbb-0000-0000-000000000002".to_string(),
                    canonical_key: "oauth-vs-oidc".to_string(),
                    title: "OAuth vs OIDC".to_string(),
                    article_type: ArticleType::Connection,
                    tags: vec!["oauth".to_string(), "oidc".to_string()],
                    last_seen: "2026-04-12".to_string(),
                },
            ],
        }
    }

    #[test]
    fn roundtrip_write_read() {
        let dir = tempfile::tempdir().unwrap();
        let idx = sample_index();
        write(dir.path(), &idx).unwrap();
        let back = read(dir.path()).unwrap().unwrap();
        assert_eq!(back.schema_version, 1);
        assert_eq!(back.generated_at, "2026-04-14T18:03:00Z");
        assert_eq!(back.entries.len(), 2);
        assert_eq!(back.entries[0].title, "PKCE Flow");
        assert_eq!(back.entries[0].article_type, ArticleType::Concept);
        assert_eq!(back.entries[0].tags, vec!["oauth", "pkce"]);
        assert_eq!(back.entries[1].article_type, ArticleType::Connection);
    }

    #[test]
    fn read_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).unwrap().is_none());
    }

    #[test]
    fn select_returns_all_under_50() {
        let idx = sample_index();
        let picked = select_articles_for_compile(&idx, &[]);
        assert_eq!(picked.len(), 2, "< 50 articles → load all");
    }

    #[test]
    fn select_substring_matches_title_when_large() {
        let mut idx = sample_index();
        // Inflate entries to trigger the selection path.
        for i in 0..60 {
            idx.entries.push(IndexEntry {
                uuid: format!("00000000-0000-0000-0000-{i:012x}"),
                canonical_key: format!("filler-{i}"),
                title: format!("Filler {i}"),
                article_type: ArticleType::Concept,
                tags: vec![],
                last_seen: "2026-04-01".to_string(),
            });
        }
        let picked = select_articles_for_compile(&idx, &["pkce".to_string()]);
        // Should pick only the pkce-flow entry.
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].canonical_key, "pkce-flow");
    }

    #[test]
    fn select_substring_matches_tag_when_large() {
        let mut idx = sample_index();
        for i in 0..60 {
            idx.entries.push(IndexEntry {
                uuid: format!("00000000-0000-0000-0000-{i:012x}"),
                canonical_key: format!("filler-{i}"),
                title: format!("Filler {i}"),
                article_type: ArticleType::Concept,
                tags: vec![],
                last_seen: "2026-04-01".to_string(),
            });
        }
        let picked = select_articles_for_compile(&idx, &["oidc".to_string()]);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].canonical_key, "oauth-vs-oidc");
    }

    #[test]
    fn relative_path_uses_subdir() {
        let e = IndexEntry {
            uuid: "x".into(),
            canonical_key: "pkce-flow".into(),
            title: "PKCE".into(),
            article_type: ArticleType::Concept,
            tags: vec![],
            last_seen: "2026-04-13".into(),
        };
        assert_eq!(e.relative_path().to_string_lossy(), "concepts/pkce-flow.md");
    }

    #[test]
    fn rendered_markdown_has_header_and_separator() {
        let out = render(&sample_index());
        assert!(out.contains("# Wiki index"));
        assert!(out.contains("| concept | uuid | type | tags | last_seen |"));
        assert!(out.contains("|---|---|---|---|---|"));
        assert!(out.contains("| [PKCE Flow](concepts/pkce-flow.md) |"));
    }

    #[test]
    fn rendered_row_links_use_subdir_by_type() {
        let idx = Index {
            schema_version: 1,
            generated_at: "now".into(),
            entries: vec![
                IndexEntry {
                    uuid: "u1".into(),
                    canonical_key: "pkce-flow".into(),
                    title: "PKCE Flow".into(),
                    article_type: ArticleType::Concept,
                    tags: vec![],
                    last_seen: "2026-04-13".into(),
                },
                IndexEntry {
                    uuid: "u2".into(),
                    canonical_key: "oauth-vs-oidc".into(),
                    title: "OAuth vs OIDC".into(),
                    article_type: ArticleType::Connection,
                    tags: vec![],
                    last_seen: "2026-04-13".into(),
                },
                IndexEntry {
                    uuid: "u3".into(),
                    canonical_key: "rate-limit-fix".into(),
                    title: "How do I fix rate limits?".into(),
                    article_type: ArticleType::Qa,
                    tags: vec![],
                    last_seen: "2026-04-13".into(),
                },
            ],
        };
        let out = render(&idx);
        assert!(out.contains("[PKCE Flow](concepts/pkce-flow.md)"));
        assert!(out.contains("[OAuth vs OIDC](connections/oauth-vs-oidc.md)"));
        assert!(out.contains("[How do I fix rate limits?](qa/rate-limit-fix.md)"));
    }

    #[test]
    fn parse_extracts_title_from_markdown_link() {
        let out = render(&sample_index());
        let back = parse(&out).unwrap();
        assert_eq!(back.entries[0].title, "PKCE Flow");
        assert_eq!(back.entries[0].canonical_key, "pkce-flow");
        assert_eq!(back.entries[1].title, "OAuth vs OIDC");
        assert_eq!(back.entries[1].canonical_key, "oauth-vs-oidc");
    }

    #[test]
    fn parse_accepts_legacy_plain_title() {
        let raw = "---\nschemaVersion: 1\ngeneratedAt: now\n---\n\n\
            | concept | uuid | type | tags | last_seen |\n\
            |---|---|---|---|---|\n\
            | PKCE Flow | u1 | concept | | 2026-04-13 |\n";
        let idx = parse(raw).unwrap();
        assert_eq!(idx.entries.len(), 1);
        assert_eq!(idx.entries[0].title, "PKCE Flow");
        assert_eq!(idx.entries[0].canonical_key, "pkce-flow");
    }

    #[test]
    fn render_escapes_closing_bracket_in_title() {
        let idx = Index {
            schema_version: 1,
            generated_at: "now".into(),
            entries: vec![IndexEntry {
                uuid: "u".into(),
                canonical_key: "array-t".into(),
                title: "Array[T]".into(),
                article_type: ArticleType::Concept,
                tags: vec![],
                last_seen: "2026-04-13".into(),
            }],
        };
        let out = render(&idx);
        assert!(
            out.contains("[Array\\[T\\]](concepts/array-t.md)"),
            "closing bracket in title must be escaped so the link doesn't close early; got:\n{out}"
        );
        let back = parse(&out).unwrap();
        assert_eq!(back.entries[0].title, "Array[T]");
    }

    #[test]
    fn parse_skips_rows_without_known_type() {
        let raw = "---\nschemaVersion: 1\ngeneratedAt: now\n---\n\n\
            | concept | uuid | type | tags | last_seen |\n\
            |---|---|---|---|---|\n\
            | PKCE | u1 | concept | | 2026-04-13 |\n\
            | Bogus | u2 | mystery | | 2026-04-13 |\n";
        let idx = parse(raw).unwrap();
        assert_eq!(idx.entries.len(), 1);
        assert_eq!(idx.entries[0].title, "PKCE");
    }

    #[test]
    fn render_escapes_pipes_in_cells() {
        let idx = Index {
            schema_version: 1,
            generated_at: "now".into(),
            entries: vec![IndexEntry {
                uuid: "u".into(),
                canonical_key: "a-or-b".into(),
                title: "A | B".into(),
                article_type: ArticleType::Concept,
                tags: vec![],
                last_seen: "2026-04-13".into(),
            }],
        };
        let out = render(&idx);
        assert!(out.contains("A \\| B"), "pipes in cells must be escaped");
    }
}
