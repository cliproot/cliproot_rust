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
├── rust-toolchain.toml     # pins Rust 1.88
├── .github/workflows/
│   ├── ci.yml              # fmt + clippy + test on push/PR
│   └── release.yml         # multi-platform binary release on tag
├── skills/
│   └── cliproot-research/  # Agent Skills package (embedded into binary)
│       ├── SKILL.md        # core research workflow (agentskills.io format)
│       ├── references/     # tool API docs + workflow examples
│       ├── scripts/        # verify-provenance.sh helper
│       └── agents/         # openai.yaml for Codex
├── crates/
│   ├── cliproot-core/      # protocol model, hashing, verification
│   ├── cliproot-store/     # hybrid storage: files + SQLite index
│   ├── cliproot-cli/       # clap-based CLI binary (cliproot), includes MCP server
│   └── cliproot-mcp/       # MCP server library + standalone binary
└── crates/cliproot-store/tests/
    └── roundtrip.rs        # integration tests
```

**Dependency graph**: `cli → mcp → store → core`

## Install

### From source (Rust)

Requires Rust 1.88+ (pinned via `rust-toolchain.toml`). No system SQLite needed — it's bundled.

```bash
cargo install --path crates/cliproot-cli
```

This installs a single `cliproot` binary that includes both the CLI and the MCP server.

### From GitHub Releases

Pre-built binaries are available for each tagged release:

| Platform | Archive |
|----------|---------|
| Linux x86_64 | `cliproot-x86_64-unknown-linux-gnu.tar.gz` |
| macOS x86_64 | `cliproot-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `cliproot-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `cliproot-x86_64-pc-windows-msvc.zip` |

Download from [Releases](https://github.com/cliproot/cliproot_rust/releases), extract, and add to your `PATH`.

## Build

```bash
cd cliproot_rust

# Check all crates compile
cargo check --workspace

# Build the CLI binary (includes MCP server)
cargo build -p cliproot-cli --release
```

The binary lands at `target/release/cliproot`.

## Code Quality

These match the checks run in CI (`ci.yml`) on every push and PR.

```bash
# Auto-fix formatting (run this before committing)
cargo fmt --all

# Check formatting without modifying files (what CI runs)
cargo fmt --all -- --check

# Lint — must pass with zero warnings
cargo clippy --workspace -- -D warnings
```

> If `cargo fmt --all -- --check` fails in CI, run `cargo fmt --all` locally, commit the result, and push.

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

## MCP Server

The MCP server is bundled into the `cliproot` binary as the `mcp` subcommand. It exposes Cliproot provenance operations as typed tools for AI agents (Claude Code, Cline, etc.) over stdin/stdout using JSON-RPC 2.0.

```bash
# Start the MCP server (discovers .cliproot/ from CWD)
cliproot mcp

# Or specify a repo path explicitly
cliproot mcp --path /path/to/project
```

The standalone `cliproot-mcp` binary is also still available for backward compatibility.

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
| `cliproot_annotate` | Annotate a document with inline citations by matching text against stored clips |
| `cliproot_cite` | Generate a bibliography/citation list for a document from clip provenance |
| `cliproot_doctor` | Generate a provenance coverage report showing which paragraphs have source provenance |

### Agent Skills

Cliproot ships a ready-made **[Agent Skills](https://agentskills.io)** package that teaches AI agents how to use the MCP tools effectively for provenance-tracked research. The skill is compatible with Claude Code, Cursor, VS Code/Copilot, OpenAI Codex, Windsurf, Gemini CLI, and any other Agent Skills-compliant tool.

Generate all platform configs in one command:

```bash
cliproot init --agent
```

This creates:

| Platform | Files generated |
|----------|----------------|
| **Claude Code** | `.mcp.json`, `.claude/skills/cliproot-research/` |
| **Cursor** | `.cursor/mcp.json`, `.cursor/rules/cliproot-research.mdc` |
| **VS Code / Copilot** | `.vscode/mcp.json` |
| **Universal (Codex, Gemini CLI, etc.)** | `.agents/skills/cliproot-research/` |
| **Windsurf** | `.windsurf/rules/cliproot-research.md` |

The skill teaches agents to follow the **CAPTURE → SYNTHESIZE → VALIDATE → OUTPUT** workflow: clip sources, derive insights, verify integrity, and produce cited documents with traceable lineage.

Skill source files live in [`skills/cliproot-research/`](skills/cliproot-research/) and are embedded in the binary at build time.

---

### Register with Claude Code

```bash
# Scoped to the current project (recommended)
claude mcp add cliproot -- cliproot mcp --path /path/to/project

# Using CLIPROOT_REPO environment variable instead of --path
claude mcp add cliproot -e CLIPROOT_REPO=/path/to/project -- cliproot mcp
```

The server discovers the `.cliproot/` repository by walking up from the `--path` argument (or `CLIPROOT_REPO`). If neither is provided it walks up from the working directory at startup.

### Verify the server is running

```bash
claude mcp list
# cliproot: /path/to/cliproot-mcp ... ✓ Connected
```

### Available MCP resources

The server also exposes clip data as MCP resources for context injection. AI clients can read these directly into their context window without explicit tool calls.

**Static resource:**

| URI | Description |
|-----|-------------|
| `cliproot://clips` | Summary list of all clips in the repository (up to 200, with content previews) |

**Resource templates (parameterized):**

| URI Template | Description |
|-------------|-------------|
| `cliproot://clips/{hash_or_id}` | Full details of a single clip by hash or ID |
| `cliproot://lineage/{hash_or_id}` | Derivation lineage trace for a clip |
| `cliproot://bundles/{hash_or_id}` | Full CRP bundle export for a clip and its lineage |

All resources return `application/json`.

### Manual smoke test

```bash
REPO=/path/to/project   # directory containing .cliproot/

# Initialize request
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}' \
  | cliproot mcp --path "$REPO"

# List available tools
(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}\n'; \
 sleep 0.2; printf '{"jsonrpc":"2.0","method":"notifications/initialized"}\n'; \
 sleep 0.2; printf '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}\n'; \
 sleep 1) \
  | cliproot mcp --path "$REPO"

# List available resources and templates
(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}\n'; \
 sleep 0.2; printf '{"jsonrpc":"2.0","method":"notifications/initialized"}\n'; \
 sleep 0.2; printf '{"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}\n'; \
 sleep 0.2; printf '{"jsonrpc":"2.0","id":3,"method":"resources/templates/list","params":{}}\n'; \
 sleep 1) \
  | cliproot mcp --path "$REPO"

# Read the clip inventory resource
(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}\n'; \
 sleep 0.2; printf '{"jsonrpc":"2.0","method":"notifications/initialized"}\n'; \
 sleep 0.2; printf '{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"cliproot://clips"}}\n'; \
 sleep 1) \
  | cliproot mcp --path "$REPO"
```

---

## CLI Usage

All commands accept `--format <text|json|table>` (default: `text`).

### Initialize a repository (CLI)

```bash
mkdir my-project && cd my-project
cliproot init
# → creates .cliproot/ in the current directory

# Also generate agent/IDE config files (MCP configs, skills, rules)
cliproot init --agent
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

### Annotate a document with inline citations

Match document text against stored clips and insert citation markers.

```bash
# Print annotated document to stdout (footnote style by default)
cliproot annotate report.md

# Choose annotation style
cliproot annotate report.md --style footnote          # [1] markers + Sources section
cliproot annotate report.md --style inline-comment    # <!-- [cliproot:sha256-...] --> comments
cliproot annotate report.md --style bracket           # [cliproot:sha256-...] inline

# Edit the file in place
cliproot annotate report.md --in-place

# Adjust match sensitivity (default: 0.4)
cliproot annotate report.md --threshold 0.6

# JSON output — full AnnotateResult with annotated_text and citations array
cliproot annotate report.md --format json
```

### Generate a citation list

Produce a numbered bibliography matching document text against stored clips.

```bash
cliproot cite report.md
# → 1. [Title] <url> (sha256-...)

cliproot cite report.md --threshold 0.5
cliproot cite report.md --format json
```

### Provenance coverage report

Audit which paragraphs in a document have source provenance and which are missing it.

```bash
cliproot doctor report.md
# → ✓ [0] Redis uses a single-threaded event loop...
# → ✗ [1] This paragraph has no provenance coverage.

cliproot doctor report.md --threshold 0.5
cliproot doctor report.md --format json
```

Coverage statuses: `covered` (strong match), `partial` (weak match), `uncovered` (no match found).

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

## CI/CD

GitHub Actions run automatically:

- **CI** (`ci.yml`) — on every push to `main` and all PRs:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo test --workspace`

- **Release** (`release.yml`) — on tag push matching `v*` (e.g. `v0.1.0`):
  - Builds release binaries for Linux, macOS (x86 + ARM), and Windows
  - Creates a GitHub Release with attached archives

### Creating a release

```bash
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds all platform binaries and publishes them to [GitHub Releases](https://github.com/cliproot/cliproot_rust/releases).

## Protocol version

Implements **CRP v0.0.2**. Key features of this version:
- `derivationEdges` are first-class top-level objects (not embedded in clips)
- Optional `selectors` on clips (textPosition, textQuote, dom, mediaTime)
- Bundle types: `document`, `clipboard`, `reuse-event`, `derivation`, `provenance-export`
