//! Parameter structs for each MCP tool. All derive JsonSchema for automatic
//! schema generation in the tool definitions.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
