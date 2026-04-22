use std::fs;
use std::path::Path;

use serde::Deserialize;

const GLOBAL_DEFAULTS: &[&str] = &["concept", "decision", "issue", "reference"];

#[derive(Deserialize)]
#[serde(untagged)]
enum ArticleTypesFile {
    Wrapped { types: Vec<String> },
    Bare(Vec<String>),
}

/// Load the merged article-type vocabulary: hardcoded global defaults first,
/// then any user-supplied types from `<cliproot_dir>/article-types.json`.
///
/// Types are lowercased and trimmed; case-insensitive duplicates are dropped
/// (first occurrence wins, so globals shadow same-name project entries —
/// name collisions are a no-op rather than an override). Missing or malformed
/// files silently fall back to globals only.
pub fn load_article_types(cliproot_dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for t in GLOBAL_DEFAULTS {
        let norm = t.trim().to_lowercase();
        if seen.insert(norm.clone()) {
            out.push(norm);
        }
    }

    let path = cliproot_dir.join("article-types.json");
    let Ok(raw) = fs::read_to_string(&path) else {
        return out;
    };
    let Ok(parsed) = serde_json::from_str::<ArticleTypesFile>(&raw) else {
        return out;
    };
    let project_types = match parsed {
        ArticleTypesFile::Wrapped { types } => types,
        ArticleTypesFile::Bare(v) => v,
    };

    for t in project_types {
        let norm = t.trim().to_lowercase();
        if norm.is_empty() {
            continue;
        }
        if seen.insert(norm.clone()) {
            out.push(norm);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_globals_when_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let types = load_article_types(dir.path());
        assert_eq!(types, vec!["concept", "decision", "issue", "reference"]);
    }

    #[test]
    fn merges_project_overrides() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("article-types.json"),
            r#"{"types":["garden","recipe"]}"#,
        )
        .unwrap();
        let types = load_article_types(dir.path());
        assert_eq!(
            types,
            vec!["concept", "decision", "issue", "reference", "garden", "recipe"]
        );
    }

    #[test]
    fn accepts_bare_array() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("article-types.json"), r#"["plant"]"#).unwrap();
        let types = load_article_types(dir.path());
        assert!(types.contains(&"plant".to_string()));
    }

    #[test]
    fn dedupes_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("article-types.json"),
            r#"{"types":["Concept","DECISION","plant"]}"#,
        )
        .unwrap();
        let types = load_article_types(dir.path());
        // Globals preserved; only "plant" added; case-collisions dropped.
        assert_eq!(
            types,
            vec!["concept", "decision", "issue", "reference", "plant"]
        );
    }

    #[test]
    fn malformed_json_returns_globals() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("article-types.json"), "{not json").unwrap();
        let types = load_article_types(dir.path());
        assert_eq!(types, vec!["concept", "decision", "issue", "reference"]);
    }

    #[test]
    fn trims_and_skips_empty() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("article-types.json"),
            r#"{"types":["  garden  ",""," "]}"#,
        )
        .unwrap();
        let types = load_article_types(dir.path());
        assert!(types.contains(&"garden".to_string()));
        assert_eq!(types.iter().filter(|t| t.is_empty()).count(), 0);
    }
}
