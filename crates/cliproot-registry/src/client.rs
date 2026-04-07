use std::path::{Path, PathBuf};

use crate::error::RegistryError;
use crate::types::{
    ApiErrorResponse, PublishResult, ProjectSummary, RegistryIndexConfig, SearchResponse,
};

pub struct RegistryClient {
    http: reqwest::blocking::Client,
    base_url: String,
    config: RegistryIndexConfig,
    token: Option<String>,
}

impl RegistryClient {
    /// Create a new registry client by fetching the index config from `base_url`.
    pub fn new(base_url: &str) -> Result<Self, RegistryError> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("cliproot/", env!("CARGO_PKG_VERSION")))
            .build()?;

        let config_url = format!("{base_url}/v1/index/config.json");
        let resp = http.get(&config_url).send()?;
        let resp = Self::check_response_static(resp)?;
        let config: RegistryIndexConfig = resp.json()?;

        Ok(Self {
            http,
            base_url,
            config,
            token: None,
        })
    }

    /// Access the registry's index configuration.
    pub fn config(&self) -> &RegistryIndexConfig {
        &self.config
    }

    /// Set a bearer token for authenticated requests.
    pub fn with_token(mut self, token: String) -> Self {
        self.token = Some(token);
        self
    }

    /// Push a `.cliprootpack` archive to the registry.
    pub fn push_pack(&self, pack_path: &Path) -> Result<PublishResult, RegistryError> {
        let data = std::fs::read(pack_path)?;
        let url = format!("{}/packs", self.config.api);

        let mut req = self
            .http
            .post(&url)
            .header("Content-Type", "application/x-cliprootpack")
            .body(data);

        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }

        let resp = req.send()?;
        let resp = self.check_response(resp)?;
        Ok(resp.json()?)
    }

    /// Fetch the latest pack for a project from the registry and write it to a temp file.
    /// Returns the path to the downloaded `.cliprootpack` file.
    pub fn pull_pack(&self, owner: &str, project: &str) -> Result<PathBuf, RegistryError> {
        // First fetch project metadata to get the latest pack hash.
        let index_url = format!(
            "{}/v1/index/projects/{}/{}",
            self.base_url, owner, project
        );
        let resp = self.http.get(&index_url).send()?;
        let resp = self.check_response(resp)?;
        let summary: ProjectSummary = resp.json()?;

        let pack_hash = summary.latest_pack_hash.ok_or_else(|| {
            RegistryError::InvalidRegistry(format!(
                "project {owner}/{project} has no published packs"
            ))
        })?;

        // Download the pack.
        let download_url = format!(
            "{}/packs/{}.cliprootpack",
            self.config.download, pack_hash
        );
        let resp = self.http.get(&download_url).send()?;
        let resp = self.check_response(resp)?;

        let tmp = tempfile::Builder::new()
            .suffix(".cliprootpack")
            .tempfile()?;
        let pack_path = tmp.path().to_path_buf();
        std::fs::write(&pack_path, resp.bytes()?)?;
        // Keep the temp file alive by leaking it — caller is responsible for cleanup.
        tmp.into_temp_path();

        Ok(pack_path)
    }

    /// Search clips on the remote registry.
    pub fn search(
        &self,
        query: &str,
        owner: Option<&str>,
        project: Option<&str>,
        limit: Option<u32>,
    ) -> Result<SearchResponse, RegistryError> {
        let mut url = format!("{}/search?q={}", self.config.api, urlencod(query));
        if let Some(o) = owner {
            url.push_str(&format!("&owner={}", urlencod(o)));
        }
        if let Some(p) = project {
            url.push_str(&format!("&project={}", urlencod(p)));
        }
        if let Some(l) = limit {
            url.push_str(&format!("&limit={l}"));
        }

        let resp = self.http.get(&url).send()?;
        let resp = self.check_response(resp)?;
        Ok(resp.json()?)
    }

    fn check_response(
        &self,
        resp: reqwest::blocking::Response,
    ) -> Result<reqwest::blocking::Response, RegistryError> {
        Self::check_response_static(resp)
    }

    fn check_response_static(
        resp: reqwest::blocking::Response,
    ) -> Result<reqwest::blocking::Response, RegistryError> {
        if resp.status().is_success() {
            return Ok(resp);
        }
        // Try to parse the registry's JSON error envelope.
        let status = resp.status();
        match resp.json::<ApiErrorResponse>() {
            Ok(api_err) => Err(RegistryError::Api {
                code: api_err.error.code,
                message: api_err.error.message,
            }),
            Err(_) => Err(RegistryError::InvalidRegistry(format!(
                "unexpected status {status} from registry"
            ))),
        }
    }
}

/// Minimal percent-encoding for query parameters.
fn urlencod(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
        .replace('#', "%23")
}
