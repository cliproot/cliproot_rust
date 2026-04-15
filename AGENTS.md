# ClipRoot Rust — Agent Guide

This is a Rust workspace implementing the **ClipRoot Protocol (CRP) v0.0.3** — a provenance-tracking system for content reuse. It provides a CLI, MCP server, and storage engine for the protocol.

## Workspace Structure

```
cliproot_rust/
├── Cargo.toml                 # Workspace root (resolver = "2", Rust 1.88)
├── rust-toolchain.toml        # Pinned Rust 1.88.0
├── skills/                    # Agent Skills packages (built into binary)
│   ├── cliproot-capture/
│   └── cliproot-session/
└── crates/
    ├── cliproot-core/         # Protocol models, hashing, verification
    ├── cliproot-store/        # Hybrid storage (files + SQLite index), pack format
    ├── cliproot-registry/     # Registry client, device flow auth, credentials
    ├── cliproot-mcp/          # MCP server library + standalone binary
    ├── cliproot-clipboard/    # OS clipboard integration
    └── cliproot-cli/          # Main `cliproot` binary (CLI + embedded MCP server)
```

**Dependency graph**: `cli → { mcp, registry } → store → core`

## Key Technologies

- **Rust 1.88** — Fixed toolchain via `rust-toolchain.toml`
- **SQLite** — Bundled via `libsqlite3-sys` (no system dependency)
- **cli** — `clap` with derive macros
- **MCP** — JSON-RPC over stdio via `rmcp` crate
- **Protocol** — Content-addressed clips/artifacts via SHA-256

## Building

```bash
cargo check --workspace              # Verify compilation
cargo build -p cliproot-cli --release  # Build main binary → target/release/cliproot
cargo test --workspace               # Run all tests
cargo fmt --all && cargo clippy --workspace -- -D warnings  # CI checks
```

## Crate Overview

### `cliproot-core`
Foundation types and protocol logic.
- `model.rs` — Core CRP types (Clip, Artifact, Bundle, Project, etc.)
- `hash.rs` — `sha256-*` content hashing, normalization
- `verify.rs` — Hash integrity verification
- `matching.rs` — Fuzzy text matching for annotate/cite/doctor features

### `cliproot-store`
Persistence layer + pack format.
- `repository.rs` — Main `Repository` struct for CRUD operations
- `index_db.rs` — SQLite schema and queries
- `object_store.rs` — File-backed object storage
- `pack.rs` — `.cliprootpack` (tar.zst) archive creation

### `cliproot-mcp`
MCP server exposing protocol ops as tools.
- `service.rs` — 24+ MCP tools (clip, derive, artifact, pack, etc.)
- `params.rs` — Tool parameter types (ref-cast pattern)
- `repo_handle.rs` — Async handle wrapper around sync Repository

### `cliproot-cli`
Main binary with 20+ subcommands.
- `main.rs` — Command routing, top-level CLI
- `commands/` — Individual command implementations (clip, derive, artifact, pack, etc.)
- `transcript/` — Claude Code session parsing and design record reconstruction

### `cliproot-registry`
Remote registry integration.
- `client.rs` — HTTP client for registry operations
- `auth.rs` — OAuth 2.0 device flow
- `credential.rs` — Keychain/fallback credential storage

## Common Tasks

**Add a new CLI command:**
1. Add mod to `crates/cliproot-cli/src/commands/mod.rs`
2. Create `crates/cliproot-cli/src/commands/mycmd.rs`
3. Register in `crates/cliproot-cli/src/main.rs` CLI commands

**Add a new MCP tool:**
1. Add parameter type to `crates/cliproot-mcp/src/params.rs`
2. Add handler to `impl ToolHandler for ClipRootService` in `crates/cliproot-mcp/src/service.rs`

**Extend protocol models:**
1. Add to `crates/cliproot-core/src/model.rs`
2. Update SQLite schema in `crates/cliproot-store/src/index_db.rs` if persistence needed
3. Add serialization tests for new fields

## DataModel Summary

- **Clip** — Indexed content with source URL/location and optional parent clips (`wasDerivedFrom`)
- **Artifact** — Content-addressed file (markdown, JSON, prompts, etc.)
- **Project** — Namespace/scope for clips and artifacts
- **Activity** — Context for a prompt/scoped operation (may include parameters)
- **Session** — Agent session tracking with start/end timestamps
- **Edge** — Generic relationships: `wasDerivedFrom`, `cited_in`, `response_to`, `corrected_in`
- **Pack** — Portable archive format (tar.zst) for sharing/exporting

## Repository Layout on Disk

```
.cliproot/
├── config.json              # protocolVersion, currentProjectId
├── index.db                 # SQLite index
├── artifacts/               # raw bytes keyed by sha256-*
├── objects/                 # CRP JSON bundles
├── agent-log/               # PostToolUse hook capture logs
└── records/                 # Human-readable design records
```

## Testing

- Unit tests in each crate's `src/` files
- Integration tests in `crates/cliproot-store/tests/roundtrip.rs`
- Run CI checks: `cargo fmt --all -- --check && cargo clippy --workspace -- -D warnings && cargo test --workspace`

## Related Repositories

- [`cliproot/cliproot`](https://github.com/cliproot/cliproot) — Protocol spec, JSON Schema, TypeScript SDK, browser extension. Usually checked out under cliproot (adjacent folder)
- This repo — Native Rust implementation only

Schema changes should start in the canonical repo; implementations follow schema updates.
