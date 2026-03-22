//! cliproot-mcp: stdio MCP server that exposes Cliproot provenance operations
//! as typed MCP tools for AI agents (Claude Code, Cline, etc.).
//!
//! Usage:
//!   cliproot-mcp                   # discovers .cliproot/ from CWD upward
//!   cliproot-mcp --path /some/dir  # opens .cliproot/ in specified dir
//!
//! Environment:
//!   CLIPROOT_REPO=/some/dir        # alternative to --path

use cliproot_store::Repository;
use rmcp::{transport::io::stdio, ServiceExt};

mod params;
mod repo_handle;
mod service;

use repo_handle::RepoHandle;
use service::ClipRootService;

fn parse_path() -> Option<std::path::PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if (args[i] == "--path" || args[i] == "-p") && i + 1 < args.len() {
            return Some(std::path::PathBuf::from(&args[i + 1]));
        }
        i += 1;
    }
    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repo = if let Some(p) = parse_path() {
        Repository::open(&p)?
    } else if let Ok(env_path) = std::env::var("CLIPROOT_REPO") {
        Repository::open(std::path::Path::new(&env_path))?
    } else {
        Repository::discover()?
    };

    let handle = RepoHandle::spawn(repo);
    let service = ClipRootService::new(handle);
    let server = service.serve(stdio()).await?;
    server.waiting().await?;

    Ok(())
}
