use std::fs;
use std::path::{Path, PathBuf};

use cliproot_core::CrpBundle;

use crate::error::StoreError;

pub struct ObjectStore {
    objects_dir: PathBuf,
    artifacts_dir: PathBuf,
}

impl ObjectStore {
    fn bundle_path(&self, hash: &str) -> PathBuf {
        self.objects_dir.join(format!("{hash}.json"))
    }

    fn artifact_path(&self, hash: &str) -> PathBuf {
        self.artifacts_dir.join(hash)
    }

    pub fn new(cliproot_dir: &Path) -> Self {
        Self {
            objects_dir: cliproot_dir.join("objects"),
            artifacts_dir: cliproot_dir.join("artifacts"),
        }
    }

    pub fn init(&self) -> Result<(), StoreError> {
        fs::create_dir_all(&self.objects_dir)?;
        fs::create_dir_all(&self.artifacts_dir)?;
        Ok(())
    }

    pub fn write_bundle(&self, hash: &str, bundle: &CrpBundle) -> Result<PathBuf, StoreError> {
        let path = self.bundle_path(hash);
        let json = serde_json::to_string_pretty(bundle)?;
        fs::write(&path, json)?;
        Ok(path)
    }

    pub fn read_bundle(&self, hash: &str) -> Result<CrpBundle, StoreError> {
        let path = self.bundle_path(hash);
        let json = fs::read_to_string(&path)?;
        let bundle: CrpBundle = serde_json::from_str(&json)?;
        Ok(bundle)
    }

    pub fn read_bundle_bytes(&self, hash: &str) -> Result<Vec<u8>, StoreError> {
        Ok(fs::read(self.bundle_path(hash))?)
    }

    pub fn list_bundles(&self) -> Result<Vec<String>, StoreError> {
        let mut hashes = Vec::new();
        if !self.objects_dir.exists() {
            return Ok(hashes);
        }
        for entry in fs::read_dir(&self.objects_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(hash) = name.strip_suffix(".json") {
                hashes.push(hash.to_string());
            }
        }
        hashes.sort();
        Ok(hashes)
    }

    pub fn has_bundle(&self, hash: &str) -> bool {
        self.bundle_path(hash).exists()
    }

    pub fn write_artifact(&self, hash: &str, bytes: &[u8]) -> Result<PathBuf, StoreError> {
        let path = self.artifact_path(hash);
        fs::write(&path, bytes)?;
        Ok(path)
    }

    pub fn read_artifact(&self, hash: &str) -> Result<Vec<u8>, StoreError> {
        let path = self.artifact_path(hash);
        Ok(fs::read(path)?)
    }

    pub fn has_artifact(&self, hash: &str) -> bool {
        self.artifact_path(hash).exists()
    }
}
