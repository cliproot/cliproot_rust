use cliproot_core::{
    create_clip_hash, create_text_hash,
    hash::ClipHashInput,
    model::*,
    verify::{verify_clip_hash, verify_text_hash},
};
use cliproot_store::Repository;
use tempfile::TempDir;

fn make_clip(content: &str, source_id: &str, project_id: &str) -> (Clip, SourceRecord) {
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
        project_id: Some(CrpId(project_id.to_string())),
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

fn make_project() -> Project {
    Project {
        id: CrpId("proj_demo".to_string()),
        name: "Demo".to_string(),
        description: Some("Roundtrip test project".to_string()),
        created_at: Some(now()),
        updated_at: Some(now()),
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[test]
fn test_full_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    repo.create_project("proj_demo", "Demo", Some("Roundtrip test project".to_string()))
        .unwrap();
    repo.use_project("proj_demo").unwrap();

    let (clip1, source1) = make_clip("Hello world", "src_01", "proj_demo");
    let clip1_hash = clip1.clip_hash.0.clone();

    let bundle1 = CrpBundle {
        protocol_version: "0.0.3".to_string(),
        bundle_type: BundleType::Document,
        created_at: now(),
        project: Some(make_project()),
        document: None,
        agents: Vec::new(),
        sources: vec![source1],
        clips: vec![clip1.clone()],
        artifacts: Vec::new(),
        clip_artifact_refs: Vec::new(),
        activities: Vec::new(),
        edges: Vec::new(),
        reuse_events: Vec::new(),
        signatures: Vec::new(),
        registry: None,
    };

    let stored_hash = repo.store_bundle(&bundle1).unwrap();
    assert_eq!(stored_hash, clip1_hash);

    let retrieved = repo.get_clip_full(&clip1_hash).unwrap().unwrap();
    assert_eq!(retrieved.content.as_deref(), Some("Hello world"));
    verify_clip_hash(&retrieved).unwrap();
    verify_text_hash(&retrieved).unwrap();

    let derived_content = "Summary of hello";
    let (mut derived_clip, derived_source) = make_clip(derived_content, "src_derived", "proj_demo");
    let derived_text_hash = create_text_hash(derived_content);
    let derived_clip_hash = create_clip_hash(ClipHashInput {
        text_hash: derived_text_hash.clone(),
        source_refs: vec!["src_derived".to_string()],
        text_quote_exact: Some(derived_content.to_string()),
    });
    derived_clip.clip_hash = derived_clip_hash.clone();
    derived_clip.text_hash = derived_text_hash;

    let edge = Edge {
        id: CrpId("edge_01".to_string()),
        edge_type: EdgeType::WasDerivedFrom,
        subject_ref: CrpId(derived_clip_hash.0.clone()),
        object_ref: CrpId(clip1_hash.clone()),
        transformation_type: Some(TransformationType::Summary),
        agent_id: None,
        confidence: None,
        created_at: now(),
    };

    let bundle2 = CrpBundle {
        protocol_version: "0.0.3".to_string(),
        bundle_type: BundleType::Derivation,
        created_at: now(),
        project: Some(make_project()),
        document: None,
        agents: Vec::new(),
        sources: vec![derived_source],
        clips: vec![derived_clip.clone()],
        artifacts: Vec::new(),
        clip_artifact_refs: Vec::new(),
        activities: Vec::new(),
        edges: vec![edge],
        reuse_events: Vec::new(),
        signatures: Vec::new(),
        registry: None,
    };

    repo.store_bundle(&bundle2).unwrap();

    let lineage = repo.trace(&derived_clip_hash.0).unwrap();
    assert!(!lineage.is_empty());
    assert_eq!(lineage[0].parent_hash, clip1_hash);

    let exported = repo.export_bundle(&derived_clip_hash.0).unwrap();
    assert_eq!(exported.protocol_version, "0.0.3");
    assert!(!exported.clips.is_empty());
    assert!(!exported.edges.is_empty());
    assert_eq!(exported.project.as_ref().map(|p| p.id.0.as_str()), Some("proj_demo"));

    let json = serde_json::to_string_pretty(&exported).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["protocolVersion"], "0.0.3");
    assert_eq!(parsed["bundleType"], "provenance-export");

    let tmp2 = TempDir::new().unwrap();
    let repo2 = Repository::init(tmp2.path()).unwrap();
    repo2.ingest_bundle(&exported).unwrap();

    let reloaded = repo2.get_clip_full(&derived_clip_hash.0).unwrap().unwrap();
    verify_clip_hash(&reloaded).unwrap();
    verify_text_hash(&reloaded).unwrap();
    assert_eq!(reloaded.content.as_deref(), Some("Summary of hello"));
}

#[test]
fn test_artifact_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    repo.create_project("proj_demo", "Demo", None).unwrap();
    repo.use_project("proj_demo").unwrap();

    let artifact = repo
        .add_artifact(
            None,
            Some(b"# Plan\n\n- Research\n- Implement"),
            Some("plan.md"),
            ArtifactType::Markdown,
            Some("text/markdown"),
            Some("artifact_plan"),
            None,
            None,
        )
        .unwrap();

    let listed = repo.list_artifacts(None).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].artifact_hash, artifact.artifact_hash);

    let restore_dir = tmp.path().join("restore");
    std::fs::create_dir_all(&restore_dir).unwrap();
    let restored = repo
        .restore_artifact(&artifact.artifact_hash.0, Some(&restore_dir))
        .unwrap();
    let restored_text = std::fs::read_to_string(restored).unwrap();
    assert!(restored_text.contains("Research"));
}

#[test]
fn test_example_bundle_deserialization() {
    let json =
        include_str!("../../../../cliproot/schema/examples/crp-v0.0.3.document.example.json");
    let bundle: CrpBundle = serde_json::from_str(json).unwrap();
    assert_eq!(bundle.protocol_version, "0.0.3");
    assert_eq!(bundle.clips.len(), 2);
    assert_eq!(bundle.edges.len(), 2);
    assert_eq!(bundle.project.unwrap().id.0, "proj_auth_refactor");
}
