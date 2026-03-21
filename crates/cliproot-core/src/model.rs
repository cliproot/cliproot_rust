use serde::{Deserialize, Serialize};

use crate::error::CoreError;

// ── Newtypes ──

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentHash(pub String);

impl ContentHash {
    pub fn validate(s: &str) -> Result<Self, CoreError> {
        if let Some(rest) = s.strip_prefix("sha256-") {
            if rest.len() >= 43
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Ok(ContentHash(s.to_string()));
            }
        }
        Err(CoreError::InvalidContentHash(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CrpId(pub String);

impl CrpId {
    pub fn validate(s: &str) -> Result<Self, CoreError> {
        if !s.is_empty()
            && s.len() <= 128
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b':' || b == b'-')
        {
            Ok(CrpId(s.to_string()))
        } else {
            Err(CoreError::InvalidCrpId(s.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for CrpId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Enums ──

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BundleType {
    Document,
    Clipboard,
    ReuseEvent,
    Derivation,
    ProvenanceExport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceType {
    HumanAuthored,
    AiGenerated,
    AiAssisted,
    ExternalQuoted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentType {
    Person,
    Organization,
    Model,
    Service,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    Create,
    Paste,
    Import,
    Edit,
    AiGenerate,
    ReuseDetected,
    ReuseNotified,
    Copy,
    Derive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformationType {
    Verbatim,
    Quote,
    Summary,
    Paraphrase,
    Translate,
    Combine,
    Edit,
    AiGenerate,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReuseStatus {
    Detected,
    Notified,
    Acknowledged,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureAlg {
    Ed25519,
    ES256,
    RS256,
}

// ── Structs ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrpBundle {
    pub protocol_version: String,
    pub bundle_type: BundleType,
    pub created_at: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<Document>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<Agent>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceRecord>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<Clip>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activities: Vec<Activity>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derivation_edges: Vec<DerivationEdge>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reuse_events: Vec<ReuseEvent>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signatures: Vec<Signature>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<RegistryRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: CrpId,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_hash: Option<ContentHash>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: CrpId,
    pub agent_type: AgentType,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    pub id: CrpId,
    pub source_type: SourceType,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub digital_source_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_uri: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_agent_id: Option<CrpId>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub clip_hash: ContentHash,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<CrpId>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<CrpId>,

    pub source_refs: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub selectors: Option<Selectors>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    pub text_hash: ContentHash,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_activity_id: Option<CrpId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selectors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_position: Option<TextPositionSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_quote: Option<TextQuoteSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub dom: Option<DomSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_time: Option<MediaTimeSelector>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_clip_hash: Option<ContentHash>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPositionSelector {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextQuoteSelector {
    pub exact: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomSelector {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_attribute: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub css_selector: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTimeSelector {
    pub start_ms: u64,
    pub end_ms: u64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_cue_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivationEdge {
    pub id: CrpId,
    pub child_clip_hash: ContentHash,
    pub parent_clip_hash: ContentHash,
    pub transformation_type: TransformationType,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<CrpId>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,

    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: CrpId,
    pub activity_type: ActivityType,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<CrpId>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used_source_refs: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_clip_refs: Vec<String>,

    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReuseEvent {
    pub id: CrpId,
    pub status: ReuseStatus,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_document_id: Option<CrpId>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_document_id: Option<CrpId>,

    pub source_ref: CrpId,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_by_activity_id: Option<CrpId>,

    pub created_at: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signature {
    pub id: CrpId,
    pub alg: SignatureAlg,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,

    pub jws: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryRef {
    pub uri: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipRef {
    pub clip_hash: ContentHash,
}
