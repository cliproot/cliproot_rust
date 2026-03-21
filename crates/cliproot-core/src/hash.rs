use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::model::ContentHash;

/// NFC normalization + \r\n/\r → \n
pub fn normalize_for_hash(text: &str) -> String {
    let nfc: String = text.nfc().collect();
    nfc.replace("\r\n", "\n").replace('\r', "\n")
}

/// SHA-256 of normalized UTF-8, base64url with `sha256-` prefix
pub fn create_text_hash(text: &str) -> ContentHash {
    let normalized = normalize_for_hash(text);
    let digest = Sha256::digest(normalized.as_bytes());
    let encoded = URL_SAFE_NO_PAD.encode(digest);
    ContentHash(format!("sha256-{encoded}"))
}

/// SHA-256 of canonical JSON of { sourceRefs (sorted), textHash, [textQuoteExact] }
pub fn create_clip_hash(input: ClipHashInput) -> ContentHash {
    let mut sorted_refs = input.source_refs.clone();
    sorted_refs.sort();

    // Build canonical JSON using serde_json::Value with sorted keys (BTreeMap is default for Map)
    let mut map = serde_json::Map::new();
    map.insert(
        "sourceRefs".to_string(),
        serde_json::Value::Array(
            sorted_refs
                .into_iter()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    map.insert(
        "textHash".to_string(),
        serde_json::Value::String(input.text_hash.0.clone()),
    );
    if let Some(exact) = &input.text_quote_exact {
        map.insert(
            "textQuoteExact".to_string(),
            serde_json::Value::String(exact.clone()),
        );
    }

    // serde_json::Map is a BTreeMap when the "preserve_order" feature is NOT enabled,
    // so keys are already alphabetically sorted.
    let canonical = serde_json::to_string(&serde_json::Value::Object(map))
        .expect("canonical JSON serialization cannot fail");

    let digest = Sha256::digest(canonical.as_bytes());
    let encoded = URL_SAFE_NO_PAD.encode(digest);
    ContentHash(format!("sha256-{encoded}"))
}

pub struct ClipHashInput {
    pub text_hash: ContentHash,
    pub source_refs: Vec<String>,
    pub text_quote_exact: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_nfc_and_newlines() {
        assert_eq!(
            normalize_for_hash("Cafe\u{0301}\r\nline2\rline3"),
            "Caf\u{00e9}\nline2\nline3"
        );
    }

    #[test]
    fn test_text_hash_nfc_equivalence() {
        let h1 = create_text_hash("Cafe\u{0301}");
        let h2 = create_text_hash("Café");
        assert_eq!(h1, h2);
        assert_eq!(h1.0, "sha256-c0c9zBK3YwhZBKUnnQSMTVs7AIxG8fMkQ7md4EqoOhQ");
    }

    #[test]
    fn test_text_hash_newline_equivalence() {
        let h1 = create_text_hash("line1\r\nline2");
        let h2 = create_text_hash("line1\nline2");
        assert_eq!(h1, h2);
        assert_eq!(h1.0, "sha256-aDN24pCCm0gsJlV0XK_6eh3M-hCvqmLawrQt1saND4M");
    }

    #[test]
    fn test_text_hash_empty() {
        let h = create_text_hash("");
        assert_eq!(h.0, "sha256-47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU");
    }

    #[test]
    fn test_clip_hash_deterministic() {
        let text_hash = create_text_hash("Hello world");
        let h1 = create_clip_hash(ClipHashInput {
            text_hash: text_hash.clone(),
            source_refs: vec!["src_01".to_string()],
            text_quote_exact: None,
        });
        let h2 = create_clip_hash(ClipHashInput {
            text_hash,
            source_refs: vec!["src_01".to_string()],
            text_quote_exact: None,
        });
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_clip_hash_source_ref_order_independent() {
        let text_hash = create_text_hash("Hello world");
        let h1 = create_clip_hash(ClipHashInput {
            text_hash: text_hash.clone(),
            source_refs: vec!["src_01".to_string(), "src_02".to_string()],
            text_quote_exact: None,
        });
        let h2 = create_clip_hash(ClipHashInput {
            text_hash,
            source_refs: vec!["src_02".to_string(), "src_01".to_string()],
            text_quote_exact: None,
        });
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_clip_hash_with_text_quote_exact() {
        let text_hash = create_text_hash("Hello world");
        let h1 = create_clip_hash(ClipHashInput {
            text_hash: text_hash.clone(),
            source_refs: vec!["src_01".to_string()],
            text_quote_exact: Some("Hello world".to_string()),
        });
        let h2 = create_clip_hash(ClipHashInput {
            text_hash,
            source_refs: vec!["src_01".to_string()],
            text_quote_exact: None,
        });
        // With and without textQuoteExact should differ
        assert_ne!(h1, h2);
    }
}
