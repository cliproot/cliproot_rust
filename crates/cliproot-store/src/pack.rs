use base64::Engine;
use cliproot_core::{
    model::{Artifact, ClipArtifactRef, Project},
    ContentHash,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PACK_FORMAT: &str = "cliproot-pack-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackRootMode {
    Project,
    Roots,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRoots {
    pub mode: PackRootMode,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clip_hashes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackCounts {
    pub bundles: usize,
    pub clips: usize,
    pub edges: usize,
    pub artifacts: usize,
    pub links: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackObjectEntry {
    pub bundle_hash: String,
    pub archive_path: String,
    pub byte_size: u64,
    pub sha256_digest: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clip_hashes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackArtifactEntry {
    pub artifact_hash: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,

    pub artifact_type: String,
    pub file_name: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub sha256_digest: String,
    pub archive_path: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

impl PackArtifactEntry {
    pub fn from_artifact(artifact: &Artifact) -> Self {
        Self {
            artifact_hash: artifact.artifact_hash.0.clone(),
            id: artifact.id.as_ref().map(|id| id.0.clone()),
            project_id: artifact.project_id.as_ref().map(|id| id.0.clone()),
            artifact_type: serde_json::to_value(&artifact.artifact_type)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_else(|| "unknown".to_string()),
            file_name: artifact.file_name.clone(),
            mime_type: artifact.mime_type.clone(),
            byte_size: artifact.byte_size,
            sha256_digest: artifact.artifact_hash.0.clone(),
            archive_path: format!("artifacts/{}", artifact.artifact_hash.0),
            metadata: artifact.metadata.clone(),
            created_at: artifact.created_at.clone(),
        }
    }

    pub fn into_artifact(self) -> Artifact {
        Artifact {
            artifact_hash: ContentHash(self.artifact_hash),
            id: self.id.map(cliproot_core::CrpId),
            project_id: self.project_id.map(cliproot_core::CrpId),
            artifact_type: serde_json::from_value(serde_json::Value::String(self.artifact_type))
                .unwrap_or(cliproot_core::ArtifactType::Unknown),
            file_name: self.file_name,
            mime_type: self.mime_type,
            byte_size: self.byte_size,
            content_base64: None,
            metadata: self.metadata,
            created_at: self.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackManifest {
    pub format: String,
    pub created_at: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<Project>,

    pub roots: PackRoots,
    pub counts: PackCounts,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<PackObjectEntry>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<PackArtifactEntry>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clip_artifact_refs: Vec<ClipArtifactRef>,
}

pub fn sha256_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    format!("sha256-{encoded}")
}

pub fn safe_restore_name(artifact_hash: &str, file_name: &str) -> String {
    let sanitized = file_name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' => '_',
            _ => ch,
        })
        .collect::<String>();
    format!("{artifact_hash}--{sanitized}")
}
