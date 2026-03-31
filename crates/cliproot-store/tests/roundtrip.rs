use cliproot_core::{
    create_clip_hash, create_text_hash,
    hash::ClipHashInput,
    model::*,
    verify::{verify_clip_hash, verify_text_hash},
};
use cliproot_store::Repository;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;
use tar::{Archive, Builder, Header};
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

fn make_bundle(
    project: Option<Project>,
    source: SourceRecord,
    clip: Clip,
    edges: Vec<Edge>,
) -> CrpBundle {
    CrpBundle {
        protocol_version: "0.0.3".to_string(),
        bundle_type: if edges.is_empty() {
            BundleType::Document
        } else {
            BundleType::Derivation
        },
        created_at: now(),
        project,
        document: None,
        agents: Vec::new(),
        sources: vec![source],
        clips: vec![clip],
        artifacts: Vec::new(),
        clip_artifact_refs: Vec::new(),
        activities: Vec::new(),
        edges,
        reuse_events: Vec::new(),
        signatures: Vec::new(),
        registry: None,
    }
}

#[test]
fn test_full_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    repo.create_project(
        "proj_demo",
        "Demo",
        Some("Roundtrip test project".to_string()),
    )
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
    assert_eq!(
        exported.project.as_ref().map(|p| p.id.0.as_str()),
        Some("proj_demo")
    );

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
fn test_activity_and_session_recovery_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    repo.create_project("proj_demo", "Demo", None).unwrap();
    repo.use_project("proj_demo").unwrap();

    let session = repo
        .start_session(
            Some("proj_demo"),
            Some("agent-demo"),
            Some(serde_json::json!({ "origin": "test" })),
        )
        .unwrap();
    let activity = repo
        .start_activity(
            ActivityType::Research,
            Some("proj_demo"),
            Some("agent-demo"),
            Some("Research the implementation details".to_string()),
            Some(serde_json::json!({ "temperature": 0.1 })),
            Some(&session.session_id),
        )
        .unwrap();

    let (mut clip, source) = make_clip("Tracked research note", "src_session_01", "proj_demo");
    clip.created_by_activity_id = Some(activity.id.clone());
    let clip_hash = clip.clip_hash.0.clone();
    let source_refs = clip.source_refs.clone();
    repo.store_bundle(&make_bundle(
        Some(make_project()),
        source,
        clip.clone(),
        Vec::new(),
    ))
    .unwrap();
    repo.record_clip_tracking(&clip_hash, Some(activity.id.as_str()), None, &source_refs)
        .unwrap();

    let reopened = Repository::open(tmp.path()).unwrap();
    let ended_activity = reopened.end_activity(activity.id.as_str()).unwrap();
    assert_eq!(ended_activity.id, activity.id);
    assert!(ended_activity.ended_at.is_some());
    assert_eq!(ended_activity.generated_clip_refs, vec![clip_hash.clone()]);
    assert_eq!(ended_activity.used_source_refs, source_refs);

    let ended_session = reopened.end_session(&session.session_id).unwrap();
    assert_eq!(ended_session.session_id, session.session_id);
    assert!(ended_session.ended_at.is_some());
    assert_eq!(ended_session.activity_ids, vec![activity.id.0.clone()]);
    assert_eq!(ended_session.generated_clip_hashes, vec![clip_hash.clone()]);

    let session_artifact_hash = ended_session.artifact_hash.clone().unwrap();
    let session_artifact = reopened
        .get_artifact(&session_artifact_hash)
        .unwrap()
        .unwrap();
    assert_eq!(session_artifact.artifact_type, ArtifactType::Session);

    let exported = reopened.export_bundle(&clip_hash).unwrap();
    assert_eq!(exported.activities.len(), 1);
    assert_eq!(exported.activities[0].id, activity.id);
    assert!(exported
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_hash.0 == session_artifact_hash));
    assert!(exported.clip_artifact_refs.iter().any(|link| {
        link.clip_hash.0 == clip_hash
            && link.artifact_hash.0 == session_artifact_hash
            && link.relationship == ClipArtifactRelationship::AttachedTo
    }));
}

#[test]
fn test_pack_includes_session_artifact_links() {
    let tmp = TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    repo.create_project("proj_demo", "Demo", None).unwrap();
    repo.use_project("proj_demo").unwrap();

    let session = repo
        .start_session(Some("proj_demo"), Some("agent-demo"), None)
        .unwrap();
    let activity = repo
        .start_activity(
            ActivityType::Plan,
            Some("proj_demo"),
            Some("agent-demo"),
            Some("Draft the plan".to_string()),
            None,
            Some(&session.session_id),
        )
        .unwrap();

    let (mut clip, source) = make_clip("Planned work item", "src_plan_01", "proj_demo");
    clip.created_by_activity_id = Some(activity.id.clone());
    let clip_hash = clip.clip_hash.0.clone();
    let source_refs = clip.source_refs.clone();
    repo.store_bundle(&make_bundle(
        Some(make_project()),
        source,
        clip.clone(),
        Vec::new(),
    ))
    .unwrap();
    repo.record_clip_tracking(&clip_hash, Some(activity.id.as_str()), None, &source_refs)
        .unwrap();
    repo.end_activity(activity.id.as_str()).unwrap();
    let ended_session = repo.end_session(&session.session_id).unwrap();
    let session_artifact_hash = ended_session.artifact_hash.unwrap();

    let pack_path = tmp.path().join("session.cliprootpack");
    let manifest = repo
        .create_pack(None, &[clip_hash.clone()], None, &pack_path)
        .unwrap();
    assert_eq!(manifest.counts.artifacts, 1);
    assert_eq!(manifest.counts.links, 1);
    assert!(manifest
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_hash == session_artifact_hash));

    let tmp2 = TempDir::new().unwrap();
    let repo2 = Repository::init(tmp2.path()).unwrap();
    repo2.import_pack(&pack_path, None).unwrap();
    assert!(repo2.get_clip_full(&clip_hash).unwrap().is_some());
    let imported_artifact = repo2.get_artifact(&session_artifact_hash).unwrap().unwrap();
    assert_eq!(imported_artifact.artifact_type, ArtifactType::Session);
}

#[test]
fn test_example_bundle_deserialization() {
    let path = std::env::var("CRP_EXAMPLE_JSON_PATH")
        .expect("CRP_EXAMPLE_JSON_PATH must be set (see .cargo/config.toml for local dev)");
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("Could not read CRP_EXAMPLE_JSON_PATH: {path}"));
    let bundle: CrpBundle = serde_json::from_str(&json).unwrap();
    assert_eq!(bundle.protocol_version, "0.0.3");
    assert_eq!(bundle.clips.len(), 2);
    assert_eq!(bundle.edges.len(), 2);
    assert_eq!(bundle.project.unwrap().id.0, "proj_auth_refactor");
}

#[test]
fn test_pack_roundtrip_project_mode_and_restore() {
    let tmp = TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    repo.create_project("proj_demo", "Demo", Some("Pack project".to_string()))
        .unwrap();
    repo.use_project("proj_demo").unwrap();

    let (clip1, source1) = make_clip("Root source", "src_pack_01", "proj_demo");
    let clip1_hash = clip1.clip_hash.0.clone();
    repo.store_bundle(&make_bundle(
        Some(make_project()),
        source1,
        clip1.clone(),
        Vec::new(),
    ))
    .unwrap();

    let (mut clip2, source2) = make_clip("Derived summary", "src_pack_02", "proj_demo");
    let clip2_hash = clip2.clip_hash.0.clone();
    clip2.selectors = Some(Selectors {
        text_position: None,
        text_quote: Some(TextQuoteSelector {
            exact: "Derived summary".to_string(),
            prefix: None,
            suffix: None,
        }),
        editor_path: None,
        dom: None,
        media_time: None,
        parent_clip_hash: Some(ContentHash(clip1_hash.clone())),
    });
    let edge = Edge {
        id: CrpId("edge_pack_01".to_string()),
        edge_type: EdgeType::WasDerivedFrom,
        subject_ref: CrpId(clip2_hash.clone()),
        object_ref: CrpId(clip1_hash.clone()),
        transformation_type: Some(TransformationType::Summary),
        agent_id: None,
        confidence: Some(0.9),
        created_at: now(),
    };
    repo.store_bundle(&make_bundle(
        Some(make_project()),
        source2,
        clip2.clone(),
        vec![edge],
    ))
    .unwrap();

    let linked_artifact = repo
        .add_artifact(
            None,
            Some(b"# Plan\nlinked"),
            Some("plan.md"),
            ArtifactType::Markdown,
            Some("text/markdown"),
            Some("artifact_linked"),
            None,
            None,
        )
        .unwrap();
    let unlinked_artifact = repo
        .add_artifact(
            None,
            Some(b"# Plan\nunlinked"),
            Some("plan.md"),
            ArtifactType::Markdown,
            Some("text/markdown"),
            Some("artifact_unlinked"),
            None,
            None,
        )
        .unwrap();
    repo.link_clip_artifact(
        &clip2_hash,
        &linked_artifact.artifact_hash.0,
        ClipArtifactRelationship::CitedIn,
    )
    .unwrap();

    let pack_path = tmp.path().join("demo.cliprootpack");
    let manifest = repo
        .create_pack(Some("proj_demo"), &[], None, &pack_path)
        .unwrap();
    assert_eq!(manifest.counts.bundles, 2);
    assert_eq!(manifest.counts.artifacts, 2);
    assert_eq!(manifest.counts.links, 1);

    let inspected = Repository::inspect_pack(&pack_path).unwrap();
    assert_eq!(inspected.roots.project_id.as_deref(), Some("proj_demo"));
    Repository::verify_pack(&pack_path).unwrap();

    let tmp2 = TempDir::new().unwrap();
    let repo2 = Repository::init(tmp2.path()).unwrap();
    let restore_dir = tmp2.path().join("restored");
    repo2.import_pack(&pack_path, Some(&restore_dir)).unwrap();

    assert!(repo2.get_clip_full(&clip1_hash).unwrap().is_some());
    assert!(repo2.get_clip_full(&clip2_hash).unwrap().is_some());
    assert_eq!(repo2.list_artifacts(None).unwrap().len(), 2);
    assert!(repo2
        .list_projects()
        .unwrap()
        .iter()
        .any(|project| project.id.0 == "proj_demo"));

    let restored_names = std::fs::read_dir(&restore_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert_eq!(restored_names.len(), 2);
    assert!(restored_names
        .iter()
        .any(|name| name == &format!("{}--plan.md", linked_artifact.artifact_hash.0)));
    assert!(restored_names
        .iter()
        .any(|name| name == &format!("{}--plan.md", unlinked_artifact.artifact_hash.0)));

    repo2.import_pack(&pack_path, None).unwrap();
    assert_eq!(
        repo2.list_clips(None, None, None, Some(50)).unwrap().len(),
        2
    );
    assert_eq!(repo2.list_artifacts(None).unwrap().len(), 2);
}

#[test]
fn test_pack_root_mode_depth_limit() {
    let tmp = TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    repo.create_project("proj_demo", "Demo", None).unwrap();
    repo.use_project("proj_demo").unwrap();

    let (clip1, source1) = make_clip("Ancestor", "src_depth_01", "proj_demo");
    let clip1_hash = clip1.clip_hash.0.clone();
    repo.store_bundle(&make_bundle(
        Some(make_project()),
        source1,
        clip1.clone(),
        Vec::new(),
    ))
    .unwrap();

    let (clip2, source2) = make_clip("Parent", "src_depth_02", "proj_demo");
    let clip2_hash = clip2.clip_hash.0.clone();
    repo.store_bundle(&make_bundle(
        Some(make_project()),
        source2,
        clip2.clone(),
        vec![Edge {
            id: CrpId("edge_depth_01".to_string()),
            edge_type: EdgeType::WasDerivedFrom,
            subject_ref: CrpId(clip2_hash.clone()),
            object_ref: CrpId(clip1_hash.clone()),
            transformation_type: Some(TransformationType::Summary),
            agent_id: None,
            confidence: None,
            created_at: now(),
        }],
    ))
    .unwrap();

    let (clip3, source3) = make_clip("Grandchild", "src_depth_03", "proj_demo");
    let clip3_hash = clip3.clip_hash.0.clone();
    repo.store_bundle(&make_bundle(
        Some(make_project()),
        source3,
        clip3.clone(),
        vec![Edge {
            id: CrpId("edge_depth_02".to_string()),
            edge_type: EdgeType::WasDerivedFrom,
            subject_ref: CrpId(clip3_hash.clone()),
            object_ref: CrpId(clip2_hash.clone()),
            transformation_type: Some(TransformationType::Summary),
            agent_id: None,
            confidence: None,
            created_at: now(),
        }],
    ))
    .unwrap();

    let pack_path = tmp.path().join("depth.cliprootpack");
    let manifest = repo
        .create_pack(None, &[clip3_hash.clone()], Some(1), &pack_path)
        .unwrap();
    assert!(manifest.project.is_none());
    assert_eq!(manifest.roots.clip_hashes, vec![clip3_hash.clone()]);

    let tmp2 = TempDir::new().unwrap();
    let repo2 = Repository::init(tmp2.path()).unwrap();
    repo2.import_pack(&pack_path, None).unwrap();

    assert!(repo2.get_clip_full(&clip3_hash).unwrap().is_some());
    assert!(repo2.get_clip_full(&clip2_hash).unwrap().is_some());
    assert!(repo2.get_clip_full(&clip1_hash).unwrap().is_none());
}

#[test]
fn test_pack_verify_detects_corruption() {
    let tmp = TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    repo.create_project("proj_demo", "Demo", None).unwrap();
    repo.use_project("proj_demo").unwrap();

    let (clip1, source1) = make_clip("Corruption source", "src_corrupt_01", "proj_demo");
    let clip1_hash = clip1.clip_hash.0.clone();
    repo.store_bundle(&make_bundle(
        Some(make_project()),
        source1,
        clip1,
        Vec::new(),
    ))
    .unwrap();
    let artifact = repo
        .add_artifact(
            None,
            Some(b"artifact bytes"),
            Some("notes.txt"),
            ArtifactType::Text,
            Some("text/plain"),
            Some("artifact_corrupt"),
            None,
            None,
        )
        .unwrap();
    repo.link_clip_artifact(
        &clip1_hash,
        &artifact.artifact_hash.0,
        ClipArtifactRelationship::AttachedTo,
    )
    .unwrap();

    let pack_path = tmp.path().join("corrupt.cliprootpack");
    let manifest = repo
        .create_pack(Some("proj_demo"), &[], None, &pack_path)
        .unwrap();

    let object_path = manifest.objects[0].archive_path.clone();
    let artifact_path = manifest.artifacts[0].archive_path.clone();

    let bad_json_path = tmp.path().join("bad-json.cliprootpack");
    let mut entries = unpack_pack_entries(&pack_path);
    let original_len = entries.get(&object_path).unwrap().len();
    let bad_json_bytes = vec![b'{'; original_len];
    entries.insert(object_path.clone(), bad_json_bytes.clone());
    let mut manifest_json: serde_json::Value =
        serde_json::from_slice(entries.get("manifest.json").unwrap()).unwrap();
    manifest_json["objects"][0]["sha256Digest"] =
        serde_json::Value::String(cliproot_store::pack::sha256_digest(&bad_json_bytes));
    entries.insert(
        "manifest.json".to_string(),
        serde_json::to_vec_pretty(&manifest_json).unwrap(),
    );
    write_pack_entries(&bad_json_path, &entries);
    let err = Repository::verify_pack(&bad_json_path)
        .unwrap_err()
        .to_string();
    assert!(err.contains("json error"));

    let bad_artifact_path = tmp.path().join("bad-artifact.cliprootpack");
    let mut entries = unpack_pack_entries(&pack_path);
    let mut manifest_json: serde_json::Value =
        serde_json::from_slice(entries.get("manifest.json").unwrap()).unwrap();
    let mut tampered_bytes = entries.get(&artifact_path).unwrap().clone();
    tampered_bytes[0] = b'X';
    entries.insert(artifact_path.clone(), tampered_bytes.clone());
    manifest_json["artifacts"][0]["sha256Digest"] =
        serde_json::Value::String(cliproot_store::pack::sha256_digest(&tampered_bytes));
    entries.insert(
        "manifest.json".to_string(),
        serde_json::to_vec_pretty(&manifest_json).unwrap(),
    );
    write_pack_entries(&bad_artifact_path, &entries);
    let err = Repository::verify_pack(&bad_artifact_path)
        .unwrap_err()
        .to_string();
    assert!(err.contains("artifact hash mismatch"));

    let bad_manifest_path = tmp.path().join("bad-manifest.cliprootpack");
    let mut entries = unpack_pack_entries(&pack_path);
    let mut manifest_json: serde_json::Value =
        serde_json::from_slice(entries.get("manifest.json").unwrap()).unwrap();
    manifest_json["objects"][0]["sha256Digest"] =
        serde_json::Value::String("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string());
    entries.insert(
        "manifest.json".to_string(),
        serde_json::to_vec_pretty(&manifest_json).unwrap(),
    );
    write_pack_entries(&bad_manifest_path, &entries);
    let err = Repository::verify_pack(&bad_manifest_path)
        .unwrap_err()
        .to_string();
    assert!(err.contains("digest mismatch"));
}

fn unpack_pack_entries(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let file = File::open(path).unwrap();
    let decoder = zstd::Decoder::new(file).unwrap();
    let mut archive = Archive::new(decoder);
    let mut entries = BTreeMap::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().to_string_lossy().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        entries.insert(name, bytes);
    }
    entries
}

fn write_pack_entries(path: &Path, entries: &BTreeMap<String, Vec<u8>>) {
    let file = File::create(path).unwrap();
    let encoder = zstd::Encoder::new(file, 3).unwrap();
    let mut builder = Builder::new(encoder);
    for (name, bytes) in entries {
        let mut header = Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, name, Cursor::new(bytes))
            .unwrap();
    }
    builder.finish().unwrap();
    builder.into_inner().unwrap().finish().unwrap();
}
