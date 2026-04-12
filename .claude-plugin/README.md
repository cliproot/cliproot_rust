# Cliproot Claude Plugin

Provenance tracking for AI-assisted research and multi-agent workflows.

## Overview

Cliproot is a provenance-tracking system that helps you:
- **Capture sources** — Clip important passages from URLs, documents, and code
- **Record derivations** — Track how you synthesize information from multiple sources
- **Maintain audit trails** — Preserve context for handoff to other agents or humans
- **Generate citations** — Produce properly cited documents with full provenance

## Installation

Install via the Claude CLI:

```bash
claude plugin install --scope user cliproot
```

Or install from this directory:

```bash
claude plugin install --scope user .
```

## Usage

Once installed, the plugin provides:

### MCP Tools
Access ~24 cliproot tools for provenance capture and management

### Slash Commands
- `/cliproot:capture` — Start a provenance capture session
- `/cliproot:session` — Begin a full-ceremony research session with activity tracking
- `/cliproot:consolidate` — Manually trigger consolidation of unhighlighted sources

### Hooks
The plugin automatically captures tool usage and prompts for consolidation when needed:
- **PostToolUse** — Logs WebFetch, Read, Write, Edit, Bash, Agent, and MCP tool calls
- **Stop** — Scans recent activity and prompts you clip important sources
- **PreCompact** — Emergency consolidation before context compaction

## Quick Start

1. **Create a project** to scope your work:
   ```
   cliproot_project_create with id="my-research" and name="My Research Project"
   ```

2. **Capture sources** as you research:
   ```
   cliproot_clip the key insight from https://example.com/article
   ```

3. **Derive syntheses** when combining information:
   ```
   cliproot_derive a summary of these findings
   ```

4. **Review** when the consolidation prompt appears — clip anything important you haven't captured yet

## Documentation

- [Cliproot Protocol](https://github.com/cliproot/cliproot)
- [Full Tool Reference](https://github.com/cliproot/cliproot_rust#tools)

## License

MIT — See [LICENSE](../LICENSE) for details.
