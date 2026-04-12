#!/bin/bash
# Cliproot PreCompact hook for Claude Code
# Emergency consolidation before context window compaction

exec cliproot consolidate-hook --harness claude-code --emergency
