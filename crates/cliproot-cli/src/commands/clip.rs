use cliproot_clipboard::ClipboardWriter;
use cliproot_core::{create_clip_hash, create_text_hash, hash::ClipHashInput, model::*};
use cliproot_store::Repository;

use crate::output::print_clip;
use crate::OutputFormat;

pub fn run(
    url: &str,
    quote: &str,
    source_type_str: &str,
    id: Option<String>,
    document_id: Option<String>,
    project_id: Option<String>,
    title: Option<String>,
    copy: bool,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let resolved_project_id = project_id.clone().or(repo.current_project_id()?).map(CrpId);

    let source_type: SourceType =
        serde_json::from_value(serde_json::Value::String(source_type_str.to_string()))?;

    let source_id = format!("src-{}", uuid::Uuid::new_v4());
    let source = SourceRecord {
        id: CrpId(source_id.clone()),
        source_type,
        digital_source_type: None,
        title: title.clone(),
        source_uri: Some(url.to_string()),
        author_agent_id: None,
        created_at: Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
    };

    let text_hash = create_text_hash(quote);
    let clip_hash = create_clip_hash(ClipHashInput {
        text_hash: text_hash.clone(),
        source_refs: vec![source_id.clone()],
        text_quote_exact: Some(quote.to_string()),
    });

    let clip = Clip {
        clip_hash: clip_hash.clone(),
        id: id.map(CrpId),
        project_id: resolved_project_id.clone(),
        document_id: document_id.map(CrpId),
        source_refs: vec![source_id],
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

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let bundle = CrpBundle {
        protocol_version: "0.0.3".to_string(),
        bundle_type: BundleType::Document,
        created_at: now,
        project: resolved_project_id.as_ref().and_then(|project_id| {
            repo.list_projects()
                .ok()?
                .into_iter()
                .find(|p| p.id == *project_id)
        }),
        document: None,
        agents: Vec::new(),
        sources: vec![source],
        clips: vec![clip.clone()],
        artifacts: Vec::new(),
        clip_artifact_refs: Vec::new(),
        activities: Vec::new(),
        edges: Vec::new(),
        reuse_events: Vec::new(),
        signatures: Vec::new(),
        registry: None,
    };

    repo.store_bundle(&bundle)?;
    print_clip(&clip, format);

    if copy {
        let bundle_json = serde_json::to_string(&bundle)?;
        let mut cb = ClipboardWriter::new()?;
        cb.write_with_html(quote, &bundle_json)?;
        println!("copied (html with data-crp-bundle): {}", clip.clip_hash.0);
    }

    Ok(())
}
