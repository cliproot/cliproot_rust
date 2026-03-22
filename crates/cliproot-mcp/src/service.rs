//! ClipRootService — MCP server implementation exposing cliproot operations as tools.

use std::sync::Arc;

use cliproot_core::{
    create_clip_hash, create_text_hash,
    hash::ClipHashInput,
    model::*,
};
use cliproot_store::StoreError;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ErrorData, Implementation, ServerInfo},
    tool, tool_handler, tool_router,
};
use serde_json::json;

use crate::params::*;
use crate::repo_handle::RepoHandle;

const PROTOCOL_VERSION: &str = "0.0.2";

// ── Helpers ────────────────────────────────────────────────────────────────

pub(crate) fn ok_json(v: impl serde::Serialize) -> Result<CallToolResult, ErrorData> {
    let json = serde_json::to_string_pretty(&v)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(crate) fn store_err(e: StoreError) -> ErrorData {
    match e {
        StoreError::NotFound => {
            ErrorData::invalid_params("clip not found", None)
        }
        StoreError::AlreadyExists(s) => {
            ErrorData::invalid_params(format!("already exists: {s}"), None)
        }
        other => ErrorData::internal_error(other.to_string(), None),
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

// ── Service ────────────────────────────────────────────────────────────────

/// MCP server that exposes Cliproot provenance operations as typed tools.
#[derive(Clone)]
pub struct ClipRootService {
    repo: Arc<RepoHandle>,
    tool_router: ToolRouter<Self>,
}

impl ClipRootService {
    pub fn new(repo: RepoHandle) -> Self {
        Self {
            repo: Arc::new(repo),
            tool_router: Self::tool_router(),
        }
    }
}

// ── Tool Implementations ───────────────────────────────────────────────────

#[tool_router]
impl ClipRootService {
    /// Capture a source clip: record a URL, exact quoted text, and source type into the
    /// Cliproot repository. Returns the content-addressed clip with its hash.
    #[tool(description = "Capture a source clip from a URL with exact quoted text. Returns the stored clip including its content-addressed hash for future reference.")]
    async fn cliproot_clip(
        &self,
        Parameters(params): Parameters<ClipParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let source_type: SourceType = serde_json::from_value(
            serde_json::Value::String(params.source_type),
        )
        .map_err(|e| ErrorData::invalid_params(format!("invalid source_type: {e}"), None))?;

        let source_id = format!("src-{}", uuid::Uuid::new_v4());
        let now = now_rfc3339();

        let source = SourceRecord {
            id: CrpId(source_id.clone()),
            source_type,
            digital_source_type: None,
            title: params.title,
            source_uri: Some(params.url),
            author_agent_id: None,
            created_at: Some(now.clone()),
        };

        let text_hash = create_text_hash(&params.quote);
        let clip_hash = create_clip_hash(ClipHashInput {
            text_hash: text_hash.clone(),
            source_refs: vec![source_id.clone()],
            text_quote_exact: Some(params.quote.clone()),
        });

        let clip = Clip {
            clip_hash: clip_hash.clone(),
            id: params.id.map(CrpId),
            document_id: params.document_id.map(CrpId),
            source_refs: vec![source_id],
            selectors: Some(Selectors {
                text_quote: Some(TextQuoteSelector {
                    exact: params.quote.clone(),
                    prefix: None,
                    suffix: None,
                }),
                text_position: None,
                editor_path: None,
                dom: None,
                media_time: None,
                parent_clip_hash: None,
            }),
            content: Some(params.quote),
            text_hash,
            created_by_activity_id: None,
        };

        let bundle = CrpBundle {
            protocol_version: PROTOCOL_VERSION.to_string(),
            bundle_type: BundleType::Document,
            created_at: now,
            document: None,
            agents: Vec::new(),
            sources: vec![source],
            clips: vec![clip.clone()],
            activities: Vec::new(),
            derivation_edges: Vec::new(),
            reuse_events: Vec::new(),
            signatures: Vec::new(),
            registry: None,
        };

        self.repo.store_bundle(bundle).await.map_err(store_err)?;
        ok_json(&clip)
    }

    /// Derive a new clip from one or more parent clips, recording the transformation type
    /// and derivation edges. Returns the derived child clip with its hash.
    #[tool(description = "Derive a new clip from one or more parent clips. Creates derivation edges recording how the content was transformed. Returns the child clip with its hash.")]
    async fn cliproot_derive(
        &self,
        Parameters(params): Parameters<DeriveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Resolve parent hashes
        let mut parent_hashes = Vec::new();
        for ref_str in &params.from {
            let h = self.repo.resolve_hash(ref_str.clone()).await.map_err(store_err)?
                .ok_or_else(|| ErrorData::invalid_params(
                    format!("parent clip not found: {ref_str}"),
                    None,
                ))?;
            parent_hashes.push(h);
        }

        // Collect source refs from parents
        let mut all_source_refs = Vec::new();
        for hash in &parent_hashes {
            if let Some(clip) = self.repo.get_clip(hash.clone()).await.map_err(store_err)? {
                for sr in &clip.source_refs {
                    if !all_source_refs.contains(sr) {
                        all_source_refs.push(sr.clone());
                    }
                }
            }
        }

        let derived_source_id = format!("src-derived-{}", uuid::Uuid::new_v4());
        let now = now_rfc3339();

        let derived_source = SourceRecord {
            id: CrpId(derived_source_id.clone()),
            source_type: SourceType::AiAssisted,
            digital_source_type: None,
            title: None,
            source_uri: None,
            author_agent_id: params.agent.as_deref().map(|a| CrpId(a.to_string())),
            created_at: Some(now.clone()),
        };

        let source_refs = vec![derived_source_id];
        let text_hash = create_text_hash(&params.quote);
        let clip_hash = create_clip_hash(ClipHashInput {
            text_hash: text_hash.clone(),
            source_refs: source_refs.clone(),
            text_quote_exact: Some(params.quote.clone()),
        });

        let clip = Clip {
            clip_hash: clip_hash.clone(),
            id: None,
            document_id: None,
            source_refs: source_refs.clone(),
            selectors: Some(Selectors {
                text_quote: Some(TextQuoteSelector {
                    exact: params.quote.clone(),
                    prefix: None,
                    suffix: None,
                }),
                text_position: None,
                editor_path: None,
                dom: None,
                media_time: None,
                parent_clip_hash: None,
            }),
            content: Some(params.quote),
            text_hash,
            created_by_activity_id: None,
        };

        let transformation_type: TransformationType = serde_json::from_value(
            serde_json::Value::String(params.transformation_type),
        )
        .unwrap_or(TransformationType::Unknown);

        let edges: Vec<DerivationEdge> = parent_hashes
            .iter()
            .map(|ph| DerivationEdge {
                id: CrpId(format!("edge-{}", uuid::Uuid::new_v4())),
                child_clip_hash: clip_hash.clone(),
                parent_clip_hash: ContentHash(ph.clone()),
                transformation_type: transformation_type.clone(),
                agent_id: params.agent.as_deref().map(|a| CrpId(a.to_string())),
                confidence: None,
                created_at: now.clone(),
            })
            .collect();

        let bundle = CrpBundle {
            protocol_version: PROTOCOL_VERSION.to_string(),
            bundle_type: BundleType::Derivation,
            created_at: now,
            document: None,
            agents: Vec::new(),
            sources: vec![derived_source],
            clips: vec![clip.clone()],
            activities: Vec::new(),
            derivation_edges: edges,
            reuse_events: Vec::new(),
            signatures: Vec::new(),
            registry: None,
        };

        self.repo.store_bundle(bundle).await.map_err(store_err)?;
        ok_json(&clip)
    }

    /// Inspect a clip by hash or ID and return its full details including content,
    /// selectors, source references, and hashes.
    #[tool(description = "Inspect a clip by its hash (sha256-...) or ID. Returns full clip details including content, selectors, source refs, and hashes.")]
    async fn cliproot_inspect(
        &self,
        Parameters(params): Parameters<InspectParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.repo.get_clip(params.hash_or_id.clone()).await.map_err(store_err)? {
            Some(clip) => ok_json(&clip),
            None => Err(ErrorData::invalid_params(
                format!("clip not found: {}", params.hash_or_id),
                None,
            )),
        }
    }

    /// Show the full ancestor lineage of a clip through derivation edges.
    /// Returns nodes ordered from direct parents to root ancestors.
    #[tool(description = "Show the full ancestor lineage of a clip through derivation edges. Returns an ordered list from direct parents to root ancestors, showing how content was derived.")]
    async fn cliproot_trace(
        &self,
        Parameters(params): Parameters<TraceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let nodes = self.repo.trace(params.hash_or_id).await.map_err(store_err)?;
        let result: Vec<serde_json::Value> = nodes
            .iter()
            .map(|n| json!({
                "clipHash": n.clip_hash,
                "parentHash": n.parent_hash,
                "transformationType": n.transformation_type,
                "depth": n.depth,
            }))
            .collect();
        ok_json(&result)
    }

    /// Verify the hash integrity of a specific clip or all clips in the store.
    #[tool(description = "Verify hash integrity of a clip (by hash or ID) or all clips if hash_or_id is omitted. Returns verification status and any errors found.")]
    async fn cliproot_verify(
        &self,
        Parameters(params): Parameters<VerifyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match params.hash_or_id {
            Some(id) => {
                self.repo.verify_clip(id.clone()).await.map_err(store_err)?;
                ok_json(&json!({ "status": "ok", "clipHashOrId": id }))
            }
            None => {
                let errors = self.repo.verify_all().await.map_err(store_err)?;
                ok_json(&json!({
                    "status": if errors.is_empty() { "ok" } else { "errors" },
                    "errorCount": errors.len(),
                    "errors": errors,
                }))
            }
        }
    }

    /// List clips in the repository with optional filtering. Returns clip summaries
    /// with a 200-character content preview.
    #[tool(description = "List clips in the repository with optional filtering by document ID or source type. Returns an array of clip summaries with content previews.")]
    async fn cliproot_list(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let clips = self.repo
            .list_clips(params.document_id, params.source_type, Some(params.limit))
            .await
            .map_err(store_err)?;

        let result: Vec<serde_json::Value> = clips
            .iter()
            .map(|c| {
                let preview = c.content.as_deref().map(|s| {
                    if s.len() > 200 { &s[..200] } else { s }
                });
                json!({
                    "clipHash": c.clip_hash,
                    "id": c.id,
                    "documentId": c.document_id,
                    "sourceRefs": c.source_refs,
                    "content": preview,
                })
            })
            .collect();

        ok_json(&json!({ "clips": result, "count": result.len() }))
    }

    /// Search clip content using case-insensitive substring matching.
    #[tool(description = "Search clips by text content using case-insensitive substring matching. Returns matching clips up to the specified limit.")]
    async fn cliproot_search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let fetch_limit = (params.limit * 10).min(2000);
        let clips = self.repo
            .list_clips(None, None, Some(fetch_limit))
            .await
            .map_err(store_err)?;

        let query_lower = params.query.to_lowercase();
        let matched: Vec<serde_json::Value> = clips
            .iter()
            .filter(|c| {
                c.content
                    .as_deref()
                    .map(|t| t.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
            })
            .take(params.limit as usize)
            .map(|c| json!({
                "clipHash": c.clip_hash,
                "id": c.id,
                "content": c.content,
                "sourceRefs": c.source_refs,
            }))
            .collect();

        ok_json(&json!({ "results": matched, "count": matched.len() }))
    }

    /// Export a clip and its full provenance lineage as a CRP bundle JSON object.
    #[tool(description = "Export a clip and its full provenance lineage as a CRP bundle. Useful for sharing or archiving provenance records.")]
    async fn cliproot_export(
        &self,
        Parameters(params): Parameters<ExportParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let bundle = self.repo
            .export_bundle(params.hash_or_id)
            .await
            .map_err(store_err)?;
        ok_json(&bundle)
    }
}

// ── ServerHandler ──────────────────────────────────────────────────────────

#[tool_handler]
impl ServerHandler for ClipRootService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(Default::default()).with_server_info(
            Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
        )
    }
}
