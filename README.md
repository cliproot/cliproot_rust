# cliproot-rust

The native runtime for the [ClipRoot Protocol (CRP)](https://github.com/cliproot/cliproot) — a CLI, MCP server, and storage engine for provenance-aware content reuse.

## Overview

`cliproot` is a single binary that provides a CLI (20+ subcommands), an MCP server (24+ tools for AI agents), hybrid storage (filesystem objects + SQLite index), OS clipboard integration, `.cliprootpack` archive support, and registry authentication (OAuth 2.0 device flow + keychain credential storage). It implements [CRP v0.0.3](https://github.com/cliproot/cliproot/tree/main/spec) — the same protocol defined by the canonical [JSON Schema](https://github.com/cliproot/cliproot/tree/main/schema) and [TypeScript packages](https://github.com/cliproot/cliproot/tree/main/packages) in the main ClipRoot repository.

Clips are content-addressed provenance records that link quoted text back to its source and track how content was derived or transformed. Artifacts are content-addressed files such as markdown plans, prompts, or JSON notes. Every clip and artifact gets a stable `sha256-*` hash that can be verified offline without a registry.

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
├── skills/                 # Agent Skills source of truth (embedded into binary)
│   ├── cliproot-capture/   # lightweight capture workflow
│   │   ├── SKILL.md        # agentskills.io format
│   │   └── agents/         # openai.yaml for Codex
│   └── cliproot-session/   # full-ceremony session workflow
│       ├── SKILL.md
│       ├── agents/
│       └── references/     # tool API docs + workflow examples
├── justfile                # developer tasks (e.g. `just sync-skills`)
├── crates/
│   ├── cliproot-core/      # protocol model, hashing, verification
│   ├── cliproot-store/     # hybrid storage: files + SQLite index
│   ├── cliproot-registry/  # registry client, device flow auth, credential storage
│   ├── cliproot-cli/       # clap-based CLI binary (cliproot), includes MCP server
│   └── cliproot-mcp/       # MCP server library + standalone binary
└── crates/cliproot-store/tests/
    └── roundtrip.rs        # integration tests
```

**Dependency graph**: `cli → { mcp, registry } → store → core`

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
| `cliproot_project_create/list/use/delete` | Manage project scopes |
| `cliproot_artifact_add/list/get/link` | Store and retrieve artifacts |
| `cliproot_pack_create/import/inspect/verify` | Create and restore `.cliprootpack` archives |
| `cliproot_activity_start/end` | Track prompt-scoped activities |
| `cliproot_session_start/end` | Track agent sessions and finalize session artifacts |
| `cliproot_inspect` | Inspect a clip by hash or ID |
| `cliproot_trace` | Show full ancestor lineage through `wasDerivedFrom` edges |
| `cliproot_verify` | Verify hash integrity of one clip or all clips |
| `cliproot_list` | List clips with optional filtering |
| `cliproot_search` | Search clip content by substring |
| `cliproot_export` | Export a clip and its full provenance lineage as a CRP bundle |
| `cliproot_annotate` | Annotate a document with inline citations by matching text against stored clips |
| `cliproot_cite` | Generate a bibliography/citation list for a document from clip provenance |
| `cliproot_doctor` | Generate a provenance coverage report showing which paragraphs have source provenance |

### Agent Skills

Cliproot ships two **[Agent Skills](https://agentskills.io)** packages that teach AI agents how to use the MCP tools effectively for provenance-tracked research. Both skills are compatible with Claude Code, Cursor, VS Code/Copilot, OpenAI Codex, Windsurf, Gemini CLI, and any other Agent Skills-compliant tool.

| Skill | Description |
|-------|-------------|
| **`cliproot-capture`** | Lightweight provenance capture — clip sources and derive syntheses during research. Low ceremony, suitable for any session. |
| **`cliproot-session`** | Full-ceremony session tracking — scoped projects, activity management, and validated session artifacts. Use for extended or shared research. |

Generate all platform configs in one command:

```bash
cliproot init --agent
```

This creates:

| Platform | Files generated |
|----------|----------------|
| **Claude Code** | `.mcp.json`, `.claude/skills/cliproot-capture/`, `.claude/skills/cliproot-session/` |
| **Cursor** | `.cursor/mcp.json`, `.cursor/rules/cliproot-capture.mdc`, `.cursor/rules/cliproot-session.mdc` |
| **VS Code / Copilot** | `.vscode/mcp.json` |
| **Universal (Codex, Gemini CLI, etc.)** | `.agents/skills/cliproot-capture/`, `.agents/skills/cliproot-session/` |
| **Windsurf** | `.windsurf/rules/cliproot-capture.md`, `.windsurf/rules/cliproot-session.md` |

`cliproot-capture` teaches a **SCOPE → CAPTURE → SYNTHESIZE → CITE** workflow. `cliproot-session` adds **PROJECT → SESSION → ACTIVITY** lifecycle management around it.

Skill source files live in [`skills/`](skills/) and are embedded in the binary at build time.

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

Options: `--source-type`, `--id`, `--document-id`, `--project`, `--title`

### Create and select a project

```bash
cliproot project create --id auth-refactor --name "Auth Refactor"
cliproot project list
cliproot project use auth-refactor
```

Once a current project is selected, `clip`, `derive`, and `artifact add` will use it by default unless you pass `--project`.

### Derive a clip from a parent

```bash
cliproot derive \
  --from sha256-abc123... \
  --quote "Summary: provenance is key." \
  --activity-type summary
```

Supported activity types: `verbatim`, `quote`, `summary`, `paraphrase`, `translate`, `combine`, `edit`, `ai_generate`, `unknown`

### Store and restore an artifact

```bash
# Add a file as an artifact
cliproot artifact add notes/plan.md --artifact-type markdown

# Or add inline content
cliproot artifact add \
  --content "# Prompt\n\nResearch OAuth PKCE tradeoffs." \
  --file-name prompt.md \
  --artifact-type markdown

# List artifacts in the current project
cliproot artifact list

# Restore an artifact back to disk
cliproot artifact restore sha256-... --output restored/
```

### Link a clip to an artifact

```bash
cliproot artifact link sha256-clip... sha256-artifact... --relationship cited_in
```

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
cliproot list --project auth-refactor
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

The exported bundle is CRP `v0.0.3` and includes project metadata, generalized edges, linked artifacts, and activities when they are reachable from the exported lineage.

### Ingest a CRP bundle

```bash
cliproot ingest bundle.json
```

### Create a `.cliprootpack`

```bash
# Export an entire project pack
cliproot pack create auth-refactor -o auth-refactor.cliprootpack

# Export from explicit roots with optional ancestor depth
cliproot pack create --root sha256-abc123 --root sha256-def456 --depth 2 -o research.cliprootpack
```

`.cliprootpack` archives are `tar.zst` files containing a pack `manifest.json`, bundled CRP
objects, and raw artifact blobs.

### Inspect or verify a pack

```bash
cliproot pack inspect auth-refactor.cliprootpack
cliproot pack verify auth-refactor.cliprootpack
```

`inspect --format json` emits the parsed manifest. `verify` checks manifest structure, archive
entry sizes/digests, bundled CRP object validity, and artifact hash integrity.

### Import a pack

```bash
cliproot pack import auth-refactor.cliprootpack

# Restore imported artifacts to a directory
cliproot pack import auth-refactor.cliprootpack --restore-artifacts ./context/
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

### Registry Authentication

When a registry has authentication enabled (`authRequired: true` in its config), write operations (`push`) require a valid token. The CLI supports OAuth 2.0 device flow login and environment-variable tokens for CI.

```bash
# Interactive login — opens browser for device authorization
cliproot login
cliproot login --remote origin

# CI/automation — store a pre-existing token directly
cliproot login --token crp_...

# Log out — removes stored credentials
cliproot logout
cliproot logout --remote origin
```

**Token resolution order:**
1. `CLIPROOT_TOKEN` environment variable (for CI pipelines)
2. System keychain (macOS Keychain, Linux Secret Service, Windows Credential Manager)
3. `~/.cliproot/credentials.json` file fallback

When pushing to an authenticated registry, the CLI automatically attaches the stored token:

```bash
export CLIPROOT_TOKEN=crp_...  # CI usage
cliproot push                  # token is attached automatically
```
### Reconstruct a design record from a Claude Code session

After a Claude Code session, reconstruct a structured design record showing what was explored, what sources were consulted, what files were touched, and what decisions were made.

```bash
# Reconstruct the most recent session (auto-detected from ~/.claude/projects/)
cliproot record

# Preview what would be captured without writing anything
cliproot record --dry-run

# Reconstruct a specific session by ID
cliproot record --session 811a6ab9

# Include the last 3 sessions (multi-day explorations)
cliproot record --last 3

# Point to an explicit JSONL transcript
cliproot record --jsonl ~/.claude/projects/-Volumes-.../session.jsonl

# Also create a .cliprootpack for sharing
cliproot record --pack
```

`cliproot record` parses the Claude Code JSONL transcript, cross-references clip/derive tool calls against `.cliproot/index.db`, merges the hook-generated agent log (from `cliproot capture-hook`) if available, infers activities from conversation turns, and produces:

- A session + activities stored in `.cliproot/` with full clip linkage
- A human-readable markdown design record at `.cliproot/records/rec-<id>.md`
- JSON output with `--format json`

The command requires either auto-detection from `~/.claude/projects/` or an explicit `--jsonl`/`--session-dir` path. If `cliproot init --hooks` was used, the hook log enriches the record with URLs fetched, files read/modified, and bash commands — even for tool calls that weren't captured as clips.

### Help

```bash
cliproot --help
cliproot <command> --help
cliproot help <command>
```

## Repository layout on disk

```
.cliproot/
├── config.json          # { "protocolVersion": "0.0.3", "currentProjectId": "..."? }
├── index.db             # SQLite — fast lookups by hash/id/document
├── artifacts/           # raw artifact bytes keyed by sha256-...
├── objects/
│   └── sha256-{hash}.json   # one bundle file per stored bundle
├── agent-log/           # PostToolUse hook capture logs (written by cliproot capture-hook)
│   └── {session-id}.jsonl
└── records/             # human-readable design records (written by cliproot record)
    └── rec-{id}.md
```

Clips are content-addressed: the same text from the same source always produces the same `clipHash`, regardless of when or where it was created.

## Contributing to Skills

Skill content lives in `skills/` and is the single source of truth — the binary embeds files from there via `include_str!()`, and the `.claude-plugin/` packaging copies are generated from the same source.

**After editing any `skills/*/SKILL.md`**, run:

```bash
just sync-skills
```

This copies the updated skill files into `.claude-plugin/skills/` so the plugin packaging stays in sync. CI enforces this — a PR with out-of-sync copies will fail the `skill-sync` check.

`just` is a command runner ([install](https://github.com/casey/just#installation): `cargo install just`, `brew install just`, or `mise use just`). The `justfile` at the repo root lists all available recipes; `just --list` shows them.

**To add a new skill:**
1. Create `skills/<skill-name>/SKILL.md` (and `agents/openai.yaml` if supporting Codex)
2. Add `include_str!()` constants to `crates/cliproot-cli/src/skills.rs`
3. Wire the new constants into `crates/cliproot-cli/src/commands/init/agent_config.rs`
4. Create the `.claude-plugin/skills/<skill-name>/` directory and run `just sync-skills`
5. Update the CI `skill-sync` job in `.github/workflows/ci.yml` to diff-check the new file

## CI/CD

GitHub Actions run automatically:

- **CI** (`ci.yml`) — on every push to `main` and all PRs:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo test --workspace`
  - Skill copies in sync (`diff -q` between `skills/` and `.claude-plugin/skills/`)

- **Release** (`release.yml`) — on tag push matching `v*` (e.g. `v0.1.0`):
  - Builds release binaries for Linux, macOS (x86 + ARM), and Windows
  - Creates a GitHub Release with attached archives

### Creating a release

```bash
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds all platform binaries and publishes them to [GitHub Releases](https://github.com/cliproot/cliproot_rust/releases).

## Protocol

This repository implements **[CRP v0.0.3](https://github.com/cliproot/cliproot/tree/main/spec)** (Draft). The protocol specification, canonical JSON Schemas, and TypeScript implementation live in the [cliproot/cliproot](https://github.com/cliproot/cliproot) repository.

| Resource | Location |
|---|---|
| Protocol specification | [`cliproot/spec/`](https://github.com/cliproot/cliproot/tree/main/spec) |
| Canonical JSON Schema | [`cliproot/schema/`](https://github.com/cliproot/cliproot/tree/main/schema) |
| TypeScript packages | [`cliproot/packages/`](https://github.com/cliproot/cliproot/tree/main/packages) |
| Web playground | [cliproot.github.io/cliproot](https://cliproot.github.io/cliproot/) |

Key features of CRP v0.0.3:
- top-level `project` metadata with single-project ownership via `projectId`
- generalized `edges` replacing `derivationEdges`
- top-level `artifacts` plus `clipArtifactRefs`
- extended `activities` with `prompt`, `parameters`, and `endedAt`
- bundle types: `document`, `clipboard`, `reuse-event`, `derivation`, `provenance-export`

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines, including how this repository relates to the main [cliproot/cliproot](https://github.com/cliproot/cliproot) repo.

## License

Apache License 2.0 — see [LICENSE](LICENSE) for details.
