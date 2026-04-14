use std::fs;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

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
                read_uuid_from_file(&file_path).unwrap_or_else(|| new_uuid())
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
        let path = write_daily_digest(dir.path(), "2026-04-13", "## Summary\nDid stuff.", None)
            .unwrap();
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
        let path1 =
            write_daily_digest(dir.path(), "2026-04-13", "first body", None).unwrap();
        let uuid1 = read_uuid_from_file(&path1).expect("uuid written");

        let path2 =
            write_daily_digest(dir.path(), "2026-04-13", "updated body", None).unwrap();
        let uuid2 = read_uuid_from_file(&path2).expect("uuid written");

        assert_eq!(path1, path2);
        assert_eq!(uuid1, uuid2, "UUID must be stable across rewrites");
    }

    #[test]
    fn content_hash_reflects_body() {
        let dir = tempfile::tempdir().unwrap();
        let path_a =
            write_daily_digest(dir.path(), "2026-04-13", "body A", None).unwrap();
        let hash_a = read_content_hash_from_file(&path_a).unwrap();

        let path_b =
            write_daily_digest(dir.path(), "2026-04-13", "body B", None).unwrap();
        let hash_b = read_content_hash_from_file(&path_b).unwrap();

        assert_ne!(hash_a, hash_b, "different bodies must yield different hashes");
    }

    #[test]
    fn same_body_same_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path =
            write_daily_digest(dir.path(), "2026-04-13", "stable body", None).unwrap();
        let hash1 = read_content_hash_from_file(&path).unwrap();
        let path =
            write_daily_digest(dir.path(), "2026-04-13", "stable body", None).unwrap();
        let hash2 = read_content_hash_from_file(&path).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn explicit_uuid_respected() {
        let dir = tempfile::tempdir().unwrap();
        let my_uuid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string();
        let path = write_daily_digest(
            dir.path(),
            "2026-04-13",
            "body",
            Some(my_uuid.clone()),
        )
        .unwrap();
        let stored = read_uuid_from_file(&path).unwrap();
        assert_eq!(stored, my_uuid);
    }

    #[test]
    fn parse_frontmatter_field_basic() {
        let doc = "---\nuuid: abc\ndate: 2026-01-01\n---\nbody";
        assert_eq!(parse_frontmatter_field(doc, "uuid"), Some("abc".to_string()));
        assert_eq!(
            parse_frontmatter_field(doc, "date"),
            Some("2026-01-01".to_string())
        );
        assert_eq!(parse_frontmatter_field(doc, "missing"), None);
    }
}
