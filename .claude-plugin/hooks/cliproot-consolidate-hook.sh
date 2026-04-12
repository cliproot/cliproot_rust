#!/bin/bash
# Cliproot Stop hook for Claude Code
# Scans recent activity and prompts for consolidation of unhighlighted sources

exec cliproot consolidate-hook --harness claude-code
