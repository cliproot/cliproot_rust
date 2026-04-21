# Cursor Integration

This directory contains sample configuration files for [Cursor](https://cursor.com) IDE integration with Cliproot.

## Prerequisites

- Cursor IDE with hook support (version 0.45+)
- `cliproot` CLI installed and available in your PATH

## Setup

### Manual Setup

1. **MCP Server**: Copy `mcp.json` to your project's `.cursor/mcp.json`

   ```bash
   mkdir -p .cursor
   cp integrations/cursor/mcp.json .cursor/mcp.json
   ```

2. **Hooks**: Copy `hooks.json` to your project's `.cursor/hooks.json`

   ```bash
   cp integrations/cursor/hooks.json .cursor/hooks.json
   ```

3. **Rules**: Copy `rules/cliproot-capture.mdc` to your project's `.cursor/rules/`

   ```bash
   mkdir -p .cursor/rules
   cp integrations/cursor/rules/cliproot-capture.mdc .cursor/rules/
   ```

### Automated Setup (via cliproot CLI)

After initializing your project with `cliproot init`, run:

```bash
cliproot init --agent
```

This will write the MCP config and rules to `.cursor/`. Then manually copy or merge `hooks.json` to enable automatic capture.

## How it Works

### postToolUse Hook

Captures WebFetch, Read, Write, Edit, Bash, and Agent tool calls to `.cliproot/agent-log/<session>.jsonl`.

### stop Hook

When you stop a conversation, scans the log for unclipped sources and surfaces candidates for consolidation. Returns a `followup_message` that appears as the next user turn in Cursor.

### preCompact Hook

Emergency consolidation that always runs before context compaction, writing candidates to an artifact if any are found. Observational only in Cursor (cannot block), but hints the next `stop` hook to tighten its interval.

## Matcher Reference

The `hooks.json` uses Cursor's matcher syntax:

- `"WebFetch|Read|Write|Edit|Bash|Agent"` — matches these exact tool names
- `"mcp__cliproot__*"` — matches any MCP tool prefixed with `mcp__cliproot__`

## Troubleshooting

**Hooks not firing?**
- Ensure Cursor has hook support enabled (check Cursor docs)
- Verify `cliproot` is in your PATH: `which cliproot`
- Check `.cliproot/agent-log/` exists and is writable

**Missing candidates?**
- Run `cliproot session consolidate --session <session-id>` manually to check
- Review the agent log: `cat .cliproot/agent-log/<session>.jsonl`

## Differences from Claude Code Hooks

| Feature | Claude Code | Cursor |
|---------|-------------|--------|
| Config file | `.claude/settings.json` | `.cursor/hooks.json` |
| Event naming | PascalCase (`PostToolUse`) | camelCase (`postToolUse`) |
| Block semantics | `{"decision": "block"}` | `{"followup_message": "..."}` |
| preCompact | Can block | Observational only |
| Env vars | `$CLAUDE_PROJECT_DIR` | `$CURSOR_PROJECT_DIR` (and `$CLAUDE_PROJECT_DIR` alias) |

The harness-aware dispatcher in `cliproot hook capture --harness cursor` and `cliproot hook consolidate --harness cursor` handles these differences automatically.
