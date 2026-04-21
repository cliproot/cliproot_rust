#!/bin/bash
# Cliproot PreCompact hook for Claude Code
# Emergency consolidation before context window compaction

"${CLAUDE_PLUGIN_ROOT}/bin/install-cliproot.sh" || exit 0
exec cliproot hook consolidate --harness claude-code --emergency
