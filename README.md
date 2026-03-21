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
│   └── cliproot-mcp/       # stub — Phase 2 MCP server
└── crates/cliproot-store/tests/
    └── roundtrip.rs        # integration tests
```

**Dependency graph**: `cli → store → core`, `mcp → store → core`

## Requirements

- Rust 1.82+ (stable)
- No system SQLite needed — `rusqlite` bundles SQLite via the `bundled` feature

## Build

```bash
cd cliproot_rust

# Check all crates compile
cargo check --workspace

# Build the CLI binary
cargo build -p cliproot-cli

# Build release binary
cargo build -p cliproot-cli --release
```

The binary is at `target/debug/cliproot` (or `target/release/cliproot`).

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

## Usage

All commands accept `--format <text|json|table>` (default: `text`).

### Initialize a repository

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
