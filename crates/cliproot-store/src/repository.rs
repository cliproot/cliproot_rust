use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cliproot_core::{
    matching::{self, AnnotateResult, AnnotationStyle, Citation, DoctorResult, MatchCandidate},
    model::*,
    verify::{verify_clip_hash, verify_text_hash},
};
use sha2::{Digest, Sha256};
use tar::{Archive, Builder, Header};

use crate::error::StoreError;
use crate::index_db::{IndexDb, LineageNode, SessionRow};
use crate::object_store::ObjectStore;
use crate::pack::{
    safe_restore_name, sha256_digest, PackArtifactEntry, PackCounts, PackManifest, PackObjectEntry,
    PackRootMode, PackRoots, PACK_FORMAT,
};

const PROTOCOL_VERSION: &str = "0.0.3";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConfig {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoConfig {
    protocol_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_project_id: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    remotes: HashMap<String, RemoteConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_remote: Option<String>,
}

pub struct Repository {
    root: PathBuf,
    cliproot_dir: PathBuf,
    objects: ObjectStore,
    index: IndexDb,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_clip_hashes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_hash: Option<String>,
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
            remotes: HashMap::new(),
            default_remote: None,
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

    pub fn cliproot_dir(&self) -> &Path {
        &self.cliproot_dir
    }

    fn config_path(&self) -> PathBuf {
        self.cliproot_dir.join("config.json")
    }

    fn ensure_config(&self) -> Result<(), StoreError> {
        if !self.config_path().exists() {
            self.write_config(&RepoConfig {
                protocol_version: PROTOCOL_VERSION.to_string(),
                current_project_id: None,
                remotes: HashMap::new(),
                default_remote: None,
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

    // ── Remote management ───────────────────────────────────────────

    pub fn add_remote(&self, name: &str, url: &str, owner: Option<&str>) -> Result<(), StoreError> {
        let mut config = self.read_config()?;
        if config.remotes.contains_key(name) {
            return Err(StoreError::Other(format!("remote already exists: {name}")));
        }
        config.remotes.insert(
            name.to_string(),
            RemoteConfig {
                url: url.to_string(),
                owner: owner.map(String::from),
            },
        );
        if config.remotes.len() == 1 {
            config.default_remote = Some(name.to_string());
        }
        self.write_config(&config)
    }

    pub fn remove_remote(&self, name: &str) -> Result<(), StoreError> {
        let mut config = self.read_config()?;
        if config.remotes.remove(name).is_none() {
            return Err(StoreError::Other(format!("remote not found: {name}")));
        }
        if config.default_remote.as_deref() == Some(name) {
            config.default_remote = None;
        }
        self.write_config(&config)
    }

    pub fn list_remotes(&self) -> Result<Vec<(String, RemoteConfig)>, StoreError> {
        let config = self.read_config()?;
        let mut remotes: Vec<_> = config.remotes.into_iter().collect();
        remotes.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(remotes)
    }

    pub fn get_remote(&self, name: &str) -> Result<RemoteConfig, StoreError> {
        let config = self.read_config()?;
        config
            .remotes
            .get(name)
            .cloned()
            .ok_or_else(|| StoreError::Other(format!("remote not found: {name}")))
    }

    pub fn default_remote(&self) -> Result<Option<(String, RemoteConfig)>, StoreError> {
        let config = self.read_config()?;
        if let Some(name) = &config.default_remote {
            if let Some(remote) = config.remotes.get(name) {
                return Ok(Some((name.clone(), remote.clone())));
            }
        }
        if config.remotes.len() == 1 {
            let (name, remote) = config.remotes.into_iter().next().unwrap();
            return Ok(Some((name, remote)));
        }
        Ok(None)
    }

    pub fn set_default_remote(&self, name: &str) -> Result<(), StoreError> {
        let mut config = self.read_config()?;
        if !config.remotes.contains_key(name) {
            return Err(StoreError::Other(format!("remote not found: {name}")));
        }
        config.default_remote = Some(name.to_string());
        self.write_config(&config)
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

    fn resolve_project(&self, explicit: Option<&str>) -> Result<Option<Project>, StoreError> {
        let Some(project_id) = self.resolve_project_id(explicit)? else {
            return Ok(None);
        };
        self.index.get_project_by_id(project_id.as_str())
    }

    fn store_activity_bundle(&self, activity: &Activity) -> Result<String, StoreError> {
        let bundle = CrpBundle {
            protocol_version: PROTOCOL_VERSION.to_string(),
            bundle_type: BundleType::ProvenanceExport,
            created_at: activity
                .ended_at
                .clone()
                .unwrap_or_else(|| activity.created_at.clone()),
            project: self.resolve_project(activity.project_id.as_ref().map(|id| id.as_str()))?,
            document: None,
            agents: Vec::new(),
            sources: Vec::new(),
            clips: Vec::new(),
            artifacts: Vec::new(),
            clip_artifact_refs: Vec::new(),
            activities: vec![activity.clone()],
            edges: Vec::new(),
            reuse_events: Vec::new(),
            signatures: Vec::new(),
            registry: None,
        };
        self.store_bundle(&bundle)
    }

    pub fn start_activity(
        &self,
        activity_type: ActivityType,
        project_id: Option<&str>,
        agent_id: Option<&str>,
        prompt: Option<String>,
        parameters: Option<serde_json::Value>,
        session_id: Option<&str>,
    ) -> Result<Activity, StoreError> {
        if let Some(session_id) = session_id {
            self.index
                .get_session(session_id)?
                .ok_or_else(|| StoreError::Other(format!("session not found: {session_id}")))?;
        }

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let activity = Activity {
            id: CrpId(format!("act-{}", uuid::Uuid::new_v4())),
            activity_type,
            project_id: self.resolve_project_id(project_id)?,
            agent_id: agent_id.map(|value| CrpId(value.to_string())),
            prompt,
            parameters,
            used_source_refs: Vec::new(),
            generated_clip_refs: Vec::new(),
            created_at: now,
            ended_at: None,
        };

        self.store_activity_bundle(&activity)?;
        if let Some(session_id) = session_id {
            self.index
                .link_session_activity(session_id, activity.id.as_str())?;
        }
        Ok(activity)
    }

    pub fn end_activity(&self, activity_id: &str) -> Result<Activity, StoreError> {
        let mut activity = self
            .index
            .get_activity_by_id(activity_id)?
            .ok_or_else(|| StoreError::Other(format!("activity not found: {activity_id}")))?;
        activity.used_source_refs = self.index.get_activity_used_refs(activity_id)?;
        activity.generated_clip_refs = self.index.get_activity_generated_clips(activity_id)?;
        activity.ended_at =
            Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        self.store_activity_bundle(&activity)?;
        Ok(activity)
    }

    pub fn start_session(
        &self,
        project_id: Option<&str>,
        agent_id: Option<&str>,
        metadata: Option<serde_json::Value>,
    ) -> Result<SessionRecord, StoreError> {
        let session_id = format!("sess-{}", uuid::Uuid::new_v4());
        let started_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let project_id = self.resolve_project_id(project_id)?.map(|id| id.0);
        self.index.upsert_session(&SessionRow {
            id: session_id.clone(),
            project_id: project_id.clone(),
            agent_id: agent_id.map(|value| value.to_string()),
            metadata: metadata.clone(),
            started_at: started_at.clone(),
            ended_at: None,
            artifact_hash: None,
        })?;
        Ok(SessionRecord {
            session_id,
            project_id,
            agent_id: agent_id.map(|value| value.to_string()),
            metadata,
            started_at,
            ended_at: None,
            activity_ids: Vec::new(),
            generated_clip_hashes: Vec::new(),
            artifact_hash: None,
        })
    }

    pub fn end_session(&self, session_id: &str) -> Result<SessionRecord, StoreError> {
        let session = self
            .index
            .get_session(session_id)?
            .ok_or_else(|| StoreError::Other(format!("session not found: {session_id}")))?;
        let activity_ids = self.index.get_session_activity_ids(session_id)?;
        let generated_clip_hashes = self.index.get_session_clip_hashes(session_id)?;
        let ended_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let record = SessionRecord {
            session_id: session.id.clone(),
            project_id: session.project_id.clone(),
            agent_id: session.agent_id.clone(),
            metadata: session.metadata.clone(),
            started_at: session.started_at.clone(),
            ended_at: Some(ended_at.clone()),
            activity_ids,
            generated_clip_hashes,
            artifact_hash: None,
        };

        let file_name = format!("{session_id}.json");
        let payload = serde_json::to_vec_pretty(&record)?;
        let artifact = self.add_artifact(
            None,
            Some(&payload),
            Some(&file_name),
            ArtifactType::Session,
            Some("application/json"),
            Some(session_id),
            session.project_id.as_deref(),
            record.metadata.clone(),
        )?;

        for clip_hash in &record.generated_clip_hashes {
            let link = ClipArtifactRef {
                clip_hash: ContentHash(clip_hash.clone()),
                artifact_hash: artifact.artifact_hash.clone(),
                relationship: ClipArtifactRelationship::AttachedTo,
            };
            self.index.link_clip_artifact(&link)?;
        }

        self.index.upsert_session(&SessionRow {
            id: session.id.clone(),
            project_id: session.project_id.clone(),
            agent_id: session.agent_id.clone(),
            metadata: session.metadata.clone(),
            started_at: session.started_at,
            ended_at: Some(ended_at),
            artifact_hash: Some(artifact.artifact_hash.0.clone()),
        })?;

        Ok(SessionRecord {
            artifact_hash: Some(artifact.artifact_hash.0),
            ..record
        })
    }

    pub fn record_clip_tracking(
        &self,
        clip_hash: &str,
        activity_id: Option<&str>,
        session_id: Option<&str>,
        used_refs: &[String],
    ) -> Result<(), StoreError> {
        let mut session_ids = BTreeSet::new();
        if let Some(session_id) = session_id {
            self.index
                .get_session(session_id)?
                .ok_or_else(|| StoreError::Other(format!("session not found: {session_id}")))?;
            session_ids.insert(session_id.to_string());
        }

        if let Some(activity_id) = activity_id {
            self.index
                .get_activity_by_id(activity_id)?
                .ok_or_else(|| StoreError::Other(format!("activity not found: {activity_id}")))?;
            self.index
                .link_activity_generated_clip(activity_id, clip_hash)?;
            for used_ref in used_refs {
                self.index.link_activity_used_ref(activity_id, used_ref)?;
            }
            for linked_session_id in self.index.get_session_ids_for_activity(activity_id)? {
                session_ids.insert(linked_session_id);
            }
        }

        for session_id in session_ids {
            self.index.link_session_clip(&session_id, clip_hash)?;
        }

        Ok(())
    }

    pub fn get_artifact_bytes(&self, artifact_hash: &str) -> Result<Vec<u8>, StoreError> {
        self.objects.read_artifact(artifact_hash)
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
                    self.objects
                        .write_artifact(&artifact.artifact_hash.0, &bytes)?;
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

    pub fn has_clip_for_uri(&self, uri: &str) -> Result<bool, StoreError> {
        let sources = self.index.find_sources_by_uri(uri)?;
        Ok(!sources.is_empty())
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
        let activity_ids = self
            .index
            .get_activity_ids_for_clip_hashes(&all_clip_hashes)?;
        for bundle_hash in self.index.get_bundle_hashes_for_activities(&activity_ids)? {
            bundle_hashes.insert(bundle_hash);
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

    #[allow(clippy::too_many_arguments)]
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

    pub fn create_pack(
        &self,
        project_id: Option<&str>,
        roots: &[String],
        depth: Option<u32>,
        output: &Path,
    ) -> Result<PackManifest, StoreError> {
        if (project_id.is_some() && !roots.is_empty()) || (project_id.is_none() && roots.is_empty())
        {
            return Err(StoreError::Other(
                "pack create requires either a project id or one or more --root values".to_string(),
            ));
        }

        let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let (project, pack_roots, included_clip_hashes) = if let Some(project_id) = project_id {
            let project = self
                .index
                .get_project_by_id(project_id)?
                .ok_or_else(|| StoreError::Other(format!("project not found: {project_id}")))?;
            let project_rows =
                self.index
                    .list_clips(None, None, Some(project_id), Some(u32::MAX))?;
            let root_hashes = project_rows
                .iter()
                .map(|row| row.clip_hash.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let included = self.collect_closure(root_hashes.to_vec(), None)?;
            (
                Some(project),
                PackRoots {
                    mode: PackRootMode::Project,
                    project_id: Some(project_id.to_string()),
                    clip_hashes: root_hashes,
                },
                included,
            )
        } else {
            let resolved_roots = self.resolve_root_hashes(roots)?;
            let root_hashes = resolved_roots.iter().cloned().collect::<BTreeSet<_>>();
            let included = self.collect_closure(root_hashes.iter().cloned().collect(), depth)?;
            (
                None,
                PackRoots {
                    mode: PackRootMode::Roots,
                    project_id: None,
                    clip_hashes: root_hashes.into_iter().collect(),
                },
                included,
            )
        };

        let mut bundle_hashes = BTreeSet::new();
        let mut clip_links = BTreeMap::<String, ClipArtifactRef>::new();
        for clip_hash in &included_clip_hashes {
            let row = self
                .index
                .find_clip_by_hash(clip_hash)?
                .ok_or_else(|| StoreError::Other(format!("clip not found: {clip_hash}")))?;
            bundle_hashes.insert(row.bundle_hash);
            for link in self.index.get_clip_artifact_refs_for_clip(clip_hash)? {
                let key = format!(
                    "{}:{}:{:?}",
                    link.clip_hash.0, link.artifact_hash.0, link.relationship
                );
                clip_links.entry(key).or_insert(link);
            }
        }
        let included_clip_hashes_vec = included_clip_hashes.iter().cloned().collect::<Vec<_>>();
        let activity_ids = self
            .index
            .get_activity_ids_for_clip_hashes(&included_clip_hashes_vec)?;
        for bundle_hash in self.index.get_bundle_hashes_for_activities(&activity_ids)? {
            bundle_hashes.insert(bundle_hash);
        }

        let mut artifacts = BTreeMap::<String, Artifact>::new();
        if let Some(project_id) = project_id {
            for artifact in self.list_artifacts(Some(project_id))? {
                artifacts.insert(artifact.artifact_hash.0.clone(), artifact);
            }
        }
        for link in clip_links.values() {
            let artifact = self
                .index
                .get_artifact_by_hash(&link.artifact_hash.0)?
                .ok_or_else(|| {
                    StoreError::Other(format!("artifact not found: {}", link.artifact_hash.0))
                })?;
            artifacts
                .entry(artifact.artifact_hash.0.clone())
                .or_insert(artifact);
        }

        let mut object_entries = Vec::new();
        let mut archive_entries = BTreeMap::<String, Vec<u8>>::new();
        let mut unique_pack_clips = BTreeSet::new();
        let mut unique_pack_edges = BTreeSet::new();

        for bundle_hash in bundle_hashes {
            let bytes = self.objects.read_bundle_bytes(&bundle_hash)?;
            let bundle: CrpBundle = serde_json::from_slice(&bytes)?;
            let clip_hashes = bundle
                .clips
                .iter()
                .map(|clip| clip.clip_hash.0.clone())
                .collect::<Vec<_>>();
            for clip_hash in &clip_hashes {
                unique_pack_clips.insert(clip_hash.clone());
            }
            for edge in &bundle.edges {
                unique_pack_edges.insert(edge.id.0.clone());
            }

            let archive_path = format!("objects/{bundle_hash}.json");
            archive_entries.insert(archive_path.clone(), bytes.clone());
            object_entries.push(PackObjectEntry {
                bundle_hash,
                archive_path,
                byte_size: bytes.len() as u64,
                sha256_digest: sha256_digest(&bytes),
                clip_hashes,
            });
        }

        let mut artifact_entries = Vec::new();
        for artifact in artifacts.into_values() {
            if !self.objects.has_artifact(&artifact.artifact_hash.0) {
                return Err(StoreError::Other(format!(
                    "artifact blob missing from object store: {}",
                    artifact.artifact_hash.0
                )));
            }
            let bytes = self.objects.read_artifact(&artifact.artifact_hash.0)?;
            let entry = PackArtifactEntry::from_artifact(&artifact);
            archive_entries.insert(entry.archive_path.clone(), bytes);
            artifact_entries.push(entry);
        }

        let manifest = PackManifest {
            format: PACK_FORMAT.to_string(),
            created_at,
            project,
            roots: pack_roots,
            counts: PackCounts {
                bundles: object_entries.len(),
                clips: unique_pack_clips.len(),
                edges: unique_pack_edges.len(),
                artifacts: artifact_entries.len(),
                links: clip_links.len(),
            },
            objects: object_entries,
            artifacts: artifact_entries,
            clip_artifact_refs: clip_links.into_values().collect(),
        };

        validate_manifest(&manifest)?;
        write_pack_archive(output, &manifest, &archive_entries)?;
        Ok(manifest)
    }

    pub fn inspect_pack(path: &Path) -> Result<PackManifest, StoreError> {
        let (manifest, _) = read_pack_archive(path)?;
        validate_manifest(&manifest)?;
        Ok(manifest)
    }

    pub fn verify_pack(path: &Path) -> Result<PackManifest, StoreError> {
        let (manifest, entries) = read_pack_archive(path)?;
        verify_pack_contents(&manifest, &entries)?;
        Ok(manifest)
    }

    pub fn import_pack(
        &self,
        path: &Path,
        restore_artifacts_to: Option<&Path>,
    ) -> Result<PackManifest, StoreError> {
        let (manifest, entries) = read_pack_archive(path)?;
        verify_pack_contents(&manifest, &entries)?;

        if let Some(project) = &manifest.project {
            self.index.upsert_project(project)?;
        }

        for object in &manifest.objects {
            let bytes = entries.get(&object.archive_path).ok_or_else(|| {
                StoreError::Other(format!("missing archive entry: {}", object.archive_path))
            })?;
            let bundle: CrpBundle = serde_json::from_slice(bytes)?;
            self.ingest_bundle(&bundle)?;
        }

        for artifact_entry in &manifest.artifacts {
            let bytes = entries.get(&artifact_entry.archive_path).ok_or_else(|| {
                StoreError::Other(format!(
                    "missing archive entry: {}",
                    artifact_entry.archive_path
                ))
            })?;
            let artifact = artifact_entry.clone().into_artifact();
            if !self.objects.has_artifact(&artifact.artifact_hash.0) {
                self.objects
                    .write_artifact(&artifact.artifact_hash.0, bytes)?;
            }
            self.index
                .upsert_artifact(&artifact, &artifact.artifact_hash.0)?;
        }

        for link in &manifest.clip_artifact_refs {
            self.index.link_clip_artifact(link)?;
        }

        if let Some(dir) = restore_artifacts_to {
            fs::create_dir_all(dir)?;
            for artifact_entry in &manifest.artifacts {
                let bytes = entries.get(&artifact_entry.archive_path).ok_or_else(|| {
                    StoreError::Other(format!(
                        "missing archive entry: {}",
                        artifact_entry.archive_path
                    ))
                })?;
                let file_name =
                    safe_restore_name(&artifact_entry.artifact_hash, &artifact_entry.file_name);
                fs::write(dir.join(file_name), bytes)?;
            }
        }

        Ok(manifest)
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

impl Repository {
    fn resolve_root_hashes(&self, roots: &[String]) -> Result<Vec<String>, StoreError> {
        let mut resolved = Vec::new();
        for root in roots {
            let clip_hash = self
                .resolve_clip_hash(root)?
                .ok_or_else(|| StoreError::Other(format!("clip not found: {root}")))?;
            resolved.push(clip_hash);
        }
        Ok(resolved)
    }

    fn collect_closure(
        &self,
        root_hashes: Vec<String>,
        depth_limit: Option<u32>,
    ) -> Result<BTreeSet<String>, StoreError> {
        let mut visited = BTreeSet::new();
        let mut queue = VecDeque::new();

        for hash in root_hashes {
            queue.push_back((hash, 0_u32));
        }

        while let Some((clip_hash, depth)) = queue.pop_front() {
            if !visited.insert(clip_hash.clone()) {
                continue;
            }

            let should_expand = match depth_limit {
                Some(limit) => depth < limit,
                None => true,
            };
            if !should_expand {
                continue;
            }

            for edge in self.index.find_derivation_parents(&clip_hash)? {
                queue.push_back((edge.object_ref, depth + 1));
            }
        }

        Ok(visited)
    }
}

fn validate_manifest(manifest: &PackManifest) -> Result<(), StoreError> {
    if manifest.format != PACK_FORMAT {
        return Err(StoreError::Other(format!(
            "unsupported pack format: {}",
            manifest.format
        )));
    }

    match manifest.roots.mode {
        PackRootMode::Project => {
            if manifest.roots.project_id.is_none() {
                return Err(StoreError::Other(
                    "project-mode pack manifest requires roots.projectId".to_string(),
                ));
            }
        }
        PackRootMode::Roots => {
            if manifest.roots.project_id.is_some() {
                return Err(StoreError::Other(
                    "root-mode pack manifest must not set roots.projectId".to_string(),
                ));
            }
        }
    }

    let mut archive_paths = BTreeSet::new();
    for object in &manifest.objects {
        if !archive_paths.insert(object.archive_path.clone()) {
            return Err(StoreError::Other(format!(
                "duplicate archive path in manifest: {}",
                object.archive_path
            )));
        }
    }
    for artifact in &manifest.artifacts {
        if !archive_paths.insert(artifact.archive_path.clone()) {
            return Err(StoreError::Other(format!(
                "duplicate archive path in manifest: {}",
                artifact.archive_path
            )));
        }
    }

    Ok(())
}

fn read_pack_archive(path: &Path) -> Result<(PackManifest, BTreeMap<String, Vec<u8>>), StoreError> {
    let file = File::open(path)?;
    let decoder = zstd::Decoder::new(file)?;
    let mut archive = Archive::new(decoder);
    let mut entries = BTreeMap::<String, Vec<u8>>::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?.to_string_lossy().to_string();
        if entries.contains_key(&entry_path) {
            return Err(StoreError::Other(format!(
                "duplicate archive entry: {entry_path}"
            )));
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        entries.insert(entry_path, bytes);
    }

    let manifest_bytes = entries
        .get("manifest.json")
        .ok_or_else(|| StoreError::Other("pack archive is missing manifest.json".to_string()))?;
    let manifest: PackManifest = serde_json::from_slice(manifest_bytes)?;
    Ok((manifest, entries))
}

fn verify_pack_contents(
    manifest: &PackManifest,
    entries: &BTreeMap<String, Vec<u8>>,
) -> Result<(), StoreError> {
    validate_manifest(manifest)?;

    let mut clip_hashes = BTreeSet::new();
    let mut edge_ids = BTreeSet::new();
    for object in &manifest.objects {
        let bytes = entries.get(&object.archive_path).ok_or_else(|| {
            StoreError::Other(format!("missing archive entry: {}", object.archive_path))
        })?;
        if bytes.len() as u64 != object.byte_size {
            return Err(StoreError::Other(format!(
                "size mismatch for {}: expected {}, found {}",
                object.archive_path,
                object.byte_size,
                bytes.len()
            )));
        }
        let digest = sha256_digest(bytes);
        if digest != object.sha256_digest {
            return Err(StoreError::Other(format!(
                "digest mismatch for {}: expected {}, found {}",
                object.archive_path, object.sha256_digest, digest
            )));
        }

        let bundle: CrpBundle = serde_json::from_slice(bytes)?;
        cliproot_core::verify::verify_bundle(&bundle)?;
        let bundle_clip_hashes = bundle
            .clips
            .iter()
            .map(|clip| clip.clip_hash.0.clone())
            .collect::<BTreeSet<_>>();
        let manifest_clip_hashes = object.clip_hashes.iter().cloned().collect::<BTreeSet<_>>();
        if bundle_clip_hashes != manifest_clip_hashes {
            return Err(StoreError::Other(format!(
                "clip hash list mismatch for {}",
                object.archive_path
            )));
        }
        clip_hashes.extend(bundle_clip_hashes);
        edge_ids.extend(bundle.edges.iter().map(|edge| edge.id.0.clone()));
    }

    let artifact_hashes = manifest
        .artifacts
        .iter()
        .map(|artifact| artifact.artifact_hash.clone())
        .collect::<BTreeSet<_>>();

    for artifact in &manifest.artifacts {
        let bytes = entries.get(&artifact.archive_path).ok_or_else(|| {
            StoreError::Other(format!("missing archive entry: {}", artifact.archive_path))
        })?;
        if bytes.len() as u64 != artifact.byte_size {
            return Err(StoreError::Other(format!(
                "size mismatch for {}: expected {}, found {}",
                artifact.archive_path,
                artifact.byte_size,
                bytes.len()
            )));
        }
        let digest = sha256_digest(bytes);
        if digest != artifact.sha256_digest {
            return Err(StoreError::Other(format!(
                "digest mismatch for {}: expected {}, found {}",
                artifact.archive_path, artifact.sha256_digest, digest
            )));
        }
        if digest != artifact.artifact_hash {
            return Err(StoreError::Other(format!(
                "artifact hash mismatch for {}: expected {}, found {}",
                artifact.archive_path, artifact.artifact_hash, digest
            )));
        }
    }

    for link in &manifest.clip_artifact_refs {
        if !clip_hashes.contains(&link.clip_hash.0) {
            return Err(StoreError::Other(format!(
                "clip-artifact link references missing clip {}",
                link.clip_hash.0
            )));
        }
        if !artifact_hashes.contains(&link.artifact_hash.0) {
            return Err(StoreError::Other(format!(
                "clip-artifact link references missing artifact {}",
                link.artifact_hash.0
            )));
        }
    }

    if manifest.counts.bundles != manifest.objects.len()
        || manifest.counts.clips != clip_hashes.len()
        || manifest.counts.edges != edge_ids.len()
        || manifest.counts.artifacts != manifest.artifacts.len()
        || manifest.counts.links != manifest.clip_artifact_refs.len()
    {
        return Err(StoreError::Other(
            "manifest counts do not match archive contents".to_string(),
        ));
    }

    Ok(())
}

fn write_pack_archive(
    output: &Path,
    manifest: &PackManifest,
    archive_entries: &BTreeMap<String, Vec<u8>>,
) -> Result<(), StoreError> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = File::create(output)?;
    let encoder = zstd::Encoder::new(file, 3)?;
    let mut builder = Builder::new(encoder);

    let manifest_bytes = serde_json::to_vec_pretty(manifest)?;
    append_tar_entry(&mut builder, "manifest.json", &manifest_bytes)?;
    for (path, bytes) in archive_entries {
        append_tar_entry(&mut builder, path, bytes)?;
    }

    builder.finish()?;
    let encoder = builder.into_inner()?;
    encoder.finish()?;
    Ok(())
}

fn append_tar_entry<W: std::io::Write>(
    builder: &mut Builder<W>,
    path: &str,
    bytes: &[u8],
) -> Result<(), StoreError> {
    let mut header = Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, path, Cursor::new(bytes))?;
    Ok(())
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
