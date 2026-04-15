#!/bin/bash
# Cliproot PostToolUse hook for Claude Code
# Captures tool usage to the agent log for later consolidation

"${CLAUDE_PLUGIN_ROOT}/bin/install-cliproot.sh" || exit 0
exec cliproot capture-hook --harness claude-code
