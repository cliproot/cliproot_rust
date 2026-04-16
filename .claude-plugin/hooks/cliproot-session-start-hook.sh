#!/usr/bin/env sh
"${CLAUDE_PLUGIN_ROOT}/bin/install-cliproot.sh" || exit 0
exec cliproot session-start-hook --harness claude-code
