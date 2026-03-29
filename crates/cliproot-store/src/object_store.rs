use std::fs;
use std::path::{Path, PathBuf};

use cliproot_core::CrpBundle;

use crate::error::StoreError;

pub struct ObjectStore {
    objects_dir: PathBuf,
    artifacts_dir: PathBuf,
}

impl ObjectStore {
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
        let filename = format!("{hash}.json");
        let path = self.objects_dir.join(filename);
        let json = serde_json::to_string_pretty(bundle)?;
        fs::write(&path, json)?;
        Ok(path)
    }

    pub fn read_bundle(&self, hash: &str) -> Result<CrpBundle, StoreError> {
        let filename = format!("{hash}.json");
        let path = self.objects_dir.join(filename);
        let json = fs::read_to_string(&path)?;
        let bundle: CrpBundle = serde_json::from_str(&json)?;
        Ok(bundle)
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
        self.objects_dir.join(format!("{hash}.json")).exists()
    }

    pub fn write_artifact(&self, hash: &str, bytes: &[u8]) -> Result<PathBuf, StoreError> {
        let path = self.artifacts_dir.join(hash);
        fs::write(&path, bytes)?;
        Ok(path)
    }

    pub fn read_artifact(&self, hash: &str) -> Result<Vec<u8>, StoreError> {
        let path = self.artifacts_dir.join(hash);
        Ok(fs::read(path)?)
    }

    pub fn has_artifact(&self, hash: &str) -> bool {
        self.artifacts_dir.join(hash).exists()
    }
}
