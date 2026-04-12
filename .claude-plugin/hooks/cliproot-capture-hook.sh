#!/bin/bash
# Cliproot PostToolUse hook for Claude Code
# Captures tool usage to the agent log for later consolidation

exec cliproot capture-hook --harness claude-code
