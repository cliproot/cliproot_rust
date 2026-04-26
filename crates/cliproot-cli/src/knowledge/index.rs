//! `index.md` — master catalog of wiki articles.
//!
//! Written by `cliproot wiki compile` at the end of each compile run; read (raw,
//! without parsing) by `cliproot hook session-start` to inject a summary into
//! Claude Code's context.
//!
//! ### On-disk format (v2)
//!
//! YAML frontmatter block with schema metadata, then a bullet-list body:
//!
//! ```text
//! ---
//! schemaVersion: 2
//! generatedAt: 2026-04-25T18:03:00Z
//! articleCount: 3
//! ---
//!
//! # Wiki Index
//!
//! 3 articles · 2 concepts · 1 Q&A · last compile 2026-04-25
//!
//! ## Recently updated
//!
//! - [PKCE Flow](concepts/pkce-flow.md) — concept · *oauth, pkce* · 2026-04-25
//!
//! ## Concepts
//!
//! - [PKCE Flow](concepts/pkce-flow.md) — *oauth, pkce* · 2026-04-25
//! ```
//!
//! Phase D — paired with `knowledge::compile`.

use std::fs;
use std::path::{Path, PathBuf};

use super::article::ArticleType;

pub const INDEX_FILENAME: &str = "index.md";
pub const SCHEMA_VERSION: u32 = 2;

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
    out.push_str(&format!(
        "---\nschemaVersion: {}\ngeneratedAt: {}\narticleCount: {}\n---\n\n",
        index.schema_version,
        index.generated_at,
        index.entries.len(),
    ));
    out.push_str("# Wiki Index\n\n");

    out.push_str(&render_summary_line(index));
    out.push_str("\n\n");

    // Recently updated (top 15 by last_seen desc, ties by title asc).
    let recent = top_n_recent(&index.entries, 15);
    if !recent.is_empty() {
        out.push_str("## Recently updated\n\n");
        for e in &recent {
            out.push_str(&render_recent_bullet(e));
            out.push('\n');
        }
        out.push('\n');
    }

    // By-type sections (alphabetical within type; empty sections omitted).
    for (heading, kind) in [
        ("Concepts", ArticleType::Concept),
        ("Connections", ArticleType::Connection),
        ("Q&A", ArticleType::Qa),
    ] {
        let mut by_type: Vec<&IndexEntry> = index
            .entries
            .iter()
            .filter(|e| e.article_type == kind)
            .collect();
        if by_type.is_empty() {
            continue;
        }
        by_type.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        out.push_str(&format!("## {heading}\n\n"));
        for e in by_type {
            out.push_str(&render_type_bullet(e));
            out.push('\n');
        }
        out.push('\n');
    }

    out
}

fn render_recent_bullet(e: &IndexEntry) -> String {
    let path = relative_link_target(e);
    let title = escape_link_text(&e.title);
    let kind = e.article_type.as_slug();
    if e.tags.is_empty() {
        format!("- [{title}]({path}) — {kind} · {}", e.last_seen)
    } else {
        let tags = e.tags.join(", ");
        format!("- [{title}]({path}) — {kind} · *{tags}* · {}", e.last_seen)
    }
}

fn render_type_bullet(e: &IndexEntry) -> String {
    let path = relative_link_target(e);
    let title = escape_link_text(&e.title);
    if e.tags.is_empty() {
        format!("- [{title}]({path}) · {}", e.last_seen)
    } else {
        let tags = e.tags.join(", ");
        format!("- [{title}]({path}) — *{tags}* · {}", e.last_seen)
    }
}

fn render_summary_line(index: &Index) -> String {
    let total = index.entries.len();
    let n_concept = index
        .entries
        .iter()
        .filter(|e| e.article_type == ArticleType::Concept)
        .count();
    let n_conn = index
        .entries
        .iter()
        .filter(|e| e.article_type == ArticleType::Connection)
        .count();
    let n_qa = index
        .entries
        .iter()
        .filter(|e| e.article_type == ArticleType::Qa)
        .count();
    let last_compile = index.generated_at.get(..10).unwrap_or(&index.generated_at);

    let mut parts: Vec<String> = vec![format!("{total} article{}", plural(total))];
    if n_concept > 0 {
        parts.push(format!("{n_concept} concept{}", plural(n_concept)));
    }
    if n_conn > 0 {
        parts.push(format!("{n_conn} connection{}", plural(n_conn)));
    }
    if n_qa > 0 {
        parts.push(format!("{n_qa} Q&A"));
    }
    parts.push(format!("last compile {last_compile}"));
    parts.join(" · ")
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn top_n_recent(entries: &[IndexEntry], n: usize) -> Vec<&IndexEntry> {
    let mut v: Vec<&IndexEntry> = entries.iter().collect();
    v.sort_by(|a, b| {
        b.last_seen
            .cmp(&a.last_seen)
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    v.into_iter().take(n).collect()
}

fn relative_link_target(e: &IndexEntry) -> String {
    let subdir = e.article_type.subdir().unwrap_or(".");
    format!("{subdir}/{slug}.md", slug = e.canonical_key)
}

fn escape_link_text(s: &str) -> String {
    s.replace('[', "\\[").replace(']', "\\]")
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
    }

    // Walk by-type sections; ignore "Recently updated" (duplicate view).
    let mut entries: Vec<IndexEntry> = Vec::new();
    let mut current_type: Option<ArticleType> = None;
    let mut in_canonical_section = false;

    for line in lines {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            current_type = match heading.trim() {
                "Concepts" => Some(ArticleType::Concept),
                "Connections" => Some(ArticleType::Connection),
                "Q&A" => Some(ArticleType::Qa),
                _ => None,
            };
            in_canonical_section = current_type.is_some();
            continue;
        }
        if !in_canonical_section {
            continue;
        }
        let Some(kind) = current_type else { continue };
        if !trimmed.starts_with("- ") {
            continue;
        }
        if let Some(entry) = parse_v2_bullet(trimmed, kind) {
            entries.push(entry);
        }
    }

    Ok(Index {
        schema_version,
        generated_at,
        entries,
    })
}

fn parse_v2_bullet(line: &str, kind: ArticleType) -> Option<IndexEntry> {
    // Shapes (type-section bullets):
    //   - [Title](path) — *tag1, tag2* · 2026-04-13
    //   - [Title](path) · 2026-04-13
    let after_dash = line.strip_prefix("- ")?.trim();
    let rest = after_dash.strip_prefix('[')?;
    let close_text = rest.find("](")?;
    let title = rest[..close_text].replace("\\]", "]").replace("\\[", "[");
    let after_link = &rest[close_text + 2..];
    let close_paren = after_link.find(')')?;
    let path = &after_link[..close_paren];
    let tail = after_link[close_paren + 1..].trim();

    let mut tags: Vec<String> = Vec::new();
    let mut last_seen = String::new();

    let tail = tail.trim_start_matches(|c: char| c == '·' || c.is_whitespace() || c == '\u{2014}');
    for segment in tail.split('\u{00B7}') {
        let seg = segment.trim();
        if seg.is_empty() {
            continue;
        }
        if let Some(inner) = seg.strip_prefix('*').and_then(|s| s.strip_suffix('*')) {
            tags = inner
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        } else if seg.chars().all(|c| c.is_ascii_digit() || c == '-') && seg.len() == 10 {
            last_seen = seg.to_string();
        }
    }

    let canonical_key = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(String::from)
        .unwrap_or_else(|| super::article::canonical_key_from_title(&title));

    Some(IndexEntry {
        uuid: String::new(),
        canonical_key,
        title,
        article_type: kind,
        tags,
        last_seen,
    })
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_index() -> Index {
        Index {
            schema_version: SCHEMA_VERSION,
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
        assert_eq!(back.schema_version, SCHEMA_VERSION);
        assert_eq!(back.generated_at, "2026-04-14T18:03:00Z");
        assert_eq!(back.entries.len(), 2);
        // Entries from the by-type sections.
        let pkce = back
            .entries
            .iter()
            .find(|e| e.canonical_key == "pkce-flow")
            .unwrap();
        assert_eq!(pkce.title, "PKCE Flow");
        assert_eq!(pkce.article_type, ArticleType::Concept);
        assert_eq!(pkce.tags, vec!["oauth", "pkce"]);
        let conn = back
            .entries
            .iter()
            .find(|e| e.canonical_key == "oauth-vs-oidc")
            .unwrap();
        assert_eq!(conn.article_type, ArticleType::Connection);
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

    // ── §7.3 v2 render tests ─────────────────────────────────────────────────

    #[test]
    fn render_v2_summary_line_counts_pluralize_correctly() {
        let idx = Index {
            schema_version: SCHEMA_VERSION,
            generated_at: "2026-04-25T00:00:00Z".to_string(),
            entries: vec![IndexEntry {
                uuid: String::new(),
                canonical_key: "only-concept".to_string(),
                title: "Only Concept".to_string(),
                article_type: ArticleType::Concept,
                tags: vec![],
                last_seen: "2026-04-25".to_string(),
            }],
        };
        let summary = render_summary_line(&idx);
        // 1 article (singular), 1 concept (singular), no connections/Q&A.
        assert!(summary.contains("1 article ·"), "got: {summary}");
        assert!(summary.contains("1 concept ·"), "got: {summary}");
        assert!(!summary.contains("connection"), "got: {summary}");
        assert!(!summary.contains("Q&A"), "got: {summary}");

        // Two concepts → plural.
        let mut idx2 = idx.clone();
        idx2.entries.push(IndexEntry {
            uuid: String::new(),
            canonical_key: "second".to_string(),
            title: "Second".to_string(),
            article_type: ArticleType::Concept,
            tags: vec![],
            last_seen: "2026-04-25".to_string(),
        });
        let s2 = render_summary_line(&idx2);
        assert!(s2.contains("2 articles ·"), "got: {s2}");
        assert!(s2.contains("2 concepts ·"), "got: {s2}");
    }

    #[test]
    fn render_v2_recently_updated_orders_by_last_seen_desc() {
        let idx = Index {
            schema_version: SCHEMA_VERSION,
            generated_at: "2026-04-25T00:00:00Z".to_string(),
            entries: vec![
                IndexEntry {
                    uuid: String::new(),
                    canonical_key: "a".to_string(),
                    title: "A".to_string(),
                    article_type: ArticleType::Concept,
                    tags: vec![],
                    last_seen: "2026-04-10".to_string(),
                },
                IndexEntry {
                    uuid: String::new(),
                    canonical_key: "b".to_string(),
                    title: "B".to_string(),
                    article_type: ArticleType::Concept,
                    tags: vec![],
                    last_seen: "2026-04-25".to_string(),
                },
                IndexEntry {
                    uuid: String::new(),
                    canonical_key: "c".to_string(),
                    title: "C".to_string(),
                    article_type: ArticleType::Concept,
                    tags: vec![],
                    last_seen: "2026-04-15".to_string(),
                },
            ],
        };
        let out = render(&idx);
        let recent_section = out
            .split("## Recently updated")
            .nth(1)
            .unwrap()
            .split("\n## ")
            .next()
            .unwrap();
        let pos_b = recent_section.find("[B]").unwrap();
        let pos_c = recent_section.find("[C]").unwrap();
        let pos_a = recent_section.find("[A]").unwrap();
        assert!(
            pos_b < pos_c,
            "B (2026-04-25) should appear before C (2026-04-15)"
        );
        assert!(
            pos_c < pos_a,
            "C (2026-04-15) should appear before A (2026-04-10)"
        );
    }

    #[test]
    fn render_v2_recently_updated_caps_at_15() {
        let entries: Vec<IndexEntry> = (0..30)
            .map(|i| IndexEntry {
                uuid: String::new(),
                canonical_key: format!("article-{i:02}"),
                title: format!("Article {i:02}"),
                article_type: ArticleType::Concept,
                tags: vec![],
                last_seen: format!("2026-04-{:02}", (i % 28) + 1),
            })
            .collect();
        let idx = Index {
            schema_version: SCHEMA_VERSION,
            generated_at: "2026-04-25T00:00:00Z".to_string(),
            entries,
        };
        let out = render(&idx);
        let recent_section = out
            .split("## Recently updated\n\n")
            .nth(1)
            .unwrap()
            .split("\n## ")
            .next()
            .unwrap();
        let bullet_count = recent_section
            .lines()
            .filter(|l| l.starts_with("- "))
            .count();
        assert_eq!(
            bullet_count, 15,
            "recently updated capped at 15; got {bullet_count}"
        );
    }

    #[test]
    fn render_v2_omits_empty_type_section() {
        let idx = Index {
            schema_version: SCHEMA_VERSION,
            generated_at: "2026-04-25T00:00:00Z".to_string(),
            entries: vec![IndexEntry {
                uuid: String::new(),
                canonical_key: "only-concept".to_string(),
                title: "Only Concept".to_string(),
                article_type: ArticleType::Concept,
                tags: vec![],
                last_seen: "2026-04-25".to_string(),
            }],
        };
        let out = render(&idx);
        assert!(
            out.contains("## Concepts"),
            "Concepts section should appear"
        );
        assert!(
            !out.contains("## Connections"),
            "Connections section should be omitted"
        );
        assert!(!out.contains("## Q&A"), "Q&A section should be omitted");
    }

    #[test]
    fn render_v2_escapes_brackets_in_titles() {
        let idx = Index {
            schema_version: SCHEMA_VERSION,
            generated_at: "2026-04-25T00:00:00Z".to_string(),
            entries: vec![IndexEntry {
                uuid: String::new(),
                canonical_key: "array-t".to_string(),
                title: "Array[T]".to_string(),
                article_type: ArticleType::Concept,
                tags: vec![],
                last_seen: "2026-04-25".to_string(),
            }],
        };
        let out = render(&idx);
        assert!(
            out.contains("[Array\\[T\\]](concepts/array-t.md)"),
            "brackets in title must be escaped; got:\n{out}"
        );
        let back = parse(&out).unwrap();
        let entry = back
            .entries
            .iter()
            .find(|e| e.canonical_key == "array-t")
            .unwrap();
        assert_eq!(entry.title, "Array[T]");
    }

    #[test]
    fn render_v2_omits_tag_segment_when_no_tags() {
        let e = IndexEntry {
            uuid: String::new(),
            canonical_key: "no-tags".to_string(),
            title: "No Tags".to_string(),
            article_type: ArticleType::Concept,
            tags: vec![],
            last_seen: "2026-04-25".to_string(),
        };
        let type_bullet = render_type_bullet(&e);
        assert!(
            !type_bullet.contains('*'),
            "no-tags bullet should have no *…* segment; got: {type_bullet}"
        );
        let recent_bullet = render_recent_bullet(&e);
        assert!(
            !recent_bullet.contains('*'),
            "no-tags recent bullet should have no *…* segment; got: {recent_bullet}"
        );
    }

    #[test]
    fn parse_v2_roundtrip() {
        let idx = sample_index();
        let out = render(&idx);
        let back = parse(&out).unwrap();
        assert_eq!(back.entries.len(), idx.entries.len());
        for orig in &idx.entries {
            let parsed = back
                .entries
                .iter()
                .find(|e| e.canonical_key == orig.canonical_key)
                .unwrap_or_else(|| panic!("missing entry {}", orig.canonical_key));
            assert_eq!(parsed.title, orig.title);
            assert_eq!(parsed.article_type, orig.article_type);
            assert_eq!(parsed.tags, orig.tags);
            assert_eq!(parsed.last_seen, orig.last_seen);
            // uuid is not written to v2 index.
            assert_eq!(parsed.uuid, "");
        }
    }

    #[test]
    fn parse_v2_ignores_recently_updated_section() {
        let idx = sample_index();
        let out = render(&idx);
        let back = parse(&out).unwrap();
        // Both entries are in their type sections; the recent section is skipped.
        // Total must equal the original entry count (no double-counting).
        assert_eq!(
            back.entries.len(),
            idx.entries.len(),
            "recently updated entries must not be double-counted"
        );
    }

    #[test]
    fn parse_v2_tolerates_missing_last_seen() {
        let raw = "---\nschemaVersion: 2\ngeneratedAt: 2026-04-25T00:00:00Z\n---\n\n\
            # Wiki Index\n\n\
            ## Concepts\n\n\
            - [PKCE Flow](concepts/pkce-flow.md)\n";
        let idx = parse(raw).unwrap();
        assert_eq!(idx.entries.len(), 1);
        assert_eq!(idx.entries[0].last_seen, "");
    }
}
