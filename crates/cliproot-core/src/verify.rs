use crate::error::CoreError;
use crate::hash::{create_clip_hash, create_text_hash, ClipHashInput};
use crate::model::{Clip, CrpBundle, EdgeType};

/// Recompute clipHash from textHash, sourceRefs, and optional textQuoteExact; compare.
pub fn verify_clip_hash(clip: &Clip) -> Result<(), CoreError> {
    let text_quote_exact = clip
        .selectors
        .as_ref()
        .and_then(|s| s.text_quote.as_ref())
        .map(|tq| tq.exact.clone());

    let computed = create_clip_hash(ClipHashInput {
        text_hash: clip.text_hash.clone(),
        source_refs: clip.source_refs.clone(),
        text_quote_exact,
    });

    if computed != clip.clip_hash {
        return Err(CoreError::HashMismatch {
            expected: clip.clip_hash.0.clone(),
            actual: computed.0,
        });
    }
    Ok(())
}

/// If content is present, recompute textHash and compare.
pub fn verify_text_hash(clip: &Clip) -> Result<(), CoreError> {
    if let Some(content) = &clip.content {
        let computed = create_text_hash(content);
        if computed != clip.text_hash {
            return Err(CoreError::HashMismatch {
                expected: clip.text_hash.0.clone(),
                actual: computed.0,
            });
        }
    }
    Ok(())
}

/// Verify all clips in a bundle plus structural checks.
pub fn verify_bundle(bundle: &CrpBundle) -> Result<(), CoreError> {
    // Verify each clip's hashes
    for clip in &bundle.clips {
        verify_clip_hash(clip)?;
        verify_text_hash(clip)?;
    }

    // Check derivation edges point to known child clips
    let clip_hashes: std::collections::HashSet<&str> = bundle
        .clips
        .iter()
        .map(|c| c.clip_hash.0.as_str())
        .collect();

    for edge in &bundle.edges {
        if matches!(edge.edge_type, EdgeType::WasDerivedFrom)
            && !clip_hashes.contains(edge.subject_ref.0.as_str())
        {
            return Err(CoreError::VerificationFailed(format!(
                "edge {} references unknown subject clip {}",
                edge.id, edge.subject_ref
            )));
        }
    }

    // Check sourceRefs point to known sources
    let source_ids: std::collections::HashSet<&str> =
        bundle.sources.iter().map(|s| s.id.0.as_str()).collect();

    for clip in &bundle.clips {
        for source_ref in &clip.source_refs {
            if !source_ids.contains(source_ref.as_str()) {
                return Err(CoreError::VerificationFailed(format!(
                    "clip {} references unknown source {}",
                    clip.clip_hash, source_ref
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::create_text_hash;
    use crate::model::*;

    fn make_test_clip(content: &str, source_refs: Vec<String>) -> Clip {
        let text_hash = create_text_hash(content);
        let clip_hash = create_clip_hash(ClipHashInput {
            text_hash: text_hash.clone(),
            source_refs: source_refs.clone(),
            text_quote_exact: None,
        });
        Clip {
            clip_hash,
            id: None,
            document_id: None,
            source_refs,
            selectors: None,
            content: Some(content.to_string()),
            text_hash,
            project_id: None,
            created_by_activity_id: None,
        }
    }

    #[test]
    fn test_verify_valid_clip() {
        let clip = make_test_clip("Hello world", vec!["src_01".to_string()]);
        assert!(verify_clip_hash(&clip).is_ok());
        assert!(verify_text_hash(&clip).is_ok());
    }

    #[test]
    fn test_verify_tampered_content() {
        let mut clip = make_test_clip("Hello world", vec!["src_01".to_string()]);
        clip.content = Some("Tampered".to_string());
        assert!(verify_text_hash(&clip).is_err());
    }

    #[test]
    fn test_verify_tampered_clip_hash() {
        let mut clip = make_test_clip("Hello world", vec!["src_01".to_string()]);
        clip.clip_hash =
            ContentHash("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string());
        assert!(verify_clip_hash(&clip).is_err());
    }
}
