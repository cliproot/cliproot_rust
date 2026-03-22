# cliproot-rust

Rust implementation of the [ClipRoot Protocol (CRP)](../cliproot/schema/crp-v0.0.2.schema.json) — a local-first provenance engine for content-addressed clips with derivation lineage.

## Overview

`cliproot` is a CLI for managing **clips**: content-addressed provenance records that link quoted text back to its source, and track how content was derived or transformed. Every clip gets a stable `sha256-*` hash that can be verified offline without a registry.

```
clip (text + source) ──[summary]──► derived clip ──[paraphrase]──► another clip
  └─ sha256-abc...                    └─ sha256-def...                └─ sha256-xyz...
```

## Workspace Structure

```
cliproot_rust/
├── Cargo.toml              # workspace root
├── crates/
│   ├── cliproot-core/      # protocol model, hashing, verification
│   ├── cliproot-store/     # hybrid storage: files + SQLite index
│   ├── cliproot-cli/       # clap-based CLI binary (cliproot)
│   └── cliproot-mcp/       # stdio MCP server (cliproot-mcp)
└── crates/cliproot-store/tests/
    └── roundtrip.rs        # integration tests
```

**Dependency graph**: `cli → store → core`, `mcp → store → core`

## Requirements

- Rust 1.85+ (stable) — required by `rmcp` 1.2 (Rust 2024 edition)
- No system SQLite needed — `rusqlite` bundles SQLite via the `bundled` feature

## Build

```bash
cd cliproot_rust

# Check all crates compile
cargo check --workspace

# Build the CLI binary
cargo build -p cliproot-cli --release

# Build the MCP server binary
cargo build -p cliproot-mcp --release
```

Binaries land in `target/release/`:
- `target/release/cliproot` — CLI
- `target/release/cliproot-mcp` — MCP server

## Test

```bash
# Run all tests
cargo test --workspace

# Run only core hash/verify tests
cargo test -p cliproot-core

# Run only store tests (includes integration roundtrip)
cargo test -p cliproot-store

# Run with output visible
cargo test --workspace -- --nocapture
```

## MCP Server (`cliproot-mcp`)

`cliproot-mcp` is a stdio MCP server that exposes Cliproot provenance operations as typed tools for AI agents (Claude Code, Cline, etc.). The MCP client spawns the process and communicates over stdin/stdout using JSON-RPC 2.0.

### Available MCP tools

| Tool | Description |
|------|-------------|
| `cliproot_clip` | Capture a source clip from a URL with exact quoted text |
| `cliproot_derive` | Derive a new clip from one or more parent clips |
| `cliproot_inspect` | Inspect a clip by hash or ID |
| `cliproot_trace` | Show full ancestor lineage through derivation edges |
| `cliproot_verify` | Verify hash integrity of one clip or all clips |
| `cliproot_list` | List clips with optional filtering |
| `cliproot_search` | Search clip content by substring |
| `cliproot_export` | Export a clip and its full provenance lineage as a CRP bundle |

### Register with Claude Code

```bash
# Scoped to the current project (recommended)
claude mcp add cliproot -- /path/to/target/release/cliproot-mcp --path /path/to/project

# Using CLIPROOT_REPO environment variable instead of --path
claude mcp add cliproot -e CLIPROOT_REPO=/path/to/project -- /path/to/target/release/cliproot-mcp
```

The server discovers the `.cliproot/` repository by walking up from the `--path` argument (or `CLIPROOT_REPO`). If neither is provided it walks up from the working directory at startup.

### Verify the server is running

```bash
claude mcp list
# cliproot: /path/to/cliproot-mcp ... ✓ Connected
```

### Manual smoke test

```bash
BINARY=./target/release/cliproot-mcp
REPO=/path/to/project   # directory containing .cliproot/

# Initialize request
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}' \
  | "$BINARY" --path "$REPO"

# List available tools
(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}\n'; \
 sleep 0.2; \
 printf '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}\n'; \
 sleep 1) \
  | "$BINARY" --path "$REPO"
```

---

## CLI Usage

All commands accept `--format <text|json|table>` (default: `text`).

### Initialize a repository (CLI)

```bash
mkdir my-project && cd my-project
cliproot init
# → creates .cliproot/ in the current directory
```

### Create a clip from a URL

```bash
cliproot clip --url https://example.com/article --quote "The key insight is provenance."
# → prints the clip hash, text hash, and content preview
```

Options: `--source-type`, `--id`, `--document-id`, `--title`

### Derive a clip from a parent

```bash
cliproot derive \
  --from sha256-abc123... \
  --quote "Summary: provenance is key." \
  --activity-type summary
```

Supported activity types: `verbatim`, `quote`, `summary`, `paraphrase`, `translate`, `combine`, `edit`, `ai_generate`, `unknown`

### Inspect a clip

```bash
cliproot inspect sha256-abc123...
cliproot inspect my-clip-id
```

### List clips

```bash
cliproot list
cliproot list --limit 20
cliproot list --document doc_01
cliproot list --format table
```

### Trace lineage

```bash
cliproot trace sha256-def456...
# → prints ancestor chain with transformation types
```

### Verify integrity

```bash
# Verify all clips in the repository
cliproot verify

# Verify a single clip
cliproot verify sha256-abc123...
```

### Export a clip + lineage as a CRP bundle

```bash
cliproot export sha256-abc123... -o bundle.json
# or pipe to stdout
cliproot export sha256-abc123...
```

### Ingest a CRP bundle

```bash
cliproot ingest bundle.json
```

### Help

```bash
cliproot --help
cliproot <command> --help
cliproot help <command>
```

## Repository layout on disk

```
.cliproot/
├── config.json          # { "protocolVersion": "0.0.2" }
├── index.db             # SQLite — fast lookups by hash/id/document
└── objects/
    └── sha256-{hash}.json   # one bundle file per stored bundle
```

Clips are content-addressed: the same text from the same source always produces the same `clipHash`, regardless of when or where it was created.

## Protocol version

Implements **CRP v0.0.2**. Key features of this version:
- `derivationEdges` are first-class top-level objects (not embedded in clips)
- Optional `selectors` on clips (textPosition, textQuote, dom, mediaTime)
- Bundle types: `document`, `clipboard`, `reuse-event`, `derivation`, `provenance-export`
