use std::fs;
use std::path::{Path, PathBuf};

use cliproot_core::{
    matching::{self, AnnotateResult, AnnotationStyle, Citation, DoctorResult, MatchCandidate},
    model::*,
    verify::{verify_clip_hash, verify_text_hash},
};

use crate::error::StoreError;
use crate::index_db::{IndexDb, LineageNode};
use crate::object_store::ObjectStore;

const PROTOCOL_VERSION: &str = "0.0.2";

pub struct Repository {
    root: PathBuf,
    objects: ObjectStore,
    index: IndexDb,
}

impl Repository {
    /// Create a new .cliproot/ repository at the given path.
    pub fn init(path: &Path) -> Result<Self, StoreError> {
        let cliproot_dir = path.join(".cliproot");
        if cliproot_dir.exists() {
            return Err(StoreError::AlreadyExists(
                cliproot_dir.display().to_string(),
            ));
        }

        fs::create_dir_all(&cliproot_dir)?;

        // Write config
        let config = serde_json::json!({ "protocolVersion": PROTOCOL_VERSION });
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
            objects,
            index,
        })
    }

    /// Open an existing .cliproot/ repository.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let cliproot_dir = path.join(".cliproot");
        if !cliproot_dir.exists() {
            return Err(StoreError::NotFound);
        }

        let objects = ObjectStore::new(&cliproot_dir);
        let index = IndexDb::open(&cliproot_dir.join("index.db"))?;

        Ok(Self {
            root: path.to_path_buf(),
            objects,
            index,
        })
    }

    /// Walk up from CWD to find .cliproot/
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

    /// Store a bundle, index it, and return the first clip hash as the bundle identifier.
    pub fn store_bundle(&self, bundle: &CrpBundle) -> Result<String, StoreError> {
        // Use first clip hash as bundle identifier, or generate a hash of the bundle
        let bundle_hash = if let Some(clip) = bundle.clips.first() {
            clip.clip_hash.0.clone()
        } else {
            // For bundles without clips, use a hash of the serialized bundle
            let json = serde_json::to_string(bundle)?;
            cliproot_core::create_text_hash(&json).0
        };

        self.objects.write_bundle(&bundle_hash, bundle)?;
        self.index.index_bundle(bundle, &bundle_hash)?;

        Ok(bundle_hash)
    }

    /// Get a clip by hash or id (index-only, selectors may be missing).
    pub fn get_clip(&self, hash_or_id: &str) -> Result<Option<Clip>, StoreError> {
        // Try by hash first
        if let Some(clip) = self.index.get_clip_full(hash_or_id)? {
            return Ok(Some(clip));
        }
        // Try by id
        if let Some(row) = self.index.find_clip_by_id(hash_or_id)? {
            return self.index.get_clip_full(&row.clip_hash);
        }
        Ok(None)
    }

    /// Get the full clip from the object store bundle (preserves selectors).
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

    /// Resolve a hash-or-id to a clip hash.
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
        limit: Option<u32>,
    ) -> Result<Vec<Clip>, StoreError> {
        let rows = self.index.list_clips(document_id, source_type, limit)?;
        let mut clips = Vec::new();
        for row in rows {
            if let Some(clip) = self.index.get_clip_full(&row.clip_hash)? {
                clips.push(clip);
            }
        }
        Ok(clips)
    }

    /// Trace lineage from a clip upward through derivation edges.
    pub fn trace(&self, hash_or_id: &str) -> Result<Vec<LineageNode>, StoreError> {
        let clip_hash = self
            .resolve_clip_hash(hash_or_id)?
            .ok_or_else(|| StoreError::Other(format!("clip not found: {hash_or_id}")))?;
        self.index.trace_lineage(&clip_hash)
    }

    /// Verify a single clip's hashes (reads from object store for full fidelity).
    pub fn verify_clip(&self, hash_or_id: &str) -> Result<(), StoreError> {
        let clip = self
            .get_clip_full(hash_or_id)?
            .ok_or_else(|| StoreError::Other(format!("clip not found: {hash_or_id}")))?;
        verify_clip_hash(&clip)?;
        verify_text_hash(&clip)?;
        Ok(())
    }

    /// Verify all clips in the repository.
    pub fn verify_all(&self) -> Result<Vec<String>, StoreError> {
        let rows = self.index.list_clips(None, None, Some(10000))?;
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
                None => {
                    errors.push(format!("{}: clip not found in object store", row.clip_hash));
                }
            }
        }
        Ok(errors)
    }

    /// Export a clip and its lineage as a CRP bundle.
    pub fn export_bundle(&self, hash_or_id: &str) -> Result<CrpBundle, StoreError> {
        let clip = self
            .get_clip_full(hash_or_id)?
            .or_else(|| self.get_clip(hash_or_id).ok().flatten())
            .ok_or_else(|| StoreError::Other(format!("clip not found: {hash_or_id}")))?;

        let clip_hash = clip.clip_hash.0.clone();

        // Collect lineage
        let lineage = self.index.trace_lineage(&clip_hash)?;
        let mut all_clip_hashes: Vec<String> = vec![clip_hash.clone()];
        for node in &lineage {
            if !all_clip_hashes.contains(&node.parent_hash) {
                all_clip_hashes.push(node.parent_hash.clone());
            }
        }

        // Gather clips (prefer full from object store)
        let mut clips = Vec::new();
        for hash in &all_clip_hashes {
            if let Some(c) = self.get_clip_full(hash)? {
                clips.push(c);
            } else if let Some(c) = self.index.get_clip_full(hash)? {
                clips.push(c);
            }
        }

        // Gather derivation edges
        let mut edges = Vec::new();
        for hash in &all_clip_hashes {
            let clip_edges = self.index.get_edges_for_clip(hash)?;
            for edge in clip_edges {
                if !edges.iter().any(|e: &DerivationEdge| e.id == edge.id) {
                    edges.push(edge);
                }
            }
        }

        // Gather sources referenced by clips
        let mut source_ids: Vec<String> = Vec::new();
        for c in &clips {
            for sr in &c.source_refs {
                if !source_ids.contains(sr) {
                    source_ids.push(sr.clone());
                }
            }
        }

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        Ok(CrpBundle {
            protocol_version: PROTOCOL_VERSION.to_string(),
            bundle_type: BundleType::ProvenanceExport,
            created_at: now,
            document: None,
            agents: Vec::new(),
            sources: Vec::new(), // sources not fully stored in index, but source_refs are on clips
            clips,
            activities: Vec::new(),
            derivation_edges: edges,
            reuse_events: Vec::new(),
            signatures: Vec::new(),
            registry: None,
        })
    }

    /// Ingest an external CRP bundle file.
    pub fn ingest_bundle(&self, bundle: &CrpBundle) -> Result<String, StoreError> {
        self.store_bundle(bundle)
    }

    // ── Phase 2b: Document analysis ──────────────────────────────────────

    /// Load all clips with their source metadata as match candidates.
    pub fn build_match_candidates(&self) -> Result<Vec<MatchCandidate>, StoreError> {
        let rows = self.index.list_clips(None, None, Some(10000))?;
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

    /// Annotate a document with inline citations matched against stored clips.
    pub fn annotate(
        &self,
        document_text: &str,
        style: AnnotationStyle,
        threshold: f64,
    ) -> Result<AnnotateResult, StoreError> {
        let candidates = self.build_match_candidates()?;
        let matches = matching::find_matches(document_text, &candidates, threshold);
        Ok(matching::annotate_document(
            document_text,
            &matches,
            &candidates,
            style,
        ))
    }

    /// Generate citations for a document matched against stored clips.
    pub fn cite(&self, document_text: &str, threshold: f64) -> Result<Vec<Citation>, StoreError> {
        let candidates = self.build_match_candidates()?;
        let matches = matching::find_matches(document_text, &candidates, threshold);
        Ok(matching::generate_citations(&matches, &candidates))
    }

    /// Generate a provenance coverage report for a document.
    pub fn doctor(&self, document_text: &str, threshold: f64) -> Result<DoctorResult, StoreError> {
        let candidates = self.build_match_candidates()?;
        let matches = matching::find_matches(document_text, &candidates, threshold);
        Ok(matching::generate_doctor_report(document_text, &matches))
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
            created_at: now,
            document: None,
            agents: Vec::new(),
            sources: vec![source],
            clips: vec![clip],
            activities: Vec::new(),
            derivation_edges: Vec::new(),
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
    }

    #[test]
    fn test_list_clips() {
        let tmp = TempDir::new().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        let bundle = make_test_bundle("Test content", "https://example.com");
        repo.store_bundle(&bundle).unwrap();

        let clips = repo.list_clips(None, None, None).unwrap();
        assert_eq!(clips.len(), 1);
    }

    #[test]
    fn test_verify_clip() {
        let tmp = TempDir::new().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();

        let bundle = make_test_bundle("Verify me", "https://example.com");
        let hash = repo.store_bundle(&bundle).unwrap();

        repo.verify_clip(&hash).unwrap();
    }
}
