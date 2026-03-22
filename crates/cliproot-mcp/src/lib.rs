// cliproot-mcp: MCP server for Cliproot provenance operations.
pub mod params;
pub mod repo_handle;
pub mod service;

use std::path::Path;

use cliproot_store::Repository;
use rmcp::{transport::io::stdio, ServiceExt};

use repo_handle::RepoHandle;
use service::ClipRootService;

/// Start the MCP stdio server.
///
/// Repository is resolved in order: explicit `path` argument, `CLIPROOT_REPO`
/// env var, or walking up from the current working directory.
pub async fn run_server(path: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let repo = if let Some(p) = path {
        Repository::open(p)?
    } else if let Ok(env_path) = std::env::var("CLIPROOT_REPO") {
        Repository::open(Path::new(&env_path))?
    } else {
        Repository::discover()?
    };

    let handle = RepoHandle::spawn(repo);
    let service = ClipRootService::new(handle);
    let server = service.serve(stdio()).await?;
    server.waiting().await?;

    Ok(())
}
