//! ClipRootService — MCP server implementation exposing cliproot operations as tools.

use std::{path::PathBuf, sync::Arc};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cliproot_core::{
    create_clip_hash, create_text_hash, hash::ClipHashInput, matching::parse_annotation_style,
    model::*,
};
use cliproot_store::StoreError;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        Annotated, CallToolResult, Content, ErrorData, Implementation, ListResourceTemplatesResult,
        ListResourcesResult, PaginatedRequestParams, RawResource, RawResourceTemplate,
        ReadResourceRequestParams, ReadResourceResult, ResourceContents, ServerCapabilities,
        ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router, RoleServer, ServerHandler,
};
use serde_json::json;
use tokio::sync::Mutex;

use crate::params::*;
use crate::repo_handle::RepoHandle;

const PROTOCOL_VERSION: &str = "0.0.3";

// ── Helpers ────────────────────────────────────────────────────────────────

pub(crate) fn ok_json(v: impl serde::Serialize) -> Result<CallToolResult, ErrorData> {
    let json = serde_json::to_string_pretty(&v)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(crate) fn store_err(e: StoreError) -> ErrorData {
    match e {
        StoreError::NotFound => ErrorData::invalid_params("clip not found", None),
        StoreError::AlreadyExists(s) => {
            ErrorData::invalid_params(format!("already exists: {s}"), None)
        }
        other => ErrorData::internal_error(other.to_string(), None),
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[derive(Debug, Default)]
struct ActiveContext {
    session_id: Option<String>,
    activity_id: Option<String>,
}

// ── Service ────────────────────────────────────────────────────────────────

/// MCP server that exposes Cliproot provenance operations as typed tools.
#[derive(Clone)]
pub struct ClipRootService {
    repo: Arc<RepoHandle>,
    active: Arc<Mutex<ActiveContext>>,
    tool_router: ToolRouter<Self>,
}

impl ClipRootService {
    pub fn new(repo: RepoHandle) -> Self {
        Self {
            repo: Arc::new(repo),
            active: Arc::new(Mutex::new(ActiveContext::default())),
            tool_router: Self::tool_router(),
        }
    }

    async fn current_activity_id(&self, explicit: Option<String>) -> Option<String> {
        if explicit.is_some() {
            return explicit;
        }
        self.active.lock().await.activity_id.clone()
    }

    async fn current_session_id(&self, explicit: Option<String>) -> Option<String> {
        if explicit.is_some() {
            return explicit;
        }
        self.active.lock().await.session_id.clone()
    }

    /// Resolve a `cliproot://` URI and return serialised JSON.
    async fn read_resource_uri(&self, uri: &str) -> Result<String, String> {
        let path = uri
            .strip_prefix("cliproot://")
            .ok_or("invalid URI scheme")?;

        if path == "clips" {
            let clips = self
                .repo
                .list_clips(None, None, None, Some(200))
                .await
                .map_err(|e| e.to_string())?;
            let summaries: Vec<serde_json::Value> = clips
                .iter()
                .map(|c| {
                    let preview =
                        c.content
                            .as_deref()
                            .map(|s| if s.len() > 200 { &s[..200] } else { s });
                    json!({
                        "clipHash": c.clip_hash,
                        "id": c.id,
                        "documentId": c.document_id,
                        "sourceRefs": c.source_refs,
                        "content": preview,
                    })
                })
                .collect();
            serde_json::to_string_pretty(&json!({
                "clips": summaries, "count": summaries.len()
            }))
            .map_err(|e| e.to_string())
        } else if let Some(hash_or_id) = path.strip_prefix("clips/") {
            let clip = self
                .repo
                .get_clip(hash_or_id.to_string())
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("clip not found: {hash_or_id}"))?;
            serde_json::to_string_pretty(&clip).map_err(|e| e.to_string())
        } else if let Some(hash_or_id) = path.strip_prefix("lineage/") {
            let nodes = self
                .repo
                .trace(hash_or_id.to_string())
                .await
                .map_err(|e| e.to_string())?;
            let result: Vec<serde_json::Value> = nodes
                .iter()
                .map(|n| {
                    json!({
                        "clipHash": n.clip_hash,
                        "parentHash": n.parent_hash,
                        "transformationType": n.transformation_type,
                        "depth": n.depth,
                    })
                })
                .collect();
            serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
        } else if let Some(hash_or_id) = path.strip_prefix("bundles/") {
            let bundle = self
                .repo
                .export_bundle(hash_or_id.to_string())
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())
        } else {
            Err(format!("unknown resource path: {path}"))
        }
    }
}

// ── Tool Implementations ───────────────────────────────────────────────────

#[tool_router]
impl ClipRootService {
    /// Capture a source clip: record a URL, exact quoted text, and source type into the
    /// Cliproot repository. Returns the content-addressed clip with its hash.
    #[tool(
        description = "Capture a source clip from a URL with exact quoted text. Returns the stored clip including its content-addressed hash for future reference."
    )]
    async fn cliproot_clip(
        &self,
        Parameters(params): Parameters<ClipParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let activity_id = self.current_activity_id(params.activity_id.clone()).await;
        let session_id = self.current_session_id(params.session_id.clone()).await;
        let source_type: SourceType =
            serde_json::from_value(serde_json::Value::String(params.source_type)).map_err(|e| {
                ErrorData::invalid_params(format!("invalid source_type: {e}"), None)
            })?;

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
            project_id: params.project.clone().map(CrpId),
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
            created_by_activity_id: activity_id.clone().map(CrpId),
        };

        let bundle = CrpBundle {
            protocol_version: PROTOCOL_VERSION.to_string(),
            bundle_type: BundleType::Document,
            created_at: now,
            project: None,
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

        self.repo.store_bundle(bundle).await.map_err(store_err)?;
        self.repo
            .record_clip_tracking(
                clip.clip_hash.0.clone(),
                activity_id,
                session_id,
                clip.source_refs.clone(),
            )
            .await
            .map_err(store_err)?;
        ok_json(&clip)
    }

    /// Derive a new clip from one or more parent clips, recording the transformation type
    /// and derivation edges. Returns the derived child clip with its hash.
    #[tool(
        description = "Derive a new clip from one or more parent clips. Creates derivation edges recording how the content was transformed. Returns the child clip with its hash."
    )]
    async fn cliproot_derive(
        &self,
        Parameters(params): Parameters<DeriveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let activity_id = self.current_activity_id(params.activity_id.clone()).await;
        let session_id = self.current_session_id(params.session_id.clone()).await;
        // Resolve parent hashes
        let mut parent_hashes = Vec::new();
        for ref_str in &params.from {
            let h = self
                .repo
                .resolve_hash(ref_str.clone())
                .await
                .map_err(store_err)?
                .ok_or_else(|| {
                    ErrorData::invalid_params(format!("parent clip not found: {ref_str}"), None)
                })?;
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
            project_id: params.project.clone().map(CrpId),
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
            created_by_activity_id: activity_id.clone().map(CrpId),
        };

        let transformation_type: TransformationType =
            serde_json::from_value(serde_json::Value::String(params.transformation_type))
                .unwrap_or(TransformationType::Unknown);

        let edges: Vec<Edge> = parent_hashes
            .iter()
            .map(|ph| Edge {
                id: CrpId(format!("edge-{}", uuid::Uuid::new_v4())),
                edge_type: EdgeType::WasDerivedFrom,
                subject_ref: CrpId(clip_hash.0.clone()),
                object_ref: CrpId(ph.clone()),
                transformation_type: Some(transformation_type.clone()),
                agent_id: params.agent.as_deref().map(|a| CrpId(a.to_string())),
                confidence: None,
                created_at: now.clone(),
            })
            .collect();

        let bundle = CrpBundle {
            protocol_version: PROTOCOL_VERSION.to_string(),
            bundle_type: BundleType::Derivation,
            created_at: now,
            project: None,
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

        self.repo.store_bundle(bundle).await.map_err(store_err)?;
        self.repo
            .record_clip_tracking(
                clip.clip_hash.0.clone(),
                activity_id,
                session_id,
                parent_hashes,
            )
            .await
            .map_err(store_err)?;
        ok_json(&clip)
    }

    #[tool(description = "Create a project in the local Cliproot repository.")]
    async fn cliproot_project_create(
        &self,
        Parameters(params): Parameters<ProjectCreateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = self
            .repo
            .create_project(params.id, params.name, params.description)
            .await
            .map_err(store_err)?;
        ok_json(&project)
    }

    #[tool(description = "List projects in the local Cliproot repository.")]
    async fn cliproot_project_list(
        &self,
        Parameters(_params): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let projects = self.repo.list_projects().await.map_err(store_err)?;
        ok_json(&projects)
    }

    #[tool(description = "Set the current default project for this repository.")]
    async fn cliproot_project_use(
        &self,
        Parameters(params): Parameters<ProjectUseParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.repo
            .use_project(params.project_id.clone())
            .await
            .map_err(store_err)?;
        ok_json(json!({ "projectId": params.project_id, "status": "ok" }))
    }

    #[tool(description = "Delete a project from the local repository.")]
    async fn cliproot_project_delete(
        &self,
        Parameters(params): Parameters<ProjectDeleteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.repo
            .delete_project(params.project_id.clone())
            .await
            .map_err(store_err)?;
        ok_json(json!({ "projectId": params.project_id, "status": "ok" }))
    }

    #[tool(description = "Store a file or inline content as an artifact.")]
    async fn cliproot_artifact_add(
        &self,
        Parameters(params): Parameters<ArtifactAddParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let artifact_type: ArtifactType = serde_json::from_value(serde_json::Value::String(
            params.artifact_type,
        ))
        .map_err(|e| ErrorData::invalid_params(format!("invalid artifact_type: {e}"), None))?;
        let artifact = self
            .repo
            .add_artifact(
                params.path.map(PathBuf::from),
                params.content.map(|content| content.into_bytes()),
                params.file_name,
                artifact_type,
                params.mime_type,
                params.id,
                params.project_id,
                params.metadata,
            )
            .await
            .map_err(store_err)?;
        ok_json(&artifact)
    }

    #[tool(description = "List artifacts with optional project filtering.")]
    async fn cliproot_artifact_list(
        &self,
        Parameters(params): Parameters<ArtifactListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let artifacts = self
            .repo
            .list_artifacts(params.project_id)
            .await
            .map_err(store_err)?;
        ok_json(&artifacts)
    }

    #[tool(description = "Get artifact metadata and inline content.")]
    async fn cliproot_artifact_get(
        &self,
        Parameters(params): Parameters<ArtifactGetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let artifact = self
            .repo
            .get_artifact(params.artifact_hash.clone())
            .await
            .map_err(store_err)?
            .ok_or_else(|| {
                ErrorData::invalid_params(
                    format!("artifact not found: {}", params.artifact_hash),
                    None,
                )
            })?;
        let bytes = self
            .repo
            .get_artifact_bytes(params.artifact_hash)
            .await
            .map_err(store_err)?;
        let content = if artifact.mime_type.starts_with("text/")
            || artifact.mime_type == "application/json"
        {
            json!({ "text": String::from_utf8_lossy(&bytes) })
        } else {
            json!({ "contentBase64": STANDARD.encode(bytes) })
        };
        ok_json(json!({ "artifact": artifact, "content": content }))
    }

    #[tool(description = "Link a clip to an artifact with a typed relationship.")]
    async fn cliproot_artifact_link(
        &self,
        Parameters(params): Parameters<ArtifactLinkParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let relationship: ClipArtifactRelationship = serde_json::from_value(
            serde_json::Value::String(params.relationship),
        )
        .map_err(|e| ErrorData::invalid_params(format!("invalid relationship: {e}"), None))?;
        let link = self
            .repo
            .link_clip_artifact(params.clip_hash_or_id, params.artifact_hash, relationship)
            .await
            .map_err(store_err)?;
        ok_json(&link)
    }

    #[tool(description = "Create a .cliprootpack archive from a project or a set of root clips.")]
    async fn cliproot_pack_create(
        &self,
        Parameters(params): Parameters<PackCreateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let manifest = self
            .repo
            .create_pack(
                params.project_id,
                params.roots,
                params.depth,
                PathBuf::from(&params.output_path),
            )
            .await
            .map_err(store_err)?;
        ok_json(json!({ "path": params.output_path, "manifest": manifest }))
    }

    #[tool(description = "Import a .cliprootpack archive into the local repository.")]
    async fn cliproot_pack_import(
        &self,
        Parameters(params): Parameters<PackImportParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let manifest = self
            .repo
            .import_pack(
                PathBuf::from(&params.path),
                params.restore_artifacts_to.map(PathBuf::from),
            )
            .await
            .map_err(store_err)?;
        ok_json(&manifest)
    }

    #[tool(description = "Inspect a .cliprootpack archive without importing it.")]
    async fn cliproot_pack_inspect(
        &self,
        Parameters(params): Parameters<PackPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let manifest = self
            .repo
            .inspect_pack(PathBuf::from(params.path))
            .await
            .map_err(store_err)?;
        ok_json(&manifest)
    }

    #[tool(description = "Verify the integrity of a .cliprootpack archive.")]
    async fn cliproot_pack_verify(
        &self,
        Parameters(params): Parameters<PackPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let manifest = self
            .repo
            .verify_pack(PathBuf::from(params.path))
            .await
            .map_err(store_err)?;
        ok_json(&manifest)
    }

    #[tool(description = "Start a prompt-scoped activity for tracked agent work.")]
    async fn cliproot_activity_start(
        &self,
        Parameters(params): Parameters<ActivityStartParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let activity_type: ActivityType = serde_json::from_value(serde_json::Value::String(
            params.activity_type,
        ))
        .map_err(|e| ErrorData::invalid_params(format!("invalid activity_type: {e}"), None))?;
        let session_id = self.current_session_id(params.session_id.clone()).await;
        let activity = self
            .repo
            .start_activity(
                activity_type,
                params.project_id,
                params.agent_id,
                params.prompt,
                params.parameters,
                session_id.clone(),
            )
            .await
            .map_err(store_err)?;
        let mut active = self.active.lock().await;
        active.activity_id = Some(activity.id.0.clone());
        if session_id.is_some() {
            active.session_id = session_id;
        }
        ok_json(&activity)
    }

    #[tool(description = "End a tracked activity and finalize its generated/used refs.")]
    async fn cliproot_activity_end(
        &self,
        Parameters(params): Parameters<ActivityEndParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let activity = self
            .repo
            .end_activity(params.activity_id.clone())
            .await
            .map_err(store_err)?;
        let mut active = self.active.lock().await;
        if active.activity_id.as_deref() == Some(params.activity_id.as_str()) {
            active.activity_id = None;
        }
        ok_json(&activity)
    }

    #[tool(description = "Start an agent-agnostic session that can be restored as an artifact.")]
    async fn cliproot_session_start(
        &self,
        Parameters(params): Parameters<SessionStartParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let session = self
            .repo
            .start_session(params.project_id, params.agent_id, params.metadata)
            .await
            .map_err(store_err)?;
        let mut active = self.active.lock().await;
        active.session_id = Some(session.session_id.clone());
        ok_json(&session)
    }

    #[tool(description = "End a session and materialize its final session artifact.")]
    async fn cliproot_session_end(
        &self,
        Parameters(params): Parameters<SessionEndParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let session = self
            .repo
            .end_session(params.session_id.clone())
            .await
            .map_err(store_err)?;
        let mut active = self.active.lock().await;
        if active.session_id.as_deref() == Some(params.session_id.as_str()) {
            active.session_id = None;
        }
        ok_json(&session)
    }

    /// Inspect a clip by hash or ID and return its full details including content,
    /// selectors, source references, and hashes.
    #[tool(
        description = "Inspect a clip by its hash (sha256-...) or ID. Returns full clip details including content, selectors, source refs, and hashes."
    )]
    async fn cliproot_inspect(
        &self,
        Parameters(params): Parameters<InspectParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self
            .repo
            .get_clip(params.hash_or_id.clone())
            .await
            .map_err(store_err)?
        {
            Some(clip) => ok_json(&clip),
            None => Err(ErrorData::invalid_params(
                format!("clip not found: {}", params.hash_or_id),
                None,
            )),
        }
    }

    /// Show the full ancestor lineage of a clip through derivation edges.
    /// Returns nodes ordered from direct parents to root ancestors.
    #[tool(
        description = "Show the full ancestor lineage of a clip through derivation edges. Returns an ordered list from direct parents to root ancestors, showing how content was derived."
    )]
    async fn cliproot_trace(
        &self,
        Parameters(params): Parameters<TraceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let nodes = self
            .repo
            .trace(params.hash_or_id)
            .await
            .map_err(store_err)?;
        let result: Vec<serde_json::Value> = nodes
            .iter()
            .map(|n| {
                json!({
                    "clipHash": n.clip_hash,
                    "parentHash": n.parent_hash,
                    "transformationType": n.transformation_type,
                    "depth": n.depth,
                })
            })
            .collect();
        ok_json(&result)
    }

    /// Verify the hash integrity of a specific clip or all clips in the store.
    #[tool(
        description = "Verify hash integrity of a clip (by hash or ID) or all clips if hash_or_id is omitted. Returns verification status and any errors found."
    )]
    async fn cliproot_verify(
        &self,
        Parameters(params): Parameters<VerifyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match params.hash_or_id {
            Some(id) => {
                self.repo.verify_clip(id.clone()).await.map_err(store_err)?;
                ok_json(json!({ "status": "ok", "clipHashOrId": id }))
            }
            None => {
                let errors = self.repo.verify_all().await.map_err(store_err)?;
                ok_json(json!({
                    "status": if errors.is_empty() { "ok" } else { "errors" },
                    "errorCount": errors.len(),
                    "errors": errors,
                }))
            }
        }
    }

    /// List clips in the repository with optional filtering. Returns clip summaries
    /// with a 200-character content preview.
    #[tool(
        description = "List clips in the repository with optional filtering by document ID or source type. Returns an array of clip summaries with content previews."
    )]
    async fn cliproot_list(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let clips = self
            .repo
            .list_clips(
                params.document_id,
                params.source_type,
                params.project_id,
                Some(params.limit),
            )
            .await
            .map_err(store_err)?;

        let result: Vec<serde_json::Value> = clips
            .iter()
            .map(|c| {
                let preview = c
                    .content
                    .as_deref()
                    .map(|s| if s.len() > 200 { &s[..200] } else { s });
                json!({
                    "clipHash": c.clip_hash,
                    "id": c.id,
                    "documentId": c.document_id,
                    "sourceRefs": c.source_refs,
                    "content": preview,
                })
            })
            .collect();

        ok_json(json!({ "clips": result, "count": result.len() }))
    }

    /// Search clip content using case-insensitive substring matching.
    #[tool(
        description = "Search clips by text content using case-insensitive substring matching. Returns matching clips up to the specified limit."
    )]
    async fn cliproot_search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let fetch_limit = (params.limit * 10).min(2000);
        let clips = self
            .repo
            .list_clips(None, None, None, Some(fetch_limit))
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
            .map(|c| {
                json!({
                    "clipHash": c.clip_hash,
                    "id": c.id,
                    "content": c.content,
                    "sourceRefs": c.source_refs,
                })
            })
            .collect();

        ok_json(json!({ "results": matched, "count": matched.len() }))
    }

    /// Export a clip and its full provenance lineage as a CRP bundle JSON object.
    #[tool(
        description = "Export a clip and its full provenance lineage as a CRP bundle. Useful for sharing or archiving provenance records."
    )]
    async fn cliproot_export(
        &self,
        Parameters(params): Parameters<ExportParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let bundle = self
            .repo
            .export_bundle(params.hash_or_id)
            .await
            .map_err(store_err)?;
        ok_json(&bundle)
    }

    /// Annotate a document with inline citations by matching text against stored clips.
    #[tool(
        description = "Annotate a document with inline citations by matching text against stored clips. Returns the annotated text with citation markers and a list of citations with source URLs."
    )]
    async fn cliproot_annotate(
        &self,
        Parameters(params): Parameters<AnnotateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let style = parse_annotation_style(&params.style)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let result = self
            .repo
            .annotate(params.document_text, style, params.threshold)
            .await
            .map_err(store_err)?;
        ok_json(&result)
    }

    /// Generate a bibliography/citation list for a document from clip provenance.
    #[tool(
        description = "Generate a bibliography/citation list for a document by matching text against stored clips. Returns numbered sources with URLs and titles."
    )]
    async fn cliproot_cite(
        &self,
        Parameters(params): Parameters<CiteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let citations = self
            .repo
            .cite(params.document_text, params.threshold)
            .await
            .map_err(store_err)?;
        ok_json(json!({ "citations": citations, "count": citations.len() }))
    }

    /// Generate a provenance coverage report for a document.
    #[tool(
        description = "Generate a provenance coverage report showing which paragraphs in a document have source provenance and which are missing it. Useful for auditing AI-generated content."
    )]
    async fn cliproot_doctor(
        &self,
        Parameters(params): Parameters<DoctorParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let report = self
            .repo
            .doctor(params.document_text, params.threshold)
            .await
            .map_err(store_err)?;
        ok_json(&report)
    }

    /// Surface unclipped sources from the agent-log for review.
    #[tool(
        description = "Surface sources consulted during the session but not yet highlighted as clips. Returns candidate sources, files, and synthesis candidates for review. For agents without hook support (Cursor, Windsurf), call this periodically to check for unhighlighted sources."
    )]
    async fn cliproot_consolidate(
        &self,
        Parameters(params): Parameters<ConsolidateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = params.session_id.unwrap_or_default();
        if session_id.is_empty() {
            return Err(ErrorData::invalid_params("session_id is required", None));
        }
        // Shell out to `cliproot session consolidate` to avoid circular crate dependency
        let mut cmd = std::process::Command::new("cliproot");
        cmd.arg("session").arg("consolidate")
            .arg("--session")
            .arg(&session_id)
            .arg("--format")
            .arg("json");
        if params.commit {
            cmd.arg("--commit");
        }
        let output = tokio::task::spawn_blocking(move || cmd.output())
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
            .map_err(|e| ErrorData::internal_error(format!("failed to run cliproot: {e}"), None))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ErrorData::internal_error(
                format!("consolidation failed: {stderr}"),
                None,
            ));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| ErrorData::internal_error(format!("invalid JSON output: {e}"), None))?;
        ok_json(&parsed)
    }

    /// Run the wiki-lint checks.
    #[tool(
        description = "Run structural and provenance lint checks over the compiled wiki at .cliproot/knowledge/. Returns the per-check finding list. Check #2 (broken [cliproot:sha256-…] citations) is the load-bearing invariant."
    )]
    async fn cliproot_wiki_lint(
        &self,
        Parameters(params): Parameters<WikiLintParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut cmd = std::process::Command::new("cliproot");
        cmd.arg("wiki").arg("lint").arg("--format").arg("json");
        if params.structural_only {
            cmd.arg("--structural-only");
        }
        if params.contradictions {
            cmd.arg("--contradictions");
        }
        if params.write_report {
            cmd.arg("--report");
        }
        let output = tokio::task::spawn_blocking(move || cmd.output())
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
            .map_err(|e| ErrorData::internal_error(format!("failed to run cliproot: {e}"), None))?;
        // wiki-lint exits 1 on broken citations; we still surface the JSON body.
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = serde_json::from_str(&stdout).map_err(|e| {
            ErrorData::internal_error(
                format!(
                    "invalid JSON output from wiki-lint: {e}\nstderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
                None,
            )
        })?;
        ok_json(&parsed)
    }

    /// Answer a question from the compiled wiki.
    #[tool(
        description = "Answer a natural-language question using the compiled wiki. Two-phase retrieval: keyword extraction + answer synthesis with [cliproot:sha256-…] citations. Records a Research Activity."
    )]
    async fn cliproot_query(
        &self,
        Parameters(params): Parameters<QueryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if params.prompt.trim().is_empty() {
            return Err(ErrorData::invalid_params("prompt is required", None));
        }
        let mut cmd = std::process::Command::new("cliproot");
        cmd.arg("wiki").arg("query")
            .arg(&params.prompt)
            .arg("--format")
            .arg("json")
            .arg("--top-k")
            .arg(params.top_k.to_string());
        if params.file_back {
            cmd.arg("--file-back");
        }
        let output = tokio::task::spawn_blocking(move || cmd.output())
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
            .map_err(|e| ErrorData::internal_error(format!("failed to run cliproot: {e}"), None))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ErrorData::internal_error(
                format!("query failed: {stderr}"),
                None,
            ));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| ErrorData::internal_error(format!("invalid JSON output: {e}"), None))?;
        ok_json(&parsed)
    }
}

// ── ServerHandler ──────────────────────────────────────────────────────────

#[tool_handler]
impl ServerHandler for ClipRootService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        ))
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourcesResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListResourcesResult {
            resources: vec![Annotated::new(
                RawResource {
                    uri: "cliproot://clips".into(),
                    name: "cliproot-clip-list".into(),
                    title: Some("All Clips".into()),
                    description: Some("Summary list of all clips in the repository".into()),
                    mime_type: Some("application/json".into()),
                    size: None,
                    icons: None,
                    meta: None,
                },
                None,
            )],
            meta: None,
            next_cursor: None,
        }))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourceTemplatesResult, ErrorData>> + Send + '_
    {
        std::future::ready(Ok(ListResourceTemplatesResult {
            resource_templates: vec![
                Annotated::new(
                    RawResourceTemplate {
                        uri_template: "cliproot://clips/{hash_or_id}".into(),
                        name: "cliproot-clip".into(),
                        title: Some("Clip Details".into()),
                        description: Some("Full details of a clip by hash or ID".into()),
                        mime_type: Some("application/json".into()),
                        icons: None,
                    },
                    None,
                ),
                Annotated::new(
                    RawResourceTemplate {
                        uri_template: "cliproot://lineage/{hash_or_id}".into(),
                        name: "cliproot-lineage".into(),
                        title: Some("Clip Lineage".into()),
                        description: Some("Derivation lineage trace for a clip".into()),
                        mime_type: Some("application/json".into()),
                        icons: None,
                    },
                    None,
                ),
                Annotated::new(
                    RawResourceTemplate {
                        uri_template: "cliproot://bundles/{hash_or_id}".into(),
                        name: "cliproot-bundle".into(),
                        title: Some("CRP Bundle Export".into()),
                        description: Some(
                            "Full provenance bundle for a clip and its lineage".into(),
                        ),
                        mime_type: Some("application/json".into()),
                        icons: None,
                    },
                    None,
                ),
            ],
            meta: None,
            next_cursor: None,
        }))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let uri = &request.uri;
        let json_str = self
            .read_resource_uri(uri)
            .await
            .map_err(|e| ErrorData::resource_not_found(e, None))?;
        Ok(ReadResourceResult::new(vec![ResourceContents::text(
            json_str,
            uri.clone(),
        )
        .with_mime_type("application/json")]))
    }
}
