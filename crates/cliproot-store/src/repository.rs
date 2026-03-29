use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cliproot_core::{
    matching::{self, AnnotateResult, AnnotationStyle, Citation, DoctorResult, MatchCandidate},
    model::*,
    verify::{verify_clip_hash, verify_text_hash},
};
use sha2::{Digest, Sha256};

use crate::error::StoreError;
use crate::index_db::{IndexDb, LineageNode};
use crate::object_store::ObjectStore;

const PROTOCOL_VERSION: &str = "0.0.3";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoConfig {
    protocol_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_project_id: Option<String>,
}

pub struct Repository {
    root: PathBuf,
    cliproot_dir: PathBuf,
    objects: ObjectStore,
    index: IndexDb,
}

impl Repository {
    pub fn init(path: &Path) -> Result<Self, StoreError> {
        let cliproot_dir = path.join(".cliproot");
        if cliproot_dir.exists() {
            return Err(StoreError::AlreadyExists(
                cliproot_dir.display().to_string(),
            ));
        }

        fs::create_dir_all(&cliproot_dir)?;

        let config = RepoConfig {
            protocol_version: PROTOCOL_VERSION.to_string(),
            current_project_id: None,
        };
        fs::write(
            cliproot_dir.join("config.json"),
            serde_json::to_string_pretty(&config)?,
        )?;

        let objects = ObjectStore::new(&cliproot_dir);
        objects.init()?;

        let index = IndexDb::open(&cliproot_dir.join("index.db"))?;
        index.init()?;

        Ok(Self {
            root: path.to_path_buf(),
            cliproot_dir,
            objects,
            index,
        })
    }

    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let cliproot_dir = path.join(".cliproot");
        if !cliproot_dir.exists() {
            return Err(StoreError::NotFound);
        }

        let objects = ObjectStore::new(&cliproot_dir);
        objects.init()?;
        let index = IndexDb::open(&cliproot_dir.join("index.db"))?;
        index.init()?;

        let repo = Self {
            root: path.to_path_buf(),
            cliproot_dir,
            objects,
            index,
        };
        repo.ensure_config()?;
        Ok(repo)
    }

    pub fn discover() -> Result<Self, StoreError> {
        let mut dir = std::env::current_dir()?;
        loop {
            if dir.join(".cliproot").exists() {
                return Self::open(&dir);
            }
            if !dir.pop() {
                return Err(StoreError::NotFound);
            }
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn config_path(&self) -> PathBuf {
        self.cliproot_dir.join("config.json")
    }

    fn ensure_config(&self) -> Result<(), StoreError> {
        if !self.config_path().exists() {
            self.write_config(&RepoConfig {
                protocol_version: PROTOCOL_VERSION.to_string(),
                current_project_id: None,
            })?;
        }
        Ok(())
    }

    fn read_config(&self) -> Result<RepoConfig, StoreError> {
        let json = fs::read_to_string(self.config_path())?;
        Ok(serde_json::from_str(&json)?)
    }

    fn write_config(&self, config: &RepoConfig) -> Result<(), StoreError> {
        fs::write(self.config_path(), serde_json::to_string_pretty(config)?)?;
        Ok(())
    }

    pub fn current_project_id(&self) -> Result<Option<String>, StoreError> {
        Ok(self.read_config()?.current_project_id)
    }

    pub fn create_project(
        &self,
        id: &str,
        name: &str,
        description: Option<String>,
    ) -> Result<Project, StoreError> {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let project = Project {
            id: CrpId(id.to_string()),
            name: name.to_string(),
            description,
            created_at: Some(now.clone()),
            updated_at: Some(now),
        };
        self.index.upsert_project(&project)?;
        Ok(project)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, StoreError> {
        self.index.list_projects()
    }

    pub fn use_project(&self, project_id: &str) -> Result<(), StoreError> {
        self.index
            .get_project_by_id(project_id)?
            .ok_or_else(|| StoreError::Other(format!("project not found: {project_id}")))?;
        let mut config = self.read_config()?;
        config.current_project_id = Some(project_id.to_string());
        self.write_config(&config)
    }

    pub fn delete_project(&self, project_id: &str) -> Result<(), StoreError> {
        self.index.delete_project(project_id)?;
        let mut config = self.read_config()?;
        if config.current_project_id.as_deref() == Some(project_id) {
            config.current_project_id = None;
            self.write_config(&config)?;
        }
        Ok(())
    }

    fn resolve_project_id(&self, explicit: Option<&str>) -> Result<Option<CrpId>, StoreError> {
        if let Some(project_id) = explicit {
            self.index
                .get_project_by_id(project_id)?
                .ok_or_else(|| StoreError::Other(format!("project not found: {project_id}")))?;
            return Ok(Some(CrpId(project_id.to_string())));
        }

        match self.current_project_id()? {
            Some(project_id) => Ok(Some(CrpId(project_id))),
            None => Ok(None),
        }
    }

    pub fn store_bundle(&self, bundle: &CrpBundle) -> Result<String, StoreError> {
        let bundle_hash = if let Some(clip) = bundle.clips.first() {
            clip.clip_hash.0.clone()
        } else if let Some(artifact) = bundle.artifacts.first() {
            artifact.artifact_hash.0.clone()
        } else {
            let json = serde_json::to_string(bundle)?;
            cliproot_core::create_text_hash(&json).0
        };

        for artifact in &bundle.artifacts {
            if let Some(content_base64) = &artifact.content_base64 {
                let bytes = STANDARD
                    .decode(content_base64.as_bytes())
                    .map_err(|e| StoreError::Other(format!("invalid artifact base64: {e}")))?;
                if !self.objects.has_artifact(&artifact.artifact_hash.0) {
                    self.objects.write_artifact(&artifact.artifact_hash.0, &bytes)?;
                }
            }
        }

        self.objects.write_bundle(&bundle_hash, bundle)?;
        self.index.index_bundle(bundle, &bundle_hash)?;

        Ok(bundle_hash)
    }

    pub fn get_clip(&self, hash_or_id: &str) -> Result<Option<Clip>, StoreError> {
        if let Some(clip) = self.index.get_clip_full(hash_or_id)? {
            return Ok(Some(clip));
        }
        if let Some(row) = self.index.find_clip_by_id(hash_or_id)? {
            return self.index.get_clip_full(&row.clip_hash);
        }
        Ok(None)
    }

    pub fn get_clip_full(&self, hash_or_id: &str) -> Result<Option<Clip>, StoreError> {
        let row = if let Some(r) = self.index.find_clip_by_hash(hash_or_id)? {
            r
        } else if let Some(r) = self.index.find_clip_by_id(hash_or_id)? {
            r
        } else {
            return Ok(None);
        };

        let bundle = self.objects.read_bundle(&row.bundle_hash)?;
        Ok(bundle
            .clips
            .into_iter()
            .find(|c| c.clip_hash.0 == row.clip_hash))
    }

    pub fn resolve_clip_hash(&self, hash_or_id: &str) -> Result<Option<String>, StoreError> {
        if self.index.find_clip_by_hash(hash_or_id)?.is_some() {
            return Ok(Some(hash_or_id.to_string()));
        }
        if let Some(row) = self.index.find_clip_by_id(hash_or_id)? {
            return Ok(Some(row.clip_hash));
        }
        Ok(None)
    }

    pub fn list_clips(
        &self,
        document_id: Option<&str>,
        source_type: Option<&str>,
        project_id: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<Clip>, StoreError> {
        let project_id = match project_id {
            Some(value) => Some(value.to_string()),
            None => self.current_project_id()?,
        };

        let rows = self
            .index
            .list_clips(document_id, source_type, project_id.as_deref(), limit)?;
        let mut clips = Vec::new();
        for row in rows {
            if let Some(clip) = self.get_clip_full(&row.clip_hash)? {
                clips.push(clip);
            }
        }
        Ok(clips)
    }

    pub fn trace(&self, hash_or_id: &str) -> Result<Vec<LineageNode>, StoreError> {
        let clip_hash = self
            .resolve_clip_hash(hash_or_id)?
            .ok_or_else(|| StoreError::Other(format!("clip not found: {hash_or_id}")))?;
        self.index.trace_lineage(&clip_hash)
    }

    pub fn verify_clip(&self, hash_or_id: &str) -> Result<(), StoreError> {
        let clip = self
            .get_clip_full(hash_or_id)?
            .ok_or_else(|| StoreError::Other(format!("clip not found: {hash_or_id}")))?;
        verify_clip_hash(&clip)?;
        verify_text_hash(&clip)?;
        Ok(())
    }

    pub fn verify_all(&self) -> Result<Vec<String>, StoreError> {
        let rows = self.index.list_clips(None, None, None, Some(10000))?;
        let mut errors = Vec::new();
        for row in &rows {
            match self.get_clip_full(&row.clip_hash)? {
                Some(clip) => {
                    if let Err(e) = verify_clip_hash(&clip) {
                        errors.push(format!("{}: {e}", clip.clip_hash));
                    }
                    if let Err(e) = verify_text_hash(&clip) {
                        errors.push(format!("{}: {e}", clip.clip_hash));
                    }
                }
                None => errors.push(format!("{}: clip not found in object store", row.clip_hash)),
            }
        }
        Ok(errors)
    }

    pub fn export_bundle(&self, hash_or_id: &str) -> Result<CrpBundle, StoreError> {
        let clip = self
            .get_clip_full(hash_or_id)?
            .or_else(|| self.get_clip(hash_or_id).ok().flatten())
            .ok_or_else(|| StoreError::Other(format!("clip not found: {hash_or_id}")))?;

        let clip_hash = clip.clip_hash.0.clone();
        let lineage = self.index.trace_lineage(&clip_hash)?;
        let mut all_clip_hashes: Vec<String> = vec![clip_hash.clone()];
        for node in &lineage {
            if !all_clip_hashes.contains(&node.parent_hash) {
                all_clip_hashes.push(node.parent_hash.clone());
            }
        }

        let clip_hash_set: BTreeSet<String> = all_clip_hashes.iter().cloned().collect();
        let mut bundle_hashes = BTreeSet::new();
        let mut project = None;
        for hash in &all_clip_hashes {
            if let Some(row) = self.index.find_clip_by_hash(hash)? {
                bundle_hashes.insert(row.bundle_hash.clone());
                if project.is_none() {
                    if let Some(project_id) = row.project_id {
                        project = self.index.get_project_by_id(&project_id)?;
                    }
                }
            }
        }

        let mut documents = BTreeMap::<String, Document>::new();
        let mut agents = BTreeMap::<String, Agent>::new();
        let mut sources = BTreeMap::<String, SourceRecord>::new();
        let mut clips = BTreeMap::<String, Clip>::new();
        let mut artifacts = BTreeMap::<String, Artifact>::new();
        let mut activities = BTreeMap::<String, Activity>::new();
        let mut registry = None;

        for bundle_hash in &bundle_hashes {
            let bundle = self.objects.read_bundle(bundle_hash)?;
            if let Some(doc) = bundle.document {
                documents.entry(doc.id.0.clone()).or_insert(doc);
            }
            for agent in bundle.agents {
                agents.entry(agent.id.0.clone()).or_insert(agent);
            }
            for source in bundle.sources {
                sources.entry(source.id.0.clone()).or_insert(source);
            }
            for clip in bundle.clips {
                if clip_hash_set.contains(&clip.clip_hash.0) {
                    clips.entry(clip.clip_hash.0.clone()).or_insert(clip);
                }
            }
            for artifact in bundle.artifacts {
                artifacts
                    .entry(artifact.artifact_hash.0.clone())
                    .or_insert(artifact);
            }
            for activity in bundle.activities {
                activities.entry(activity.id.0.clone()).or_insert(activity);
            }
            if registry.is_none() {
                registry = bundle.registry;
            }
        }

        let mut edges = BTreeMap::<String, Edge>::new();
        let mut clip_artifact_refs = BTreeMap::<String, ClipArtifactRef>::new();
        for hash in &all_clip_hashes {
            for edge in self.index.get_edges_for_subject(hash)? {
                edges.entry(edge.id.0.clone()).or_insert(edge);
            }
            for link in self.index.get_clip_artifact_refs_for_clip(hash)? {
                let key = format!(
                    "{}:{}:{:?}",
                    link.clip_hash.0, link.artifact_hash.0, link.relationship
                );
                clip_artifact_refs.entry(key).or_insert(link);
            }
        }

        let mut artifact_values = Vec::new();
        for link in clip_artifact_refs.values() {
            if let Some(mut artifact) = self.index.get_artifact_by_hash(&link.artifact_hash.0)? {
                if self.objects.has_artifact(&artifact.artifact_hash.0) {
                    let bytes = self.objects.read_artifact(&artifact.artifact_hash.0)?;
                    artifact.content_base64 = Some(STANDARD.encode(bytes));
                }
                artifacts.insert(artifact.artifact_hash.0.clone(), artifact);
            }
        }
        artifact_values.extend(artifacts.into_values());

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        Ok(CrpBundle {
            protocol_version: PROTOCOL_VERSION.to_string(),
            bundle_type: BundleType::ProvenanceExport,
            created_at: now,
            project,
            document: documents.into_values().next(),
            agents: agents.into_values().collect(),
            sources: sources.into_values().collect(),
            clips: clips.into_values().collect(),
            artifacts: artifact_values,
            clip_artifact_refs: clip_artifact_refs.into_values().collect(),
            activities: activities.into_values().collect(),
            edges: edges.into_values().collect(),
            reuse_events: Vec::new(),
            signatures: Vec::new(),
            registry,
        })
    }

    pub fn ingest_bundle(&self, bundle: &CrpBundle) -> Result<String, StoreError> {
        self.store_bundle(bundle)
    }

    pub fn add_artifact(
        &self,
        path: Option<&Path>,
        content: Option<&[u8]>,
        file_name: Option<&str>,
        artifact_type: ArtifactType,
        mime_type: Option<&str>,
        id: Option<&str>,
        project_id: Option<&str>,
        metadata: Option<serde_json::Value>,
    ) -> Result<Artifact, StoreError> {
        let bytes = if let Some(content) = content {
            content.to_vec()
        } else if let Some(path) = path {
            fs::read(path)?
        } else {
            return Err(StoreError::Other(
                "artifact add requires either a path or content".to_string(),
            ));
        };

        let name = if let Some(file_name) = file_name {
            file_name.to_string()
        } else if let Some(path) = path {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("artifact.bin")
                .to_string()
        } else {
            "artifact.bin".to_string()
        };

        let artifact_hash = hash_artifact_bytes(&bytes);
        if !self.objects.has_artifact(&artifact_hash.0) {
            self.objects.write_artifact(&artifact_hash.0, &bytes)?;
        }

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let artifact = Artifact {
            artifact_hash: artifact_hash.clone(),
            id: id.map(|value| CrpId(value.to_string())),
            project_id: self.resolve_project_id(project_id)?,
            artifact_type,
            file_name: name.clone(),
            mime_type: mime_type
                .map(|value| value.to_string())
                .unwrap_or_else(|| guess_mime_type(&name)),
            byte_size: bytes.len() as u64,
            content_base64: None,
            metadata,
            created_at: Some(now),
        };

        self.index.upsert_artifact(&artifact, &artifact_hash.0)?;
        Ok(artifact)
    }

    pub fn list_artifacts(&self, project_id: Option<&str>) -> Result<Vec<Artifact>, StoreError> {
        let project_id = match project_id {
            Some(value) => Some(value.to_string()),
            None => self.current_project_id()?,
        };
        self.index.list_artifacts(project_id.as_deref())
    }

    pub fn get_artifact(&self, artifact_hash: &str) -> Result<Option<Artifact>, StoreError> {
        self.index.get_artifact_by_hash(artifact_hash)
    }

    pub fn restore_artifact(
        &self,
        artifact_hash: &str,
        output: Option<&Path>,
    ) -> Result<PathBuf, StoreError> {
        let artifact = self
            .index
            .get_artifact_by_hash(artifact_hash)?
            .ok_or_else(|| StoreError::Other(format!("artifact not found: {artifact_hash}")))?;
        let bytes = self.objects.read_artifact(&artifact.artifact_hash.0)?;

        let output_path = match output {
            Some(path) if path.is_dir() => path.join(&artifact.file_name),
            Some(path) => path.to_path_buf(),
            None => self.root.join(&artifact.file_name),
        };

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&output_path, bytes)?;
        Ok(output_path)
    }

    pub fn link_clip_artifact(
        &self,
        hash_or_id: &str,
        artifact_hash: &str,
        relationship: ClipArtifactRelationship,
    ) -> Result<ClipArtifactRef, StoreError> {
        let clip_hash = self
            .resolve_clip_hash(hash_or_id)?
            .ok_or_else(|| StoreError::Other(format!("clip not found: {hash_or_id}")))?;
        self.index
            .get_artifact_by_hash(artifact_hash)?
            .ok_or_else(|| StoreError::Other(format!("artifact not found: {artifact_hash}")))?;
        let link = ClipArtifactRef {
            clip_hash: ContentHash(clip_hash),
            artifact_hash: ContentHash(artifact_hash.to_string()),
            relationship,
        };
        self.index.link_clip_artifact(&link)?;
        Ok(link)
    }

    pub fn build_match_candidates(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<MatchCandidate>, StoreError> {
        let project_id_owned = match project_id {
            Some(value) => Some(value.to_string()),
            None => self.current_project_id()?,
        };
        let rows = self
            .index
            .list_clips(None, None, project_id_owned.as_deref(), Some(10000))?;
        let mut candidates = Vec::new();

        for row in rows {
            if let Some(content) = &row.content {
                let source_refs = self.index.get_source_refs(&row.clip_hash)?;
                let (source_url, source_title) = if let Some(sr_id) = source_refs.first() {
                    if let Some(src) = self.index.get_source_by_id(sr_id)? {
                        (src.source_uri, src.title)
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };

                candidates.push(MatchCandidate {
                    clip_hash: row.clip_hash.clone(),
                    clip_content: content.clone(),
                    source_url,
                    source_title,
                });
            }
        }

        Ok(candidates)
    }

    pub fn annotate(
        &self,
        document_text: &str,
        style: AnnotationStyle,
        threshold: f64,
    ) -> Result<AnnotateResult, StoreError> {
        let candidates = self.build_match_candidates(None)?;
        let matches = matching::find_matches(document_text, &candidates, threshold);
        Ok(matching::annotate_document(
            document_text,
            &matches,
            &candidates,
            style,
        ))
    }

    pub fn cite(&self, document_text: &str, threshold: f64) -> Result<Vec<Citation>, StoreError> {
        let candidates = self.build_match_candidates(None)?;
        let matches = matching::find_matches(document_text, &candidates, threshold);
        Ok(matching::generate_citations(&matches, &candidates))
    }

    pub fn doctor(&self, document_text: &str, threshold: f64) -> Result<DoctorResult, StoreError> {
        let candidates = self.build_match_candidates(None)?;
        let matches = matching::find_matches(document_text, &candidates, threshold);
        Ok(matching::generate_doctor_report(document_text, &matches))
    }
}

fn hash_artifact_bytes(bytes: &[u8]) -> ContentHash {
    let digest = Sha256::digest(bytes);
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    ContentHash(format!("sha256-{encoded}"))
}

fn guess_mime_type(file_name: &str) -> String {
    if file_name.ends_with(".md") {
        "text/markdown".to_string()
    } else if file_name.ends_with(".json") {
        "application/json".to_string()
    } else if file_name.ends_with(".txt") {
        "text/plain".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cliproot_core::{create_clip_hash, create_text_hash, hash::ClipHashInput};
    use tempfile::TempDir;

    fn make_test_bundle(content: &str, url: &str) -> CrpBundle {
        let text_hash = create_text_hash(content);
        let source = SourceRecord {
            id: CrpId("src_01".to_string()),
            source_type: SourceType::ExternalQuoted,
            digital_source_type: None,
            title: None,
            source_uri: Some(url.to_string()),
            author_agent_id: None,
            created_at: None,
        };
        let clip_hash = create_clip_hash(ClipHashInput {
            text_hash: text_hash.clone(),
            source_refs: vec!["src_01".to_string()],
            text_quote_exact: Some(content.to_string()),
        });
        let clip = Clip {
            clip_hash,
            id: Some(CrpId("clip_01".to_string())),
            project_id: Some(CrpId("proj_demo".to_string())),
            document_id: None,
            source_refs: vec!["src_01".to_string()],
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

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        CrpBundle {
            protocol_version: PROTOCOL_VERSION.to_string(),
            bundle_type: BundleType::Document,
            created_at: now.clone(),
            project: Some(Project {
                id: CrpId("proj_demo".to_string()),
                name: "Demo".to_string(),
                description: None,
                created_at: Some(now.clone()),
                updated_at: Some(now),
            }),
            document: None,
            agents: Vec::new(),
            sources: vec![source],
            clips: vec![clip],
            artifacts: Vec::new(),
            clip_artifact_refs: Vec::new(),
            activities: Vec::new(),
            edges: Vec::new(),
            reuse_events: Vec::new(),
            signatures: Vec::new(),
            registry: None,
        }
    }

    #[test]
    fn test_init_and_store() {
        let tmp = TempDir::new().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        let bundle = make_test_bundle("Hello world", "https://example.com");
        let hash = repo.store_bundle(&bundle).unwrap();

        let clip = repo.get_clip(&hash).unwrap().unwrap();
        assert_eq!(clip.content.as_deref(), Some("Hello world"));
        assert_eq!(clip.project_id.unwrap().0, "proj_demo");
    }
}
