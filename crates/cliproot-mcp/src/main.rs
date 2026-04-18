//! cliproot-mcp: stdio MCP server that exposes Cliproot provenance operations
//! as typed MCP tools for AI agents (Claude Code, Cline, etc.).
//!
//! Usage:
//!   cliproot-mcp                   # discovers .cliproot/ from CWD upward
//!   cliproot-mcp --path /some/dir  # opens .cliproot/ in specified dir
//!
//! Environment:
//!   CLIPROOT_REPO=/some/dir        # alternative to --path
//!   CLAUDE_PROJECT_DIR=/some/dir   # fallback path when set by Claude Code

use std::path::PathBuf;

fn parse_path() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if (args[i] == "--path" || args[i] == "-p") && i + 1 < args.len() {
            return Some(PathBuf::from(&args[i + 1]));
        }
        i += 1;
    }
    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = parse_path();
    cliproot_mcp::run_server(path.as_deref()).await
}
