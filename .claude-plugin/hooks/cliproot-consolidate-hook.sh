#!/bin/bash
# Cliproot Stop hook for Claude Code
# Scans recent activity and prompts for consolidation of unhighlighted sources

"${CLAUDE_PLUGIN_ROOT}/bin/install-cliproot.sh" || exit 0
exec cliproot hook consolidate --harness claude-code
