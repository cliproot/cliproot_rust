use std::fs;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

// ── Article types ─────────────────────────────────────────────────────────────

/// Kind of article stored under `.cliproot/knowledge/`.  The compile pipeline
/// (Phase D) writes concept, connection, and qa articles; the flush pipeline
/// (Phase C) writes daily-digest articles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ArticleType {
    Concept,
    Connection,
    Qa,
    DailyDigest,
    Index,
}

impl ArticleType {
    /// Frontmatter `articleType` slug.
    pub fn as_slug(&self) -> &'static str {
        match self {
            Self::Concept => "concept",
            Self::Connection => "connection",
            Self::Qa => "qa",
            Self::DailyDigest => "daily-digest",
            Self::Index => "index",
        }
    }

    /// Subdirectory under `knowledge/` that holds articles of this type.
    pub fn subdir(&self) -> Option<&'static str> {
        match self {
            Self::Concept => Some("concepts"),
            Self::Connection => Some("connections"),
            Self::Qa => Some("qa"),
            Self::DailyDigest => Some("daily"),
            Self::Index => None, // index.md lives at knowledge root
        }
    }
}

/// Result of `write_article`: the path written, the UUID persisted (new or
/// preserved), and the base64url-encoded content hash of the body.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ArticleWriteResult {
    pub path: PathBuf,
    pub uuid: String,
    pub canonical_key: String,
    pub content_hash_b64url: String,
}

// ── Daily digest writer ───────────────────────────────────────────────────────

/// Write (or overwrite) the daily digest file at
/// `<knowledge_dir>/daily/<date>.md`.
///
/// The file gets YAML frontmatter with a stable UUID (preserved across
/// rewrites) and a content hash of the body text.
///
/// Returns the path of the file written.
pub fn write_daily_digest(
    knowledge_dir: &Path,
    date: &str,
    body: &str,
    existing_uuid: Option<String>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let daily_dir = knowledge_dir.join("daily");
    fs::create_dir_all(&daily_dir)?;

    let file_path = daily_dir.join(format!("{date}.md"));

    // Preserve UUID from an existing file if the caller did not supply one.
    let uuid = match existing_uuid {
        Some(u) if !u.is_empty() => u,
        _ => {
            if file_path.exists() {
                read_uuid_from_file(&file_path).unwrap_or_else(new_uuid)
            } else {
                new_uuid()
            }
        }
    };

    let content_hash = sha256_base64url(body.as_bytes());
    let title = format!("Daily Digest {date}");

    let frontmatter = format!(
        "---\nuuid: {uuid}\ncontentHash: sha256-{content_hash}\ntitle: \"{title}\"\ndate: {date}\narticleType: daily-digest\n---\n"
    );

    fs::write(&file_path, format!("{frontmatter}\n{body}"))?;
    Ok(file_path)
}

/// Read the `uuid:` value from the YAML frontmatter of an existing article.
/// Returns `None` if the file has no frontmatter or the field is absent.
pub fn read_uuid_from_file(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    parse_frontmatter_field(&content, "uuid")
}

/// Read the `contentHash:` value from the YAML frontmatter of an existing
/// article.  Returns `None` if absent.
#[allow(dead_code)]
pub fn read_content_hash_from_file(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    parse_frontmatter_field(&content, "contentHash")
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn sha256_base64url(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

// ── Wiki article writer (Phase D) ────────────────────────────────────────────

/// UUIDv5 namespace for canonical article identities.  Pinned constant; do
/// NOT change — it is part of the on-disk identity contract.  Any 128-bit
/// value is a valid UUIDv5 namespace per RFC 4122 §4.3.
const ARTICLE_UUID_NAMESPACE: uuid::Uuid =
    uuid::Uuid::from_u128(0x4d3b_9a35_7d7b_4e61_9c86_c1c1_ff00_cafe);

/// Write (or overwrite) a wiki article at
/// `<knowledge_dir>/<subdir>/<slug>.md`.
///
/// - `uuid` is preserved if a file already exists at the target path; otherwise
///   it is derived from `UUIDv5(ARTICLE_UUID_NAMESPACE, canonical_key)` so the
///   same canonical key yields a stable identity across recompiles even when
///   the title cosmetically changes.
/// - `content_hash` is SHA-256 of the raw body (not the frontmatter), encoded
///   as unpadded base64url — same convention as `write_daily_digest`.
/// - `sources` and `clip_hashes` are emitted as YAML inline arrays so the
///   frontmatter remains parseable by the lightweight `parse_frontmatter_field`
///   helper used by the session-start hook.
///
/// Not used for daily digests — use `write_daily_digest` instead so its
/// bespoke shape (no canonicalKey, different article_type) is preserved.
#[allow(clippy::too_many_arguments)]
pub fn write_article(
    knowledge_dir: &Path,
    article_type: ArticleType,
    slug: &str,
    title: &str,
    body: &str,
    sources: &[String],
    clip_hashes: &[String],
    canonical_key_override: Option<&str>,
) -> Result<ArticleWriteResult, Box<dyn std::error::Error>> {
    let subdir = article_type.subdir().ok_or_else(|| {
        format!("article type {article_type:?} is not writable via write_article")
    })?;
    let dir = knowledge_dir.join(subdir);
    fs::create_dir_all(&dir)?;

    let file_path = dir.join(format!("{slug}.md"));

    let canonical_key = canonical_key_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| slug.to_string());

    // Preserve existing UUID if present; otherwise derive deterministically.
    let uuid = if file_path.exists() {
        read_uuid_from_file(&file_path).unwrap_or_else(|| uuidv5_from_canonical_key(&canonical_key))
    } else {
        uuidv5_from_canonical_key(&canonical_key)
    };

    let content_hash = sha256_base64url(body.as_bytes());

    let sources_inline = format_yaml_inline_list(sources);
    let clip_hashes_inline = format_yaml_inline_list(clip_hashes);

    let frontmatter = format!(
        "---\n\
         uuid: {uuid}\n\
         canonicalKey: {canonical_key}\n\
         contentHash: sha256-{content_hash}\n\
         title: \"{title_escaped}\"\n\
         articleType: {article_type_slug}\n\
         sources: {sources_inline}\n\
         clipHashes: {clip_hashes_inline}\n\
         ---\n",
        title_escaped = escape_yaml_scalar(title),
        article_type_slug = article_type.as_slug(),
    );

    fs::write(&file_path, format!("{frontmatter}\n{body}"))?;

    Ok(ArticleWriteResult {
        path: file_path,
        uuid,
        canonical_key,
        content_hash_b64url: content_hash,
    })
}

/// Normalise a human-readable title into a stable slug used both as the
/// filename and as the input to UUIDv5.  Lower-cases ASCII, replaces any run
/// of non-alphanumeric characters with a single `-`, and trims leading /
/// trailing dashes.
pub fn canonical_key_from_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut last_was_sep = true; // suppress leading dashes
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('-');
            last_was_sep = true;
        }
    }
    // Trim trailing dash
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Scan markdown body text for inline citations of the form
/// `[cliproot:sha256-<hash>]` and return the unique list of full clip hashes
/// (including the `sha256-` prefix), preserving first-seen order.
///
/// Only accepts hashes of ≥ 40 base64url characters after the `sha256-`
/// prefix — shorter values are noise.  No regex dep; hand-rolled scanner for
/// parity with `flush::extract_clip_hashes_from_line`.
pub fn extract_citations_from_markdown(body: &str) -> Vec<String> {
    const OPEN: &str = "[cliproot:sha256-";
    let bytes = body.as_bytes();
    let open_len = OPEN.len();
    let mut out: Vec<String> = Vec::new();
    let mut i: usize = 0;
    while i + open_len <= bytes.len() {
        if &bytes[i..i + open_len] == OPEN.as_bytes() {
            let hash_start = i + "[cliproot:".len(); // points at 's' of "sha256-"
            let mut j = hash_start;
            while j < bytes.len() {
                let b = bytes[j];
                if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
                    j += 1;
                } else {
                    break;
                }
            }
            if j < bytes.len() && bytes[j] == b']' {
                let raw = &body[hash_start..j];
                if let Some(after_prefix) = raw.strip_prefix("sha256-") {
                    if after_prefix.len() >= 40 {
                        let full = raw.to_string();
                        if !out.contains(&full) {
                            out.push(full);
                        }
                    }
                }
                i = j + 1;
                continue;
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

fn uuidv5_from_canonical_key(canonical_key: &str) -> String {
    uuid::Uuid::new_v5(&ARTICLE_UUID_NAMESPACE, canonical_key.as_bytes()).to_string()
}

fn format_yaml_inline_list(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    let parts: Vec<String> = items
        .iter()
        .map(|s| format!("\"{}\"", escape_yaml_scalar(s)))
        .collect();
    format!("[{}]", parts.join(", "))
}

fn escape_yaml_scalar(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Extract a scalar value from a simple YAML frontmatter block (lines between
/// the opening and closing `---` delimiters).
fn parse_frontmatter_field(content: &str, field: &str) -> Option<String> {
    let mut lines = content.lines();

    // Must start with "---"
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_frontmatter_and_body() {
        let dir = tempfile::tempdir().unwrap();
        let path =
            write_daily_digest(dir.path(), "2026-04-13", "## Summary\nDid stuff.", None).unwrap();
        assert!(path.exists());

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("---\n"));
        assert!(content.contains("articleType: daily-digest"));
        assert!(content.contains("date: 2026-04-13"));
        assert!(content.contains("## Summary\nDid stuff."));
    }

    #[test]
    fn uuid_stable_across_rewrites() {
        let dir = tempfile::tempdir().unwrap();
        let path1 = write_daily_digest(dir.path(), "2026-04-13", "first body", None).unwrap();
        let uuid1 = read_uuid_from_file(&path1).expect("uuid written");

        let path2 = write_daily_digest(dir.path(), "2026-04-13", "updated body", None).unwrap();
        let uuid2 = read_uuid_from_file(&path2).expect("uuid written");

        assert_eq!(path1, path2);
        assert_eq!(uuid1, uuid2, "UUID must be stable across rewrites");
    }

    #[test]
    fn content_hash_reflects_body() {
        let dir = tempfile::tempdir().unwrap();
        let path_a = write_daily_digest(dir.path(), "2026-04-13", "body A", None).unwrap();
        let hash_a = read_content_hash_from_file(&path_a).unwrap();

        let path_b = write_daily_digest(dir.path(), "2026-04-13", "body B", None).unwrap();
        let hash_b = read_content_hash_from_file(&path_b).unwrap();

        assert_ne!(
            hash_a, hash_b,
            "different bodies must yield different hashes"
        );
    }

    #[test]
    fn same_body_same_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_daily_digest(dir.path(), "2026-04-13", "stable body", None).unwrap();
        let hash1 = read_content_hash_from_file(&path).unwrap();
        let path = write_daily_digest(dir.path(), "2026-04-13", "stable body", None).unwrap();
        let hash2 = read_content_hash_from_file(&path).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn explicit_uuid_respected() {
        let dir = tempfile::tempdir().unwrap();
        let my_uuid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string();
        let path =
            write_daily_digest(dir.path(), "2026-04-13", "body", Some(my_uuid.clone())).unwrap();
        let stored = read_uuid_from_file(&path).unwrap();
        assert_eq!(stored, my_uuid);
    }

    #[test]
    fn parse_frontmatter_field_basic() {
        let doc = "---\nuuid: abc\ndate: 2026-01-01\n---\nbody";
        assert_eq!(
            parse_frontmatter_field(doc, "uuid"),
            Some("abc".to_string())
        );
        assert_eq!(
            parse_frontmatter_field(doc, "date"),
            Some("2026-01-01".to_string())
        );
        assert_eq!(parse_frontmatter_field(doc, "missing"), None);
    }

    // ── Phase D: write_article + helpers ─────────────────────────────────────

    #[test]
    fn canonical_key_normalises_title() {
        assert_eq!(canonical_key_from_title("PKCE Flow"), "pkce-flow");
        assert_eq!(
            canonical_key_from_title("  OAuth 2.0  (RFC 6749) "),
            "oauth-2-0-rfc-6749"
        );
        assert_eq!(canonical_key_from_title("---leading"), "leading");
        assert_eq!(canonical_key_from_title("trailing---"), "trailing");
        assert_eq!(canonical_key_from_title(""), "");
    }

    #[test]
    fn write_article_emits_full_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let res = write_article(
            dir.path(),
            ArticleType::Concept,
            "pkce-flow",
            "PKCE Flow",
            "Body with [cliproot:sha256-abcdefghijabcdefghijabcdefghijabcdefghij].",
            &["daily/2026-04-13.md".to_string()],
            &["sha256-abcdefghijabcdefghijabcdefghijabcdefghij".to_string()],
            None,
        )
        .unwrap();

        let content = fs::read_to_string(&res.path).unwrap();
        assert!(content.starts_with("---\n"));
        assert!(content.contains("articleType: concept"));
        assert!(content.contains("canonicalKey: pkce-flow"));
        assert!(content.contains("sources: [\"daily/2026-04-13.md\"]"));
        assert!(content.contains("clipHashes: [\"sha256-"));
        assert!(content.contains("contentHash: sha256-"));
        // Filename lands in concepts/
        assert!(res.path.to_string_lossy().contains("/concepts/"));
    }

    #[test]
    fn write_article_uuid_stable_across_rewrites() {
        let dir = tempfile::tempdir().unwrap();
        let r1 = write_article(
            dir.path(),
            ArticleType::Concept,
            "pkce-flow",
            "PKCE Flow",
            "first body",
            &[],
            &[],
            None,
        )
        .unwrap();
        let r2 = write_article(
            dir.path(),
            ArticleType::Concept,
            "pkce-flow",
            "PKCE Flow",
            "second body, rewritten",
            &[],
            &[],
            None,
        )
        .unwrap();
        assert_eq!(r1.path, r2.path);
        assert_eq!(r1.uuid, r2.uuid);
        assert_ne!(r1.content_hash_b64url, r2.content_hash_b64url);
    }

    #[test]
    fn write_article_uuid_matches_uuidv5_on_first_write() {
        let dir = tempfile::tempdir().unwrap();
        let r = write_article(
            dir.path(),
            ArticleType::Concept,
            "pkce-flow",
            "PKCE Flow",
            "body",
            &[],
            &[],
            None,
        )
        .unwrap();
        let expected =
            uuid::Uuid::new_v5(&ARTICLE_UUID_NAMESPACE, "pkce-flow".as_bytes()).to_string();
        assert_eq!(r.uuid, expected);
    }

    #[test]
    fn write_article_rejects_index_type() {
        let dir = tempfile::tempdir().unwrap();
        let err = write_article(
            dir.path(),
            ArticleType::Index,
            "unused",
            "Index",
            "body",
            &[],
            &[],
            None,
        );
        assert!(err.is_err(), "index articles are written via index::write");
    }

    #[test]
    fn extract_citations_finds_single() {
        let body = "Some text [cliproot:sha256-abcdefghijabcdefghijabcdefghijabcdefghij] more.";
        let got = extract_citations_from_markdown(body);
        assert_eq!(
            got,
            vec!["sha256-abcdefghijabcdefghijabcdefghijabcdefghij".to_string()]
        );
    }

    #[test]
    fn extract_citations_deduplicates() {
        let h = "sha256-abcdefghijabcdefghijabcdefghijabcdefghij";
        let body = format!("A [cliproot:{h}] B [cliproot:{h}] C");
        let got = extract_citations_from_markdown(&body);
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn extract_citations_rejects_short_hashes() {
        let body = "Trash [cliproot:sha256-tooshort] ignore.";
        let got = extract_citations_from_markdown(body);
        assert!(got.is_empty());
    }

    #[test]
    fn extract_citations_handles_base64url_chars() {
        let h = "sha256-AB_CD-EFabcdefghijabcdefghijabcdefghij1234";
        let body = format!("[cliproot:{h}]");
        let got = extract_citations_from_markdown(&body);
        assert_eq!(got, vec![h.to_string()]);
    }

    #[test]
    fn extract_citations_ignores_unclosed() {
        let body = "[cliproot:sha256-abcdefghijabcdefghijabcdefghijabcdefghij no close bracket";
        let got = extract_citations_from_markdown(body);
        assert!(got.is_empty());
    }

    #[test]
    fn extract_citations_finds_multiple() {
        let h1 = "sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let h2 = "sha256-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let body = format!("First [cliproot:{h1}] then [cliproot:{h2}] end.");
        let got = extract_citations_from_markdown(&body);
        assert_eq!(got, vec![h1.to_string(), h2.to_string()]);
    }
}
