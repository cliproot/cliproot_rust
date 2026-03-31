//! Parameter structs for each MCP tool. All derive JsonSchema for automatic
//! schema generation in the tool definitions.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct EmptyParams {}

// ── cliproot_clip ──────────────────────────────────────────────────────────

/// Parameters for the cliproot_clip tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ClipParams {
    /// The source URL where the quoted text was found
    pub url: String,
    /// The exact quoted text to capture
    pub quote: String,
    /// Source type: "external-quoted" (default), "human-authored", "ai-generated", "ai-assisted", "unknown"
    #[serde(default = "default_source_type")]
    pub source_type: String,
    /// Optional stable human-readable clip ID (e.g. "clip-redis-001")
    pub id: Option<String>,
    /// Optional document ID to group this clip with others in a document
    pub document_id: Option<String>,
    /// Optional human-readable title for the source
    pub title: Option<String>,
    /// Optional project id (falls back to current project)
    pub project: Option<String>,
    /// Optional activity id for prompt-scoped provenance
    pub activity_id: Option<String>,
    /// Optional session id for session-scoped provenance
    pub session_id: Option<String>,
}

fn default_source_type() -> String {
    "external-quoted".to_string()
}

// ── cliproot_derive ────────────────────────────────────────────────────────

/// Parameters for the cliproot_derive tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeriveParams {
    /// One or more parent clip hashes (sha256-...) or clip IDs to derive from
    pub from: Vec<String>,
    /// The derived text content
    pub quote: String,
    /// Transformation type: "verbatim", "quote", "summary", "paraphrase", "translate", "combine", "edit", "ai_generate", "unknown"
    pub transformation_type: String,
    /// Optional agent ID (e.g. model identifier like "claude-opus-4")
    pub agent: Option<String>,
    /// Optional project id (falls back to current project)
    pub project: Option<String>,
    /// Optional activity id for prompt-scoped provenance
    pub activity_id: Option<String>,
    /// Optional session id for session-scoped provenance
    pub session_id: Option<String>,
}

// ── cliproot_inspect ───────────────────────────────────────────────────────

/// Parameters for the cliproot_inspect tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct InspectParams {
    /// Clip hash (sha256-...) or clip ID
    pub hash_or_id: String,
}

// ── cliproot_trace ─────────────────────────────────────────────────────────

/// Parameters for the cliproot_trace tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct TraceParams {
    /// Clip hash (sha256-...) or clip ID to trace lineage for
    pub hash_or_id: String,
}

// ── cliproot_verify ────────────────────────────────────────────────────────

/// Parameters for the cliproot_verify tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct VerifyParams {
    /// Clip hash or ID to verify. If null/omitted, verifies all clips in the store.
    pub hash_or_id: Option<String>,
}

// ── cliproot_list ──────────────────────────────────────────────────────────

/// Parameters for the cliproot_list tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ListParams {
    /// Filter clips by document ID
    pub document_id: Option<String>,
    /// Filter clips by source type string
    pub source_type: Option<String>,
    /// Filter clips by project id
    pub project_id: Option<String>,
    /// Maximum number of clips to return (default: 50)
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    50
}

// ── cliproot_search ────────────────────────────────────────────────────────

/// Parameters for the cliproot_search tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchParams {
    /// Text to search for in clip content (case-insensitive substring match)
    pub query: String,
    /// Maximum number of results to return (default: 20)
    #[serde(default = "default_search_limit")]
    pub limit: u32,
}

fn default_search_limit() -> u32 {
    20
}

// ── cliproot_export ────────────────────────────────────────────────────────

/// Parameters for the cliproot_export tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ExportParams {
    /// Clip hash (sha256-...) or clip ID to export with its full provenance lineage
    pub hash_or_id: String,
}

// ── cliproot_annotate ─────────────────────────────────────────────────────

/// Parameters for the cliproot_annotate tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AnnotateParams {
    /// The document text to annotate with citations
    pub document_text: String,
    /// Annotation style: "footnote" (default), "inline-comment", "bracket"
    #[serde(default = "default_annotation_style")]
    pub style: String,
    /// Minimum match confidence threshold (0.0-1.0, default: 0.4)
    #[serde(default = "default_threshold")]
    pub threshold: f64,
}

fn default_annotation_style() -> String {
    "footnote".to_string()
}

fn default_threshold() -> f64 {
    0.4
}

// ── cliproot_cite ─────────────────────────────────────────────────────────

/// Parameters for the cliproot_cite tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CiteParams {
    /// The document text to generate citations for
    pub document_text: String,
    /// Minimum match confidence threshold (0.0-1.0, default: 0.4)
    #[serde(default = "default_threshold")]
    pub threshold: f64,
}

// ── cliproot_doctor ───────────────────────────────────────────────────────

/// Parameters for the cliproot_doctor tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DoctorParams {
    /// The document text to analyze for provenance coverage
    pub document_text: String,
    /// Minimum match confidence threshold (0.0-1.0, default: 0.4)
    #[serde(default = "default_threshold")]
    pub threshold: f64,
}

// ── cliproot_project_* ────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ProjectCreateParams {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ProjectUseParams {
    pub project_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ProjectDeleteParams {
    pub project_id: String,
}

// ── cliproot_artifact_* ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ArtifactAddParams {
    pub path: Option<String>,
    pub content: Option<String>,
    pub file_name: Option<String>,
    pub artifact_type: String,
    pub mime_type: Option<String>,
    pub id: Option<String>,
    pub project_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ArtifactListParams {
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ArtifactGetParams {
    pub artifact_hash: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ArtifactLinkParams {
    pub clip_hash_or_id: String,
    pub artifact_hash: String,
    pub relationship: String,
}

// ── cliproot_pack_* ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PackCreateParams {
    pub project_id: Option<String>,
    #[serde(default)]
    pub roots: Vec<String>,
    pub depth: Option<u32>,
    pub output_path: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PackImportParams {
    pub path: String,
    pub restore_artifacts_to: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PackPathParams {
    pub path: String,
}

// ── cliproot_activity_* ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ActivityStartParams {
    pub activity_type: String,
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub prompt: Option<String>,
    pub parameters: Option<serde_json::Value>,
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ActivityEndParams {
    pub activity_id: String,
}

// ── cliproot_session_* ────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionStartParams {
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionEndParams {
    pub session_id: String,
}
