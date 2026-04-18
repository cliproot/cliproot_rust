// cliproot-mcp: MCP server for Cliproot provenance operations.
pub mod params;
pub mod repo_handle;
pub mod service;

use std::path::{Path, PathBuf};

use cliproot_store::{Repository, StoreError};
use rmcp::{transport::io::stdio, ServiceExt};

use repo_handle::RepoHandle;
use service::ClipRootService;

/// Resolve the repository path from (in order): explicit arg, `CLIPROOT_REPO`,
/// `CLAUDE_PROJECT_DIR` (when it contains a `.cliproot/`), then CWD discovery.
fn resolve_repo(path: Option<&Path>) -> Result<Repository, StoreError> {
    if let Some(p) = path {
        eprintln!(
            "[cliproot-mcp] opening repository from --path: {}",
            p.display()
        );
        return Repository::open(p);
    }
    if let Ok(env_path) = std::env::var("CLIPROOT_REPO") {
        if !env_path.is_empty() {
            eprintln!("[cliproot-mcp] opening repository from CLIPROOT_REPO: {env_path}");
            return Repository::open(Path::new(&env_path));
        }
    }
    if let Ok(project_dir) = std::env::var("CLAUDE_PROJECT_DIR") {
        if !project_dir.is_empty() {
            let p = PathBuf::from(&project_dir);
            if p.join(".cliproot").exists() {
                eprintln!(
                    "[cliproot-mcp] opening repository from CLAUDE_PROJECT_DIR: {project_dir}"
                );
                return Repository::open(&p);
            }
        }
    }
    let cwd = std::env::current_dir().ok();
    eprintln!(
        "[cliproot-mcp] discovering repository from CWD: {}",
        cwd.as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".to_string())
    );
    Repository::discover()
}

/// Start the MCP stdio server.
///
/// If the repository cannot be resolved the server still starts and answers
/// `initialize`; every tool call will return the resolution error. This keeps
/// the MCP handshake from racing a process exit when Claude Code launches the
/// server outside a cliproot workspace.
pub async fn run_server(path: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!(
        "[cliproot-mcp] starting v{} (pid={})",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );

    let repo_result = resolve_repo(path);
    match &repo_result {
        Ok(repo) => {
            eprintln!(
                "[cliproot-mcp] repository open at {}",
                repo.root().display()
            );
        }
        Err(e) => {
            eprintln!("[cliproot-mcp] repository unavailable: {e}");
            eprintln!(
                "[cliproot-mcp] server will answer initialize; tool calls will error until a repo is configured (set CLIPROOT_REPO or pass --path)"
            );
        }
    }

    let handle = RepoHandle::spawn(repo_result);
    let service = ClipRootService::new(handle);

    eprintln!("[cliproot-mcp] stdio transport ready; awaiting client");
    let server = service.serve(stdio()).await?;
    server.waiting().await?;

    eprintln!("[cliproot-mcp] server stopped");
    Ok(())
}
