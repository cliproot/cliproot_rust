# Cliproot Claude Plugin

Provenance tracking for AI-assisted research and multi-agent workflows.

## Overview

Cliproot is a provenance-tracking system that helps you:
- **Capture sources** — Clip important passages from URLs, documents, and code
- **Record derivations** — Track how you synthesize information from multiple sources
- **Maintain audit trails** — Preserve context for handoff to other agents or humans
- **Generate citations** — Produce properly cited documents with full provenance

## Installation

### Prerequisites

The plugin does **not** install the `cliproot` binary for you. Hooks and the MCP server silently no-op if it's not on `PATH`:

```bash
cargo install --git https://github.com/cliproot/cliproot_rust cliproot-cli
which cliproot   # confirm it's on PATH
```

### Option A — Install from the published marketplace

```bash
claude plugin marketplace add cliproot/cliproot_rust
claude plugin install cliproot@cliproot_marketplace --scope user
```

### Option B — Install from a local checkout (dev)

Use this when iterating on the plugin locally and you want edits to apply immediately without pushing to GitHub. From the repo root:

```bash
claude plugin marketplace add ./.claude-plugin/marketplace.json
claude plugin install cliproot@cliproot_marketplace --scope user
```

Re-run the install command after editing `plugin.json`, `hooks/`, `skills/`, or `commands/` to pick up changes.

## Usage

Once installed, the plugin provides:

### MCP Tools
Access 28 cliproot tools for provenance capture and management

### Slash Commands
- `/cliproot:capture` — Start a provenance capture session
- `/cliproot:session` — Begin a full-ceremony research session with activity tracking
- `/cliproot:consolidate` — Manually trigger consolidation of unhighlighted sources

### Hooks
The plugin registers five hook scripts that capture tool usage, prompt for consolidation, and (at higher knowledge levels) distill the session log:
- **PostToolUse** (`cliproot-capture-hook`) — Appends each tool call to `.cliproot/agent-log/{session_id}.jsonl`. Captures `WebFetch`, `Read`, `Write`, `Edit`, `Bash`, `Agent`, and `mcp__cliproot__*`; skips `Glob`/`Grep`; truncates any field > 50 KB.
- **Stop** (`cliproot-consolidate-hook`) — Scans recent activity against the clip index and blocks the turn with a remediation prompt when sources were consulted but not clipped.
- **Stop** (`cliproot-flush-hook`) — At knowledge level `digest` or `wiki` only, spawns a detached Haiku call that writes `.cliproot/knowledge/daily/YYYY-MM-DD.md`. No-op at the default `curator` level.
- **PreCompact** (`cliproot-precompact-hook`) — Emergency consolidation before context compaction. Always runs, bypasses the normal interval check.
- **SessionStart** (`cliproot-session-start-hook`) — At level `wiki`, injects a short wiki snapshot (≤5 KB) as context. Hard 500 ms timeout — never blocks session start.

## Quick Start

1. **Initialize a cliproot store** in your project — hooks and MCP tools all no-op without one:
   ```bash
   cd ~/your/project
   cliproot init
   ```
   This creates `.cliproot/` with `config.json` and `index.db`. The store is discovered by walking up from CWD.

2. **Verify the plugin is wired** inside Claude Code:
   - `/plugin` lists `cliproot` as enabled.
   - `/mcp` shows a `cliproot` server with 28 tools.

   If `/mcp` is empty, start Claude with `claude --debug` and confirm `cliproot` is on `PATH` — the MCP server spawns silently otherwise.

3. **Create a project scope** inside the store:
   ```
   cliproot_project_create with id="my-research" and name="My Research Project"
   ```

4. **Capture sources** as you research:
   ```
   cliproot_clip the key insight from https://example.com/article
   ```

5. **Derive syntheses** when combining information:
   ```
   cliproot_derive a summary of these findings
   ```

6. **Review** when the consolidation prompt appears — clip anything important you haven't captured yet.

## Documentation

- [Cliproot Protocol](https://github.com/cliproot/cliproot)
- [Full Tool Reference](https://github.com/cliproot/cliproot_rust#tools)

## License

MIT — See [LICENSE](../LICENSE) for details.
