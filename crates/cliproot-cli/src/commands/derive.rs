use cliproot_core::{create_clip_hash, create_text_hash, hash::ClipHashInput, model::*};
use cliproot_store::Repository;

use crate::output::print_clip;
use crate::OutputFormat;

pub fn run(
    from: &[String],
    quote: &str,
    activity_type_str: &str,
    agent: Option<&str>,
    project_id: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let resolved_project_id = project_id
        .map(|value| value.to_string())
        .or(repo.current_project_id()?)
        .map(CrpId);

    // Resolve parent clip hashes
    let mut parent_hashes = Vec::new();
    for hash_or_id in from {
        let hash = repo
            .resolve_clip_hash(hash_or_id)?
            .ok_or_else(|| format!("parent clip not found: {hash_or_id}"))?;
        parent_hashes.push(hash);
    }

    // Get parent clips to collect source refs
    let mut all_source_refs = Vec::new();
    for hash in &parent_hashes {
        if let Some(clip) = repo.get_clip(hash)? {
            for sr in &clip.source_refs {
                if !all_source_refs.contains(sr) {
                    all_source_refs.push(sr.clone());
                }
            }
        }
    }

    // Create a derived source record
    let derived_source_id = format!("src-derived-{}", uuid::Uuid::new_v4());
    let derived_source = SourceRecord {
        id: CrpId(derived_source_id.clone()),
        source_type: SourceType::AiAssisted,
        digital_source_type: None,
        title: None,
        source_uri: None,
        author_agent_id: agent.map(|a| CrpId(a.to_string())),
        created_at: Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
    };

    let source_refs = vec![derived_source_id.clone()];

    let text_hash = create_text_hash(quote);
    let clip_hash = create_clip_hash(ClipHashInput {
        text_hash: text_hash.clone(),
        source_refs: source_refs.clone(),
        text_quote_exact: Some(quote.to_string()),
    });

    let clip = Clip {
        clip_hash: clip_hash.clone(),
        id: None,
        project_id: resolved_project_id.clone(),
        document_id: None,
        source_refs: source_refs.clone(),
        selectors: Some(Selectors {
            text_position: None,
            text_quote: Some(TextQuoteSelector {
                exact: quote.to_string(),
                prefix: None,
                suffix: None,
            }),
            editor_path: None,
            dom: None,
            media_time: None,
            parent_clip_hash: None,
        }),
        content: Some(quote.to_string()),
        text_hash,
        created_by_activity_id: None,
    };

    // Parse transformation type from activity type
    let transformation_type: TransformationType =
        serde_json::from_value(serde_json::Value::String(activity_type_str.to_string()))
            .unwrap_or(TransformationType::Unknown);

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // Create derivation edges
    let edges: Vec<Edge> = parent_hashes
        .iter()
        .map(|parent_hash| Edge {
            id: CrpId(format!("edge-{}", uuid::Uuid::new_v4())),
            edge_type: EdgeType::WasDerivedFrom,
            subject_ref: CrpId(clip_hash.0.clone()),
            object_ref: CrpId(parent_hash.clone()),
            transformation_type: Some(transformation_type.clone()),
            agent_id: agent.map(|a| CrpId(a.to_string())),
            confidence: None,
            created_at: now.clone(),
        })
        .collect();

    let bundle = CrpBundle {
        protocol_version: "0.0.3".to_string(),
        bundle_type: BundleType::Derivation,
        created_at: now,
        project: resolved_project_id.as_ref().and_then(|project_id| {
            repo.list_projects()
                .ok()?
                .into_iter()
                .find(|p| p.id == *project_id)
        }),
        document: None,
        agents: Vec::new(),
        sources: vec![derived_source],
        clips: vec![clip.clone()],
        artifacts: Vec::new(),
        clip_artifact_refs: Vec::new(),
        activities: Vec::new(),
        edges,
        reuse_events: Vec::new(),
        signatures: Vec::new(),
        registry: None,
    };

    repo.store_bundle(&bundle)?;
    print_clip(&clip, format);

    Ok(())
}
