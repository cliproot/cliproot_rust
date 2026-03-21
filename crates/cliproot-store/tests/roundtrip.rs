use cliproot_core::{
    create_clip_hash, create_text_hash,
    hash::ClipHashInput,
    model::*,
    verify::{verify_clip_hash, verify_text_hash},
};
use cliproot_store::Repository;
use tempfile::TempDir;

fn make_clip(content: &str, source_id: &str) -> (Clip, SourceRecord) {
    let source = SourceRecord {
        id: CrpId(source_id.to_string()),
        source_type: SourceType::ExternalQuoted,
        digital_source_type: None,
        title: None,
        source_uri: Some("https://example.com".to_string()),
        author_agent_id: None,
        created_at: None,
    };

    let text_hash = create_text_hash(content);
    let clip_hash = create_clip_hash(ClipHashInput {
        text_hash: text_hash.clone(),
        source_refs: vec![source_id.to_string()],
        text_quote_exact: Some(content.to_string()),
    });

    let clip = Clip {
        clip_hash,
        id: Some(CrpId(format!("clip-{source_id}"))),
        document_id: None,
        source_refs: vec![source_id.to_string()],
        selectors: Some(Selectors {
            text_position: None,
            text_quote: Some(TextQuoteSelector {
                exact: content.to_string(),
                prefix: None,
                suffix: None,
            }),
            editor_path: None,
            dom: None,
            media_time: None,
            parent_clip_hash: None,
        }),
        content: Some(content.to_string()),
        text_hash,
        created_by_activity_id: None,
    };

    (clip, source)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[test]
fn test_full_roundtrip() {
    let tmp = TempDir::new().unwrap();

    // 1. Init repository
    let repo = Repository::init(tmp.path()).unwrap();

    // 2. Store a clip
    let (clip1, source1) = make_clip("Hello world", "src_01");
    let clip1_hash = clip1.clip_hash.0.clone();

    let bundle1 = CrpBundle {
        protocol_version: "0.0.2".to_string(),
        bundle_type: BundleType::Document,
        created_at: now(),
        document: None,
        agents: Vec::new(),
        sources: vec![source1],
        clips: vec![clip1.clone()],
        activities: Vec::new(),
        derivation_edges: Vec::new(),
        reuse_events: Vec::new(),
        signatures: Vec::new(),
        registry: None,
    };

    let stored_hash = repo.store_bundle(&bundle1).unwrap();
    assert_eq!(stored_hash, clip1_hash);

    // 3. Retrieve by hash and verify integrity
    let retrieved = repo.get_clip_full(&clip1_hash).unwrap().unwrap();
    assert_eq!(retrieved.content.as_deref(), Some("Hello world"));
    verify_clip_hash(&retrieved).unwrap();
    verify_text_hash(&retrieved).unwrap();

    // 4. Derive a new clip from it
    let derived_content = "Summary of hello";
    let (mut derived_clip, derived_source) = make_clip(derived_content, "src_derived");
    // Recalculate with proper source refs
    let derived_text_hash = create_text_hash(derived_content);
    let derived_clip_hash = create_clip_hash(ClipHashInput {
        text_hash: derived_text_hash.clone(),
        source_refs: vec!["src_derived".to_string()],
        text_quote_exact: Some(derived_content.to_string()),
    });
    derived_clip.clip_hash = derived_clip_hash.clone();
    derived_clip.text_hash = derived_text_hash;

    let edge = DerivationEdge {
        id: CrpId("edge_01".to_string()),
        child_clip_hash: derived_clip_hash.clone(),
        parent_clip_hash: ContentHash(clip1_hash.clone()),
        transformation_type: TransformationType::Summary,
        agent_id: None,
        confidence: None,
        created_at: now(),
    };

    let bundle2 = CrpBundle {
        protocol_version: "0.0.2".to_string(),
        bundle_type: BundleType::Derivation,
        created_at: now(),
        document: None,
        agents: Vec::new(),
        sources: vec![derived_source],
        clips: vec![derived_clip.clone()],
        activities: Vec::new(),
        derivation_edges: vec![edge],
        reuse_events: Vec::new(),
        signatures: Vec::new(),
        registry: None,
    };

    repo.store_bundle(&bundle2).unwrap();

    // 5. Trace lineage from derived → original
    let lineage = repo.trace(&derived_clip_hash.0).unwrap();
    assert!(!lineage.is_empty());
    assert_eq!(lineage[0].parent_hash, clip1_hash);

    // 6. Export as bundle JSON
    let exported = repo.export_bundle(&derived_clip_hash.0).unwrap();
    assert_eq!(exported.protocol_version, "0.0.2");
    assert!(!exported.clips.is_empty());
    assert!(!exported.derivation_edges.is_empty());

    // 7. Validate exported JSON structure
    let json = serde_json::to_string_pretty(&exported).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["protocolVersion"], "0.0.2");
    assert_eq!(parsed["bundleType"], "provenance-export");

    // 8. Ingest into a fresh repository
    let tmp2 = TempDir::new().unwrap();
    let repo2 = Repository::init(tmp2.path()).unwrap();
    repo2.ingest_bundle(&exported).unwrap();

    // 9. Verify round-trip integrity
    let reloaded = repo2.get_clip_full(&derived_clip_hash.0).unwrap().unwrap();
    verify_clip_hash(&reloaded).unwrap();
    verify_text_hash(&reloaded).unwrap();
    assert_eq!(reloaded.content.as_deref(), Some("Summary of hello"));
}

#[test]
fn test_verify_all() {
    let tmp = TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();

    let (clip, source) = make_clip("Test content", "src_v");
    let bundle = CrpBundle {
        protocol_version: "0.0.2".to_string(),
        bundle_type: BundleType::Document,
        created_at: now(),
        document: None,
        agents: Vec::new(),
        sources: vec![source],
        clips: vec![clip],
        activities: Vec::new(),
        derivation_edges: Vec::new(),
        reuse_events: Vec::new(),
        signatures: Vec::new(),
        registry: None,
    };

    repo.store_bundle(&bundle).unwrap();

    let errors = repo.verify_all().unwrap();
    assert!(errors.is_empty());
}

#[test]
fn test_example_bundle_deserialization() {
    let json = include_str!("../../../../cliproot/schema/examples/crp-v0.0.2.document.example.json");
    let bundle: CrpBundle = serde_json::from_str(json).unwrap();
    assert_eq!(bundle.protocol_version, "0.0.2");
    assert_eq!(bundle.clips.len(), 2);
    assert_eq!(bundle.derivation_edges.len(), 1);
}
